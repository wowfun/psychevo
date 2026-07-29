use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use futures::future::BoxFuture;
use psychevo_agent_core::{ToolBinding, ToolDisplaySpec, ToolExecutionMode, ToolOutput};
use psychevo_ai::AbortSignal;
use serde_json::{Value, json};
use tempfile::tempdir;

use super::*;

fn source(kind: &str, hooks: Value) -> HookSourceDescriptor {
    let kind = if kind == "agent" { "runtime" } else { kind };
    HookSourceDescriptor::new(format!("{kind}:test"), kind, None, None, hooks)
}

fn abort_signal() -> AbortSignal {
    let (_tx, rx) = tokio::sync::watch::channel(false);
    AbortSignal::new(rx)
}

struct RecordingExecTool {
    seen: Arc<Mutex<Vec<Value>>>,
}

impl ToolBinding for RecordingExecTool {
    fn name(&self) -> &str {
        "exec_command"
    }

    fn description(&self) -> &str {
        "record exec arguments"
    }

    fn parameters(&self) -> Value {
        json!({"type": "object", "properties": {"cmd": {"type": "string"}}})
    }

    fn execution_mode(&self) -> ToolExecutionMode {
        ToolExecutionMode::Sequential
    }

    fn display_spec(&self) -> ToolDisplaySpec {
        ToolDisplaySpec::for_name(self.name())
    }

    fn execute(
        &self,
        _tool_call_id: String,
        args: Value,
        _abort: AbortSignal,
    ) -> BoxFuture<'static, ToolOutput> {
        let seen = Arc::clone(&self.seen);
        Box::pin(async move {
            seen.lock().expect("seen").push(args.clone());
            ToolOutput::ok(args)
        })
    }
}

#[tokio::test]
async fn pre_tool_use_exit_two_blocks_with_stderr() {
    let temp = tempdir().expect("temp");
    let hooks = json!({"PreToolUse": ["cat >/dev/null; echo blocked >&2; exit 2"]});
    let result = run_hook_sources(
        &[source("agent", hooks)],
        "PreToolUse",
        temp.path(),
        &json!({"tool": "read"}),
    )
    .await;

    assert_eq!(result.blocked_reason.as_deref(), Some("blocked"));
    assert_eq!(result.summaries[0].status, HookRunStatus::Blocked);
}

#[tokio::test]
async fn non_blocking_hook_failures_are_diagnostics() {
    let temp = tempdir().expect("temp");
    let hooks = json!({"PreToolUse": ["echo nope >&2; exit 1"]});
    let result = run_hook_sources(
        &[source("agent", hooks)],
        "PreToolUse",
        temp.path(),
        &json!({}),
    )
    .await;

    assert_eq!(result.blocked_reason, None);
    assert_eq!(result.summaries[0].status, HookRunStatus::Failed);
    assert_eq!(result.summaries[0].exit_code, Some(1));
}

#[cfg(unix)]
#[tokio::test]
async fn concurrent_events_share_the_runtime_wide_eight_handler_limit() {
    let temp = tempdir().expect("temp");
    let counter = temp.path().join("counter.txt");
    fs::write(&counter, "0 0").expect("counter");
    let script = temp.path().join("counter.py");
    fs::write(
        &script,
        r#"import fcntl
import pathlib
import sys
import time

path = pathlib.Path(sys.argv[1])
with path.open("r+") as state:
    fcntl.flock(state, fcntl.LOCK_EX)
    active, maximum = map(int, state.read().split())
    active += 1
    maximum = max(active, maximum)
    state.seek(0)
    state.truncate()
    state.write(f"{active} {maximum}")
    state.flush()
    fcntl.flock(state, fcntl.LOCK_UN)
time.sleep(0.15)
with path.open("r+") as state:
    fcntl.flock(state, fcntl.LOCK_EX)
    active, maximum = map(int, state.read().split())
    state.seek(0)
    state.truncate()
    state.write(f"{active - 1} {maximum}")
    state.flush()
"#,
    )
    .expect("counter script");
    let command = format!(
        "python3 {} {}",
        script.display(),
        counter.display()
    );
    let handlers = (0..8)
        .map(|_| json!({"type": "command", "command": command, "timeout": 5}))
        .collect::<Vec<_>>();
    let runtime = HookRuntime::new(
        temp.path().to_path_buf(),
        HookRuntimeConfig {
            sources: vec![source(
                "agent",
                json!({"PostToolUse": [{"hooks": handlers}]}),
            )],
            ..HookRuntimeConfig::default()
        },
    );

    let first_payload = json!({"tool": "read"});
    let second_payload = json!({"tool": "write"});
    let (first, second) = tokio::join!(
        runtime.run_event("PostToolUse", &first_payload),
        runtime.run_event("PostToolUse", &second_payload),
    );
    assert_eq!(first.summaries.len(), 8);
    assert_eq!(second.summaries.len(), 8);
    let counter_result = fs::read_to_string(counter).expect("counter result");
    let (_, maximum) = counter_result
        .split_once(' ')
        .expect("counter shape");
    assert_eq!(
        maximum.trim().parse::<usize>().expect("maximum"),
        8,
        "one HookRuntime admitted more than eight concurrent handlers"
    );
}

#[test]
fn canonical_and_legacy_hook_shapes_normalize_to_same_hash() {
    let temp = tempdir().expect("temp");
    let legacy = HookRuntime::new(
        temp.path().to_path_buf(),
        HookRuntimeConfig {
            sources: vec![source("agent", json!({"PreToolUse": ["echo ok"]}))],
            ..HookRuntimeConfig::default()
        },
    );
    let canonical = HookRuntime::new(
        temp.path().to_path_buf(),
        HookRuntimeConfig {
            sources: vec![source(
                "agent",
                json!({"PreToolUse": [{"matcher": "*", "hooks": [{"type": "command", "command": "echo ok"}]}]}),
            )],
            ..HookRuntimeConfig::default()
        },
    );
    assert_eq!(
        legacy.metadata()[0].current_hash,
        canonical.metadata()[0].current_hash
    );
}

#[test]
fn project_hooks_require_trusted_hash_and_detect_modification() {
    let temp = tempdir().expect("temp");
    let hooks = json!({"PreToolUse": [{"matcher": "Bash", "hooks": [{"type": "command", "command": "echo ok"}]}]});
    let untrusted = HookRuntime::new(
        temp.path().to_path_buf(),
        HookRuntimeConfig {
            sources: vec![source("project", hooks.clone())],
            ..HookRuntimeConfig::default()
        },
    );
    let metadata = untrusted.metadata()[0].clone();
    assert_eq!(metadata.trust_status, HookTrustStatus::Untrusted);
    assert_eq!(metadata.skipped_reason.as_deref(), Some("untrusted"));

    let mut state = HookStateStore::default();
    state.state.insert(
        metadata.key.clone(),
        HookStateRecord {
            enabled: true,
            trusted_hash: Some(metadata.current_hash.clone()),
        },
    );
    let trusted = HookRuntime::new(
        temp.path().to_path_buf(),
        HookRuntimeConfig {
            sources: vec![source("project", hooks)],
            state: state.clone(),
            bypass_trust: false,
        },
    );
    assert_eq!(trusted.metadata()[0].trust_status, HookTrustStatus::Trusted);

    let modified = HookRuntime::new(
        temp.path().to_path_buf(),
        HookRuntimeConfig {
            sources: vec![source(
                "project",
                json!({"PreToolUse": [{"matcher": "Bash", "hooks": [{"type": "command", "command": "echo changed"}]}]}),
            )],
            state,
            bypass_trust: false,
        },
    );
    assert_eq!(
        modified.metadata()[0].trust_status,
        HookTrustStatus::Modified
    );
}

#[tokio::test]
async fn capability_root_hooks_are_source_qualified_and_fail_closed() {
    let temp = tempdir().expect("temp");
    let marker = temp.path().join("capability-root-ran");
    let hooks = json!({"SessionStart": [{"hooks": [{
        "type": "command",
        "command": format!("printf ok > {}", marker.display())
    }]}]});
    let untrusted = HookRuntime::new(
        temp.path().to_path_buf(),
        HookRuntimeConfig {
            sources: vec![source("capability_root", hooks.clone())],
            ..HookRuntimeConfig::default()
        },
    );
    let metadata = untrusted.metadata()[0].clone();

    assert_eq!(metadata.source_kind, "capability_root");
    assert_eq!(metadata.trust_status, HookTrustStatus::Untrusted);
    assert_eq!(metadata.skipped_reason.as_deref(), Some("untrusted"));
    let result = untrusted.run_event("SessionStart", &json!({})).await;
    assert_eq!(result.summaries[0].status, HookRunStatus::Skipped);
    assert!(!marker.exists());

    let mut state = HookStateStore::default();
    state.state.insert(
        metadata.key.clone(),
        HookStateRecord {
            enabled: true,
            trusted_hash: Some(metadata.current_hash.clone()),
        },
    );
    let trusted = HookRuntime::new(
        temp.path().to_path_buf(),
        HookRuntimeConfig {
            sources: vec![source("capability_root", hooks)],
            state,
            ..HookRuntimeConfig::default()
        },
    );
    assert_eq!(trusted.metadata()[0].trust_status, HookTrustStatus::Trusted);
}

#[test]
fn hook_keys_ignore_unrelated_sources_events_and_matcher_groups() {
    let temp = tempdir().expect("temp");
    let hooks = json!({"PreToolUse": [{"matcher": "Bash", "hooks": [{"type": "command", "command": "echo ok"}]}]});
    let original = HookRuntime::new(
        temp.path().to_path_buf(),
        HookRuntimeConfig {
            sources: vec![source("project", hooks.clone())],
            ..HookRuntimeConfig::default()
        },
    );
    let metadata = original.metadata()[0].clone();
    let mut state = HookStateStore::default();
    state.state.insert(
        metadata.key.clone(),
        HookStateRecord {
            enabled: true,
            trusted_hash: Some(metadata.current_hash.clone()),
        },
    );

    let shifted = HookRuntime::new(
        temp.path().to_path_buf(),
        HookRuntimeConfig {
            sources: vec![
                source("profile", json!({"SessionStart": ["echo profile"]})),
                source(
                    "project",
                    json!({
                        "SessionStart": ["echo project-start"],
                        "PreToolUse": [
                            {"matcher": "Write", "hooks": [{"type": "command", "command": "echo write"}]},
                            {"matcher": "Bash", "hooks": [{"type": "command", "command": "echo ok"}]}
                        ]
                    }),
                ),
            ],
            state,
            ..HookRuntimeConfig::default()
        },
    );
    let shifted_metadata = shifted
        .metadata()
        .into_iter()
        .find(|item| {
            item.source_id == "project:test"
                && item.event == "PreToolUse"
                && item.matcher.as_deref() == Some("Bash")
        })
        .expect("project bash hook");

    assert_eq!(shifted_metadata.key, metadata.key);
    assert_eq!(shifted_metadata.trust_status, HookTrustStatus::Trusted);
}

#[test]
fn same_matcher_group_same_type_handlers_get_distinct_keys() {
    let temp = tempdir().expect("temp");
    let runtime = HookRuntime::new(
        temp.path().to_path_buf(),
        HookRuntimeConfig {
            sources: vec![source(
                "project",
                json!({"PreToolUse": [{"matcher": "Bash", "hooks": [
                    {"type": "command", "command": "echo one"},
                    {"type": "command", "command": "echo two"}
                ]}]}),
            )],
            ..HookRuntimeConfig::default()
        },
    );
    let metadata = runtime.metadata();

    assert_ne!(metadata[0].key, metadata[1].key);
}

#[tokio::test]
async fn bounded_hook_output_truncates_on_utf8_boundary() {
    let temp = tempdir().expect("temp");
    let hooks = json!({"PreToolUse": [{"hooks": [{
        "type": "command",
        "command": "python3 -c 'import sys; sys.stdout.buffer.write(b\"a\" + bytes([0xc3, 0xa9]) * 4096)'"
    }]}]});
    let result = run_hook_sources(
        &[source("agent", hooks)],
        "PreToolUse",
        temp.path(),
        &json!({"tool": "exec_command"}),
    )
    .await;
    let stdout = &result.summaries[0].stdout;

    assert!(stdout.ends_with("...[truncated]"), "{stdout:?}");
    assert!(stdout.is_char_boundary(stdout.len()));
}

#[tokio::test]
async fn hook_output_drains_large_stdout_and_stderr_with_explicit_diagnostics() {
    let temp = tempdir().expect("temp");
    let hooks = json!({"PostToolUse": [{"hooks": [{
        "type": "command",
        "command": "python3 -c 'import sys; sys.stdout.write(\"o\" * 1048576); sys.stderr.write(\"e\" * 1048576)'"
    }]}]});
    let result = run_hook_sources(
        &[source("agent", hooks)],
        "PostToolUse",
        temp.path(),
        &json!({"tool": "exec_command"}),
    )
    .await;
    let summary = &result.summaries[0];

    assert!(summary.stdout.ends_with("...[truncated]"));
    assert!(summary.stderr.ends_with("...[truncated]"));
    assert!(summary.stdout.len() < 4200);
    assert!(summary.stderr.len() < 4200);
    assert!(
        summary
            .diagnostics
            .iter()
            .any(|item| item == "hook stdout truncated after 4096 bytes")
    );
    assert!(
        summary
            .diagnostics
            .iter()
            .any(|item| item == "hook stderr truncated after 4096 bytes")
    );
}

#[tokio::test]
async fn matching_command_hooks_launch_concurrently_and_summaries_keep_declaration_order() {
    let temp = tempdir().expect("temp");
    let marker = temp.path().join("marker");
    let hooks = json!({
        "PreToolUse": [
            {"matcher": "Bash", "hooks": [
                {"type": "command", "command": format!("sleep 0.2; echo slow >> {}", marker.display())}
            ]},
            {"matcher": "Bash", "hooks": [
                {"type": "command", "command": format!("echo fast >> {}", marker.display())}
            ]}
        ]
    });
    let started = Instant::now();
    let result = run_hook_sources(
        &[source("agent", hooks)],
        "PreToolUse",
        temp.path(),
        &json!({"tool": "exec_command"}),
    )
    .await;
    assert!(started.elapsed() < Duration::from_millis(350));
    assert_eq!(result.summaries.len(), 2);
    assert_eq!(result.summaries[0].display_order, 0);
    assert_eq!(result.summaries[1].display_order, 1);
    let marker = fs::read_to_string(marker).expect("marker");
    assert!(marker.contains("fast"));
    assert!(marker.contains("slow"));
}

#[tokio::test]
async fn pre_tool_use_declaration_order_resolves_updated_input() {
    let temp = tempdir().expect("temp");
    let hooks = json!({
        "PreToolUse": [
            {"matcher": "Bash", "hooks": [
                {"type": "command", "command": "printf '{\"updatedInput\":{\"cmd\":\"slow\"}}'; sleep 0.2"}
            ]},
            {"matcher": "Bash", "hooks": [
                {"type": "command", "command": "printf '{\"updatedInput\":{\"cmd\":\"fast\"}}'"}
            ]}
        ]
    });
    let result = run_hook_sources(
        &[source("agent", hooks)],
        "PreToolUse",
        temp.path(),
        &json!({"tool": "exec_command"}),
    )
    .await;
    assert_eq!(
        result.updated_input.as_ref().unwrap(),
        &json!({"cmd": "fast"})
    );
}

#[tokio::test]
async fn hook_runtime_caps_matching_handlers_at_eight() {
    let temp = tempdir().expect("temp");
    let handlers = (0..9)
        .map(|_| json!({"type": "command", "command": "sleep 1"}))
        .collect::<Vec<_>>();
    let hooks = json!({"Notification": [{"hooks": handlers}]});
    let started = Instant::now();
    let result = run_hook_sources(
        &[source("agent", hooks)],
        "Notification",
        temp.path(),
        &json!({}),
    )
    .await;
    let elapsed = started.elapsed();

    assert_eq!(result.summaries.len(), 9);
    assert!(
        elapsed >= Duration::from_millis(1800),
        "nine one-second handlers exceeded the eight-handler cap too quickly: {elapsed:?}"
    );
    assert!(
        elapsed < Duration::from_secs(4),
        "handlers did not retain bounded concurrency: {elapsed:?}"
    );
}

#[test]
fn agent_hook_provenance_maps_source_and_path_without_blanket_trust() {
    let project_path = PathBuf::from("/workspace/.psychevo/agents/reviewer.md");
    let project = crate::agents::parse_agent_definition_text(
        "---\ndescription: review changes\nhooks:\n  PreToolUse:\n    - echo ok\n---\nreview",
        "reviewer",
        Some(project_path.clone()),
        crate::agents::AgentSource::Project,
    )
    .expect("project agent");
    let project_source = agent_hook_source(&project).expect("project hook source");
    assert_eq!(project_source.source_kind, "project");
    assert_eq!(project_source.path.as_ref(), Some(&project_path));
    assert_eq!(project_source.source_id, "agent:project:reviewer");

    let generated = crate::agents::parse_agent_definition_text(
        "---\ndescription: review changes\nhooks:\n  PreToolUse:\n    - echo ok\n---\nreview",
        "generated-reviewer",
        None,
        crate::agents::AgentSource::Generated,
    )
    .expect("generated agent");
    let generated_source = agent_hook_source(&generated).expect("generated hook source");
    assert_eq!(generated_source.source_kind, "runtime");
    assert_eq!(
        generated_source.source_id,
        "agent:generated:generated-reviewer"
    );
}

#[tokio::test]
async fn permission_request_deny_wins_over_allow() {
    let temp = tempdir().expect("temp");
    let hooks = json!({
        "PermissionRequest": [
            {"matcher": "Bash", "hooks": [
                {"type": "command", "command": "printf '{\"decision\":\"allow\"}'"}
            ]},
            {"matcher": "Bash", "hooks": [
                {"type": "command", "command": "printf '{\"decision\":\"deny\"}'"}
            ]}
        ]
    });
    let result = run_hook_sources(
        &[source("agent", hooks)],
        "PermissionRequest",
        temp.path(),
        &json!({"tool": "exec_command"}),
    )
    .await;
    assert_eq!(
        result.permission_decision,
        Some(HookPermissionDecision::Deny)
    );
}

#[tokio::test]
async fn command_timeout_is_bounded_diagnostic() {
    let temp = tempdir().expect("temp");
    let hooks = json!({"PreToolUse": [{"hooks": [{"type": "command", "command": "sleep 2", "timeout": 1}]}]});
    let result = run_hook_sources(
        &[source("agent", hooks)],
        "PreToolUse",
        temp.path(),
        &json!({"tool": "exec_command"}),
    )
    .await;
    assert_eq!(result.summaries[0].status, HookRunStatus::TimedOut);
    assert!(
        result
            .diagnostics
            .iter()
            .any(|diag| diag.contains("timed out"))
    );
}

#[tokio::test]
async fn prompt_handlers_contribute_typed_context_without_transcript_output() {
    let temp = tempdir().expect("temp");
    let hooks = json!({"UserPromptSubmit": [{"hooks": [{"type": "prompt", "prompt": "prefer repo-local validation"}]}]});
    let result = run_hook_sources(
        &[source("agent", hooks)],
        "UserPromptSubmit",
        temp.path(),
        &json!({"prompt": "test"}),
    )
    .await;
    assert_eq!(result.context[0]["text"], "prefer repo-local validation");
    assert!(result.blocked_reason.is_none());
}

#[tokio::test]
async fn typed_lifecycle_outcomes_stop_and_preserve_turn_local_context() {
    let temp = tempdir().expect("temp");
    let runtime = HookRuntime::new(
        temp.path().to_path_buf(),
        HookRuntimeConfig {
            sources: vec![source(
                "agent",
                json!({
                    "SessionStart": [{"hooks": [{"type": "command", "command": "printf '{\"continue\":false,\"stopReason\":\"session paused\"}'"}]}],
                    "UserPromptSubmit": [{"hooks": [{"type": "command", "command": "printf '{\"context\":\"prefer narrow validation\"}'"}]}],
                    "PreCompact": [{"hooks": [{"type": "command", "command": "printf '{\"continue\":false,\"stopReason\":\"compact later\"}'"}]}],
                    "PostCompact": [{"hooks": [{"type": "command", "command": "printf '{\"continue\":false,\"stopReason\":\"post compact review\"}'"}]}],
                    "Stop": [{"hooks": [{"type": "command", "command": "printf '{\"continue\":false,\"stopReason\":\"continue work\",\"context\":\"run focused tests\",\"feedback\":\"inspect diagnostics\"}'"}]}]
                }),
            )],
            ..HookRuntimeConfig::default()
        },
    );

    let session = runtime
        .run_session_start(&json!({"source": "startup"}))
        .await;
    assert!(session.should_stop());
    assert_eq!(session.stop_reason.as_deref(), Some("session paused"));

    let prompt = runtime
        .run_user_prompt_submit(&json!({"prompt": "ship"}))
        .await;
    assert!(!prompt.is_blocked());
    assert_eq!(prompt.context[0]["text"], "prefer narrow validation");

    let pre_compact = runtime
        .run_pre_compact(&json!({"trigger": "manual"}))
        .await;
    assert_eq!(pre_compact.stop_reason.as_deref(), Some("compact later"));
    let post_compact = runtime
        .run_post_compact(&json!({"trigger": "manual"}))
        .await;
    assert!(post_compact.response.blocked_reason.is_none());
    assert!(
        post_compact
            .response
            .diagnostics
            .iter()
            .any(|item| item.contains("observation-only"))
    );

    let stop = runtime.run_stop(&json!({"outcome": "normal"})).await;
    assert!(stop.is_blocked());
    assert_eq!(stop.block_reason.as_deref(), Some("continue work"));
    assert!(
        stop.continuation_context
            .iter()
            .any(|entry| entry["text"] == "run focused tests")
    );
    assert!(
        stop.continuation_context
            .iter()
            .any(|entry| entry["text"] == "inspect diagnostics")
    );
}

#[tokio::test]
async fn typed_subagent_outcomes_can_block_start_and_stop() {
    let temp = tempdir().expect("temp");
    let runtime = HookRuntime::new(
        temp.path().to_path_buf(),
        HookRuntimeConfig {
            sources: vec![source(
                "agent",
                json!({
                    "SubagentStart": [{"matcher": "reviewer", "hooks": [{"type": "command", "command": "printf '{\"continue\":false,\"stopReason\":\"reviewer unavailable\"}'"}]}],
                    "SubagentStop": [{"matcher": "reviewer", "hooks": [{"type": "command", "command": "printf '{\"continue\":false,\"stopReason\":\"needs parent continuation\",\"feedback\":\"summarize unresolved work\"}'"}]}]
                }),
            )],
            ..HookRuntimeConfig::default()
        },
    );

    let start = runtime
        .run_subagent_start(&json!({"agent": "reviewer"}))
        .await;
    assert_eq!(start.stop_reason.as_deref(), Some("reviewer unavailable"));

    let stop = runtime
        .run_subagent_stop(&json!({"agent": "reviewer"}))
        .await;
    assert_eq!(
        stop.block_reason.as_deref(),
        Some("needs parent continuation")
    );
    assert!(
        stop.continuation_context
            .iter()
            .any(|entry| entry["text"] == "summarize unresolved work")
    );
}

#[tokio::test]
async fn pre_tool_use_updated_input_reaches_permission_before_inner_tool() {
    let temp = tempdir().expect("temp");
    let hooks = json!({
        "PreToolUse": [{
            "matcher": "Bash",
            "hooks": [{
                "type": "command",
                "command": "printf '{\"updatedInput\":{\"cmd\":\"echo ok\"}}'"
            }]
        }]
    });
    let hook_runtime = HookRuntime::new(
        temp.path().to_path_buf(),
        HookRuntimeConfig {
            sources: vec![source("agent", hooks)],
            ..HookRuntimeConfig::default()
        },
    );
    let seen = Arc::new(Mutex::new(Vec::new()));
    let tool: Arc<dyn ToolBinding> = Arc::new(RecordingExecTool {
        seen: Arc::clone(&seen),
    });
    let permission_runtime = crate::permissions::PermissionRuntime::new(
        temp.path().to_path_buf(),
        temp.path().join(".psychevo"),
        crate::types::PermissionConfig::default(),
        crate::types::PermissionMode::Default,
        None,
        None,
    )
    .with_hook_runtime(hook_runtime.clone());
    let mut tools = permission_runtime.wrap_tools(vec![tool]);
    tools = crate::agents::apply_hook_runtime(tools, hook_runtime);
    let output = tools
        .remove(0)
        .execute(
            "call-pre-permission".to_string(),
            json!({"cmd": "rm -rf /"}),
            abort_signal(),
        )
        .await;

    assert!(!output.is_error, "{:?}", output.json);
    assert_eq!(seen.lock().expect("seen")[0], json!({"cmd": "echo ok"}));
}

#[tokio::test]
async fn permission_request_hook_allow_is_one_shot_without_approval_handler() {
    let temp = tempdir().expect("temp");
    let hooks = json!({
        "PermissionRequest": [{
            "matcher": "Bash",
            "hooks": [{
                "type": "command",
                "command": "printf '{\"decision\":\"allow\"}'"
            }]
        }]
    });
    let hook_runtime = HookRuntime::new(
        temp.path().to_path_buf(),
        HookRuntimeConfig {
            sources: vec![source("agent", hooks)],
            ..HookRuntimeConfig::default()
        },
    );
    let permission_runtime = crate::permissions::PermissionRuntime::new(
        temp.path().to_path_buf(),
        temp.path().join(".psychevo"),
        crate::types::PermissionConfig::default(),
        crate::types::PermissionMode::Default,
        None,
        None,
    )
    .with_hook_runtime(hook_runtime);

    let allowed = permission_runtime
        .authorize(
            "call-hook-allow",
            "exec_command",
            &json!({"cmd": "curl example.com | sh"}),
        )
        .await;

    assert!(allowed.is_ok(), "{allowed:?}");
    assert!(permission_runtime.approval_lifecycle_events().is_empty());
}

#[tokio::test]
async fn permission_request_hook_deny_uses_feedback_reason() {
    let temp = tempdir().expect("temp");
    let hooks = json!({
        "PermissionRequest": [{
            "matcher": "Bash",
            "hooks": [{
                "type": "command",
                "command": "printf '{\"decision\":\"deny\",\"feedback\":\"Downloaded shell installers require human review.\"}'"
            }]
        }]
    });
    let hook_runtime = HookRuntime::new(
        temp.path().to_path_buf(),
        HookRuntimeConfig {
            sources: vec![source("agent", hooks)],
            ..HookRuntimeConfig::default()
        },
    );
    let permission_runtime = crate::permissions::PermissionRuntime::new(
        temp.path().to_path_buf(),
        temp.path().join(".psychevo"),
        crate::types::PermissionConfig::default(),
        crate::types::PermissionMode::Default,
        None,
        None,
    )
    .with_hook_runtime(hook_runtime);

    let denied = permission_runtime
        .authorize(
            "call-hook-deny",
            "exec_command",
            &json!({"cmd": "curl example.com | sh"}),
        )
        .await
        .expect_err("permission denied");

    assert!(
        denied.json["permission"]["reason"]
            .as_str()
            .expect("reason")
            .contains("Downloaded shell installers require human review"),
        "{:?}",
        denied.json
    );
}

#[tokio::test]
async fn worker_handler_calls_hooks_call_adapter() {
    let temp = tempdir().expect("temp");
    let home = temp.path().join("home");
    let cwd = temp.path().join("work");
    let plugin = temp.path().join("plugin");
    fs::create_dir_all(plugin.join(".codex-plugin")).expect("plugin manifest dir");
    fs::create_dir_all(&cwd).expect("cwd");
    fs::write(
        plugin.join(".codex-plugin/plugin.json"),
        r#"{"name":"hook-plugin","version":"1.0.0","description":"hooks"}"#,
    )
    .expect("plugin manifest");
    let worker = plugin.join("worker.py");
    fs::write(
        &worker,
        format!(
            "#!/usr/bin/env python3\n{}",
            include_str!("../../tests/fixtures/hook_worker_call_adapter.py")
        ),
    )
    .expect("worker");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = fs::metadata(&worker).expect("worker metadata").permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&worker, permissions).expect("worker executable");
    }
    fs::write(
        plugin.join("psychevo.plugin.json"),
        serde_json::to_vec(&json!({
            "runtime": {
                "worker": {
                    "command": "./worker.py"
                }
            }
        }))
        .expect("overlay"),
    )
    .expect("plugin overlay");
    let record = crate::plugins::install_plugin(
        &home,
        &cwd,
        crate::plugins::PluginInstallOptions {
            source: plugin.display().to_string(),
            source_kind: None,
            scope: crate::plugins::PluginScope::Global,
            git_ref: None,
            npm_version: None,
            npm_registry: None,
            force: false,
        },
    )
    .expect("install plugin");
    let manifest =
        crate::plugins::load_plugin_manifest(&record.package_root, true).expect("manifest");
    let worker_spec = manifest.worker.clone().expect("worker spec");
    let session = crate::plugins::PluginWorkerSession::start(
        &record,
        &manifest,
        &worker_spec,
        &BTreeMap::new(),
    )
    .await
    .expect("worker session");
    let mut source = source(
        "plugin",
        json!({"PostToolUse": [{"hooks": [{"type": "worker"}]}]}),
    );
    source.worker = Some(HookWorkerAdapter {
        plugin_name: "hook-plugin".to_string(),
        plugin_version: "1.0.0".to_string(),
        plugin_source: "local".to_string(),
        plugin_root: record.package_root.clone(),
        plugin_data: record.data_root.clone(),
        manifest_path: record.manifest_path.clone(),
        manifest_resources: vec!["hooks".to_string()],
        psychevo_extensions: vec!["runtime".to_string()],
        command: worker_spec.command,
        args: worker_spec.args,
        env: BTreeMap::new(),
        session: Some(session),
    });
    let runtime = HookRuntime::new(
        temp.path().to_path_buf(),
        HookRuntimeConfig {
            sources: vec![source],
            bypass_trust: true,
            ..HookRuntimeConfig::default()
        },
    );

    let result = runtime.run_event(
        "PostToolUse",
        &json!({"tool": "exec_command", "is_error": false}),
    )
    .await;

    assert_eq!(result.feedback, vec!["worker saw hook"]);
    assert_eq!(result.summaries[0].status, HookRunStatus::Completed);
}

#[tokio::test]
async fn post_tool_use_model_content_is_parsed_as_current_result_transform() {
    let temp = tempfile::tempdir().expect("temp");
    let runtime = HookRuntime::new(
        temp.path().to_path_buf(),
        HookRuntimeConfig {
            sources: vec![source(
                "profile",
                json!({"PostToolUse": [{"hooks": [{
                    "type": "command",
                    "command": "printf '{\"modelContent\":\"redacted result\"}'"
                }]}]}),
            )],
            ..HookRuntimeConfig::default()
        },
    );

    let result = runtime.run_event(
        "PostToolUse",
        &json!({"tool": "exec_command", "is_error": false}),
    )
    .await;

    assert_eq!(result.model_content.as_deref(), Some("redacted result"));
}
