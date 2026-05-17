use pretty_assertions::assert_eq;
use psychevo_ai::Outcome;
use psychevo_runtime::{SmokeControl, SmokeOptions, prune_context, run_smoke};
use rusqlite::Connection;
use tempfile::tempdir;

#[tokio::test]
async fn smoke_text_only_persists_session_and_messages() {
    let temp = tempdir().expect("temp");
    let db = temp.path().join("state.db");
    let workdir = temp.path().join("work");
    let result = run_smoke(SmokeOptions {
        db_path: db.clone(),
        workdir: workdir.clone(),
        session: None,
        prompt: None,
        max_context_messages: None,
        control: SmokeControl::None,
        reset: false,
    })
    .await
    .expect("smoke");
    assert_eq!(result.outcome, Outcome::Normal);
    assert_eq!(result.final_answer, "smoke text: smoke");

    let conn = Connection::open(db).expect("db");
    let user_version: i64 = conn
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .expect("user_version");
    assert_eq!(user_version, 8);
    let message_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM messages WHERE session_id = ?1",
            [&result.session_id],
            |row| row.get(0),
        )
        .expect("message count");
    assert_eq!(message_count, 2);
    let seqs = conn
        .prepare("SELECT session_seq FROM messages WHERE session_id = ?1 ORDER BY session_seq")
        .expect("stmt")
        .query_map([&result.session_id], |row| row.get::<_, i64>(0))
        .expect("rows")
        .collect::<rusqlite::Result<Vec<_>>>()
        .expect("seqs");
    assert_eq!(seqs, vec![1, 2]);
}

#[tokio::test]
async fn smoke_tools_touch_only_smoke_files_and_resume_session() {
    let temp = tempdir().expect("temp");
    let db = temp.path().join("state.db");
    let workdir = temp.path().join("work");
    let first = run_smoke(SmokeOptions {
        db_path: db.clone(),
        workdir: workdir.clone(),
        session: None,
        prompt: Some("please read write edit bash".to_string()),
        max_context_messages: Some(4),
        control: SmokeControl::None,
        reset: false,
    })
    .await
    .expect("first smoke");
    assert_eq!(first.outcome, Outcome::Normal);
    assert_eq!(first.tool_failures, 0);
    assert_eq!(
        std::fs::read_to_string(workdir.join(".psychevo-smoke/subject.txt")).expect("subject"),
        "edited psychevo smoke\nsecond line\n"
    );
    assert_eq!(
        std::fs::read_to_string(workdir.join(".psychevo-smoke/generated.txt")).expect("generated"),
        "written by psychevo smoke\n"
    );

    let second = run_smoke(SmokeOptions {
        db_path: db.clone(),
        workdir: workdir.clone(),
        session: Some(first.session_id.clone()),
        prompt: Some("plain followup".to_string()),
        max_context_messages: Some(2),
        control: SmokeControl::None,
        reset: false,
    })
    .await
    .expect("second smoke");
    assert_eq!(second.session_id, first.session_id);
    assert_eq!(second.outcome, Outcome::Normal);

    let conn = Connection::open(db).expect("db");
    let sessions: i64 = conn
        .query_row("SELECT COUNT(*) FROM sessions", [], |row| row.get(0))
        .expect("sessions");
    assert_eq!(sessions, 1);
    let rows: i64 = conn
        .query_row("SELECT COUNT(*) FROM messages", [], |row| row.get(0))
        .expect("messages");
    assert!(rows > 8);
}

#[tokio::test]
async fn reset_uses_manifest_and_preserves_unknown_files() {
    let temp = tempdir().expect("temp");
    let db = temp.path().join("state.db");
    let workdir = temp.path().join("work");
    run_smoke(SmokeOptions {
        db_path: db.clone(),
        workdir: workdir.clone(),
        session: None,
        prompt: Some("write".to_string()),
        max_context_messages: None,
        control: SmokeControl::None,
        reset: false,
    })
    .await
    .expect("first");
    let unknown = workdir.join(".psychevo-smoke/unknown.txt");
    std::fs::write(&unknown, "keep").expect("unknown");
    run_smoke(SmokeOptions {
        db_path: db,
        workdir: workdir.clone(),
        session: None,
        prompt: None,
        max_context_messages: None,
        control: SmokeControl::None,
        reset: true,
    })
    .await
    .expect("reset run");
    assert!(unknown.exists());
    assert!(!workdir.join(".psychevo-smoke/generated.txt").exists());
}

#[test]
fn context_pruning_extends_to_preserve_tool_pairing() {
    let assistant = psychevo_agent_core::Message::Assistant {
        content: vec![psychevo_agent_core::AssistantBlock::ToolCall(
            psychevo_agent_core::ToolCallBlock {
                id: "call-1".to_string(),
                name: "read".to_string(),
                arguments: serde_json::json!({}),
                arguments_json: "{}".to_string(),
                arguments_error: None,
                content_index: 0,
                call_index: 0,
            },
        )],
        timestamp_ms: 1,
        finish_reason: None,
        outcome: Outcome::Normal,
        model: None,
        provider: None,
    };
    let tool_result = psychevo_agent_core::Message::ToolResult {
        tool_call_id: "call-1".to_string(),
        tool_name: "read".to_string(),
        content: "{}".to_string(),
        is_error: false,
        timestamp_ms: 2,
    };
    let pruned = prune_context(
        vec![
            psychevo_agent_core::user_text_message("old"),
            assistant.clone(),
            tool_result.clone(),
        ],
        Some(1),
    );
    assert_eq!(pruned, vec![assistant, tool_result]);
}

#[tokio::test]
async fn smoke_control_modes_return_expected_outcomes() {
    let temp = tempdir().expect("temp");
    let stop = run_smoke(SmokeOptions {
        db_path: temp.path().join("stop.db"),
        workdir: temp.path().join("stop-work"),
        session: None,
        prompt: Some("read write".to_string()),
        max_context_messages: None,
        control: SmokeControl::StopAfterTurn,
        reset: false,
    })
    .await
    .expect("stop");
    assert_eq!(stop.outcome, Outcome::Stopped);

    let abort = run_smoke(SmokeOptions {
        db_path: temp.path().join("abort.db"),
        workdir: temp.path().join("abort-work"),
        session: None,
        prompt: Some("read".to_string()),
        max_context_messages: None,
        control: SmokeControl::AbortOnAgentStart,
        reset: false,
    })
    .await
    .expect("abort");
    assert_eq!(abort.outcome, Outcome::Aborted);
}
