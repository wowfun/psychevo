#[test]
fn default_global_config_uses_home_psychevo_config_toml() {
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
fn models_dev_cache_enriches_configured_model_by_base_url() {
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
    assert_eq!(model.metadata.cost.as_ref().and_then(|cost| cost.input), Some(0.0));
}

#[test]
fn models_dev_cache_enriches_xiaomi_omni_capabilities_and_modalities() {
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
async fn refresh_model_metadata_cache_writes_models_dev_cache() {
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
async fn refresh_model_metadata_cache_writes_only_requested_models() {
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
async fn refresh_model_metadata_cache_preserves_old_cache_on_failure() {
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
fn explicit_metadata_config_override_wins_and_disables_reasoning() {
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

    let workdir = canonical_workdir(&options.workdir).expect("workdir");
    let loaded = load_run_config(&options, &workdir).expect("config");
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
fn legacy_context_limit_config_field_is_rejected() {
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

    let workdir = canonical_workdir(&options.workdir).expect("workdir");
    let err = load_run_config(&options, &workdir).expect_err("legacy field");
    assert!(err.to_string().contains("use limit.context"));
}

#[test]
fn raw_api_keys_are_rejected() {
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

    let workdir = canonical_workdir(&options.workdir).expect("workdir");
    let err = load_run_config(&options, &workdir).expect_err("raw key");
    assert!(err.to_string().contains("raw API keys"));
}

#[test]
fn provider_label_is_display_only() {
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

    let workdir = canonical_workdir(&options.workdir).expect("workdir");
    let loaded = load_run_config(&options, &workdir).expect("config");
    let resolved = resolve_run_provider(&options, &loaded).expect("provider");
    assert_eq!(resolved.provider, "custom");
    assert_eq!(resolved.display_label, "Xiaomi Token Plan CN");
    let models = configured_models(&options).expect("models");
    assert_eq!(models[0].provider_label, "Xiaomi Token Plan CN");
}

#[test]
fn create_global_custom_provider_writes_config_and_env() {
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
fn create_global_custom_provider_reuses_existing_env_without_overwrite() {
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
    })
    .expect("create provider");

    assert!(!result.wrote_api_key);
    assert!(result.reused_existing_api_key);
    let env = fs::read_to_string(config_dir.join(".env")).expect("env");
    assert_eq!(env, "XIAOMI_TOKEN_PLAN_CN_API_KEY=existing-key\n");
}

#[test]
fn create_global_custom_provider_rejects_duplicates_aliases_and_raw_keyless_create() {
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
    })
    .expect_err("duplicate");
    assert!(duplicate.to_string().contains("already exists"));

    let alias = create_global_custom_provider(CustomProviderInput {
        home: config_dir.clone(),
        provider_id: "mimo".to_string(),
        label: "Mimo".to_string(),
        base_url: "https://api.example/v1".to_string(),
        api_key: Some("key".to_string()),
    })
    .expect_err("alias");
    assert!(alias.to_string().contains("collides"));

    let missing_key = create_global_custom_provider(CustomProviderInput {
        home: config_dir,
        provider_id: "custom-two".to_string(),
        label: "Custom Two".to_string(),
        base_url: "https://api.example/v1".to_string(),
        api_key: None,
    })
    .expect_err("missing key");
    assert!(missing_key
        .to_string()
        .contains(&custom_provider_api_key_env("custom-two")));
}

#[test]
fn unique_model_default_and_multiple_model_rejection() {
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

    let workdir = canonical_workdir(&options.workdir).expect("workdir");
    let loaded = load_run_config(&options, &workdir).expect("config");
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
    write_config(
        config_dir.join("config.toml"),
        r#"
[provider.dashscope.models."qwen-test"]
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
    write_config(
        config_dir.join("config.toml"),
        r#"
[provider.openrouter.models."auto-model"]

[provider.deepseek.models."auto-model"]
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
    write_config(
        home_dir(&temp).join("config.toml"),
        r#"
model = "deepseek/ignored"

[provider.deepseek.models.ignored]
"#,
    )
    .expect("home config");
    write_config(
        project_dir.join("config.toml"),
        r#"model = "deepseek/project-ignored""#,
    )
    .expect("project config");
    let explicit = explicit_dir.join("config.toml");
    write_config(
        &explicit,
        r#"
model = "custom/local"

[provider.custom.options]
base_url = "http://127.0.0.1:1234/v1"
api_key_env = "CUSTOM_KEY"

[provider.custom.models.local]
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
    write_config(
        old_dir.join("config.toml"),
        r#"
model = "deepseek/old"

[provider.deepseek.models.old]
"#,
    )
    .expect("old config");
    let explicit = explicit_dir.join("config.toml");
    write_config(
        &explicit,
        r#"
model = "custom/local"

[provider.custom.options]
base_url = "http://127.0.0.1:1234/v1"

[provider.custom.models.local]
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
fn config_jsonc_without_toml_is_ignored_and_missing_home_rejects() {
    let temp = tempdir().expect("temp");
    let options = base_options(&temp);
    let config_dir = home_dir(&temp);
    fs::create_dir_all(&config_dir).expect("config dir");
    fs::write(config_dir.join("config.jsonc"), r#"{"model":"deepseek/old"}"#).expect("jsonc");

    let workdir = canonical_workdir(&options.workdir).expect("workdir");
    let err = load_run_config(&options, &workdir).expect_err("missing toml");
    assert!(err.to_string().contains("pevo init"));
    assert!(err.to_string().contains("config.toml"));
    assert!(!err.to_string().contains("config.jsonc"));
}

#[test]
fn config_jsonc_is_ignored_when_toml_exists() {
    let temp = tempdir().expect("temp");
    let options = base_options(&temp);
    let config_dir = home_dir(&temp);
    fs::create_dir_all(&config_dir).expect("config dir");
    fs::write(config_dir.join("config.toml"), r#"model = "deepseek/deepseek-chat""#)
        .expect("toml config");
    fs::write(config_dir.join("config.jsonc"), r#"{ this is not valid jsonc"#).expect("jsonc");

    let workdir = canonical_workdir(&options.workdir).expect("workdir");
    load_run_config(&options, &workdir).expect("config.jsonc ignored");
}

#[test]
fn reasoning_effort_values_are_validated_and_none_disables() {
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

[provider.custom.models.local]
reasoning_effort = "high"
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
