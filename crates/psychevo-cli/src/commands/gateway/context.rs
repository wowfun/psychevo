use std::env;
use std::fs;
use std::path::PathBuf;

use anyhow::Result;

use crate::env::{
    ensure_home_initialized, env_path, env_value, inherited_env, resolve_psychevo_home,
};

use super::managed::{ManagedPaths, ensure_managed_dir, managed_paths};

pub(super) struct GatewayContext {
    pub(super) cwd: PathBuf,
    pub(super) home: PathBuf,
    pub(super) profile_name: String,
    pub(super) env_map: std::collections::BTreeMap<String, String>,
    pub(super) paths: ManagedPaths,
}

impl GatewayContext {
    pub(super) fn load() -> Result<Self> {
        let env_map = inherited_env();
        let cwd = env::current_dir()?;
        let home = resolve_psychevo_home(&env_map, &cwd)?;
        let profile_name = env_value(crate::profiles::PROFILE_ENV, &env_map)
            .unwrap_or_else(|| crate::profiles::DEFAULT_PROFILE.to_string());
        let config_path = env_path("PSYCHEVO_CONFIG", &env_map, &cwd)?;
        let bypass_home = config_path.is_some() && env_value("PSYCHEVO_DB", &env_map).is_some();
        if !bypass_home {
            ensure_home_initialized(&home)?;
        }
        let paths = managed_paths(&home);
        ensure_managed_dir(&paths)?;
        Ok(Self {
            cwd,
            home,
            profile_name,
            env_map,
            paths,
        })
    }

    pub(super) fn load_for_setup() -> Result<Self> {
        let env_map = inherited_env();
        let cwd = env::current_dir()?;
        let home = resolve_psychevo_home(&env_map, &cwd)?;
        let profile_name = env_value(crate::profiles::PROFILE_ENV, &env_map)
            .unwrap_or_else(|| crate::profiles::DEFAULT_PROFILE.to_string());
        fs::create_dir_all(&home)?;
        for dir in ["sessions", "logs", "cache", "skills", "agents"] {
            fs::create_dir_all(home.join(dir))?;
        }
        let config = home.join("config.toml");
        if !config.exists() {
            fs::write(&config, "# Psychevo profile config.\n")?;
        }
        let env_file = home.join(".env");
        if !env_file.exists() {
            fs::write(
                &env_file,
                "# Psychevo live credentials.\n# Keep raw secrets here or in your shell environment, not in config.toml.\n",
            )?;
        }
        crate::profiles::protect_env_file(&env_file)?;
        let paths = managed_paths(&home);
        ensure_managed_dir(&paths)?;
        Ok(Self {
            cwd,
            home,
            profile_name,
            env_map,
            paths,
        })
    }

    pub(super) fn run_options(&self, cwd: PathBuf) -> Result<psychevo_runtime::RunOptions> {
        Ok(psychevo_runtime::RunOptions {
            state: psychevo_runtime::StateRuntime::open(self.home.join("state.db"))?,
            cwd,
            snapshot_root: Some(self.home.join("snapshots")),
            session: None,
            continue_latest: false,
            prompt: String::new(),
            image_inputs: Vec::new(),
            extract_prompt_image_sources: true,
            prompt_display: None,
            max_context_messages: None,
            config_path: None,
            project_context_override: None,
            sandbox_override: None,
            model: None,
            reasoning_effort: None,
            runtime_ref: None,
            runtime_session_id: None,
            runtime_options: std::collections::BTreeMap::new(),
            external_agent_delegate: None,
            include_reasoning: false,
            mode: psychevo_runtime::RunMode::Default,
            permission_mode: None,
            approval_mode: None,
            approval_handler: None,
            clarify_enabled: false,
            inherited_env: Some(self.env_map.clone()),
            agent: None,
            no_agents: false,
            no_skills: false,
            skill_inputs: Vec::new(),
            mcp_servers: Vec::new(),
            runtime_tools: Vec::new(),
        })
    }
}
