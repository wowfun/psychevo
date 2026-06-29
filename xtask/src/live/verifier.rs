use std::fs;
use std::path::Path;

use anyhow::{Context, Result, bail};
use serde::Serialize;
use serde_json::Value;

#[derive(Debug, Eq, PartialEq, Serialize)]
pub(crate) struct ProviderSmokeVerification {
    pub(crate) reasoning_seen: bool,
    pub(crate) read_tool_seen: bool,
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
        entry_blocks(event).any(|block| {
            block.get("kind").and_then(Value::as_str) == Some("reasoning")
                && block
                    .get("body")
                    .and_then(Value::as_str)
                    .is_some_and(|body| !body.trim().is_empty())
        })
    });
    if !reasoning_seen {
        bail!("{provider}: missing reasoning transcript entry");
    }

    let read_tool_seen = first.iter().any(|event| {
        entry_blocks(event).any(|block| {
            let metadata = block.get("metadata").unwrap_or(&Value::Null);
            metadata.get("tool_name").and_then(Value::as_str) == Some("read")
                && metadata.get("outcome").and_then(Value::as_str) == Some("normal")
        })
    });
    if !read_tool_seen {
        bail!("{provider}: first run did not complete read");
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
        read_tool_seen,
        reused_thread,
        token_seen_in_first,
        token_seen_in_second,
    })
}

fn load_events(path: &Path) -> Result<Vec<Value>> {
    let text = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    text.lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str(line).with_context(|| format!("parse {}", path.display())))
        .collect()
}

fn entry_blocks(event: &Value) -> impl Iterator<Item = &Value> {
    event
        .get("entry")
        .filter(|_| event.get("type").and_then(Value::as_str) == Some("entry.completed"))
        .and_then(|entry| entry.get("blocks"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
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
    let final_answer = events.iter().rev().find_map(|event| {
        matches!(
            event.get("type").and_then(Value::as_str),
            Some("turn.completed" | "turn.failed")
        )
        .then(|| event.get("finalAnswer").and_then(Value::as_str))
        .flatten()
        .filter(|text| !text.trim().is_empty())
        .map(str::to_string)
    });
    if let Some(final_answer) = final_answer {
        return final_answer;
    }

    events
        .iter()
        .filter_map(|event| {
            event
                .get("entry")
                .filter(|_| event.get("type").and_then(Value::as_str) == Some("entry.completed"))
        })
        .filter(|entry| entry.get("role").and_then(Value::as_str) == Some("assistant"))
        .flat_map(|entry| {
            entry
                .get("blocks")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
        })
        .filter(|block| block.get("kind").and_then(Value::as_str) == Some("text"))
        .filter_map(|block| block.get("body").and_then(Value::as_str))
        .collect::<Vec<_>>()
        .join("\n")
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
{"type":"entry.completed","entry":{"role":"assistant","blocks":[{"kind":"reasoning","body":"thinking"},{"kind":"tool","metadata":{"tool_name":"read","outcome":"normal"}}]}}
{"type":"turn.completed","finalAnswer":"token ABC"}
"#,
        )
        .expect("first");
        fs::write(
            &second,
            r#"{"type":"thread.started","threadId":"thread-1"}
{"type":"entry.completed","entry":{"role":"assistant","blocks":[{"kind":"text","body":"token ABC"}]}}
"#,
        )
        .expect("second");

        let verified = verify_provider_smoke("demo", "ABC", &first, &second).expect("verified");
        assert_eq!(
            verified,
            ProviderSmokeVerification {
                reasoning_seen: true,
                read_tool_seen: true,
                reused_thread: true,
                token_seen_in_first: true,
                token_seen_in_second: true,
            }
        );
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn verifier_rejects_missing_read_tool() {
        let dir = temp_dir("psychevo-xtask-live-verify-fail");
        fs::create_dir_all(&dir).expect("dir");
        let first = dir.join("first.ndjson");
        let second = dir.join("second.ndjson");
        fs::write(
            &first,
            r#"{"type":"thread.started","threadId":"thread-1"}
{"type":"entry.completed","entry":{"role":"assistant","blocks":[{"kind":"reasoning","body":"thinking"}]}}
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
        assert!(err.to_string().contains("did not complete read"));
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
