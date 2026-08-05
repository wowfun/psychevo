use std::path::{Path, PathBuf};

use psychevo::Error;
use psychevo::agents::resolve_agent_definition;
use psychevo::config::REASONING_EFFORT_VALUES;
use psychevo::model_state::ModelState;
use psychevo::{
    Configuration, ConfigurationQuery, PermissionMode, RunMode, SetThreadMainAgentSelection,
    ThreadMainAgentSelection,
};
use psychevo_gateway_protocol as wire;
use serde_json::{Value, json};

use super::super::channels::channel_list_result_from_value;
use super::super::scope_session::{ResolvedScope, gateway_backend_info_for_thread};
use super::models::model_options_with_cached_catalog;
use super::{WebState, discover_gateway_agents};

pub(in super::super) async fn settings_read_value(
    state: &WebState,
    cwd: &Path,
    thread_id: Option<&str>,
) -> psychevo::Result<Value> {
    let normalized_cwd = psychevo::host_paths::normalized_native_path(cwd);
    let cwd = normalized_cwd.as_path();
    let mut query = ConfigurationQuery::new(cwd);
    query.inherited_env = Some(state.inner.inherited_env.clone());
    let configuration = state.inner.framework.configuration(query)?;
    let controls = workbench_controls_value(state, cwd, thread_id, &configuration).await?;
    let project = workbench_project_value(cwd);
    let channels = configuration
        .channels()
        .and_then(|value| channel_list_result_from_value(state, value))
        .unwrap_or_default();
    let web_search = web_search_settings_result(
        configuration
            .web_search_settings()
            .unwrap_or_else(|_| default_web_search_settings()),
    );
    Ok(json!({
        "cwd": cwd.display().to_string(),
        "project": project,
        "channels": channels,
        "memoryResources": {"mode": "status_only", "available": true},
        "secrets": {"frontendPersistence": "disabled"},
        "controls": controls
        ,"webSearch": web_search
    }))
}

pub(in super::super) fn web_search_settings_value(
    state: &WebState,
    cwd: &Path,
) -> psychevo::Result<Value> {
    let mut query = ConfigurationQuery::new(cwd);
    query.inherited_env = Some(state.inner.inherited_env.clone());
    let configuration = state.inner.framework.configuration(query)?;
    let value = configuration
        .web_search_settings()
        .unwrap_or_else(|_| default_web_search_settings());
    Ok(web_search_settings_result(value))
}

fn default_web_search_settings() -> Value {
    json!({
        "execution": "local", "backend": "exa", "external_access": "live",
        "context_size": "medium", "return_token_budget": "default",
        "content_types": ["text"], "allowed_domains": [], "blocked_domains": [],
        "background_storage_acknowledged": false,
        "location": {"country":"", "region":"", "city":"", "timezone":""},
        "image": {"max_results": 3, "caption": true},
        "credentials": {"exa":"missing", "parallel":"missing", "brave":"missing", "searxng":"missing"}
    })
}

fn web_search_settings_result(value: Value) -> Value {
    json!({
        "execution": value["execution"],
        "backend": value["backend"],
        "externalAccess": value["external_access"],
        "contextSize": value["context_size"],
        "returnTokenBudget": value["return_token_budget"],
        "contentTypes": value["content_types"],
        "allowedDomains": value["allowed_domains"],
        "blockedDomains": value["blocked_domains"],
        "backgroundStorageAcknowledged": value["background_storage_acknowledged"],
        "location": value["location"],
        "image": value["image"],
        "credentials": value["credentials"],
    })
}

pub(in super::super) fn web_search_settings_update_value(
    state: &WebState,
    cwd: &Path,
    params: wire::settings_workspace_context::WebSearchSettingsUpdateParams,
) -> psychevo::Result<Value> {
    let search = params.search;
    let value = json!({
        "execution": search.execution,
        "backend": search.backend,
        "external_access": search.external_access,
        "context_size": search.context_size,
        "return_token_budget": search.return_token_budget,
        "content_types": search.content_types,
        "allowed_domains": search.allowed_domains,
        "blocked_domains": search.blocked_domains,
        "background_storage_acknowledged": search.background_storage_acknowledged,
        "location": search.location,
        "image": search.image,
    });
    psychevo::config::update_global_web_search_settings(
        &state.inner.home,
        value,
        params.credential_values,
    )?;
    web_search_settings_value(state, cwd)
}

async fn workbench_controls_value(
    state: &WebState,
    cwd: &Path,
    thread_id: Option<&str>,
    configuration: &Configuration,
) -> psychevo::Result<wire::settings_workspace_context::WorkbenchControlsView> {
    let agent = session_control_agent(state, thread_id).await?;
    let model_state = ModelState::load(&ModelState::path_for_home(&state.inner.home))?;
    let cwd_key = cwd.to_string_lossy().to_string();
    let session_selection = match thread_id {
        Some(thread_id) => session_model_state_selection(state, thread_id).await?,
        None => None,
    };
    let state_model = model_state.model_for(&cwd_key);
    let state_reasoning_effort = model_state.reasoning_effort_for(&cwd_key);
    let selected = configuration.selected_model();
    let (config_model, config_status, config_error) = match selected {
        Ok(Some(model)) => (
            Some(format!("{}/{}", model.provider, model.model)),
            wire::settings_workspace_context::WorkbenchModelStatus::Resolved,
            None,
        ),
        Ok(None) => (
            None,
            wire::settings_workspace_context::WorkbenchModelStatus::Unconfigured,
            None,
        ),
        Err(error) if model_resolution_unconfigured_error(&error.to_string()) => (
            None,
            wire::settings_workspace_context::WorkbenchModelStatus::Unconfigured,
            None,
        ),
        Err(error) => (
            None,
            wire::settings_workspace_context::WorkbenchModelStatus::Error,
            Some(error.to_string()),
        ),
    };
    let model = session_selection
        .as_ref()
        .and_then(|selection| selection.model.clone())
        .or(state_model)
        .or(config_model);
    let model_status = if model.is_some() {
        wire::settings_workspace_context::WorkbenchModelStatus::Resolved
    } else {
        config_status
    };
    let model_error = if model.is_some() { None } else { config_error };
    let variant = session_selection
        .as_ref()
        .and_then(|selection| selection.reasoning_effort.clone())
        .or(state_reasoning_effort)
        .or_else(|| Some("none".to_string()));
    let configured = configuration.configured_models().unwrap_or_default();
    let model_details = model_options_with_cached_catalog(configuration, &configured);
    let model_options = model_details
        .iter()
        .map(|model| model.value.clone())
        .collect();
    let runtime_ref = match thread_id {
        Some(thread_id) => {
            gateway_backend_info_for_thread(state, thread_id)
                .await?
                .runtime_ref
        }
        None => None,
    }
    .unwrap_or_else(|| "native".to_string());
    Ok(wire::settings_workspace_context::WorkbenchControlsView {
        permission_mode: PermissionMode::Default.as_str().to_string(),
        mode: RunMode::Default.as_str().to_string(),
        runtime_ref,
        agent,
        model,
        model_status,
        model_error,
        variant,
        permission_mode_options: ["default", "acceptEdits", "dontAsk", "bypassPermissions"]
            .into_iter()
            .map(str::to_string)
            .collect(),
        mode_options: ["default", "plan"]
            .into_iter()
            .map(str::to_string)
            .collect(),
        model_options,
        model_details,
        recent_models: model_state.recent_model_values(),
        variant_options: REASONING_EFFORT_VALUES
            .iter()
            .map(|value| (*value).to_string())
            .collect(),
    })
}

#[derive(Debug, Clone)]
pub(super) struct ComposerModelSelection {
    pub(super) model: Option<String>,
    pub(super) reasoning_effort: Option<String>,
}

pub(super) async fn session_model_state_selection(
    state: &WebState,
    thread_id: &str,
) -> psychevo::Result<Option<ComposerModelSelection>> {
    let Some(selection) = state
        .inner
        .framework
        .thread_model_selection(thread_id)
        .await?
    else {
        return Ok(None);
    };
    Ok(Some(ComposerModelSelection {
        model: Some(format!("{}/{}", selection.provider, selection.model)),
        reasoning_effort: selection.reasoning_effort,
    }))
}

fn model_resolution_unconfigured_error(message: &str) -> bool {
    message.contains("auto provider could not find usable credentials and model")
        || message.contains("Psychevo home is not initialized")
}

pub(in super::super) fn native_runtime_mode_option()
-> wire::thread_command_turn::RuntimeConfigOptionView {
    wire::thread_command_turn::RuntimeConfigOptionView {
        id: "mode".to_string(),
        name: "Psychevo mode".to_string(),
        description: None,
        category: Some("mode".to_string()),
        option_type: "select".to_string(),
        current_value: Some(RunMode::Default.as_str().to_string()),
        values: [RunMode::Default, RunMode::Plan]
            .into_iter()
            .map(
                |mode| wire::thread_command_turn::RuntimeConfigOptionValueView {
                    value: mode.as_str().to_string(),
                    name: mode.as_str().to_string(),
                    description: None,
                    group: None,
                },
            )
            .collect(),
    }
}

pub(in super::super) async fn session_control_agent(
    state: &WebState,
    thread_id: Option<&str>,
) -> psychevo::Result<Option<String>> {
    let Some(thread_id) = thread_id else {
        return Ok(None);
    };
    let selection = state
        .inner
        .framework
        .resume_thread(thread_id)
        .await?
        .main_agent_selection()
        .await?;
    Ok(match selection {
        ThreadMainAgentSelection::Agent { input } => Some(input),
        ThreadMainAgentSelection::Default { .. } | ThreadMainAgentSelection::Missing { .. } => None,
    })
}

pub(in super::super) async fn update_session_agent_setting(
    state: &WebState,
    scope: &ResolvedScope,
    thread_id: &str,
    input: Option<&str>,
) -> psychevo::Result<()> {
    let thread = state.inner.framework.resume_thread(thread_id).await?;
    let summary = thread.summary().await?;
    if Path::new(&summary.cwd) != scope.cwd.as_path() {
        return Err(Error::Message(format!(
            "session {thread_id} does not belong to {}",
            scope.cwd.display()
        )));
    }
    let Some(input) = input else {
        thread
            .set_main_agent_selection(SetThreadMainAgentSelection::Default)
            .await?;
        return Ok(());
    };
    let input = input.trim();
    if input.is_empty() {
        return Err(Error::Message(
            "settings/update agent must be null or a concrete agent".to_string(),
        ));
    }
    let catalog = discover_gateway_agents(state, scope)?;
    if catalog.shadowed_agents.iter().any(|agent| {
        agent
            .file_path
            .as_ref()
            .is_some_and(|path| path.to_string_lossy() == input)
    }) {
        return Err(Error::Message(format!(
            "shadowed agent definitions cannot be used as main: {input}"
        )));
    }
    let agent = resolve_agent_definition(&catalog, input, &scope.cwd, &state.inner.inherited_env)?;
    thread
        .set_main_agent_selection(SetThreadMainAgentSelection::Agent {
            input: input.to_string(),
            name: agent.name,
            source: agent.source,
            path: agent.file_path,
        })
        .await?;
    Ok(())
}

fn workbench_project_value(cwd: &Path) -> wire::settings_workspace_context::WorkbenchProjectView {
    let cwd = psychevo::host_paths::normalized_native_path(cwd);
    wire::settings_workspace_context::WorkbenchProjectView {
        path: cwd.display().to_string(),
        display_path: display_cwd(&cwd),
        branch: current_git_branch(&cwd),
    }
}

pub(in super::super) fn display_cwd(cwd: &Path) -> String {
    let cwd_display = psychevo::host_paths::display_path_for_native_path(cwd);
    if let Some(home) = std::env::var_os("HOME").map(PathBuf::from)
        && let Some(display) = display_relative_to_home(
            &cwd_display,
            &psychevo::host_paths::display_path_for_native_path(&home),
        )
    {
        return display;
    }
    cwd_display
}

pub(in super::super) fn display_relative_to_home(
    cwd_display: &str,
    home_display: &str,
) -> Option<String> {
    let home = if home_display == "/" {
        home_display
    } else {
        home_display.trim_end_matches('/')
    };
    if home.is_empty() {
        return None;
    }
    if cwd_display == home {
        return Some("~".to_string());
    }
    cwd_display
        .strip_prefix(&format!("{home}/"))
        .map(|relative| format!("~/{relative}"))
}

fn current_git_branch(cwd: &Path) -> Option<String> {
    let output = std::process::Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .current_dir(cwd)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let branch = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!branch.is_empty()).then_some(branch)
}
