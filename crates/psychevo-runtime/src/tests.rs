use super::*;
use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use crate::config::{ResolvedRunProvider, load_run_config, resolve_run_provider};
use crate::events::{PersistenceSink, project_agent_event, project_run_stream_event};
use crate::paths::canonical_workdir;
use crate::run::{SESSION_TITLE_MAX_CHARS, ensure_new_tui_session_title};
use crate::snapshot::SnapshotStore;
use crate::tools::{WorkdirTool, list_tool_impl, search_tool_impl};
use pretty_assertions::assert_eq;
use psychevo_agent_core::{AgentEvent, AssistantBlock, EventSink, Message};
use psychevo_ai::{FakeProvider, Outcome, RawStreamEvent};
use rusqlite::Connection;
use serde_json::{Value, json};
use tempfile::tempdir;

fn base_options(temp: &tempfile::TempDir) -> RunOptions {
    RunOptions {
        db_path: temp.path().join("state.db"),
        workdir: temp.path().join("work"),
        snapshot_root: Some(temp.path().join("snapshots")),
        session: None,
        continue_latest: false,
        prompt: "hello".to_string(),
        max_context_messages: None,
        config_path: None,
        model: None,
        reasoning_effort: None,
        include_reasoning: false,
        mode: RunMode::Build,
        inherited_env: Some(BTreeMap::from([(
            "HOME".to_string(),
            temp.path().to_string_lossy().to_string(),
        )])),
    }
}

fn home_dir(temp: &tempfile::TempDir) -> PathBuf {
    temp.path().join(".psychevo")
}

#[test]
fn run_mode_tool_names_enforce_plan_read_only_surface() {
    assert_eq!(RunMode::Build.as_str(), "default");
    assert_eq!(RunMode::parse("default"), Some(RunMode::Build));
    assert_eq!(RunMode::parse("build"), None);
    assert_eq!(
        tool_names_for_mode(RunMode::Plan),
        vec!["read", "list", "search"]
    );
    assert_eq!(
        tool_names_for_mode(RunMode::Build),
        vec!["read", "write", "edit", "bash"]
    );
}

#[test]
fn plan_list_and_search_tools_are_read_only_and_bounded() {
    let temp = tempdir().expect("temp");
    let workdir = temp.path().join("work");
    fs::create_dir_all(workdir.join("src")).expect("dirs");
    fs::write(workdir.join("src/lib.rs"), "alpha\nneedle one\n").expect("file");
    fs::write(workdir.join("README.md"), "needle two\n").expect("file");
    let tool = WorkdirTool::new(workdir.canonicalize().expect("canonical"));

    let listed = list_tool_impl(tool.clone(), json!({"path":".","limit":1})).expect("list");
    assert_eq!(listed["entries"].as_array().expect("entries").len(), 1);
    assert_eq!(listed["truncated"], true);

    let searched =
        search_tool_impl(tool, json!({"query":"needle","path":".","limit":10})).expect("search");
    let matches = searched["matches"].as_array().expect("matches");
    assert_eq!(matches.len(), 2);
    assert!(
        matches
            .iter()
            .all(|entry| entry["line"].as_str().unwrap().contains("needle"))
    );
}

#[test]
fn default_global_config_uses_home_psychevo_config_jsonc() {
    let temp = tempdir().expect("temp");
    let options = base_options(&temp);
    let global_dir = home_dir(&temp);
    fs::create_dir_all(&global_dir).expect("global dir");
    fs::write(
        global_dir.join("config.jsonc"),
        r#"
            {
              "model": "deepseek/deepseek-chat",
              "provider": {
                "deepseek": {
                  "options": {
                    "base_url": "http://home.example/v1",
                    "api_key_env": "DEEPSEEK_API_KEY"
                  },
                  "models": { "deepseek-chat": {} }
                }
              }
            }
            "#,
    )
    .expect("global config");
    fs::write(global_dir.join(".env"), "DEEPSEEK_API_KEY=home-key\n").expect("global env");

    let workdir = canonical_workdir(&options.workdir).expect("workdir");
    let loaded = load_run_config(&options, &workdir).expect("config");
    let resolved = resolve_run_provider(&options, &loaded).expect("provider");
    assert_eq!(resolved.provider, "deepseek");
    assert_eq!(resolved.model, "deepseek-chat");
    assert_eq!(resolved.base_url, "http://home.example/v1");
    assert_eq!(resolved.api_key, "home-key");
}

#[test]
fn psychevo_home_overrides_default_home() {
    let temp = tempdir().expect("temp");
    let custom_home = temp.path().join("custom-home");
    let mut options = base_options(&temp);
    options.inherited_env = Some(BTreeMap::from([
        (
            "HOME".to_string(),
            temp.path().join("ignored").to_string_lossy().to_string(),
        ),
        (
            "PSYCHEVO_HOME".to_string(),
            custom_home.to_string_lossy().to_string(),
        ),
    ]));
    fs::create_dir_all(&custom_home).expect("home");
    fs::write(
        custom_home.join("config.jsonc"),
        r#"
            {
              "model": "deepseek/deepseek-chat",
              "provider": {
                "deepseek": {
                  "options": {
                    "base_url": "http://custom-home.example/v1",
                    "api_key_env": "DEEPSEEK_API_KEY"
                  },
                  "models": { "deepseek-chat": {} }
                }
              }
            }
            "#,
    )
    .expect("config");
    fs::write(custom_home.join(".env"), "DEEPSEEK_API_KEY=custom-key\n").expect("env");

    let workdir = canonical_workdir(&options.workdir).expect("workdir");
    let loaded = load_run_config(&options, &workdir).expect("config");
    let resolved = resolve_run_provider(&options, &loaded).expect("provider");
    assert_eq!(resolved.base_url, "http://custom-home.example/v1");
    assert_eq!(resolved.api_key, "custom-key");
}

#[test]
fn config_merge_dotenv_precedence_and_provider_qualified_model() {
    let temp = tempdir().expect("temp");
    let options = base_options(&temp);
    let config_dir = home_dir(&temp);
    let project_dir = options.workdir.join(".psychevo");
    fs::create_dir_all(&config_dir).expect("config dir");
    fs::create_dir_all(&project_dir).expect("project dir");
    fs::write(
        config_dir.join("config.jsonc"),
        r#"
            {
              // global default
              "model": "deepseek/deepseek-chat",
              "provider": {
                "deepseek": {
                  "options": {
                    "base_url": "http://global.example/v1",
                    "api_key_env": "DEEPSEEK_API_KEY"
                  },
                  "models": {
                    "deepseek-chat": { "reasoning_effort": "low" }
                  }
                }
              }
            }
            "#,
    )
    .expect("global config");
    fs::write(config_dir.join(".env"), "DEEPSEEK_API_KEY=global-key\n").expect("global env");
    fs::write(
        project_dir.join("config.jsonc"),
        r#"
            {
              "provider": {
                "deepseek": {
                  "options": { "base_url": "http://project.example/v1" },
                  "models": {
                    "deepseek-chat": { "reasoning_effort": "high" }
                  }
                }
              }
            }
            "#,
    )
    .expect("project config");
    fs::write(project_dir.join(".env"), "DEEPSEEK_API_KEY='project-key'\n").expect("project env");

    let workdir = canonical_workdir(&options.workdir).expect("workdir");
    let loaded = load_run_config(&options, &workdir).expect("config");
    let resolved = resolve_run_provider(&options, &loaded).expect("provider");
    assert_eq!(resolved.provider, "deepseek");
    assert_eq!(resolved.model, "deepseek-chat");
    assert_eq!(resolved.base_url, "http://project.example/v1");
    assert_eq!(resolved.api_key, "project-key");
    assert_eq!(resolved.reasoning_effort.as_deref(), Some("high"));
}

#[test]
fn model_object_provider_and_reasoning_effort_are_resolved() {
    let temp = tempdir().expect("temp");
    let options = base_options(&temp);
    let config_dir = home_dir(&temp);
    fs::create_dir_all(&config_dir).expect("config dir");
    fs::write(
        config_dir.join("config.jsonc"),
        r#"
            {
              "model": {
                "provider": "mimo",
                "id": "mimo-v2.5",
                "reasoning_effort": "medium"
              },
              "provider": {
                "xiaomi": {
                  "models": { "mimo-v2.5": {} }
                }
              }
            }
            "#,
    )
    .expect("config");
    fs::write(config_dir.join(".env"), "XIAOMI_API_KEY=xiaomi-key\n").expect("env");

    let workdir = canonical_workdir(&options.workdir).expect("workdir");
    let loaded = load_run_config(&options, &workdir).expect("config");
    let resolved = resolve_run_provider(&options, &loaded).expect("provider");
    assert_eq!(resolved.provider, "xiaomi");
    assert_eq!(resolved.model, "mimo-v2.5");
    assert_eq!(resolved.api_key_env.as_deref(), Some("XIAOMI_API_KEY"));
    assert_eq!(resolved.reasoning_effort.as_deref(), Some("medium"));
}

#[test]
fn selected_configured_model_reports_effective_reasoning_without_credentials() {
    let temp = tempdir().expect("temp");
    let mut options = base_options(&temp);
    let config_dir = home_dir(&temp);
    fs::create_dir_all(&config_dir).expect("config dir");
    fs::write(
        config_dir.join("config.jsonc"),
        r#"
            {
              "model": "custom/local",
              "provider": {
                "custom": {
                  "options": {
                    "base_url": "http://127.0.0.1:1234/v1",
                    "api_key_env": "CUSTOM_KEY"
                  },
                  "models": {
                    "local": { "reasoning_effort": "high" },
                    "other": { "reasoning_effort": "low" }
                  }
                }
              }
            }
            "#,
    )
    .expect("config");

    let selected = selected_configured_model(&options)
        .expect("selected")
        .expect("model");
    assert_eq!(selected.provider, "custom");
    assert_eq!(selected.model, "local");
    assert_eq!(selected.reasoning_effort.as_deref(), Some("high"));

    options.model = Some("custom/other".to_string());
    let selected = selected_configured_model(&options)
        .expect("selected")
        .expect("model");
    assert_eq!(selected.model, "other");
    assert_eq!(selected.reasoning_effort.as_deref(), Some("low"));

    options.reasoning_effort = Some("xhigh".to_string());
    let selected = selected_configured_model(&options)
        .expect("selected")
        .expect("model");
    assert_eq!(selected.model, "other");
    assert_eq!(selected.reasoning_effort.as_deref(), Some("xhigh"));
}

#[test]
fn raw_api_keys_are_rejected() {
    let temp = tempdir().expect("temp");
    let options = base_options(&temp);
    let config_dir = home_dir(&temp);
    fs::create_dir_all(&config_dir).expect("config dir");
    fs::write(
        config_dir.join("config.jsonc"),
        r#"
            {
              "provider": {
                "custom": {
                  "options": {
                    "base_url": "http://127.0.0.1:1234/v1",
                    "api_key": "secret"
                  },
                  "models": { "local": {} }
                }
              }
            }
            "#,
    )
    .expect("config");

    let workdir = canonical_workdir(&options.workdir).expect("workdir");
    let err = load_run_config(&options, &workdir).expect_err("raw key");
    assert!(err.to_string().contains("raw API keys"));
}

#[test]
fn unique_model_default_and_multiple_model_rejection() {
    let temp = tempdir().expect("temp");
    let options = base_options(&temp);
    let config_dir = home_dir(&temp);
    fs::create_dir_all(&config_dir).expect("config dir");
    fs::write(
        config_dir.join("config.jsonc"),
        r#"
            {
              "provider": {
                "xiaomi": {
                  "models": { "mimo-v2.5": {} }
                }
              }
            }
            "#,
    )
    .expect("config");
    fs::write(config_dir.join(".env"), "XIAOMI_API_KEY=xiaomi-key\n").expect("env");

    let workdir = canonical_workdir(&options.workdir).expect("workdir");
    let loaded = load_run_config(&options, &workdir).expect("config");
    let resolved = resolve_run_provider(&options, &loaded).expect("provider");
    assert_eq!(resolved.model, "mimo-v2.5");

    let mut explicit_options = options.clone();
    explicit_options.model = Some("xiaomi/mimo-v2.5".to_string());
    fs::write(
        config_dir.join("config.jsonc"),
        r#"
            {
              "provider": {
                "xiaomi": {
                  "models": { "one": {}, "two": {} }
                }
              }
            }
            "#,
    )
    .expect("config");
    let loaded = load_run_config(&explicit_options, &workdir).expect("config");
    let resolved = resolve_run_provider(&explicit_options, &loaded).expect("provider");
    assert_eq!(resolved.model, "mimo-v2.5");
}

#[test]
fn cli_provider_qualified_model_selects_provider() {
    let temp = tempdir().expect("temp");
    let mut options = base_options(&temp);
    options.model = Some("deepseek/deepseek-chat".to_string());
    let config_dir = home_dir(&temp);
    fs::create_dir_all(&config_dir).expect("config dir");
    fs::write(
        config_dir.join("config.jsonc"),
        r#"
            {
              "model": "xiaomi/mimo-v2.5",
              "provider": {
                "deepseek": {
                  "models": { "deepseek-chat": {} }
                },
                "xiaomi": {
                  "models": { "mimo-v2.5": {} }
                }
              }
            }
            "#,
    )
    .expect("config");
    fs::write(
        config_dir.join(".env"),
        "DEEPSEEK_API_KEY=deepseek-key\nXIAOMI_API_KEY=xiaomi-key\n",
    )
    .expect("env");

    let workdir = canonical_workdir(&options.workdir).expect("workdir");
    let loaded = load_run_config(&options, &workdir).expect("config");
    let resolved = resolve_run_provider(&options, &loaded).expect("provider");
    assert_eq!(resolved.provider, "deepseek");
    assert_eq!(resolved.model, "deepseek-chat");
}

#[test]
fn aliases_and_auto_resolution_use_local_env_map() {
    let temp = tempdir().expect("temp");
    let mut options = base_options(&temp);
    options.model = Some("qwen/qwen-test".to_string());
    let config_dir = home_dir(&temp);
    fs::create_dir_all(&config_dir).expect("config dir");
    fs::write(
        config_dir.join("config.jsonc"),
        r#"
            {
              "provider": {
                "dashscope": {
                  "models": { "qwen-test": {} }
                }
              }
            }
            "#,
    )
    .expect("config");
    fs::write(config_dir.join(".env"), "DASHSCOPE_API_KEY=qwen-key\n").expect("env");

    let workdir = canonical_workdir(&options.workdir).expect("workdir");
    let loaded = load_run_config(&options, &workdir).expect("config");
    let resolved = resolve_run_provider(&options, &loaded).expect("provider");
    assert_eq!(resolved.provider, "dashscope");
    assert_eq!(resolved.api_key_env.as_deref(), Some("DASHSCOPE_API_KEY"));

    options.model = None;
    options.inherited_env = Some(BTreeMap::from([
        (
            "HOME".to_string(),
            temp.path().to_string_lossy().to_string(),
        ),
        (
            "PSYCHEVO_INFERENCE_MODEL".to_string(),
            "auto-model".to_string(),
        ),
    ]));
    fs::write(
        config_dir.join("config.jsonc"),
        r#"
            {
              "provider": {
                "openrouter": { "models": { "auto-model": {} } },
                "deepseek": { "models": { "auto-model": {} } }
              }
            }
            "#,
    )
    .expect("config");
    fs::write(
        config_dir.join(".env"),
        "DEEPSEEK_API_KEY=deepseek-key\nOPENAI_API_KEY=openai-key\n",
    )
    .expect("env");
    let loaded = load_run_config(&options, &workdir).expect("config");
    let resolved = resolve_run_provider(&options, &loaded).expect("auto");
    assert_eq!(resolved.provider, "openrouter");
}

#[test]
fn explicit_config_replaces_home_and_project_config_but_loads_project_env() {
    let temp = tempdir().expect("temp");
    let mut options = base_options(&temp);
    let explicit_dir = temp.path().join("explicit");
    let project_dir = options.workdir.join(".psychevo");
    fs::create_dir_all(&explicit_dir).expect("explicit dir");
    fs::create_dir_all(&project_dir).expect("project dir");
    fs::create_dir_all(home_dir(&temp)).expect("home dir");
    fs::write(
            home_dir(&temp).join("config.jsonc"),
            r#"{ "model": "deepseek/ignored", "provider": { "deepseek": { "models": { "ignored": {} } } } }"#,
        )
        .expect("home config");
    fs::write(
        project_dir.join("config.jsonc"),
        r#"{ "model": "deepseek/project-ignored" }"#,
    )
    .expect("project config");
    let explicit = explicit_dir.join("config.jsonc");
    fs::write(
        &explicit,
        r#"
            {
              "model": "custom/local",
              "provider": {
                "custom": {
                  "options": {
                    "base_url": "http://127.0.0.1:1234/v1",
                    "api_key_env": "CUSTOM_KEY"
                  },
                  "models": { "local": {} }
                }
              }
            }
            "#,
    )
    .expect("explicit config");
    fs::write(explicit_dir.join(".env"), "CUSTOM_KEY=explicit-key\n").expect("explicit env");
    fs::write(project_dir.join(".env"), "CUSTOM_KEY=project-key\n").expect("project env");
    options.config_path = Some(explicit);

    let workdir = canonical_workdir(&options.workdir).expect("workdir");
    let loaded = load_run_config(&options, &workdir).expect("config");
    let resolved = resolve_run_provider(&options, &loaded).expect("provider");
    assert_eq!(resolved.provider, "custom");
    assert_eq!(resolved.model, "local");
    assert_eq!(resolved.api_key, "project-key");
}

#[test]
fn psychevo_config_env_is_supported_and_config_dir_is_ignored() {
    let temp = tempdir().expect("temp");
    let mut options = base_options(&temp);
    let old_dir = temp.path().join("old-config-dir");
    let explicit_dir = temp.path().join("explicit");
    fs::create_dir_all(&old_dir).expect("old dir");
    fs::create_dir_all(&explicit_dir).expect("explicit dir");
    fs::write(
        old_dir.join("config.jsonc"),
        r#"{ "model": "deepseek/old", "provider": { "deepseek": { "models": { "old": {} } } } }"#,
    )
    .expect("old config");
    let explicit = explicit_dir.join("config.jsonc");
    fs::write(
        &explicit,
        r#"
            {
              "model": "custom/local",
              "provider": {
                "custom": {
                  "options": { "base_url": "http://127.0.0.1:1234/v1" },
                  "models": { "local": {} }
                }
              }
            }
            "#,
    )
    .expect("explicit config");
    options.inherited_env = Some(BTreeMap::from([
        (
            "HOME".to_string(),
            temp.path().to_string_lossy().to_string(),
        ),
        (
            "PSYCHEVO_CONFIG".to_string(),
            explicit.to_string_lossy().to_string(),
        ),
        (
            "PSYCHEVO_CONFIG_DIR".to_string(),
            old_dir.to_string_lossy().to_string(),
        ),
    ]));

    let workdir = canonical_workdir(&options.workdir).expect("workdir");
    let loaded = load_run_config(&options, &workdir).expect("config");
    let resolved = resolve_run_provider(&options, &loaded).expect("provider");
    assert_eq!(resolved.provider, "custom");
    assert_eq!(resolved.model, "local");
}

#[test]
fn missing_home_config_rejects_before_agent_start() {
    let temp = tempdir().expect("temp");
    let options = base_options(&temp);
    let workdir = canonical_workdir(&options.workdir).expect("workdir");
    let err = load_run_config(&options, &workdir).expect_err("missing home");
    assert!(err.to_string().contains("pevo init"));
}

#[test]
fn reasoning_effort_values_are_validated_and_none_disables() {
    let temp = tempdir().expect("temp");
    let mut options = base_options(&temp);
    let config_dir = home_dir(&temp);
    fs::create_dir_all(&config_dir).expect("config dir");
    fs::write(
        config_dir.join("config.jsonc"),
        r#"
            {
              "model": "custom/local",
              "provider": {
                "custom": {
                  "options": { "base_url": "http://127.0.0.1:1234/v1" },
                  "models": { "local": { "reasoning_effort": "high" } }
                }
              }
            }
            "#,
    )
    .expect("config");

    let workdir = canonical_workdir(&options.workdir).expect("workdir");
    let loaded = load_run_config(&options, &workdir).expect("config");
    let resolved = resolve_run_provider(&options, &loaded).expect("provider");
    assert_eq!(resolved.reasoning_effort.as_deref(), Some("high"));

    options.reasoning_effort = Some("none".to_string());
    let resolved = resolve_run_provider(&options, &loaded).expect("provider");
    assert_eq!(resolved.reasoning_effort, None);

    options.reasoning_effort = Some("turbo".to_string());
    let err = resolve_run_provider(&options, &loaded).expect_err("invalid");
    assert!(err.to_string().contains("reasoning_effort"));
}

#[test]
fn latest_run_session_filters_source_and_workdir() {
    let temp = tempdir().expect("temp");
    let db = temp.path().join("state.db");
    let workdir = canonical_workdir(&temp.path().join("work")).expect("workdir");
    let other_workdir = canonical_workdir(&temp.path().join("other")).expect("other");
    let store = SqliteStore::open(&db).expect("store");
    let smoke = store.create_session(&workdir).expect("smoke");
    let other = store
        .create_session_with_metadata(&other_workdir, "run", "model", "provider", None)
        .expect("other");
    let first = store
        .create_session_with_metadata(&workdir, "run", "model", "provider", None)
        .expect("first");
    let second = store
        .create_session_with_metadata(&workdir, "run", "model", "provider", None)
        .expect("second");
    thread::sleep(Duration::from_millis(2));
    store.touch_session(&first).expect("touch");

    let latest = latest_run_session_for_workdir(&db, &workdir)
        .expect("latest")
        .expect("session");
    assert_eq!(latest, first);
    assert_ne!(latest, second);
    assert_ne!(latest, smoke);
    assert_ne!(latest, other);
}

#[test]
fn session_title_setter_normalizes_and_bounds_title() {
    let temp = tempdir().expect("temp");
    let db = temp.path().join("state.db");
    let workdir = canonical_workdir(&temp.path().join("work")).expect("workdir");
    let store = SqliteStore::open(&db).expect("store");
    let session_id = store
        .create_session_with_metadata(&workdir, "tui", "model", "provider", None)
        .expect("session");

    let title = store
        .set_session_title(&session_id, &format!("  hello\n\t{}  ", "x".repeat(120)))
        .expect("title");
    assert_eq!(title.chars().count(), SESSION_TITLE_MAX_CHARS);
    assert!(title.starts_with("hello x"));
    let summary = store
        .session_summary(&session_id)
        .expect("summary")
        .expect("session");
    assert_eq!(summary.title.as_deref(), Some(title.as_str()));
    assert!(store.set_session_title(&session_id, "   ").is_err());
}

#[tokio::test]
async fn new_tui_session_title_uses_model_generated_title_without_messages() {
    let temp = tempdir().expect("temp");
    let db = temp.path().join("state.db");
    let workdir = canonical_workdir(&temp.path().join("work")).expect("workdir");
    let store = SqliteStore::open(&db).expect("store");
    let session_id = store
        .create_session_with_metadata(&workdir, "tui", "model", "provider", None)
        .expect("session");
    let provider = Arc::new(FakeProvider::new(vec![vec![
        RawStreamEvent::Text("  \"Investigate TUI Copy\"  \nextra".to_string()),
        RawStreamEvent::Done(Outcome::Normal),
    ]]));
    let resolved = resolved_title_provider();

    ensure_new_tui_session_title(
        &store,
        &session_id,
        "please inspect copy behavior",
        provider,
        &resolved,
    )
    .await
    .expect("title");

    let summary = store
        .session_summary(&session_id)
        .expect("summary")
        .expect("session");
    assert_eq!(summary.title.as_deref(), Some("Investigate TUI Copy"));
    assert_eq!(summary.message_count, 0);
    assert_eq!(summary.tool_call_count, 0);
}

#[tokio::test]
async fn new_tui_session_title_falls_back_when_model_title_fails() {
    let temp = tempdir().expect("temp");
    let db = temp.path().join("state.db");
    let workdir = canonical_workdir(&temp.path().join("work")).expect("workdir");
    let store = SqliteStore::open(&db).expect("store");
    let session_id = store
        .create_session_with_metadata(&workdir, "tui", "model", "provider", None)
        .expect("session");
    let provider = Arc::new(FakeProvider::new(Vec::new()));
    let resolved = resolved_title_provider();

    ensure_new_tui_session_title(
        &store,
        &session_id,
        "  inspect\nsidebar   title  behavior  ",
        provider,
        &resolved,
    )
    .await
    .expect("fallback title");

    let summary = store
        .session_summary(&session_id)
        .expect("summary")
        .expect("session");
    assert_eq!(
        summary.title.as_deref(),
        Some("inspect sidebar title behavior")
    );
}

fn resolved_title_provider() -> ResolvedRunProvider {
    ResolvedRunProvider {
        provider: "fake".to_string(),
        display_label: "Fake".to_string(),
        model: "model".to_string(),
        base_url: "http://127.0.0.1:9/v1".to_string(),
        api_key_env: None,
        api_key: "test-key".to_string(),
        reasoning_effort: None,
        context_limit: None,
    }
}

fn user_message(text: &str, timestamp_ms: i64) -> Message {
    Message::User {
        content: vec![psychevo_agent_core::TextBlock {
            text: text.to_string(),
        }],
        timestamp_ms,
    }
}

fn assistant_message(text: &str, timestamp_ms: i64) -> Message {
    Message::Assistant {
        content: vec![AssistantBlock::Text {
            text: text.to_string(),
        }],
        timestamp_ms,
        finish_reason: Some("stop".to_string()),
        outcome: Outcome::Normal,
        model: Some("model".to_string()),
        provider: Some("provider".to_string()),
    }
}

#[test]
fn undo_redo_restore_git_snapshots_and_visible_message_ranges() {
    let temp = tempdir().expect("temp");
    let db = temp.path().join("state.db");
    let workdir = temp.path().join("work");
    fs::create_dir_all(&workdir).expect("workdir");
    assert!(
        std::process::Command::new("git")
            .arg("-C")
            .arg(&workdir)
            .arg("init")
            .output()
            .expect("git init")
            .status
            .success()
    );
    let file = workdir.join("tracked.txt");
    fs::write(&file, "base\n").expect("base");

    let store = SqliteStore::open(&db).expect("store");
    let session_id = store
        .create_session_with_metadata(&workdir, "tui", "model", "provider", None)
        .expect("session");
    let snapshots = SnapshotStore::new(
        temp.path().join("snapshots"),
        session_id.clone(),
        workdir.clone(),
    );
    let before_first = snapshots
        .track()
        .expect("track first")
        .expect("first snapshot");
    store
        .append_message_with_undo_snapshot(
            &session_id,
            &user_message("first prompt", 1),
            Some(before_first),
        )
        .expect("user first");
    fs::write(&file, "after first\n").expect("after first");
    store
        .append_message(&session_id, &assistant_message("first answer", 2))
        .expect("assistant first");
    let before_second = snapshots
        .track()
        .expect("track second")
        .expect("second snapshot");
    store
        .append_message_with_undo_snapshot(
            &session_id,
            &user_message("second prompt", 3),
            Some(before_second),
        )
        .expect("user second");
    fs::write(&file, "after second\n").expect("after second");
    store
        .append_message(&session_id, &assistant_message("second answer", 4))
        .expect("assistant second");

    let options = SessionUndoOptions {
        db_path: db.clone(),
        workdir: workdir.clone(),
        snapshot_root: temp.path().join("snapshots"),
        session_id: session_id.clone(),
    };
    let undo = undo_session(options.clone()).expect("undo latest");
    assert_eq!(undo.prompt, "second prompt");
    assert_eq!(fs::read_to_string(&file).expect("file"), "after first\n");
    assert_eq!(
        store
            .load_tui_message_summaries(&session_id)
            .expect("visible")
            .len(),
        2
    );

    let undo = undo_session(options.clone()).expect("undo previous");
    assert_eq!(undo.prompt, "first prompt");
    assert_eq!(fs::read_to_string(&file).expect("file"), "base\n");
    assert_eq!(
        store
            .load_tui_message_summaries(&session_id)
            .expect("visible")
            .len(),
        0
    );

    let redo = redo_session(options.clone()).expect("redo first");
    assert!(!redo.complete);
    assert_eq!(fs::read_to_string(&file).expect("file"), "after first\n");
    assert_eq!(
        store
            .load_tui_message_summaries(&session_id)
            .expect("visible")
            .len(),
        2
    );

    let redo = redo_session(options).expect("redo complete");
    assert!(redo.complete);
    assert_eq!(fs::read_to_string(&file).expect("file"), "after second\n");
    assert_eq!(
        store
            .load_tui_message_summaries(&session_id)
            .expect("visible")
            .len(),
        4
    );
    assert!(
        store
            .session_revert_state(&session_id)
            .expect("revert state")
            .is_none()
    );
}

#[test]
fn cleanup_reverted_messages_deletes_hidden_range() {
    let temp = tempdir().expect("temp");
    let db = temp.path().join("state.db");
    let workdir = temp.path().join("work");
    fs::create_dir_all(&workdir).expect("workdir");
    assert!(
        std::process::Command::new("git")
            .arg("-C")
            .arg(&workdir)
            .arg("init")
            .output()
            .expect("git init")
            .status
            .success()
    );
    let file = workdir.join("tracked.txt");
    fs::write(&file, "base\n").expect("base");
    let store = SqliteStore::open(&db).expect("store");
    let session_id = store
        .create_session_with_metadata(&workdir, "tui", "model", "provider", None)
        .expect("session");
    let snapshots = SnapshotStore::new(
        temp.path().join("snapshots"),
        session_id.clone(),
        workdir.clone(),
    );
    let before_first = snapshots
        .track()
        .expect("track first")
        .expect("first snapshot");
    store
        .append_message_with_undo_snapshot(
            &session_id,
            &user_message("first prompt", 1),
            Some(before_first),
        )
        .expect("user first");
    fs::write(&file, "after first\n").expect("after first");
    store
        .append_message(&session_id, &assistant_message("first answer", 2))
        .expect("assistant first");
    let before_second = snapshots
        .track()
        .expect("track second")
        .expect("second snapshot");
    store
        .append_message_with_undo_snapshot(
            &session_id,
            &user_message("second prompt", 3),
            Some(before_second),
        )
        .expect("user second");
    fs::write(&file, "after second\n").expect("after second");
    store
        .append_message(&session_id, &assistant_message("second answer", 4))
        .expect("assistant second");

    undo_session(SessionUndoOptions {
        db_path: db.clone(),
        workdir,
        snapshot_root: temp.path().join("snapshots"),
        session_id: session_id.clone(),
    })
    .expect("undo");

    let removed = store
        .cleanup_reverted_messages(&session_id)
        .expect("cleanup");
    assert_eq!(removed, 2);
    assert_eq!(store.load_messages(&session_id).expect("messages").len(), 2);
    let summary = store
        .session_summary(&session_id)
        .expect("summary")
        .expect("session");
    assert_eq!(summary.message_count, 2);
    assert!(
        store
            .session_revert_state(&session_id)
            .expect("revert state")
            .is_none()
    );
}

#[test]
fn undo_redo_error_paths_do_not_mutate_revert_state() {
    let temp = tempdir().expect("temp");
    let db = temp.path().join("state.db");
    let workdir = temp.path().join("work");
    fs::create_dir_all(&workdir).expect("workdir");
    let store = SqliteStore::open(&db).expect("store");
    let session_id = store
        .create_session_with_metadata(&workdir, "tui", "model", "provider", None)
        .expect("session");
    let options = SessionUndoOptions {
        db_path: db.clone(),
        workdir: workdir.clone(),
        snapshot_root: temp.path().join("snapshots"),
        session_id: session_id.clone(),
    };

    let err = undo_session(options.clone()).expect_err("nothing to undo");
    assert!(err.to_string().contains("nothing to undo"));
    let err = redo_session(options.clone()).expect_err("nothing to redo");
    assert!(err.to_string().contains("nothing to redo"));

    store
        .append_message(&session_id, &user_message("no snapshot", 1))
        .expect("user");
    let err = undo_session(options).expect_err("missing snapshot");
    assert!(err.to_string().contains("undo snapshot is unavailable"));
    assert!(
        store
            .session_revert_state(&session_id)
            .expect("revert state")
            .is_none()
    );
    assert_eq!(store.load_messages(&session_id).expect("messages").len(), 1);
}

#[test]
fn sqlite_schema_v3_rejects_old_state_databases() {
    let temp = tempdir().expect("temp");
    let db = temp.path().join("old.db");
    {
        let conn = Connection::open(&db).expect("db");
        conn.pragma_update(None, "user_version", 1)
            .expect("version");
        conn.execute_batch("CREATE TABLE sessions (id TEXT);")
            .expect("schema");
    }

    let err = match SqliteStore::open(&db) {
        Ok(_) => panic!("old db opened successfully"),
        Err(err) => err,
    };
    assert!(err.to_string().contains("schema version 1"));
    assert!(err.to_string().contains("--reset-state"));

    let v2_db = temp.path().join("v2.db");
    {
        let conn = Connection::open(&v2_db).expect("db");
        conn.pragma_update(None, "user_version", 2)
            .expect("version");
        conn.execute_batch("CREATE TABLE sessions (id TEXT);")
            .expect("schema");
    }
    let err = match SqliteStore::open(&v2_db) {
        Ok(_) => panic!("v2 db opened successfully"),
        Err(err) => err,
    };
    assert!(err.to_string().contains("schema version 2"));
    assert!(err.to_string().contains("--reset-state"));
}

#[test]
fn sqlite_schema_v3_stores_reasoning_only_in_message_json_and_metrics_separately() {
    let temp = tempdir().expect("temp");
    let db = temp.path().join("state.db");
    let workdir = canonical_workdir(&temp.path().join("work")).expect("workdir");
    let store = SqliteStore::open(&db).expect("store");
    let session_id = store
        .create_session_with_metadata(&workdir, "run", "model", "provider", None)
        .expect("session");
    store
        .append_message_with_metrics(
            &session_id,
            &Message::Assistant {
                content: vec![
                    AssistantBlock::Reasoning {
                        text: "folded".to_string(),
                        provider_evidence: Some(json!({
                            "reasoning_details": [{ "type": "thinking", "text": "opaque" }]
                        })),
                    },
                    AssistantBlock::Text {
                        text: "visible".to_string(),
                    },
                ],
                timestamp_ms: 1,
                finish_reason: Some("stop".to_string()),
                outcome: Outcome::Normal,
                model: Some("model".to_string()),
                provider: Some("provider".to_string()),
            },
            Some(json!({"total_tokens": 12, "input_tokens": 5, "output_tokens": 7})),
            Some(json!({"provider_response_id": "resp_1", "model": "model"})),
        )
        .expect("append");

    let conn = Connection::open(&db).expect("db");
    let columns = conn
        .prepare("PRAGMA table_info(messages)")
        .expect("schema stmt")
        .query_map([], |row| row.get::<_, String>(1))
        .expect("schema rows")
        .collect::<rusqlite::Result<Vec<_>>>()
        .expect("columns");
    assert!(!columns.iter().any(|name| name == "reasoning_json"));
    assert!(!columns.iter().any(|name| name == "reasoning_content"));
    assert!(!columns.iter().any(|name| name == "reasoning_details_json"));

    let (message_json, usage_json, metadata_json): (String, Option<String>, Option<String>) = conn
        .query_row(
            "SELECT message_json, usage_json, metadata_json FROM messages WHERE session_id = ?1",
            [&session_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("message row");
    let message: Value = serde_json::from_str(&message_json).expect("message");
    assert_eq!(message["content"][0]["type"], "reasoning");
    assert_eq!(message["content"][0]["text"], "folded");
    assert_eq!(
        message["content"][0]["provider_evidence"]["reasoning_details"][0]["type"],
        "thinking"
    );
    assert!(message.get("reasoning_content").is_none());
    assert!(message.get("reasoning_details").is_none());
    assert!(message.get("usage").is_none());
    assert!(message.get("metadata").is_none());

    let usage: Value = serde_json::from_str(&usage_json.expect("usage")).expect("usage json");
    let metadata: Value =
        serde_json::from_str(&metadata_json.expect("metadata")).expect("metadata json");
    assert_eq!(usage["total_tokens"], 12);
    assert_eq!(metadata["provider_response_id"], "resp_1");

    let summaries = store
        .load_sanitized_message_summaries(&session_id)
        .expect("summaries");
    assert_eq!(summaries[0].usage.as_ref().unwrap()["total_tokens"], 12);
    assert_eq!(
        summaries[0].metadata.as_ref().unwrap()["provider_response_id"],
        "resp_1"
    );
    let sanitized = serde_json::to_string(&summaries[0].message).expect("sanitized");
    assert!(!sanitized.contains("folded"));

    let tui_summaries = store
        .load_tui_message_summaries(&session_id)
        .expect("tui summaries");
    let tui_message = serde_json::to_value(&tui_summaries[0].message).expect("tui message");
    assert_eq!(tui_message["content"][0]["type"], "reasoning");
    assert_eq!(tui_message["content"][0]["text"], "folded");
    assert!(tui_message["content"][0].get("provider_evidence").is_none());
}

#[tokio::test]
async fn persistence_sink_streams_elapsed_metadata_for_assistant_message_end() {
    let temp = tempdir().expect("temp");
    let db = temp.path().join("state.db");
    let workdir = canonical_workdir(&temp.path().join("work")).expect("workdir");
    let store = SqliteStore::open(&db).expect("store");
    let session_id = store
        .create_session_with_metadata(&workdir, "tui", "model", "provider", None)
        .expect("session");
    let captured = Arc::new(Mutex::new(Vec::new()));
    let captured_for_stream = Arc::clone(&captured);
    let stream: RunStreamSink = Arc::new(move |event| {
        captured_for_stream
            .lock()
            .expect("captured stream lock")
            .push(event);
    });
    let started = Instant::now()
        .checked_sub(Duration::from_millis(1200))
        .expect("instant");
    let sink = PersistenceSink {
        store: store.clone(),
        session_id: session_id.clone(),
        prompt_snapshot: None,
        prompt_snapshot_written: Arc::new(Mutex::new(false)),
        started,
        tool_elapsed_ms: Arc::new(Mutex::new(BTreeMap::new())),
        control: SmokeControl::None,
        control_handle: None,
        events: None,
        stream_events: Some(stream),
        include_reasoning: false,
        reasoning_effort: None,
    };

    sink.emit(AgentEvent::MessageEnd {
        message: Message::Assistant {
            content: vec![AssistantBlock::Text {
                text: "hi".to_string(),
            }],
            timestamp_ms: 1,
            finish_reason: Some("stop".to_string()),
            outcome: Outcome::Normal,
            model: Some("model".to_string()),
            provider: Some("provider".to_string()),
        },
        usage: None,
        metadata: Some(json!({"provider_response_id": "resp_1"})),
    })
    .await
    .expect("message end");

    let elapsed = match captured.lock().expect("captured stream lock").as_slice() {
        [RunStreamEvent::Event(value)] => value["metadata"]["elapsed_ms"]
            .as_u64()
            .expect("stream elapsed"),
        other => panic!("unexpected stream events: {other:?}"),
    };
    assert!(elapsed >= 1200);
    let summaries = store
        .load_tui_message_summaries(&session_id)
        .expect("summaries");
    assert_eq!(
        summaries[0].metadata.as_ref().unwrap()["provider_response_id"],
        "resp_1"
    );
    assert_eq!(
        summaries[0].metadata.as_ref().unwrap()["elapsed_ms"]
            .as_u64()
            .expect("stored elapsed"),
        elapsed
    );
    assert!(
        summaries[0].metadata.as_ref().unwrap()["reasoning_effort"].is_null(),
        "absent or none reasoning effort must not be stored"
    );
}

#[tokio::test]
async fn persistence_sink_persists_assistant_reasoning_effort_metadata() {
    let temp = tempdir().expect("temp");
    let db = temp.path().join("state.db");
    let workdir = canonical_workdir(&temp.path().join("work")).expect("workdir");
    let store = SqliteStore::open(&db).expect("store");
    let session_id = store
        .create_session_with_metadata(&workdir, "tui", "model", "provider", None)
        .expect("session");
    let captured = Arc::new(Mutex::new(Vec::new()));
    let captured_for_stream = Arc::clone(&captured);
    let stream: RunStreamSink = Arc::new(move |event| {
        captured_for_stream
            .lock()
            .expect("captured stream lock")
            .push(event);
    });
    let sink = PersistenceSink {
        store: store.clone(),
        session_id: session_id.clone(),
        prompt_snapshot: None,
        prompt_snapshot_written: Arc::new(Mutex::new(false)),
        started: Instant::now(),
        tool_elapsed_ms: Arc::new(Mutex::new(BTreeMap::new())),
        control: SmokeControl::None,
        control_handle: None,
        events: None,
        stream_events: Some(stream),
        include_reasoning: false,
        reasoning_effort: Some("high".to_string()),
    };

    sink.emit(AgentEvent::MessageEnd {
        message: Message::Assistant {
            content: vec![AssistantBlock::Text {
                text: "hi".to_string(),
            }],
            timestamp_ms: 1,
            finish_reason: Some("stop".to_string()),
            outcome: Outcome::Normal,
            model: Some("model".to_string()),
            provider: Some("provider".to_string()),
        },
        usage: None,
        metadata: None,
    })
    .await
    .expect("message end");

    match captured.lock().expect("captured stream lock").as_slice() {
        [RunStreamEvent::Event(value)] => {
            assert_eq!(value["metadata"]["reasoning_effort"], "high");
            assert!(value["metadata"]["elapsed_ms"].as_u64().is_some());
        }
        other => panic!("unexpected stream events: {other:?}"),
    }
    let summaries = store
        .load_tui_message_summaries(&session_id)
        .expect("summaries");
    assert_eq!(
        summaries[0].metadata.as_ref().unwrap()["reasoning_effort"],
        "high"
    );
}

#[tokio::test]
async fn persistence_sink_persists_tool_elapsed_metadata() {
    let temp = tempdir().expect("temp");
    let db = temp.path().join("state.db");
    let workdir = canonical_workdir(&temp.path().join("work")).expect("workdir");
    let store = SqliteStore::open(&db).expect("store");
    let session_id = store
        .create_session_with_metadata(&workdir, "tui", "model", "provider", None)
        .expect("session");
    let captured = Arc::new(Mutex::new(Vec::new()));
    let captured_for_stream = Arc::clone(&captured);
    let stream: RunStreamSink = Arc::new(move |event| {
        captured_for_stream
            .lock()
            .expect("captured stream lock")
            .push(event);
    });
    let sink = PersistenceSink {
        store: store.clone(),
        session_id: session_id.clone(),
        prompt_snapshot: None,
        prompt_snapshot_written: Arc::new(Mutex::new(false)),
        started: Instant::now(),
        tool_elapsed_ms: Arc::new(Mutex::new(BTreeMap::new())),
        control: SmokeControl::None,
        control_handle: None,
        events: None,
        stream_events: Some(stream),
        include_reasoning: false,
        reasoning_effort: None,
    };

    sink.emit(AgentEvent::ToolExecutionEnd {
        tool_call_id: "call_read".to_string(),
        tool_name: "read".to_string(),
        result: json!({"content":"done"}),
        outcome: Outcome::Normal,
        elapsed_ms: 321,
    })
    .await
    .expect("tool end");
    sink.emit(AgentEvent::MessageEnd {
        message: Message::ToolResult {
            tool_call_id: "call_read".to_string(),
            tool_name: "read".to_string(),
            content: "{\"content\":\"done\"}".to_string(),
            is_error: false,
            timestamp_ms: 2,
        },
        usage: None,
        metadata: None,
    })
    .await
    .expect("tool result");

    let stream_events = captured.lock().expect("captured stream lock");
    assert!(
        stream_events.iter().any(|event| {
            matches!(
                event,
                RunStreamEvent::Event(value)
                    if value["type"] == "tool_execution_end"
                        && value["elapsed_ms"] == 321
            )
        }),
        "tool_execution_end should expose elapsed_ms"
    );
    assert!(
        stream_events.iter().any(|event| {
            matches!(
                event,
                RunStreamEvent::Event(value)
                    if value["type"] == "message_end"
                        && value["metadata"]["elapsed_ms"] == 321
            )
        }),
        "tool_result message_end should expose elapsed metadata"
    );
    drop(stream_events);

    let summaries = store
        .load_tui_message_summaries(&session_id)
        .expect("summaries");
    assert_eq!(
        summaries[0].metadata.as_ref().unwrap()["elapsed_ms"]
            .as_u64()
            .expect("stored elapsed"),
        321
    );
}

#[test]
fn json_projection_hides_reasoning_unless_included() {
    let message = Message::Assistant {
        content: vec![
            AssistantBlock::Reasoning {
                text: "private".to_string(),
                provider_evidence: Some(json!({
                    "reasoning_details": [{ "type": "thinking" }]
                })),
            },
            AssistantBlock::Text {
                text: "visible".to_string(),
            },
        ],
        timestamp_ms: 1,
        finish_reason: Some("stop".to_string()),
        outcome: Outcome::Normal,
        model: Some("model".to_string()),
        provider: Some("provider".to_string()),
    };
    let event = AgentEvent::MessageEnd {
        message: message.clone(),
        usage: Some(json!({"total_tokens": 2})),
        metadata: Some(json!({"provider_response_id": "resp"})),
    };
    let hidden = project_agent_event(&event, false).expect("hidden");
    let hidden_text = serde_json::to_string(&hidden).expect("hidden json");
    assert!(hidden_text.contains("visible"));
    assert!(!hidden_text.contains("private"));
    assert!(!hidden_text.contains("reasoning_content"));
    assert!(!hidden_text.contains("total_tokens"));

    assert!(project_agent_event(&AgentEvent::ReasoningDelta { text: "x".into() }, false).is_none());
    let shown =
        project_agent_event(&AgentEvent::ReasoningDelta { text: "x".into() }, true).expect("shown");
    assert_eq!(shown, json!({"type":"reasoning_delta","text":"x"}));

    let stream =
        project_run_stream_event(&AgentEvent::ReasoningDelta { text: "x".into() }).expect("stream");
    assert_eq!(
        stream,
        RunStreamEvent::ReasoningDelta {
            text: "x".to_string()
        }
    );
    let metrics = project_run_stream_event(&event).expect("metrics");
    match metrics {
        RunStreamEvent::Event(value) => {
            assert_eq!(value["usage"]["total_tokens"], 2);
            assert_eq!(value["metadata"]["provider_response_id"], "resp");
            assert!(!serde_json::to_string(&value).unwrap().contains("private"));
        }
        other => panic!("unexpected stream event: {other:?}"),
    }
}
