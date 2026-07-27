use std::fs;
use std::path::Path;

use anyhow::{Context, Result, bail};
use serde::Serialize;
use serde_json::Value;

#[derive(Debug, Eq, PartialEq, Serialize)]
pub(crate) struct ProviderSmokeVerification {
    pub(crate) reasoning_seen: bool,
    pub(crate) file_inspection_seen: bool,
    pub(crate) reused_thread: bool,
    pub(crate) token_seen_in_first: bool,
    pub(crate) token_seen_in_second: bool,
}

pub(crate) fn verify_provider_smoke(
    provider: &str,
    token: &str,
    first_path: &Path,
    second_path: &Path,
) -> Result<ProviderSmokeVerification> {
    let first = load_events(first_path)?;
    let second = load_events(second_path)?;
    let combined = first.iter().chain(second.iter()).collect::<Vec<_>>();

    let reasoning_seen = combined.iter().any(|event| {
        event.get("type").and_then(Value::as_str) == Some("item.updated")
            && blocks(event).any(|block| {
                block.get("kind").and_then(Value::as_str) == Some("reasoning")
                    && ["body", "preview", "detail"].iter().any(|field| {
                        block
                            .get(*field)
                            .and_then(Value::as_str)
                            .is_some_and(|text| !text.trim().is_empty())
                    })
            })
    });
    if !reasoning_seen {
        bail!("{provider}: missing reasoning transcript entry");
    }

    let file_inspection_seen = first.iter().any(|event| {
        event.get("type").and_then(Value::as_str) == Some("item.completed")
            && blocks(event).any(|block| {
                let metadata = block.get("metadata");
                metadata
                    .and_then(|value| value.get("projection"))
                    .and_then(Value::as_str)
                    == Some("tool")
                    && metadata
                        .and_then(|value| value.get("type"))
                        .and_then(Value::as_str)
                        == Some("tool_execution_end")
                    && metadata
                        .and_then(|value| value.get("outcome"))
                        .and_then(Value::as_str)
                        == Some("normal")
                    && tool_result_contains(block, token)
            })
    });
    if !file_inspection_seen {
        bail!("{provider}: first run did not complete file inspection");
    }

    let first_thread = thread_id(&first);
    let second_thread = thread_id(&second);
    let reused_thread = first_thread.is_some() && first_thread == second_thread;
    if !reused_thread {
        bail!("{provider}: --continue did not reuse the session");
    }

    let first_text = final_text(&first);
    let second_text = final_text(&second);
    let token_seen_in_first = first_text.contains(token);
    if !token_seen_in_first {
        bail!("{provider}: first final answer did not contain token {token}");
    }
    let token_seen_in_second = second_text.contains(token);
    if !token_seen_in_second {
        bail!("{provider}: continue final answer did not contain token {token}");
    }

    Ok(ProviderSmokeVerification {
        reasoning_seen,
        file_inspection_seen,
        reused_thread,
        token_seen_in_first,
        token_seen_in_second,
    })
}

fn blocks(event: &Value) -> impl Iterator<Item = &Value> {
    event
        .pointer("/item/blocks")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
}

fn tool_result_contains(block: &Value, expected: &str) -> bool {
    block
        .pointer("/metadata/result")
        .is_some_and(|result| value_contains(result, expected))
        || ["body", "preview", "detail"].iter().any(|field| {
            block
                .get(*field)
                .is_some_and(|value| value_contains(value, expected))
        })
}

fn value_contains(value: &Value, expected: &str) -> bool {
    match value {
        Value::String(text) => text.contains(expected),
        Value::Array(values) => values.iter().any(|value| value_contains(value, expected)),
        Value::Object(values) => values.values().any(|value| value_contains(value, expected)),
        Value::Null | Value::Bool(_) | Value::Number(_) => false,
    }
}

fn load_events(path: &Path) -> Result<Vec<Value>> {
    let text = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    text.lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str(line).with_context(|| format!("parse {}", path.display())))
        .collect()
}

fn thread_id(events: &[Value]) -> Option<String> {
    events.iter().find_map(|event| {
        (event.get("type").and_then(Value::as_str) == Some("thread.started"))
            .then(|| event.get("threadId").and_then(Value::as_str))
            .flatten()
            .map(str::to_string)
    })
}

fn final_text(events: &[Value]) -> String {
    events
        .iter()
        .rev()
        .find_map(|event| {
            matches!(
                event.get("type").and_then(Value::as_str),
                Some("turn.completed" | "turn.failed")
            )
            .then(|| event.get("finalAnswer").and_then(Value::as_str))
            .flatten()
            .filter(|text| !text.trim().is_empty())
            .map(str::to_string)
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;

    #[test]
    fn verifier_accepts_reasoning_read_continue_and_tokens() {
        let dir = temp_dir("psychevo-xtask-live-verify-ok");
        fs::create_dir_all(&dir).expect("dir");
        let first = dir.join("first.ndjson");
        let second = dir.join("second.ndjson");
        fs::write(
            &first,
            r#"{"type":"thread.started","threadId":"thread-1"}
{"type":"item.updated","item":{"blocks":[{"kind":"reasoning","body":"thinking"}]}}
{"type":"item.completed","item":{"blocks":[{"kind":"file","metadata":{"projection":"tool","type":"tool_execution_end","tool_name":"read","outcome":"normal","result":{"content":"token ABC"}}}]}}
{"type":"turn.completed","finalAnswer":"token ABC"}
"#,
        )
        .expect("first");
        fs::write(
            &second,
            r#"{"type":"thread.started","threadId":"thread-1"}
{"type":"turn.completed","finalAnswer":"token ABC"}
"#,
        )
        .expect("second");

        let verified = verify_provider_smoke("demo", "ABC", &first, &second).expect("verified");
        assert_eq!(
            verified,
            ProviderSmokeVerification {
                reasoning_seen: true,
                file_inspection_seen: true,
                reused_thread: true,
                token_seen_in_first: true,
                token_seen_in_second: true,
            }
        );
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn verifier_accepts_successful_exec_command_file_inspection() {
        let dir = temp_dir("psychevo-xtask-live-verify-exec");
        fs::create_dir_all(&dir).expect("dir");
        let first = dir.join("first.ndjson");
        let second = dir.join("second.ndjson");
        fs::write(
            &first,
            r#"{"type":"thread.started","threadId":"thread-1"}
{"type":"item.updated","item":{"blocks":[{"kind":"reasoning","body":"thinking"}]}}
{"type":"item.completed","item":{"blocks":[{"kind":"shell","body":"{\"exit_code\":0,\"output\":\"probe token: ABC\\n\"}","metadata":{"projection":"tool","type":"tool_execution_end","tool_name":"exec_command","outcome":"normal","result":{"exit_code":0,"output":"probe token: ABC\n"}}}]}}
{"type":"turn.completed","finalAnswer":"token ABC"}
"#,
        )
        .expect("first");
        fs::write(
            &second,
            r#"{"type":"thread.started","threadId":"thread-1"}
{"type":"turn.completed","finalAnswer":"token ABC"}
"#,
        )
        .expect("second");

        let verified = verify_provider_smoke("demo", "ABC", &first, &second).expect("verified");
        assert!(verified.file_inspection_seen);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn verifier_rejects_missing_file_inspection() {
        let dir = temp_dir("psychevo-xtask-live-verify-fail");
        fs::create_dir_all(&dir).expect("dir");
        let first = dir.join("first.ndjson");
        let second = dir.join("second.ndjson");
        fs::write(
            &first,
            r#"{"type":"thread.started","threadId":"thread-1"}
{"type":"item.updated","item":{"blocks":[{"kind":"reasoning","body":"thinking"}]}}
{"type":"turn.completed","finalAnswer":"token ABC"}
"#,
        )
        .expect("first");
        fs::write(
            &second,
            r#"{"type":"thread.started","threadId":"thread-1"}
{"type":"turn.completed","finalAnswer":"token ABC"}
"#,
        )
        .expect("second");
        let err = verify_provider_smoke("demo", "ABC", &first, &second).expect_err("failure");
        assert!(err.to_string().contains("did not complete file inspection"));
        let _ = fs::remove_dir_all(dir);
    }

    fn temp_dir(prefix: &str) -> std::path::PathBuf {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        std::env::temp_dir().join(format!("{prefix}-{}-{now}", std::process::id()))
    }
}
