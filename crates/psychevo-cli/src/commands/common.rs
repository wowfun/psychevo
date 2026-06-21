use std::collections::BTreeMap;
use std::io::{self, IsTerminal, Read};
use std::path::{Path, PathBuf};

use anyhow::{Result, anyhow};
use psychevo_runtime::{RunMode, RunOptions, StateRuntime, canonicalize_workdir};
use serde_json::json;

use crate::env::{env_path, resolve_state_db};

pub(crate) fn print_json_error(err: &anyhow::Error) -> Result<()> {
    println!(
        "{}",
        serde_json::to_string(&json!({
            "type": "error",
            "message": format!("{err:#}"),
        }))?
    );
    Ok(())
}

pub(crate) fn base_run_options(
    env_map: &BTreeMap<String, String>,
    home: &Path,
    cwd: &Path,
) -> Result<RunOptions> {
    let db_path = resolve_state_db(env_map, home, cwd)?;
    Ok(RunOptions {
        state: StateRuntime::open(&db_path)?,
        workdir: cwd.to_path_buf(),
        snapshot_root: None,
        session: None,
        continue_latest: false,
        prompt: String::new(),
        image_inputs: Vec::new(),
        extract_prompt_image_sources: false,
        prompt_display: None,
        max_context_messages: None,
        config_path: env_path("PSYCHEVO_CONFIG", env_map, cwd)?,
        project_context_override: None,
        model: None,
        reasoning_effort: None,
        runtime_ref: None,
        runtime_session_id: None,
        runtime_options: BTreeMap::new(),
        external_agent_delegate: None,
        include_reasoning: false,
        mode: RunMode::Default,
        permission_mode: None,
        approval_mode: None,
        approval_handler: None,
        clarify_enabled: false,
        inherited_env: Some(env_map.clone()),
        agent: None,
        no_agents: false,
        no_skills: false,
        skill_inputs: Vec::new(),
        mcp_servers: Vec::new(),
    })
}

pub(crate) fn config_scope_dir(home: &Path, cwd: &Path, local: bool) -> Result<PathBuf> {
    if local {
        Ok(canonicalize_workdir(cwd)?.join(".psychevo"))
    } else {
        Ok(home.to_path_buf())
    }
}

pub(crate) fn scope_label(local: bool) -> &'static str {
    if local { "local" } else { "global" }
}

pub(crate) fn scoped_config_dir(home: &Path, cwd: &Path, global: bool) -> Result<PathBuf> {
    config_scope_dir(home, cwd, !global)
}

pub(crate) fn scoped_label(global: bool) -> &'static str {
    if global { "global" } else { "local" }
}

pub(crate) fn read_secret_from_stdin(required: bool) -> Result<Option<String>> {
    if !required {
        return Ok(None);
    }
    if io::stdin().is_terminal() {
        return Err(anyhow!(
            "stdin secret input requires piped stdin; interactive secret input is unavailable here"
        ));
    }
    let mut secret = String::new();
    io::stdin().read_to_string(&mut secret)?;
    let secret = secret.trim().to_string();
    if secret.is_empty() {
        return Err(anyhow!("stdin secret input requires a non-empty value"));
    }
    Ok(Some(secret))
}
