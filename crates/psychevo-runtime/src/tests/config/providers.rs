#[allow(unused_imports)]
pub(crate) use super::*;
#[allow(unused_imports)]
pub(crate) use super::*;

#[test]
pub(crate) fn default_global_config_uses_home_psychevo_config_toml() {
    let temp = tempdir().expect("temp");
    let options = base_options(&temp);
    let global_dir = home_dir(&temp);
    fs::create_dir_all(&global_dir).expect("global dir");
    write_config(
        global_dir.join("config.toml"),
        r#"
model = "deepseek/deepseek-chat"

[provider.deepseek.options]
base_url = "http://home.example/v1"
api_key_env = "DEEPSEEK_API_KEY"

[provider.deepseek.models."deepseek-chat"]
"#,
    )
    .expect("global config");
    fs::write(global_dir.join(".env"), "DEEPSEEK_API_KEY=home-key\n").expect("global env");

    let cwd = canonical_cwd(&options.cwd).expect("cwd");
    let loaded = load_run_config(&options, &cwd).expect("config");
    let resolved = resolve_run_provider(&options, &loaded).expect("provider");
    assert_eq!(resolved.provider, "deepseek");
    assert_eq!(resolved.model, "deepseek-chat");
    assert_eq!(resolved.base_url, "http://home.example/v1");
    assert_eq!(resolved.api_key, "home-key");
}

#[test]
pub(crate) fn opencode_zen_no_auth_resolves_without_api_key() {
    let temp = tempdir().expect("temp");
    let options = base_options(&temp);
    let config_dir = home_dir(&temp);
    fs::create_dir_all(&config_dir).expect("config dir");
    write_config(
        config_dir.join("config.toml"),
        r#"
model = "opencode-zen/mimo-v2.5-free"

[provider."opencode-zen".options]
base_url = "https://opencode.ai/zen/v1"
no_auth = true

[provider."opencode-zen".models."mimo-v2.5-free"]
"#,
    )
    .expect("config");

    let cwd = canonical_cwd(&options.cwd).expect("cwd");
    let loaded = load_run_config(&options, &cwd).expect("config");
    let resolved = resolve_run_provider(&options, &loaded).expect("provider");
    assert_eq!(resolved.provider, "opencode-zen");
    assert_eq!(resolved.model, "mimo-v2.5-free");
    assert_eq!(resolved.api_key_env, None);
    assert_eq!(resolved.api_key, "");
}

#[test]
pub(crate) fn psychevo_home_overrides_default_home() {
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
    write_config(
        custom_home.join("config.toml"),
        r#"
model = "deepseek/deepseek-chat"

[provider.deepseek.options]
base_url = "http://custom-home.example/v1"
api_key_env = "DEEPSEEK_API_KEY"

[provider.deepseek.models."deepseek-chat"]
"#,
    )
    .expect("config");
    fs::write(custom_home.join(".env"), "DEEPSEEK_API_KEY=custom-key\n").expect("env");

    let cwd = canonical_cwd(&options.cwd).expect("cwd");
    let loaded = load_run_config(&options, &cwd).expect("config");
    let resolved = resolve_run_provider(&options, &loaded).expect("provider");
    assert_eq!(resolved.base_url, "http://custom-home.example/v1");
    assert_eq!(resolved.api_key, "custom-key");
}

#[test]
pub(crate) fn config_merge_dotenv_precedence_and_provider_qualified_model() {
    let temp = tempdir().expect("temp");
    let options = base_options(&temp);
    let config_dir = home_dir(&temp);
    let project_dir = options.cwd.join(".psychevo");
    fs::create_dir_all(&config_dir).expect("config dir");
    fs::create_dir_all(&project_dir).expect("project dir");
    write_config(
        config_dir.join("config.toml"),
        r#"
# global default
model = "deepseek/deepseek-chat"

[provider.deepseek.options]
base_url = "http://global.example/v1"
api_key_env = "DEEPSEEK_API_KEY"

[provider.deepseek.models."deepseek-chat"]
reasoning_effort = "low"
"#,
    )
    .expect("global config");
    fs::write(config_dir.join(".env"), "DEEPSEEK_API_KEY=global-key\n").expect("global env");
    write_config(
        project_dir.join("config.toml"),
        r#"
[provider.deepseek.options]
base_url = "http://project.example/v1"

[provider.deepseek.models."deepseek-chat"]
reasoning_effort = "high"
"#,
    )
    .expect("project config");
    fs::write(project_dir.join(".env"), "DEEPSEEK_API_KEY='project-key'\n").expect("project env");

    let cwd = canonical_cwd(&options.cwd).expect("cwd");
    let loaded = load_run_config(&options, &cwd).expect("config");
    let resolved = resolve_run_provider(&options, &loaded).expect("provider");
    assert_eq!(resolved.provider, "deepseek");
    assert_eq!(resolved.model, "deepseek-chat");
    assert_eq!(resolved.base_url, "http://project.example/v1");
    assert_eq!(resolved.api_key, "project-key");
    assert_eq!(resolved.reasoning_effort.as_deref(), Some("high"));
}

#[test]
pub(crate) fn model_object_provider_and_reasoning_effort_are_resolved() {
    let temp = tempdir().expect("temp");
    let options = base_options(&temp);
    let config_dir = home_dir(&temp);
    fs::create_dir_all(&config_dir).expect("config dir");
    write_config(
        config_dir.join("config.toml"),
        r#"
[model]
provider = "mimo"
id = "mimo-v2.5"
reasoning_effort = "medium"

[provider.xiaomi.models."mimo-v2.5"]
"#,
    )
    .expect("config");
    fs::write(config_dir.join(".env"), "XIAOMI_API_KEY=xiaomi-key\n").expect("env");

    let cwd = canonical_cwd(&options.cwd).expect("cwd");
    let loaded = load_run_config(&options, &cwd).expect("config");
    let resolved = resolve_run_provider(&options, &loaded).expect("provider");
    assert_eq!(resolved.provider, "xiaomi");
    assert_eq!(resolved.model, "mimo-v2.5");
    assert_eq!(resolved.api_key_env.as_deref(), Some("XIAOMI_API_KEY"));
    assert_eq!(resolved.reasoning_effort.as_deref(), Some("medium"));
}

#[test]
pub(crate) fn xiaomi_token_plan_builtin_resolves_default_url_and_env() {
    let temp = tempdir().expect("temp");
    let options = base_options(&temp);
    let config_dir = home_dir(&temp);
    fs::create_dir_all(&config_dir).expect("config dir");
    write_config(
        config_dir.join("config.toml"),
        r#"
model = "xiaomi-token-plan/mimo-v2.5-pro"
"#,
    )
    .expect("config");
    fs::write(
        config_dir.join(".env"),
        "XIAOMI_TOKEN_PLAN_API_KEY=token-plan-key\n",
    )
    .expect("env");

    let cwd = canonical_cwd(&options.cwd).expect("cwd");
    let loaded = load_run_config(&options, &cwd).expect("config");
    let resolved = resolve_run_provider(&options, &loaded).expect("provider");
    assert_eq!(resolved.provider, "xiaomi-token-plan");
    assert_eq!(resolved.base_url, "https://token-plan-cn.xiaomimimo.com/v1");
    assert_eq!(
        resolved.api_key_env.as_deref(),
        Some("XIAOMI_TOKEN_PLAN_API_KEY")
    );
    assert_eq!(resolved.model, "mimo-v2.5-pro");
}

#[test]
pub(crate) fn selected_configured_model_reports_effective_reasoning_without_credentials() {
    let temp = tempdir().expect("temp");
    let mut options = base_options(&temp);
    let config_dir = home_dir(&temp);
    fs::create_dir_all(&config_dir).expect("config dir");
    write_config(
        config_dir.join("config.toml"),
        r#"
model = "custom/local"

[provider.custom.options]
base_url = "http://127.0.0.1:1234/v1"
api_key_env = "CUSTOM_KEY"

[provider.custom.models.local]
reasoning_effort = "high"

[provider.custom.models.other]
reasoning_effort = "low"
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
pub(crate) fn models_dev_cache_enriches_configured_model_by_base_url() {
    let temp = tempdir().expect("temp");
    let options = base_options(&temp);
    let config_dir = home_dir(&temp);
    fs::create_dir_all(&config_dir).expect("config dir");
    fs::write(
        config_dir.join("models_dev_cache.json"),
        r#"
        {
          "xiaomi-token-plan-cn": {
            "api": "https://token-plan-cn.xiaomimimo.com/v1",
            "models": {
              "mimo-v2.5-pro": {
                "id": "mimo-v2.5-pro",
                "reasoning": true,
                "tool_call": true,
                "cost": { "input": 0, "output": 0 },
                "limit": { "context": 1048576, "output": 65536 },
                "modalities": { "input": ["text"], "output": ["text"] }
              }
            }
          }
        }
        "#,
    )
    .expect("cache");
    write_config(
        config_dir.join("config.toml"),
        r#"
[provider."xiaomi-token-plan"]
label = "Xiaomi Token Plan"

[provider."xiaomi-token-plan".options]
base_url = "https://token-plan-cn.xiaomimimo.com/v1"
api_key_env = "XIAOMI_KEY"

[provider."xiaomi-token-plan".models."mimo-v2.5-pro"]
"#,
    )
    .expect("config");

    let models = configured_models(&options).expect("models");
    let model = models
        .iter()
        .find(|model| model.provider == "xiaomi-token-plan")
        .expect("token plan model");
    assert_eq!(model.context_limit, Some(1_048_576));
    assert_eq!(model.metadata.limits.output, Some(65_536));
    assert_eq!(model.metadata.capabilities.tool_call, Some(true));
    assert_eq!(
        model.metadata.cost.as_ref().and_then(|cost| cost.input),
        Some(0.0)
    );
}

#[test]
pub(crate) fn models_dev_cache_enriches_xiaomi_omni_capabilities_and_modalities() {
    let temp = tempdir().expect("temp");
    let options = base_options(&temp);
    let config_dir = home_dir(&temp);
    fs::create_dir_all(&config_dir).expect("config dir");
    fs::write(
        config_dir.join("models_dev_cache.json"),
        r#"
        {
          "xiaomi-token-plan-cn": {
            "api": "https://token-plan-cn.xiaomimimo.com/v1",
            "models": {
              "mimo-v2-omni": {
                "id": "mimo-v2-omni",
                "reasoning": true,
                "tool_call": true,
                "temperature": true,
                "attachment": true,
                "interleaved": { "field": "reasoning_content" },
                "limit": { "context": 262144, "output": 131072 },
                "modalities": {
                  "input": ["text", "image", "audio", "video", "pdf"],
                  "output": ["text"]
                }
              }
            }
          }
        }
        "#,
    )
    .expect("cache");
    write_config(
        config_dir.join("config.toml"),
        r#"
[provider."xiaomi-token-plan"]
label = "Xiaomi Token Plan"

[provider."xiaomi-token-plan".options]
base_url = "https://token-plan-cn.xiaomimimo.com/v1"
api_key_env = "XIAOMI_KEY"

[provider."xiaomi-token-plan".models."mimo-v2-omni"]
"#,
    )
    .expect("config");

    let models = configured_models(&options).expect("models");
    let model = models
        .iter()
        .find(|model| model.provider == "xiaomi-token-plan")
        .expect("token plan model");
    assert_eq!(model.context_limit, Some(262_144));
    assert_eq!(model.metadata.limits.output, Some(131_072));
    assert_eq!(model.metadata.capabilities.reasoning, Some(true));
    assert_eq!(model.metadata.capabilities.tool_call, Some(true));
    assert_eq!(model.metadata.capabilities.attachment, Some(true));
    assert_eq!(
        model.metadata.capabilities.input_modalities,
        ["text", "image", "audio", "video", "pdf"]
            .into_iter()
            .map(str::to_string)
            .collect::<Vec<_>>()
    );
    assert_eq!(
        model.metadata.capabilities.output_modalities,
        vec!["text".to_string()]
    );
    assert_eq!(
        model.metadata.source.as_deref(),
        Some("models.dev:xiaomi-token-plan-cn")
    );
}

#[tokio::test]
pub(crate) async fn refresh_model_metadata_cache_writes_models_dev_cache() {
    let temp = tempdir().expect("temp");
    let home = home_dir(&temp);
    fs::create_dir_all(&home).expect("home");
    let server = CatalogServer::new(
        r#"
        {
          "mock": {
            "models": {
              "mock-model": {
                "id": "mock-model",
                "limit": { "context": 8192 }
              }
            }
          }
        }
        "#,
    );
    let env_map = BTreeMap::from([(
        "PSYCHEVO_MODELS_DEV_URL".to_string(),
        server.base_url.clone(),
    )]);

    refresh_model_metadata_cache(
        home.clone(),
        env_map,
        vec![ModelMetadataCacheTarget {
            provider: "mock".to_string(),
            model: "mock-model".to_string(),
            base_url: None,
        }],
    )
    .await
    .expect("refresh");

    let cache = fs::read_to_string(home.join("models_dev_cache.json")).expect("cache");
    assert!(cache.contains("mock-model"), "{cache}");
    assert!(server.request().starts_with("GET /v1 "));
}

#[tokio::test]
pub(crate) async fn refresh_model_metadata_cache_writes_only_requested_models() {
    let temp = tempdir().expect("temp");
    let home = home_dir(&temp);
    fs::create_dir_all(&home).expect("home");
    let server = CatalogServer::new(
        r#"
        {
          "mock": {
            "api": "http://mock.example/v1",
            "models": {
              "used-model": { "id": "used-model", "limit": { "context": 8192 } },
              "unused-model": { "id": "unused-model", "limit": { "context": 16384 } }
            }
          },
          "other": {
            "models": {
              "other-model": { "id": "other-model" }
            }
          }
        }
        "#,
    );
    let env_map = BTreeMap::from([(
        "PSYCHEVO_MODELS_DEV_URL".to_string(),
        server.base_url.clone(),
    )]);

    refresh_model_metadata_cache(
        home.clone(),
        env_map,
        vec![ModelMetadataCacheTarget {
            provider: "custom-mock".to_string(),
            model: "used-model".to_string(),
            base_url: Some("http://mock.example/v1".to_string()),
        }],
    )
    .await
    .expect("refresh");

    let cache = fs::read_to_string(home.join("models_dev_cache.json")).expect("cache");
    assert!(cache.contains("used-model"), "{cache}");
    assert!(!cache.contains("unused-model"), "{cache}");
    assert!(!cache.contains("other-model"), "{cache}");
}

#[tokio::test]
pub(crate) async fn refresh_model_metadata_cache_preserves_old_cache_on_failure() {
    let temp = tempdir().expect("temp");
    let home = home_dir(&temp);
    fs::create_dir_all(&home).expect("home");
    let path = home.join("models_dev_cache.json");
    fs::write(&path, r#"{"old":true}"#).expect("old cache");
    let env_map = BTreeMap::from([(
        "PSYCHEVO_MODELS_DEV_URL".to_string(),
        "http://127.0.0.1:1/api.json".to_string(),
    )]);

    let err = refresh_model_metadata_cache(
        home,
        env_map,
        vec![ModelMetadataCacheTarget {
            provider: "mock".to_string(),
            model: "mock-model".to_string(),
            base_url: None,
        }],
    )
    .await
    .expect_err("refresh failure");

    assert!(!err.to_string().is_empty(), "{err}");
    assert_eq!(fs::read_to_string(path).expect("cache"), r#"{"old":true}"#);
}

#[test]
pub(crate) fn explicit_metadata_config_override_wins_and_disables_reasoning() {
    let temp = tempdir().expect("temp");
    let options = base_options(&temp);
    let config_dir = home_dir(&temp);
    fs::create_dir_all(&config_dir).expect("config dir");
    write_config(
        config_dir.join("config.toml"),
        r#"
model = "deepseek/deepseek-chat"

[provider.deepseek.options]
base_url = "http://deepseek.example/v1"
api_key_env = "DEEPSEEK_API_KEY"

[provider.deepseek.models."deepseek-chat"]
reasoning_effort = "high"
reasoning = false
tool_call = false

[provider.deepseek.models."deepseek-chat".limit]
context = 1234
output = 99

[provider.deepseek.models."deepseek-chat".cost]
input = 1.5
output = 2.5
"#,
    )
    .expect("config");
    fs::write(config_dir.join(".env"), "DEEPSEEK_API_KEY=key\n").expect("env");

    let cwd = canonical_cwd(&options.cwd).expect("cwd");
    let loaded = load_run_config(&options, &cwd).expect("config");
    let resolved = resolve_run_provider(&options, &loaded).expect("provider");
    assert_eq!(resolved.context_limit, Some(1234));
    assert_eq!(resolved.reasoning_effort, None);
    assert_eq!(resolved.metadata.limits.output, Some(99));
    assert_eq!(resolved.metadata.capabilities.tool_call, Some(false));
    assert_eq!(
        resolved.metadata.cost.as_ref().and_then(|cost| cost.output),
        Some(2.5)
    );
}

#[test]
pub(crate) fn legacy_context_limit_config_field_is_rejected() {
    let temp = tempdir().expect("temp");
    let options = base_options(&temp);
    let config_dir = home_dir(&temp);
    fs::create_dir_all(&config_dir).expect("config dir");
    write_config(
        config_dir.join("config.toml"),
        r#"
model = "custom/local"

[provider.custom.options]
base_url = "http://127.0.0.1:1234/v1"

[provider.custom.models.local]
context_limit = 1234
"#,
    )
    .expect("config");

    let cwd = canonical_cwd(&options.cwd).expect("cwd");
    let err = load_run_config(&options, &cwd).expect_err("legacy field");
    assert!(err.to_string().contains("use limit.context"));
}

#[test]
pub(crate) fn raw_api_keys_are_rejected() {
    let temp = tempdir().expect("temp");
    let options = base_options(&temp);
    let config_dir = home_dir(&temp);
    fs::create_dir_all(&config_dir).expect("config dir");
    write_config(
        config_dir.join("config.toml"),
        r#"
[provider.custom.options]
base_url = "http://127.0.0.1:1234/v1"
api_key = "secret"

[provider.custom.models.local]
"#,
    )
    .expect("config");

    let cwd = canonical_cwd(&options.cwd).expect("cwd");
    let err = load_run_config(&options, &cwd).expect_err("raw key");
    assert!(err.to_string().contains("raw API keys"));
}

#[test]
pub(crate) fn provider_label_is_display_only() {
    let temp = tempdir().expect("temp");
    let options = base_options(&temp);
    let config_dir = home_dir(&temp);
    fs::create_dir_all(&config_dir).expect("config dir");
    write_config(
        config_dir.join("config.toml"),
        r#"
model = "custom/local"

[provider.custom]
label = "Xiaomi Token Plan CN"

[provider.custom.options]
base_url = "http://127.0.0.1:1234/v1"
api_key_env = "CUSTOM_KEY"

[provider.custom.models.local]
"#,
    )
    .expect("config");
    fs::write(config_dir.join(".env"), "CUSTOM_KEY=custom-key\n").expect("env");

    let cwd = canonical_cwd(&options.cwd).expect("cwd");
    let loaded = load_run_config(&options, &cwd).expect("config");
    let resolved = resolve_run_provider(&options, &loaded).expect("provider");
    assert_eq!(resolved.provider, "custom");
    assert_eq!(resolved.display_label, "Xiaomi Token Plan CN");
    let models = configured_models(&options).expect("models");
    assert_eq!(models[0].provider_label, "Xiaomi Token Plan CN");
}

#[test]
pub(crate) fn create_global_custom_provider_writes_config_and_env() {
    let temp = tempdir().expect("temp");
    let config_dir = home_dir(&temp);
    fs::create_dir_all(&config_dir).expect("config dir");
    write_config(
        config_dir.join("config.toml"),
        r#"
# keep this comment
model = "deepseek/deepseek-chat"
"#,
    )
    .expect("config");

    let result = create_global_custom_provider(CustomProviderInput {
        home: config_dir.clone(),
        provider_id: "xiaomi-token-plan-cn".to_string(),
        label: "Xiaomi Token Plan CN".to_string(),
        base_url: "https://token-plan-cn.xiaomimimo.com/v1/".to_string(),
        api_key: Some("secret-key".to_string()),
        no_auth: false,
    })
    .expect("create provider");

    assert_eq!(result.provider_id, "xiaomi-token-plan-cn");
    assert_eq!(result.api_key_env, "XIAOMI_TOKEN_PLAN_CN_API_KEY");
    assert!(result.wrote_api_key);
    assert!(!result.reused_existing_api_key);
    let config = fs::read_to_string(config_dir.join("config.toml")).expect("config");
    assert!(config.contains("label = \"Xiaomi Token Plan CN\""));
    assert!(config.contains("base_url = \"https://token-plan-cn.xiaomimimo.com/v1\""));
    assert!(config.contains("api_key_env = \"XIAOMI_TOKEN_PLAN_CN_API_KEY\""));
    assert!(!config.contains("secret-key"));
    let env = fs::read_to_string(config_dir.join(".env")).expect("env");
    assert_eq!(env, "XIAOMI_TOKEN_PLAN_CN_API_KEY=secret-key\n");
}

#[test]
pub(crate) fn set_default_model_writes_local_by_default_and_global_when_requested() {
    let temp = tempdir().expect("temp");
    let options = base_options(&temp);
    let home = home_dir(&temp);
    let cwd = options.cwd.clone();
    fs::create_dir_all(&home).expect("home");
    fs::create_dir_all(cwd.join(".psychevo")).expect("local config dir");
    write_config(
        home.join("config.toml"),
        r#"
[provider.mock.options]
base_url = "http://127.0.0.1:9"
"#,
    )
    .expect("global config");
    write_config(
        cwd.join(".psychevo/config.toml"),
        r#"
[provider.localmock.options]
base_url = "http://127.0.0.1:9"
"#,
    )
    .expect("local config");

    let local = set_default_model(&home, &cwd, false, "localmock/local-model")
        .expect("local default model");
    assert_eq!(local["scope"], "local");
    let local_config = fs::read_to_string(cwd.join(".psychevo/config.toml")).expect("local config");
    assert!(local_config.contains("model = \"localmock/local-model\""));
    assert!(
        !fs::read_to_string(home.join("config.toml"))
            .expect("global config")
            .contains("local-model")
    );

    let global =
        set_default_model(&home, &cwd, true, "mock/global-model").expect("global default model");
    assert_eq!(global["scope"], "global");
    let global_config = fs::read_to_string(home.join("config.toml")).expect("global config");
    assert!(global_config.contains("model = \"mock/global-model\""));
}

#[test]
pub(crate) fn set_default_model_with_reasoning_writes_model_object() {
    let temp = tempdir().expect("temp");
    let home = home_dir(&temp);
    let cwd = temp.path().join("work");
    fs::create_dir_all(&home).expect("home");
    write_config(
        home.join("config.toml"),
        r#"
[provider.mock.options]
base_url = "http://127.0.0.1:9"
"#,
    )
    .expect("global config");

    let value =
        set_default_model_with_reasoning(&home, &cwd, true, "mock/global-model", Some("high"))
            .expect("global default model with reasoning");

    assert_eq!(value["scope"], "global");
    assert_eq!(value["model"], "mock/global-model");
    assert_eq!(value["reasoning_effort"], "high");
    let global_config = fs::read_to_string(home.join("config.toml")).expect("global config");
    assert!(global_config.contains("[model]"));
    assert!(global_config.contains("id = \"mock/global-model\""));
    assert!(global_config.contains("reasoning_effort = \"high\""));

    let err =
        set_default_model_with_reasoning(&home, &cwd, true, "mock/global-model", Some("turbo"))
            .expect_err("invalid reasoning effort");
    assert!(err.to_string().contains("reasoning_effort"));
}

#[test]
pub(crate) fn set_default_model_validates_provider_scope_without_catalog_fetch() {
    let temp = tempdir().expect("temp");
    let home = home_dir(&temp);
    let cwd = temp.path().join("work");
    fs::create_dir_all(home.join("cache")).expect("home");
    fs::create_dir_all(cwd.join(".psychevo")).expect("local");
    write_config(
        cwd.join(".psychevo/config.toml"),
        r#"
[provider.localonly.options]
base_url = "http://127.0.0.1:9"
"#,
    )
    .expect("local config");

    set_default_model(&home, &cwd, false, "localonly/model").expect("local provider");
    let global_err = set_default_model(&home, &cwd, true, "localonly/model")
        .expect_err("local provider cannot be global default");
    assert!(global_err.to_string().contains("unknown provider"));
    let format_err = set_default_model(&home, &cwd, false, "unqualified")
        .expect_err("provider-qualified model required");
    assert!(format_err.to_string().contains("provider/model"));
    let unknown_err =
        set_default_model(&home, &cwd, false, "unknown/model").expect_err("unknown provider");
    assert!(unknown_err.to_string().contains("unknown provider"));
    assert!(!home.join("models_dev_cache.json").exists());
}

#[test]
pub(crate) fn auxiliary_compression_model_precedes_legacy_compression_model() {
    let temp = tempdir().expect("temp");
    let options = base_options(&temp);
    let home = home_dir(&temp);
    fs::create_dir_all(&home).expect("home");
    write_config(
        home.join("config.toml"),
        r#"
model = "main/main-model"

[provider.main.options]
base_url = "http://127.0.0.1:9/v1"

[provider.main.models.main-model]

[provider.legacy.options]
base_url = "http://127.0.0.1:10/v1"

[provider.legacy.models.legacy-model]

[provider.aux.options]
base_url = "http://127.0.0.1:11/v1"

[provider.aux.models.aux-model]

[compression]
model = "legacy/legacy-model"

[auxiliary.compression]
provider = "aux"
model = "aux-model"
"#,
    )
    .expect("config");

    let cwd = canonical_cwd(&options.cwd).expect("cwd");
    let loaded = load_run_config(&options, &cwd).expect("config");
    let current = resolve_run_provider(&options, &loaded).expect("main provider");
    let compression =
        resolve_compression_config(&options, &loaded, &current).expect("compression provider");

    assert!(compression.model_configured);
    assert_eq!(compression.provider.provider, "aux");
    assert_eq!(compression.provider.model, "aux-model");
}

#[test]
pub(crate) fn auxiliary_model_assignment_writes_hermes_style_task_slot() {
    let temp = tempdir().expect("temp");
    let home = home_dir(&temp);
    let cwd = temp.path().join("work");
    fs::create_dir_all(&home).expect("home");
    write_config(
        home.join("config.toml"),
        r#"
[provider."opencode-zen".options]
base_url = "https://opencode.ai/zen/v1"
no_auth = true
"#,
    )
    .expect("config");

    let value = set_auxiliary_model_with_reasoning(
        &home,
        &cwd,
        true,
        "title_generation",
        "zen",
        "mimo-v2.5-free",
        Some("high"),
    )
    .expect("aux model");

    assert_eq!(value["task"], "title_generation");
    assert_eq!(value["provider"], "opencode-zen");
    assert_eq!(value["model"], "mimo-v2.5-free");
    assert_eq!(value["reasoning_effort"], "high");
    let config = fs::read_to_string(home.join("config.toml")).expect("config");
    assert!(config.contains("[auxiliary.title_generation]"));
    assert!(config.contains("provider = \"opencode-zen\""));
    assert!(config.contains("id = \"mimo-v2.5-free\""));
    assert!(config.contains("reasoning_effort = \"high\""));
}

#[test]
pub(crate) fn create_global_custom_provider_reuses_existing_env_without_overwrite() {
    let temp = tempdir().expect("temp");
    let config_dir = home_dir(&temp);
    fs::create_dir_all(&config_dir).expect("config dir");
    write_config(config_dir.join("config.toml"), "\n").expect("config");
    fs::write(
        config_dir.join(".env"),
        "XIAOMI_TOKEN_PLAN_CN_API_KEY=existing-key\n",
    )
    .expect("env");

    let result = create_global_custom_provider(CustomProviderInput {
        home: config_dir.clone(),
        provider_id: "xiaomi-token-plan-cn".to_string(),
        label: "Xiaomi Token Plan CN".to_string(),
        base_url: "https://token-plan-cn.xiaomimimo.com/v1".to_string(),
        api_key: Some("new-key".to_string()),
        no_auth: false,
    })
    .expect("create provider");

    assert!(!result.wrote_api_key);
    assert!(result.reused_existing_api_key);
    let env = fs::read_to_string(config_dir.join(".env")).expect("env");
    assert_eq!(env, "XIAOMI_TOKEN_PLAN_CN_API_KEY=existing-key\n");
}

#[test]
pub(crate) fn create_global_custom_provider_rejects_duplicates_aliases_and_raw_keyless_create() {
    let temp = tempdir().expect("temp");
    let config_dir = home_dir(&temp);
    fs::create_dir_all(&config_dir).expect("config dir");
    write_config(
        config_dir.join("config.toml"),
        r#"
[provider."custom-one".models]
"#,
    )
    .expect("config");

    let duplicate = create_global_custom_provider(CustomProviderInput {
        home: config_dir.clone(),
        provider_id: "custom-one".to_string(),
        label: "Custom One".to_string(),
        base_url: "https://api.example/v1".to_string(),
        api_key: Some("key".to_string()),
        no_auth: false,
    })
    .expect_err("duplicate");
    assert!(duplicate.to_string().contains("already exists"));

    let alias = create_global_custom_provider(CustomProviderInput {
        home: config_dir.clone(),
        provider_id: "mimo".to_string(),
        label: "Mimo".to_string(),
        base_url: "https://api.example/v1".to_string(),
        api_key: Some("key".to_string()),
        no_auth: false,
    })
    .expect_err("alias");
    assert!(alias.to_string().contains("collides"));

    let missing_key = create_global_custom_provider(CustomProviderInput {
        home: config_dir,
        provider_id: "custom-two".to_string(),
        label: "Custom Two".to_string(),
        base_url: "https://api.example/v1".to_string(),
        api_key: None,
        no_auth: false,
    })
    .expect_err("missing key");
    assert!(
        missing_key
            .to_string()
            .contains(&custom_provider_api_key_env("custom-two"))
    );
}

#[test]
pub(crate) fn unique_model_default_and_multiple_model_rejection() {
    let temp = tempdir().expect("temp");
    let options = base_options(&temp);
    let config_dir = home_dir(&temp);
    fs::create_dir_all(&config_dir).expect("config dir");
    write_config(
        config_dir.join("config.toml"),
        r#"
[provider.xiaomi.models."mimo-v2.5"]
"#,
    )
    .expect("config");
    fs::write(config_dir.join(".env"), "XIAOMI_API_KEY=xiaomi-key\n").expect("env");

    let cwd = canonical_cwd(&options.cwd).expect("cwd");
    let loaded = load_run_config(&options, &cwd).expect("config");
    let resolved = resolve_run_provider(&options, &loaded).expect("provider");
    assert_eq!(resolved.model, "mimo-v2.5");

    let mut explicit_options = options.clone();
    explicit_options.model = Some("xiaomi/mimo-v2.5".to_string());
    write_config(
        config_dir.join("config.toml"),
        r#"
[provider.xiaomi.models.one]

[provider.xiaomi.models.two]
"#,
    )
    .expect("config");
    let loaded = load_run_config(&explicit_options, &cwd).expect("config");
    let resolved = resolve_run_provider(&explicit_options, &loaded).expect("provider");
    assert_eq!(resolved.model, "mimo-v2.5");
}

#[test]
pub(crate) fn cli_provider_qualified_model_selects_provider() {
    let temp = tempdir().expect("temp");
    let mut options = base_options(&temp);
    options.model = Some("deepseek/deepseek-chat".to_string());
    let config_dir = home_dir(&temp);
    fs::create_dir_all(&config_dir).expect("config dir");
    write_config(
        config_dir.join("config.toml"),
        r#"
model = "xiaomi/mimo-v2.5"

[provider.deepseek.models."deepseek-chat"]

[provider.xiaomi.models."mimo-v2.5"]
"#,
    )
    .expect("config");
    fs::write(
        config_dir.join(".env"),
        "DEEPSEEK_API_KEY=deepseek-key\nXIAOMI_API_KEY=xiaomi-key\n",
    )
    .expect("env");

    let cwd = canonical_cwd(&options.cwd).expect("cwd");
    let loaded = load_run_config(&options, &cwd).expect("config");
    let resolved = resolve_run_provider(&options, &loaded).expect("provider");
    assert_eq!(resolved.provider, "deepseek");
    assert_eq!(resolved.model, "deepseek-chat");
}
