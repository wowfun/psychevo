use std::env;
use std::path::PathBuf;

use psychevo_ai::Outcome;
use psychevo_runtime::{
    RunOptions, StateRuntime, fetch_and_cache_model_catalog, model_catalog_provider,
    provider_models_cache_path_for_home, read_cached_model_catalog, run_live,
};
use rusqlite::Connection;
use tempfile::tempdir;

const PRIMARY_XIAOMI_FAMILY_PROVIDER: &str = "xiaomi-token-plan";
const PRIMARY_XIAOMI_FAMILY_MODEL: &str = "xiaomi-token-plan/mimo-v2.5-pro";

fn live_model(provider: &str) -> &'static str {
    match provider {
        PRIMARY_XIAOMI_FAMILY_PROVIDER => PRIMARY_XIAOMI_FAMILY_MODEL,
        other => panic!("missing live model for provider: {other}"),
    }
}

pub(crate) fn live_config_available() -> bool {
    env::var_os("PSYCHEVO_CONFIG").is_some() || env::var_os("PSYCHEVO_HOME").is_some()
}

pub(crate) async fn run_live_read_tool(provider: &str) {
    if !live_config_available() {
        eprintln!("skipping live {provider}: PSYCHEVO_CONFIG or PSYCHEVO_HOME is not set");
        return;
    }
    let temp = tempdir().expect("temp");
    let cwd = temp.path().join("work");
    std::fs::create_dir_all(&cwd).expect("cwd");
    std::fs::write(cwd.join("fixture.txt"), format!("fixture for {provider}\n")).expect("fixture");
    let db = temp.path().join("state.db");
    let mut inherited_env = env::vars().collect::<std::collections::BTreeMap<_, _>>();
    inherited_env.insert(
        "PSYCHEVO_INFERENCE_PROVIDER".to_string(),
        provider.to_string(),
    );
    let result = run_live(RunOptions {
        state: StateRuntime::open(&db).expect("state runtime"),
        cwd: cwd.clone(),
        snapshot_root: Some(temp.path().join("snapshots")),
        session: None,
        continue_latest: false,
        prompt: "Use the read tool to read fixture.txt, then answer with one short sentence."
            .to_string(),
        image_inputs: Vec::new(),
        extract_prompt_image_sources: true,
        prompt_display: None,
        max_context_messages: None,
        config_path: None,
        project_context_override: None,
        sandbox_override: None,
        model: Some(live_model(provider).to_string()),
        reasoning_effort: None,
        runtime_ref: None,
        runtime_session_id: None,
        runtime_options: std::collections::BTreeMap::new(),
        runtime_tools: Vec::new(),
        include_reasoning: true,
        mode: psychevo_runtime::RunMode::Default,
        permission_mode: None,
        approval_mode: None,
        approval_handler: None,
        clarify_enabled: false,
        inherited_env: Some(inherited_env),
        agent: None,
        external_agent_delegate: None,
        no_agents: false,
        no_skills: false,
        selected_capability_roots: Vec::new(),
        skill_inputs: Vec::new(),
        mcp_servers: Vec::new(),
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

fn live_provider_options_with_temp_home(
    temp: &tempfile::TempDir,
) -> (RunOptions, PathBuf, PathBuf) {
    let home = temp.path().join("home");
    let cwd = temp.path().join("work");
    std::fs::create_dir_all(&home).expect("home");
    std::fs::create_dir_all(&cwd).expect("cwd");
    let mut inherited_env = env::vars().collect::<std::collections::BTreeMap<_, _>>();
    let explicit_config = inherited_env.get("PSYCHEVO_CONFIG").map(PathBuf::from);
    if explicit_config.is_none() {
        if let Some(real_home) = inherited_env
            .get("PSYCHEVO_HOME")
            .map(PathBuf::from)
            .or_else(|| {
                inherited_env
                    .get("HOME")
                    .map(|home| PathBuf::from(home).join(".psychevo"))
            })
        {
            let real_config = real_home.join("config.toml");
            if real_config.exists() {
                std::fs::copy(&real_config, home.join("config.toml")).expect("copy config");
                let real_env = real_home.join(".env");
                if real_env.exists() {
                    std::fs::copy(real_env, home.join(".env")).expect("copy env");
                }
            } else {
                std::fs::write(home.join("config.toml"), "# live provider validation\n")
                    .expect("empty config");
            }
        } else {
            std::fs::write(home.join("config.toml"), "# live provider validation\n")
                .expect("empty config");
        }
    }
    inherited_env.insert(
        "PSYCHEVO_HOME".to_string(),
        home.to_string_lossy().to_string(),
    );
    (
        RunOptions {
            state: StateRuntime::open(temp.path().join("state.db")).expect("state runtime"),
            cwd: cwd.clone(),
            snapshot_root: Some(temp.path().join("snapshots")),
            session: None,
            continue_latest: false,
            prompt: String::new(),
            image_inputs: Vec::new(),
            extract_prompt_image_sources: true,
            prompt_display: None,
            max_context_messages: None,
            config_path: explicit_config,
            project_context_override: None,
            sandbox_override: None,
            model: None,
            reasoning_effort: None,
            runtime_ref: None,
            runtime_session_id: None,
            runtime_options: std::collections::BTreeMap::new(),
            runtime_tools: Vec::new(),
            include_reasoning: false,
            mode: psychevo_runtime::RunMode::Default,
            permission_mode: None,
            approval_mode: None,
            approval_handler: None,
            clarify_enabled: false,
            inherited_env: Some(inherited_env),
            agent: None,
            external_agent_delegate: None,
            no_agents: false,
            no_skills: false,
            selected_capability_roots: Vec::new(),
            skill_inputs: Vec::new(),
            mcp_servers: Vec::new(),
        },
        home,
        cwd,
    )
}

fn skip_if_xiaomi_catalog_unavailable(options: &RunOptions) -> bool {
    match model_catalog_provider(options, PRIMARY_XIAOMI_FAMILY_PROVIDER) {
        Ok(Some(provider)) if provider.fetchable() => false,
        Ok(Some(provider)) => {
            eprintln!(
                "skipping live xiaomi-token-plan model fetch: {}",
                provider
                    .unavailable_reason
                    .or(provider.missing_credentials)
                    .unwrap_or_else(|| "provider is not fetchable".to_string())
            );
            true
        }
        Ok(None) => {
            eprintln!("skipping live xiaomi-token-plan model fetch: provider is not configured");
            true
        }
        Err(err) => {
            eprintln!("skipping live xiaomi-token-plan model fetch: {err}");
            true
        }
    }
}

#[tokio::test]
#[ignore = "live provider opt-in"]
pub(crate) async fn live_xiaomi_token_plan_read_tool() {
    run_live_read_tool(PRIMARY_XIAOMI_FAMILY_PROVIDER).await;
}

#[tokio::test]
#[ignore = "live provider opt-in"]
pub(crate) async fn live_xiaomi_token_plan_model_fetch() {
    let temp = tempdir().expect("temp");
    let (options, home, _cwd) = live_provider_options_with_temp_home(&temp);
    if skip_if_xiaomi_catalog_unavailable(&options) {
        return;
    }
    let provider = model_catalog_provider(&options, PRIMARY_XIAOMI_FAMILY_PROVIDER)
        .expect("provider lookup")
        .expect("provider");
    let models = fetch_and_cache_model_catalog(&home, &provider)
        .await
        .expect("live model catalog fetch");
    assert!(!models.is_empty(), "expected live /models to return models");
    let cached = read_cached_model_catalog(&home, &provider).expect("cached live models");
    assert_eq!(cached.len(), models.len());
    let cache_text =
        std::fs::read_to_string(provider_models_cache_path_for_home(&home)).expect("cache text");
    let visible_api_key = provider.api_key_env.as_deref().and_then(|key| {
        env::var(key).ok().or_else(|| {
            options
                .inherited_env
                .as_ref()
                .and_then(|env_map| env_map.get(key).cloned())
        })
    });
    if let Some(api_key) = visible_api_key.as_deref() {
        assert!(
            !cache_text.contains(api_key),
            "provider model cache must not contain the API key"
        );
    }
    assert!(cache_text.contains(&models[0].id));
}
