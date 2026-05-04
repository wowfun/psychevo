use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::io::{self, IsTerminal, Read};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Result, anyhow};
use clap::{Parser, Subcommand};
use psychevo_ai::Outcome;
use psychevo_runtime::{RunOptions, SmokeControl, SmokeOptions, SqliteStore, run_live, run_smoke};

const STARTER_CONFIG: &str = r#"{
  "model": "deepseek/deepseek-chat",
  "provider": {
    "deepseek": {
      "options": {
        "base_url": "https://api.deepseek.com/v1",
        "api_key_env": "DEEPSEEK_API_KEY"
      },
      "models": {
        "deepseek-chat": {
          "reasoning_effort": "medium"
        }
      }
    }
  }
}
"#;

const STARTER_ENV: &str = r#"# Psychevo live provider credentials.
# Keep raw API keys here or in your shell environment, not in config.jsonc.
#
# DEEPSEEK_API_KEY=sk-...
"#;

#[derive(Debug, Parser)]
#[command(name = "pevo")]
#[command(about = "Psychevo command-line entrypoint")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    Init(InitArgs),
    Smoke(SmokeArgs),
    Run(RunArgs),
}

#[derive(Debug, Parser)]
struct InitArgs {
    #[arg(long)]
    reset_state: bool,
}

#[derive(Debug, Parser)]
struct SmokeArgs {
    #[arg(long)]
    db: PathBuf,
    #[arg(long)]
    workdir: PathBuf,
    #[arg(long)]
    session: Option<String>,
    #[arg(long)]
    prompt: Option<String>,
    #[arg(long)]
    max_context_messages: Option<usize>,
    #[arg(long, value_enum, default_value_t = ControlArg::None)]
    control: ControlArg,
    #[arg(long)]
    reset: bool,
}

#[derive(Debug, Parser)]
struct RunArgs {
    #[arg(long = "dir")]
    dir: Option<PathBuf>,
    #[arg(short = 'm', long)]
    model: Option<String>,
    #[arg(long, value_enum)]
    variant: Option<VariantArg>,
    #[arg(short = 's', long, conflicts_with = "continue_latest")]
    session: Option<String>,
    #[arg(short = 'c', long = "continue", conflicts_with = "session")]
    continue_latest: bool,
    #[arg(long, value_enum, default_value_t = RunFormatArg::Default)]
    format: RunFormatArg,
    #[arg(long)]
    include_reasoning: bool,
    #[arg()]
    message: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
#[value(rename_all = "kebab-case")]
enum ControlArg {
    None,
    StopAfterTurn,
    AbortOnAgentStart,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
#[value(rename_all = "kebab-case")]
enum VariantArg {
    None,
    Minimal,
    Low,
    Medium,
    High,
    Xhigh,
    Max,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
#[value(rename_all = "kebab-case")]
enum RunFormatArg {
    Default,
    Json,
}

impl From<ControlArg> for SmokeControl {
    fn from(value: ControlArg) -> Self {
        match value {
            ControlArg::None => SmokeControl::None,
            ControlArg::StopAfterTurn => SmokeControl::StopAfterTurn,
            ControlArg::AbortOnAgentStart => SmokeControl::AbortOnAgentStart,
        }
    }
}

impl VariantArg {
    fn as_str(self) -> &'static str {
        match self {
            VariantArg::None => "none",
            VariantArg::Minimal => "minimal",
            VariantArg::Low => "low",
            VariantArg::Medium => "medium",
            VariantArg::High => "high",
            VariantArg::Xhigh => "xhigh",
            VariantArg::Max => "max",
        }
    }
}

#[tokio::main]
async fn main() -> ExitCode {
    match run().await {
        Ok(code) => code,
        Err(err) => {
            eprintln!("error: {err:#}");
            ExitCode::from(1)
        }
    }
}

async fn run() -> Result<ExitCode> {
    let cli = Cli::parse();
    match cli.command {
        Commands::Init(args) => run_init_command(args),
        Commands::Smoke(args) => run_smoke_command(args).await,
        Commands::Run(args) => run_run_command(args).await,
    }
}

fn run_init_command(args: InitArgs) -> Result<ExitCode> {
    let env_map = inherited_env();
    let cwd = env::current_dir()?;
    let home = resolve_psychevo_home(&env_map, &cwd)?;
    let config = home.join("config.jsonc");
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

async fn run_smoke_command(args: SmokeArgs) -> Result<ExitCode> {
    let result = run_smoke(SmokeOptions {
        db_path: args.db,
        workdir: args.workdir,
        session: args.session,
        prompt: args.prompt,
        max_context_messages: args.max_context_messages,
        control: args.control.into(),
        reset: args.reset,
    })
    .await?;

    println!("session_id: {}", result.session_id);
    println!("outcome: {}", result.outcome.as_str());
    println!("final_answer: {}", result.final_answer);
    println!("db: {}", result.db_path.display());
    println!("workdir: {}", result.workdir.display());

    let success = if let Some(expected) = result.expected_control_outcome {
        result.outcome == expected
    } else {
        result.outcome == Outcome::Normal && result.tool_failures == 0
    };
    Ok(if success {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    })
}

async fn run_run_command(args: RunArgs) -> Result<ExitCode> {
    match run_run_command_inner(&args).await {
        Ok(code) => Ok(code),
        Err(err) if args.format == RunFormatArg::Json => {
            println!(
                "{}",
                serde_json::to_string(&serde_json::json!({
                    "type": "error",
                    "message": format!("{err:#}"),
                }))?
            );
            Ok(ExitCode::from(1))
        }
        Err(err) => Err(err),
    }
}

async fn run_run_command_inner(args: &RunArgs) -> Result<ExitCode> {
    if args.include_reasoning && args.format != RunFormatArg::Json {
        return Err(anyhow!("--include-reasoning requires --format json"));
    }
    let env_map = inherited_env();
    let cwd = env::current_dir()?;
    let home = resolve_psychevo_home(&env_map, &cwd)?;
    let config_path = env_path("PSYCHEVO_CONFIG", &env_map, &cwd)?;
    let db_path = resolve_state_db(&env_map, &home, &cwd)?;
    let bypass_home = config_path.is_some() && env_value("PSYCHEVO_DB", &env_map).is_some();
    if !bypass_home {
        ensure_home_initialized(&home)?;
    }

    let workdir = match &args.dir {
        Some(dir) => resolve_explicit_path(dir, &env_map, &cwd)?,
        None => cwd,
    };
    let prompt = read_prompt(&args.message)?;
    if prompt.trim().is_empty() {
        return Err(anyhow!("You must provide a message"));
    }

    let result = run_live(RunOptions {
        db_path,
        workdir,
        session: args.session.clone(),
        continue_latest: args.continue_latest,
        prompt,
        max_context_messages: None,
        config_path,
        model: args.model.clone(),
        reasoning_effort: args.variant.map(|variant| variant.as_str().to_string()),
        include_reasoning: args.include_reasoning,
        inherited_env: Some(env_map),
    })
    .await?;

    if args.format == RunFormatArg::Json {
        for event in &result.events {
            println!("{}", serde_json::to_string(event)?);
        }
    } else {
        println!("{}", result.final_answer);
    }

    let success = result.outcome == Outcome::Normal && result.tool_failures == 0;
    Ok(if success {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    })
}

fn read_prompt(message: &[String]) -> Result<String> {
    let mut prompt = message.join(" ");
    if !io::stdin().is_terminal() {
        let mut stdin = String::new();
        io::stdin().read_to_string(&mut stdin)?;
        if !stdin.is_empty() {
            if prompt.is_empty() {
                prompt = stdin;
            } else {
                prompt.push('\n');
                prompt.push_str(&stdin);
            }
        }
    }
    Ok(prompt)
}

fn ensure_home_initialized(home: &Path) -> Result<()> {
    let config = home.join("config.jsonc");
    if !config.exists() {
        return Err(anyhow!(
            "Psychevo home is not initialized; run `pevo init` to create {}",
            config.display()
        ));
    }
    Ok(())
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

fn resolve_state_db(
    env_map: &BTreeMap<String, String>,
    home: &Path,
    cwd: &Path,
) -> Result<PathBuf> {
    if let Some(value) = env_value("PSYCHEVO_DB", env_map) {
        if value == ":memory:" {
            Ok(PathBuf::from(value))
        } else {
            resolve_explicit_path(Path::new(&value), env_map, cwd)
        }
    } else {
        Ok(home.join("state.db"))
    }
}

fn resolve_psychevo_home(env_map: &BTreeMap<String, String>, cwd: &Path) -> Result<PathBuf> {
    if let Some(value) = env_value("PSYCHEVO_HOME", env_map) {
        resolve_explicit_path(Path::new(&value), env_map, cwd)
    } else {
        resolve_explicit_path(Path::new("~/.psychevo"), env_map, cwd)
    }
}

fn env_path(name: &str, env_map: &BTreeMap<String, String>, cwd: &Path) -> Result<Option<PathBuf>> {
    env_value(name, env_map)
        .map(|value| resolve_explicit_path(Path::new(&value), env_map, cwd))
        .transpose()
}

fn resolve_explicit_path(
    path: &Path,
    env_map: &BTreeMap<String, String>,
    cwd: &Path,
) -> Result<PathBuf> {
    let expanded = expand_tilde(path, env_map)?;
    if expanded.is_absolute() {
        Ok(expanded)
    } else {
        Ok(cwd.join(expanded))
    }
}

fn expand_tilde(path: &Path, env_map: &BTreeMap<String, String>) -> Result<PathBuf> {
    let raw = path.to_string_lossy();
    if raw == "~" {
        return home_path(env_map);
    }
    if let Some(rest) = raw.strip_prefix("~/") {
        return Ok(home_path(env_map)?.join(rest));
    }
    Ok(path.to_path_buf())
}

fn home_path(env_map: &BTreeMap<String, String>) -> Result<PathBuf> {
    env_value("HOME", env_map)
        .map(PathBuf::from)
        .ok_or_else(|| anyhow!("HOME is required to expand ~"))
}

fn env_value(name: &str, env_map: &BTreeMap<String, String>) -> Option<String> {
    env_map
        .get(name)
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn inherited_env() -> BTreeMap<String, String> {
    env::vars().collect()
}
