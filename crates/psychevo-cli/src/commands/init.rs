use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Result, anyhow};
use psychevo_runtime::state::StateRuntime;

use crate::args::InitArgs;
use crate::commands::gateway::stop_managed_for_home;
use crate::env::{inherited_env, resolve_psychevo_home};
use crate::profiles::protect_env_file;

pub(crate) const STARTER_CONFIG: &str = include_str!("../../templates/starter-config.toml");

pub(crate) const STARTER_ENV: &str = r#"# Psychevo live provider credentials.
# Keep raw API keys here or in your shell environment, not in config.toml.
#
# DEEPSEEK_API_KEY=sk-...
"#;

pub(crate) async fn run_init_command(args: InitArgs) -> Result<ExitCode> {
    let env_map = inherited_env();
    let cwd = env::current_dir()?;
    let home = resolve_psychevo_home(&env_map, &cwd)?;
    let config = home.join("config.toml");
    let env_file = home.join(".env");
    let state = home.join("state.db");
    let sessions = home.join("sessions");
    let logs = home.join("logs");
    let cache = home.join("cache");
    let skills = home.join("skills");
    let agents = home.join("agents");

    fs::create_dir_all(&home)?;
    fs::create_dir_all(&sessions)?;
    fs::create_dir_all(&logs)?;
    fs::create_dir_all(&cache)?;
    fs::create_dir_all(&skills)?;
    fs::create_dir_all(&agents)?;
    if !config.exists() {
        fs::write(&config, STARTER_CONFIG)?;
    }
    if !env_file.exists() {
        fs::write(&env_file, STARTER_ENV)?;
    }
    protect_env_file(&env_file)?;
    if args.reset_state {
        let _ = stop_managed_for_home(&home).await?;
        backup_state_files(&home, &state)?;
    }
    StateRuntime::open(&state)?;

    println!("home: {}", home.display());
    println!("config: {}", config.display());
    println!("env: {}", env_file.display());
    println!("state: {}", state.display());
    println!("sessions: {}", sessions.display());
    println!("logs: {}", logs.display());
    println!("cache: {}", cache.display());
    println!("skills: {}", skills.display());
    println!("agents: {}", agents.display());
    Ok(ExitCode::SUCCESS)
}

pub(crate) fn backup_state_files(home: &Path, state: &Path) -> Result<()> {
    let timestamp_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let backup_dir = home.join("backups").join(format!("state-{timestamp_ms}"));
    let files = [
        state.to_path_buf(),
        PathBuf::from(format!("{}-wal", state.display())),
        PathBuf::from(format!("{}-shm", state.display())),
    ];
    let mut moved_any = false;
    for file in files {
        if !file.exists() {
            continue;
        }
        fs::create_dir_all(&backup_dir)?;
        let name = file
            .file_name()
            .ok_or_else(|| anyhow!("state file has no file name: {}", file.display()))?;
        fs::rename(&file, backup_dir.join(name))?;
        moved_any = true;
    }
    if moved_any {
        println!("state_backup: {}", backup_dir.display());
    }
    Ok(())
}
