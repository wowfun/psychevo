use super::support::*;
#[allow(unused_imports)]
use super::*;
use pretty_assertions::assert_eq;

#[test]
pub(crate) fn acp_agent_executes_stdio_session() {
    let temp = tempfile::tempdir().expect("temp");
    let root = temp.path().join("acp-coding");
    fs::create_dir_all(&root).expect("project root");
    write_local_task(&root, "rust-swe-add", "swe-style");
    fs::write(
        root.join("benchmark.toml"),
        r#"schema_version = 5
id = "acp-coding"
name = "acp-coding"

[[sources.peval_agent]]
id = "local"
path = "tasks"

[[sources.peval_agent.sets]]
id = "rust-swe"
include = ["rust-swe-add"]
"#,
    )
    .expect("benchmark");
    fs::write(
        root.join("tasks").join("rust-swe-add").join("mock-acp.py"),
        r#"import json
import pathlib
import sys

cwd = None

def send(value):
    print(json.dumps(value), flush=True)

for line in sys.stdin:
    if not line.strip():
        continue
    message = json.loads(line)
    method = message.get("method")
    if method == "initialize":
        send({"jsonrpc": "2.0", "id": message["id"], "result": {"protocolVersion": 1, "agentCapabilities": {}, "authMethods": []}})
    elif method == "session/new":
        cwd = pathlib.Path(message["params"]["cwd"])
        send({"jsonrpc": "2.0", "id": message["id"], "result": {"sessionId": "mock-session"}})
    elif method == "session/prompt":
        send({"jsonrpc": "2.0", "method": "session/update", "params": {"sessionId": "mock-session", "update": {"sessionUpdate": "agent_thought_chunk", "content": {"type": "text", "text": "inspect status"}}}})
        send({"jsonrpc": "2.0", "method": "session/update", "params": {"sessionId": "mock-session", "update": {"sessionUpdate": "tool_call", "toolCallId": "call-1", "kind": "edit", "title": "Tool: edit", "status": "in_progress", "rawInput": {"path": "status.txt"}}}})
        (cwd / "status.txt").write_text("fixed")
        send({"jsonrpc": "2.0", "method": "session/update", "params": {"sessionId": "mock-session", "update": {"sessionUpdate": "tool_call_update", "toolCallId": "call-1", "title": "Tool: edit", "status": "completed", "rawOutput": {"diff": "diff --git a/status.txt b/status.txt\n--- a/status.txt\n+++ b/status.txt\n@@ -1 +1 @@\n-pending\n+fixed\n", "success": True}}}})
        send({"jsonrpc": "2.0", "method": "session/update", "params": {"sessionId": "mock-session", "update": {"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": "done"}}}})
        send({"jsonrpc": "2.0", "method": "session/update", "params": {"sessionId": "mock-session", "update": {"sessionUpdate": "usage_update", "_meta": {"psychevo": {"model": "runtime-model"}}}}})
        send({"jsonrpc": "2.0", "id": message["id"], "result": {"stopReason": "end_turn"}})
        break
    else:
        send({"jsonrpc": "2.0", "id": message["id"], "result": {}})
"#,
    )
    .expect("mock acp");
    fs::write(
        root.join("eval.toml"),
        r#"schema_version = 5
id = "acp-coding-eval"
name = "acp coding eval"

[benchmark]
path = "benchmark.toml"

[select]
agents = ["mock-acp"]
sets = ["local/rust-swe"]

[[agents]]
id = "mock-acp"
kind = "acp"

[agents.acp]
command = "python3"
args = ["mock-acp.py"]
timeout_seconds = 10
"#,
    )
    .expect("eval");

    let run = run_evaluation(RunRequest {
        config: Some(root.join("eval.toml")),
        benchmark: None,
        task_set: Some("local/rust-swe".to_string()),
        task: None,
        agent: Some("mock-acp".to_string()),
        overwrite: false,
        store_root: Some(init_workspace(temp.path().join("evals"))),
        output_root: None,
        include_artifacts: Vec::new(),
    })
    .expect("acp run");
    assert_eq!(run.status, RunStatus::Passed);
    assert_eq!(run.passed_cells, 1);
    assert!(run.cells[0].cell_root.join("prompt.md").is_file());

    let view = build_view(ViewRequest {
        config: Some(root.join("eval.toml")),
        benchmark: None,
        report: None,
        store_root: Some(init_workspace(temp.path().join("view-evals"))),
        paths: vec![run.cells[0].cell_root.clone()],
        task_set: None,
        agent: None,
        task: None,
        status: None,
        group_by: Vec::new(),
        include: vec![ViewInclude::Core, ViewInclude::Comparison],
        notes: Vec::new(),
    })
    .expect("acp view");
    let atif = &view.trajectory[0];
    let meta = &view.trajectory_meta[0];
    let comparison = view.comparison.as_ref().expect("comparison");
    assert_eq!(
        comparison.matrix.cells[0].model_name.as_deref(),
        Some("runtime-model")
    );
    assert_eq!(
        comparison.leaderboard.entries[0].model_name.as_deref(),
        Some("runtime-model")
    );
    assert_eq!(atif.agent.model_name.as_deref(), Some("runtime-model"));
    assert_eq!(atif.schema_version, "ATIF-v1.7");
    assert_eq!(atif.trajectory_id.as_deref(), Some(meta.trial_key.as_str()));
    assert_eq!(atif.steps.len(), 3);
    assert_eq!(
        atif.final_metrics
            .as_ref()
            .expect("final metrics")
            .total_steps,
        3
    );
    assert_eq!(atif.steps[0].source, "user");
    assert!(
        atif.steps
            .iter()
            .all(|step| step.source.as_str() != "system"),
        "system steps should only appear when the adapter exposes them"
    );
    let reasoning_step = atif
        .steps
        .iter()
        .find(|step| step.reasoning_content.as_deref() == Some("inspect status"))
        .expect("reasoning step");
    assert_eq!(reasoning_step.source, "agent");
    assert_eq!(reasoning_step.tool_calls[0].tool_call_id, "call-1");
    assert_eq!(
        reasoning_step
            .observation
            .as_ref()
            .expect("observation")
            .results[0]
            .source_call_id
            .as_deref(),
        Some("call-1")
    );
    let tool_step = atif
        .steps
        .iter()
        .find(|step| {
            step.tool_calls
                .iter()
                .any(|tool| tool.function_name == "edit")
        })
        .expect("tool call step");
    let observation_step = atif
        .steps
        .iter()
        .find(|step| step.observation.is_some())
        .expect("observation step");
    assert_eq!(
        tool_step.step_id, observation_step.step_id,
        "ACP tool call and observation should stay under the same agent step"
    );
    assert_eq!(tool_step.tool_calls[0].tool_call_id, "call-1");
    assert_eq!(
        observation_step
            .observation
            .as_ref()
            .expect("observation")
            .results[0]
            .source_call_id
            .as_deref(),
        Some("call-1")
    );
    assert!(
        observation_step
            .observation
            .as_ref()
            .expect("observation")
            .results[0]
            .content
            .as_ref()
            .expect("content")
            .to_string()
            .contains("status.txt")
    );
    assert!(
        atif.steps
            .iter()
            .any(|step| step.message.as_str() == Some("done"))
    );

    assert_eq!(atif.session_id.as_deref(), Some("mock-session"));
    assert_eq!(meta.steps.len(), atif.steps.len());
    assert!(
        atif.steps
            .iter()
            .any(|step| step.reasoning_content.as_deref() == Some("inspect status"))
    );
    assert!(atif.steps.iter().any(|step| {
        step.tool_calls
            .iter()
            .any(|tool| tool.function_name == "edit")
    }));
    assert!(atif.steps.iter().any(|step| {
        step.observation.as_ref().is_some_and(|observation| {
            observation
                .results
                .iter()
                .any(|result| result.source_call_id.as_deref() == Some("call-1"))
        })
    }));
    assert!(atif.steps.iter().any(|step| {
        step.reasoning_content.as_deref() == Some("inspect status")
            && step
                .tool_calls
                .iter()
                .any(|tool| tool.tool_call_id == "call-1")
            && step.observation.as_ref().is_some_and(|observation| {
                observation
                    .results
                    .iter()
                    .any(|result| result.source_call_id.as_deref() == Some("call-1"))
            })
    }));
    assert!(
        meta.steps
            .iter()
            .filter(|step| step.timestamp_ms.is_some())
            .skip(1)
            .any(|step| step.elapsed_ms.is_some() && step.duration_ms.is_some())
    );
}
