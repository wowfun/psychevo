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

    let cwd = canonical_cwd(&options.cwd).expect("cwd");
    let loaded = load_run_config(&options, &cwd).expect("config");
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
        "DEEPSEEK_API_KEY=deepseek-key\nOPENROUTER_API_KEY=openrouter-key\n",
    )
    .expect("env");
    let loaded = load_run_config(&options, &cwd).expect("config");
    let resolved = resolve_run_provider(&options, &loaded).expect("auto");
    assert_eq!(resolved.provider, "openrouter");
}

#[test]
pub(crate) fn explicit_config_replaces_home_and_project_config_but_loads_project_env() {
    let temp = tempdir().expect("temp");
    let mut options = base_options(&temp);
    let explicit_dir = temp.path().join("explicit");
    let project_dir = options.cwd.join(".psychevo");
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

[provider.custom]
api = "http://127.0.0.1:1234/v1"

[provider.custom.models.local]
"#,
    )
    .expect("explicit config");
    fs::write(explicit_dir.join(".env"), "CUSTOM_API_KEY=explicit-key\n").expect("explicit env");
    fs::write(project_dir.join(".env"), "CUSTOM_API_KEY=project-key\n").expect("project env");
    options.config_path = Some(explicit);

    let cwd = canonical_cwd(&options.cwd).expect("cwd");
    let loaded = load_run_config(&options, &cwd).expect("config");
    let resolved = resolve_run_provider(&options, &loaded).expect("provider");
    assert_eq!(resolved.provider, "custom");
    assert_eq!(resolved.model, "local");
    assert_eq!(resolved.api_key, "project-key");
}

#[test]
pub(crate) fn explicit_config_agent_backends_still_load_project_overlay() {
    let temp = tempdir().expect("temp");
    let options = base_options(&temp);
    let home = home_dir(&temp);
    let explicit_dir = temp.path().join("explicit");
    let project_dir = options.cwd.join(".psychevo");
    fs::create_dir_all(&home).expect("home dir");
    fs::create_dir_all(&explicit_dir).expect("explicit dir");
    fs::create_dir_all(&project_dir).expect("project dir");
    write_config(
        home.join("config.toml"),
        r#"
[agents.backends.ignored]
kind = "acp"
description = "Home backend should be replaced by explicit config."
command = "ignored"
"#,
    )
    .expect("home config");
    let explicit = explicit_dir.join("config.toml");
    write_config(
        &explicit,
        r#"
[agents.backends.cursor]
kind = "acp"
description = "Cursor ACP coding agent."
command = "cursor-agent"
args = ["--acp"]
"#,
    )
    .expect("explicit config");
    write_config(
        project_dir.join("config.toml"),
        r#"
[agents.backends.opencode]
kind = "acp"
description = "OpenCode ACP coding agent."
command = "opencode"
args = ["acp"]
"#,
    )
    .expect("project config");
    let env = BTreeMap::from([
        (
            "PSYCHEVO_CONFIG".to_string(),
            explicit.to_string_lossy().to_string(),
        ),
        (
            "PSYCHEVO_HOME".to_string(),
            home.to_string_lossy().to_string(),
        ),
    ]);

    let backends = load_agent_backend_configs(&home, &options.cwd, &env).expect("backends");

    assert!(backends.contains_key("cursor"));
    assert!(backends.contains_key("opencode"));
    assert!(!backends.contains_key("ignored"));
}

#[test]
pub(crate) fn runtime_profile_configs_merge_profile_and_project_overlay() {
    let temp = tempdir().expect("temp");
    let options = base_options(&temp);
    let home = home_dir(&temp);
    let project_dir = options.cwd.join(".psychevo");
    fs::create_dir_all(&home).expect("home dir");
    fs::create_dir_all(&project_dir).expect("project dir");
    write_config(
        home.join("config.toml"),
        r#"
[runtime_profiles.codex]
runtime = "codex"
label = "Codex"
command = "codex"
args = ["app-server", "--stdio"]
default_mode = "default"
"#,
    )
    .expect("home config");
    write_config(
        project_dir.join("config.toml"),
        r#"
[runtime_profiles.codex]
enabled = false
default_mode = "auto-review"

[runtime_profiles.opencode]
runtime = "opencode"
command = "opencode"
args = ["serve"]
default_agent = "build"
"#,
    )
    .expect("project config");
    let env = BTreeMap::from([(
        "PSYCHEVO_HOME".to_string(),
        home.to_string_lossy().to_string(),
    )]);

    let profiles = load_runtime_profile_configs(&home, &options.cwd, &env).expect("profiles");

    let codex = profiles.get("codex").expect("codex");
    assert_eq!(codex.runtime, RuntimeProfileKind::Codex);
    assert!(!codex.enabled);
    assert_eq!(codex.default_mode.as_deref(), Some("auto-review"));
    assert_eq!(codex.args, vec!["app-server", "--stdio"]);
    let opencode = profiles.get("opencode").expect("opencode");
    assert_eq!(opencode.runtime, RuntimeProfileKind::OpenCode);
    assert_eq!(opencode.default_agent.as_deref(), Some("build"));
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

[provider.custom]
api = "http://127.0.0.1:1234/v1"

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

    let cwd = canonical_cwd(&options.cwd).expect("cwd");
    let loaded = load_run_config(&options, &cwd).expect("config");
    let resolved = resolve_run_provider(&options, &loaded).expect("provider");
    assert_eq!(resolved.provider, "custom");
    assert_eq!(resolved.model, "local");
}

#[test]
pub(crate) fn missing_home_config_rejects_before_agent_start() {
    let temp = tempdir().expect("temp");
    let options = base_options(&temp);
    let cwd = canonical_cwd(&options.cwd).expect("cwd");
    let err = load_run_config(&options, &cwd).expect_err("missing home");
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

    let cwd = canonical_cwd(&options.cwd).expect("cwd");
    let err = load_run_config(&options, &cwd).expect_err("missing toml");
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

    let cwd = canonical_cwd(&options.cwd).expect("cwd");
    load_run_config(&options, &cwd).expect("config.jsonc ignored");
}

#[test]
pub(crate) fn workspace_root_defaults_to_home_workspaces() {
    let temp = tempdir().expect("temp");
    let options = base_options(&temp);
    let config_dir = home_dir(&temp);
    fs::create_dir_all(&config_dir).expect("config dir");
    write_config(config_dir.join("config.toml"), "").expect("toml config");

    let cwd = canonical_cwd(&options.cwd).expect("cwd");
    let root = resolve_workspace_root(&options, &cwd).expect("workspace root");
    let default_cwd = resolve_default_workspace_cwd(&options, &cwd).expect("default workspace");

    assert_eq!(root, temp.path().join("workspaces"));
    assert_eq!(default_cwd, temp.path().join("workspaces").join("general"));
}

#[test]
pub(crate) fn workspace_root_uses_profile_config_without_cwd_overlay() {
    let temp = tempdir().expect("temp");
    let options = base_options(&temp);
    let config_dir = home_dir(&temp);
    let project_dir = options.cwd.join(".psychevo");
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

    let cwd = canonical_cwd(&options.cwd).expect("cwd");
    let root = resolve_workspace_root(&options, &cwd).expect("workspace root");

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

    let cwd = canonical_cwd(&options.cwd).expect("cwd");
    let err = resolve_workspace_root(&options, &cwd).expect_err("empty root");

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

    let cwd = canonical_cwd(&options.cwd).expect("cwd");
    let loaded = load_run_config(&options, &cwd).expect("config");
    assert_eq!(
        loaded.config.project_context.instructions,
        ProjectContextInstructionMode::Cwd
    );

    options.project_context_override = Some(ProjectContextInstructionMode::Off);
    let loaded = load_run_config(&options, &cwd).expect("override");
    assert_eq!(
        loaded.config.project_context.instructions,
        ProjectContextInstructionMode::Off
    );
}

#[test]
pub(crate) fn project_context_lightweight_load_does_not_require_home_config() {
    let temp = tempdir().expect("temp");
    let options = base_options(&temp);
    let cwd = canonical_cwd(&options.cwd).expect("cwd");
    fs::create_dir_all(cwd.join(".psychevo")).expect("project config dir");
    write_config(
        cwd.join(".psychevo/config.toml"),
        r#"
[project_context]
instructions = "cwd"
"#,
    )
    .expect("project config");

    let mode = load_project_context_instruction_mode(&options, &cwd).expect("mode");
    assert_eq!(mode, ProjectContextInstructionMode::Cwd);
    assert!(load_run_config(&options, &cwd).is_err());
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

    let cwd = canonical_cwd(&options.cwd).expect("cwd");
    let err = load_run_config(&options, &cwd).expect_err("invalid context mode");
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

[provider.custom]
api = "http://127.0.0.1:1234/v1"

[provider.custom.models.local]
reasoning_effort = "high"
"#,
    )
    .expect("config");

    let cwd = canonical_cwd(&options.cwd).expect("cwd");
    let loaded = load_run_config(&options, &cwd).expect("config");
    let resolved = resolve_run_provider(&options, &loaded).expect("provider");
    assert_eq!(resolved.reasoning_effort.as_deref(), Some("high"));

    options.reasoning_effort = Some("none".to_string());
    let resolved = resolve_run_provider(&options, &loaded).expect("provider");
    assert_eq!(resolved.reasoning_effort, None);

    options.reasoning_effort = Some("turbo".to_string());
    let err = resolve_run_provider(&options, &loaded).expect_err("invalid");
    assert!(err.to_string().contains("reasoning_effort"));
}
