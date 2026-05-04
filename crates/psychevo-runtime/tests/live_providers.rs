use std::env;

use psychevo_ai::Outcome;
use psychevo_runtime::{RunOptions, run_live};
use rusqlite::Connection;
use tempfile::tempdir;

fn live_config_available() -> bool {
    env::var_os("PSYCHEVO_CONFIG").is_some() || env::var_os("PSYCHEVO_HOME").is_some()
}

async fn run_live_read_tool(provider: &str) {
    if !live_config_available() {
        eprintln!("skipping live {provider}: PSYCHEVO_CONFIG or PSYCHEVO_HOME is not set");
        return;
    }
    let temp = tempdir().expect("temp");
    let workdir = temp.path().join("work");
    std::fs::create_dir_all(&workdir).expect("workdir");
    std::fs::write(
        workdir.join("fixture.txt"),
        format!("fixture for {provider}\n"),
    )
    .expect("fixture");
    let db = temp.path().join("state.db");
    let mut inherited_env = env::vars().collect::<std::collections::BTreeMap<_, _>>();
    inherited_env.insert(
        "PSYCHEVO_INFERENCE_PROVIDER".to_string(),
        provider.to_string(),
    );
    let result = run_live(RunOptions {
        db_path: db.clone(),
        workdir: workdir.clone(),
        session: None,
        continue_latest: false,
        prompt: "Use the read tool to read fixture.txt, then answer with one short sentence."
            .to_string(),
        max_context_messages: None,
        config_path: None,
        model: None,
        reasoning_effort: None,
        include_reasoning: true,
        inherited_env: Some(inherited_env),
    })
    .await
    .expect("live run");
    assert_eq!(result.outcome, Outcome::Normal);

    let conn = Connection::open(db).expect("db");
    let read_results: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM messages WHERE role = 'tool_result' AND tool_name = 'read' AND outcome = 'normal'",
            [],
            |row| row.get(0),
        )
        .expect("read results");
    assert!(
        read_results >= 1,
        "expected {provider} to complete at least one successful read tool call"
    );
}

#[tokio::test]
#[ignore = "live provider opt-in"]
async fn live_deepseek_read_tool() {
    run_live_read_tool("deepseek").await;
}

#[tokio::test]
#[ignore = "live provider opt-in"]
async fn live_xiaomi_read_tool() {
    run_live_read_tool("xiaomi").await;
}
