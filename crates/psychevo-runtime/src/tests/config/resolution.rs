#[allow(unused_imports)]
pub(crate) use super::*;

#[test]
pub(crate) fn aliases_and_auto_resolution_use_local_env_map() {
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
pub(crate) fn explicit_config_replaces_home_and_project_config_but_loads_project_env() {
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
pub(crate) fn psychevo_config_env_is_supported_and_config_dir_is_ignored() {
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
pub(crate) fn missing_home_config_rejects_before_agent_start() {
    let temp = tempdir().expect("temp");
    let options = base_options(&temp);
    let workdir = canonical_workdir(&options.workdir).expect("workdir");
    let err = load_run_config(&options, &workdir).expect_err("missing home");
    assert!(err.to_string().contains("pevo init"));
}

#[test]
pub(crate) fn config_jsonc_without_toml_is_ignored_and_missing_home_rejects() {
    let temp = tempdir().expect("temp");
    let options = base_options(&temp);
    let config_dir = home_dir(&temp);
    fs::create_dir_all(&config_dir).expect("config dir");
    fs::write(
        config_dir.join("config.jsonc"),
        r#"{"model":"deepseek/old"}"#,
    )
    .expect("jsonc");

    let workdir = canonical_workdir(&options.workdir).expect("workdir");
    let err = load_run_config(&options, &workdir).expect_err("missing toml");
    assert!(err.to_string().contains("pevo init"));
    assert!(err.to_string().contains("config.toml"));
    assert!(!err.to_string().contains("config.jsonc"));
}

#[test]
pub(crate) fn config_jsonc_is_ignored_when_toml_exists() {
    let temp = tempdir().expect("temp");
    let options = base_options(&temp);
    let config_dir = home_dir(&temp);
    fs::create_dir_all(&config_dir).expect("config dir");
    fs::write(
        config_dir.join("config.toml"),
        r#"model = "deepseek/deepseek-chat""#,
    )
    .expect("toml config");
    fs::write(
        config_dir.join("config.jsonc"),
        r#"{ this is not valid jsonc"#,
    )
    .expect("jsonc");

    let workdir = canonical_workdir(&options.workdir).expect("workdir");
    load_run_config(&options, &workdir).expect("config.jsonc ignored");
}

#[test]
pub(crate) fn workspace_root_defaults_to_home_workspaces() {
    let temp = tempdir().expect("temp");
    let options = base_options(&temp);
    let config_dir = home_dir(&temp);
    fs::create_dir_all(&config_dir).expect("config dir");
    write_config(config_dir.join("config.toml"), "").expect("toml config");

    let workdir = canonical_workdir(&options.workdir).expect("workdir");
    let root = resolve_workspace_root(&options, &workdir).expect("workspace root");
    let default_workdir =
        resolve_default_workspace_workdir(&options, &workdir).expect("default workspace");

    assert_eq!(root, temp.path().join("workspaces"));
    assert_eq!(
        default_workdir,
        temp.path().join("workspaces").join("general")
    );
}

#[test]
pub(crate) fn workspace_root_uses_profile_config_without_workdir_overlay() {
    let temp = tempdir().expect("temp");
    let options = base_options(&temp);
    let config_dir = home_dir(&temp);
    let project_dir = options.workdir.join(".psychevo");
    fs::create_dir_all(&config_dir).expect("config dir");
    fs::create_dir_all(&project_dir).expect("project dir");
    write_config(
        config_dir.join("config.toml"),
        r#"
[workspaces]
root = "~/shared-workspaces"
"#,
    )
    .expect("home config");
    write_config(
        project_dir.join("config.toml"),
        r#"
[workspaces]
root = "~/ignored-workspaces"
"#,
    )
    .expect("project config");

    let workdir = canonical_workdir(&options.workdir).expect("workdir");
    let root = resolve_workspace_root(&options, &workdir).expect("workspace root");

    assert_eq!(root, temp.path().join("shared-workspaces"));
}

#[test]
pub(crate) fn empty_workspace_root_is_rejected() {
    let temp = tempdir().expect("temp");
    let options = base_options(&temp);
    let config_dir = home_dir(&temp);
    fs::create_dir_all(&config_dir).expect("config dir");
    write_config(
        config_dir.join("config.toml"),
        r#"
[workspaces]
root = "  "
"#,
    )
    .expect("home config");

    let workdir = canonical_workdir(&options.workdir).expect("workdir");
    let err = resolve_workspace_root(&options, &workdir).expect_err("empty root");

    assert!(
        err.to_string()
            .contains("workspaces.root must not be empty")
    );
}

#[test]
pub(crate) fn project_context_config_parses_and_cli_override_wins() {
    let temp = tempdir().expect("temp");
    let mut options = base_options(&temp);
    let config_dir = home_dir(&temp);
    fs::create_dir_all(&config_dir).expect("config dir");
    write_config(
        config_dir.join("config.toml"),
        r#"
[project_context]
instructions = "cwd"
"#,
    )
    .expect("toml config");

    let workdir = canonical_workdir(&options.workdir).expect("workdir");
    let loaded = load_run_config(&options, &workdir).expect("config");
    assert_eq!(
        loaded.config.project_context.instructions,
        ProjectContextInstructionMode::Cwd
    );

    options.project_context_override = Some(ProjectContextInstructionMode::Off);
    let loaded = load_run_config(&options, &workdir).expect("override");
    assert_eq!(
        loaded.config.project_context.instructions,
        ProjectContextInstructionMode::Off
    );
}

#[test]
pub(crate) fn project_context_lightweight_load_does_not_require_home_config() {
    let temp = tempdir().expect("temp");
    let options = base_options(&temp);
    let workdir = canonical_workdir(&options.workdir).expect("workdir");
    fs::create_dir_all(workdir.join(".psychevo")).expect("project config dir");
    write_config(
        workdir.join(".psychevo/config.toml"),
        r#"
[project_context]
instructions = "cwd"
"#,
    )
    .expect("project config");

    let mode = load_project_context_instruction_mode(&options, &workdir).expect("mode");
    assert_eq!(mode, ProjectContextInstructionMode::Cwd);
    assert!(load_run_config(&options, &workdir).is_err());
}

#[test]
pub(crate) fn invalid_project_context_config_is_rejected() {
    let temp = tempdir().expect("temp");
    let options = base_options(&temp);
    let config_dir = home_dir(&temp);
    fs::create_dir_all(&config_dir).expect("config dir");
    write_config(
        config_dir.join("config.toml"),
        r#"
[project_context]
instructions = "repo-root"
"#,
    )
    .expect("toml config");

    let workdir = canonical_workdir(&options.workdir).expect("workdir");
    let err = load_run_config(&options, &workdir).expect_err("invalid context mode");
    assert!(
        err.to_string()
            .contains("project_context.instructions must be git-root, cwd, or off")
    );
}

#[test]
pub(crate) fn reasoning_effort_values_are_validated_and_none_disables() {
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
