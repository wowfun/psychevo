use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Result, anyhow};
use psychevo_runtime::SqliteStore;

use crate::args::InitArgs;
use crate::env::{inherited_env, resolve_psychevo_home};

const STARTER_CONFIG: &str = r#"model = "deepseek/deepseek-chat"

[provider.deepseek.options]
base_url = "https://api.deepseek.com/v1"
api_key_env = "DEEPSEEK_API_KEY"

[provider.deepseek.models.deepseek-chat]
reasoning_effort = "medium"
"#;

const STARTER_ENV: &str = r#"# Psychevo live provider credentials.
# Keep raw API keys here or in your shell environment, not in config.toml.
#
# DEEPSEEK_API_KEY=sk-...
"#;

pub(crate) fn run_init_command(args: InitArgs) -> Result<ExitCode> {
    let env_map = inherited_env();
    let cwd = env::current_dir()?;
    let home = resolve_psychevo_home(&env_map, &cwd)?;
    let config = home.join("config.toml");
    let env_file = home.join(".env");
    let state = home.join("state.db");
    let sessions = home.join("sessions");
    let logs = home.join("logs");
    let cache = home.join("cache");

    fs::create_dir_all(&home)?;
    fs::create_dir_all(&sessions)?;
    fs::create_dir_all(&logs)?;
    fs::create_dir_all(&cache)?;
    if !config.exists() {
        fs::write(&config, STARTER_CONFIG)?;
    }
    if !env_file.exists() {
        fs::write(&env_file, STARTER_ENV)?;
    }
    if args.reset_state {
        backup_state_files(&home, &state)?;
    }
    SqliteStore::open(&state)?;

    println!("home: {}", home.display());
    println!("config: {}", config.display());
    println!("env: {}", env_file.display());
    println!("state: {}", state.display());
    println!("sessions: {}", sessions.display());
    println!("logs: {}", logs.display());
    println!("cache: {}", cache.display());
    Ok(ExitCode::SUCCESS)
}

fn backup_state_files(home: &Path, state: &Path) -> Result<()> {
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
