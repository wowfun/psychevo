use std::collections::BTreeMap;
use std::env;
use std::path::PathBuf;

use psychevo::RunMode;
use psychevo::application::{
    Application, Configuration, ConfigurationQuery, Message, StartThreadRequest, TurnOutcome,
    TurnRequest,
};
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
    let mut environment = live_environment(&temp);
    let cwd = environment.cwd.clone();
    std::fs::write(cwd.join("fixture.txt"), format!("fixture for {provider}\n")).expect("fixture");
    let db = temp.path().join("state.db");
    environment.inherited_env.insert(
        "PSYCHEVO_INFERENCE_PROVIDER".to_string(),
        provider.to_string(),
    );
    let application = build_live_application(&environment, db).await;
    let thread = application
        .client()
        .start_thread(StartThreadRequest::new(&cwd))
        .await
        .expect("live thread");
    let result = thread
        .start_turn(
            TurnRequest::new(
                "Use the read tool to read fixture.txt, then answer with one short sentence.",
            )
            .with_identity("live-test", None)
            .with_model(Some(live_model(provider).to_string()), None)
            .with_reasoning_output(true)
            .with_execution_policy(RunMode::Default, None, environment.config_path.clone())
            .with_environment(Some(environment.inherited_env.clone()), None, None)
            .with_framework_context(
                Some(temp.path().join("snapshots")),
                None,
                Vec::new(),
                None,
            ),
        )
        .await
        .expect("accepted live turn")
        .wait()
        .await
        .expect("live turn");
    assert_eq!(result.outcome, TurnOutcome::Completed);

    let history = thread
        .history()
        .latest(Some(200))
        .await
        .expect("live history")
        .items;
    let read_results = history
        .iter()
        .filter(|item| {
            matches!(
                &item.message,
                Message::ToolResult {
                    tool_name,
                    is_error: false,
                    ..
                } if tool_name == "read"
            )
        })
        .count();
    assert!(
        read_results >= 1,
        "expected {provider} to complete at least one successful read tool call"
    );
    application.shutdown().await.expect("shutdown");
}

struct LiveEnvironment {
    home: PathBuf,
    cwd: PathBuf,
    config_path: Option<PathBuf>,
    inherited_env: BTreeMap<String, String>,
}

fn live_environment(temp: &tempfile::TempDir) -> LiveEnvironment {
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
    LiveEnvironment {
        home,
        cwd,
        config_path: explicit_config,
        inherited_env,
    }
}

async fn build_live_application(environment: &LiveEnvironment, database: PathBuf) -> Application {
    let mut builder = Application::builder()
        .home(&environment.home)
        .database_path(database);
    if let Some(config_path) = &environment.config_path {
        builder = builder.config_path(config_path.clone());
    }
    builder.build().await.expect("live application")
}

fn skip_if_xiaomi_catalog_unavailable(configuration: &Configuration) -> bool {
    match configuration.model_catalog_provider(PRIMARY_XIAOMI_FAMILY_PROVIDER) {
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
    let environment = live_environment(&temp);
    let application = build_live_application(&environment, temp.path().join("state.db")).await;
    let mut query = ConfigurationQuery::new(&environment.cwd);
    query.inherited_env = Some(environment.inherited_env.clone());
    let configuration = application
        .client()
        .configuration(query)
        .expect("configuration");
    if skip_if_xiaomi_catalog_unavailable(&configuration) {
        application.shutdown().await.expect("shutdown");
        return;
    }
    let provider = configuration
        .model_catalog_provider(PRIMARY_XIAOMI_FAMILY_PROVIDER)
        .expect("provider lookup")
        .expect("provider");
    let models = configuration
        .fetch_and_cache_model_catalog(&provider)
        .await
        .expect("live model catalog fetch");
    assert!(!models.is_empty(), "expected live /models to return models");
    let cached = configuration
        .cached_model_catalog(&provider)
        .expect("cached live models");
    assert_eq!(cached.len(), models.len());
    let cache_text =
        std::fs::read_to_string(configuration.model_catalog_cache_path()).expect("cache text");
    let visible_api_key = provider.api_key_env.as_deref().and_then(|key| {
        env::var(key)
            .ok()
            .or_else(|| environment.inherited_env.get(key).cloned())
    });
    if let Some(api_key) = visible_api_key.as_deref() {
        assert!(
            !cache_text.contains(api_key),
            "provider model cache must not contain the API key"
        );
    }
    assert!(cache_text.contains(&models[0].id));
    application.shutdown().await.expect("shutdown");
}
