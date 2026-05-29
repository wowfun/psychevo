#[allow(unused_imports)]
pub(crate) use super::*;

#[allow(unused_imports)]
pub(crate) use super::*;
#[allow(unused_imports)]
use crate::*;

#[allow(unused_imports)]
use anyhow::{Context, Result, bail};
#[allow(unused_imports)]
use clap::{Parser, Subcommand, ValueEnum};
#[allow(unused_imports)]
use serde_json::{Value, json};
#[allow(unused_imports)]
use std::collections::{BTreeMap, BTreeSet};
#[allow(unused_imports)]
use std::env;
#[allow(unused_imports)]
use std::ffi::OsString;
#[allow(unused_imports)]
use std::fs;
#[allow(unused_imports)]
use std::io::{BufRead, BufReader};
#[allow(unused_imports)]
use std::path::{Component, Path, PathBuf};
#[allow(unused_imports)]
use std::process::{Command, Stdio};
#[allow(unused_imports)]
use std::thread;
#[allow(unused_imports)]
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
#[allow(unused_imports)]
use uuid::Uuid;

pub(crate) fn looks_like_relative_path(value: &str) -> bool {
    if value.starts_with('{') || value.contains('\n') {
        return false;
    }
    let path = Path::new(value);
    !path.is_absolute()
        && (value.starts_with("./")
            || value.starts_with("../")
            || value.contains('/')
            || value.contains('\\'))
}

pub(crate) fn is_declared_path(value: &str, task_dir: &Path) -> bool {
    task_dir.join(value).exists() || looks_like_relative_path(value)
}

pub(crate) fn sanitize_id(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                ch
            } else {
                '_'
            }
        })
        .collect()
}

pub(crate) fn slugify(value: &str) -> String {
    let slug = sanitize_id(&value.to_ascii_lowercase())
        .trim_matches('_')
        .to_string();
    if slug.is_empty() {
        "evaluation".to_string()
    } else {
        slug
    }
}

pub(crate) fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

pub(crate) fn reject_unsupported(schema_version: u32, path: &Path) -> Result<()> {
    if schema_version != MANIFEST_SCHEMA_VERSION {
        bail!(
            "{} uses unsupported schema_version {}; supported schema_version is {}. v4 benchmark, eval, and task manifests are no longer supported; see docs/evaluation/authoring.md for v5 authoring.",
            path.display(),
            schema_version,
            MANIFEST_SCHEMA_VERSION
        );
    }
    Ok(())
}

pub(crate) fn reject_unsupported_artifact(schema_version: u32, path: &Path) -> Result<()> {
    if schema_version != ARTIFACT_SCHEMA_VERSION {
        bail!(
            "{} uses unsupported artifact schema_version {}; supported artifact schema_version is {}",
            path.display(),
            schema_version,
            ARTIFACT_SCHEMA_VERSION
        );
    }
    Ok(())
}

pub(crate) fn reject_unsupported_index(schema_version: u32, path: &Path) -> Result<()> {
    if schema_version != INDEX_SCHEMA_VERSION {
        bail!(
            "{} uses unsupported index schema_version {}; supported index schema_version is {}",
            path.display(),
            schema_version,
            INDEX_SCHEMA_VERSION
        );
    }
    Ok(())
}

pub(crate) fn reject_unsupported_workspace(schema_version: u32, path: &Path) -> Result<()> {
    if schema_version != WORKSPACE_SCHEMA_VERSION {
        bail!(
            "{} uses unsupported workspace schema_version {}; supported workspace schema_version is {}",
            path.display(),
            schema_version,
            WORKSPACE_SCHEMA_VERSION
        );
    }
    Ok(())
}

#[derive(Debug)]
pub struct CliOutcome {
    pub code: u8,
    pub stdout: String,
    pub stderr: String,
}

#[derive(Debug, Parser)]
#[command(name = "peval")]
#[command(about = "Run local Psychevo evaluation task sets")]
pub(crate) struct Cli {
    #[command(subcommand)]
    pub(crate) command: Commands,
}

#[derive(Debug, Subcommand)]
pub(crate) enum Commands {
    #[command(about = "Initialize a peval workspace")]
    Init(InitArgs),
    #[command(subcommand, hide = true, about = "Removed project registry commands")]
    Project(ProjectCommands),
    #[command(about = "Inspect local evaluation readiness")]
    Doctor(ProjectArgs),
    #[command(about = "List task sets, agents, tasks, view formats, and datasets")]
    List(ListArgs),
    #[command(about = "Validate evaluation manifests without executing cases")]
    Check(SelectArgs),
    #[command(about = "Run an evaluation matrix and write artifacts")]
    Run(RunArgs),
    #[command(about = "Render dynamic views over stored cell artifacts")]
    View(ViewArgs),
    #[command(about = "Serve the local peval workspace viewer")]
    Serve(ServeArgs),
    #[command(subcommand, about = "Manage local evaluation datasets")]
    Dataset(DatasetCommands),
}

#[derive(Debug, Parser)]
pub(crate) struct InitArgs {
    #[arg(short = 'r', long = "root", value_name = "DIR")]
    pub(crate) root: Option<PathBuf>,
    #[arg(long = "default")]
    pub(crate) make_default: bool,
    #[arg(long)]
    pub(crate) force: bool,
    #[arg(long)]
    pub(crate) json: bool,
}

#[derive(Debug, Subcommand)]
pub(crate) enum ProjectCommands {
    #[command(about = "Removed project registry command")]
    Add(ProjectAddArgs),
    #[command(about = "Removed project registry command")]
    List(ProjectListArgs),
    #[command(name = "rm", about = "Removed project registry command")]
    Remove(ProjectRemoveArgs),
}

#[derive(Debug, Parser)]
pub(crate) struct ProjectAddArgs {
    #[arg(short = 'r', long = "root", value_name = "DIR")]
    pub(crate) store_root: Option<PathBuf>,
    #[arg(short = 'c', long = "config", value_name = "PATH")]
    pub(crate) config: PathBuf,
    #[arg(long)]
    pub(crate) id: Option<String>,
    #[arg(long)]
    pub(crate) force: bool,
    #[arg(long)]
    pub(crate) json: bool,
}

#[derive(Debug, Parser)]
pub(crate) struct ProjectListArgs {
    #[arg(short = 'r', long = "root", value_name = "DIR")]
    pub(crate) store_root: Option<PathBuf>,
    #[arg(long)]
    pub(crate) json: bool,
}

#[derive(Debug, Parser)]
pub(crate) struct ProjectRemoveArgs {
    pub(crate) id: String,
    #[arg(short = 'r', long = "root", value_name = "DIR")]
    pub(crate) store_root: Option<PathBuf>,
    #[arg(long)]
    pub(crate) json: bool,
}

#[derive(Debug, Subcommand)]
pub(crate) enum DatasetCommands {
    #[command(about = "Register a local dataset payload")]
    Import(DatasetImportArgs),
}

#[derive(Debug, Parser)]
pub(crate) struct ProjectArgs {
    #[arg(short = 'c', long = "config", value_name = "PATH")]
    pub(crate) config: Option<PathBuf>,
    #[arg(long = "benchmark", value_name = "ID_OR_PATH")]
    pub(crate) benchmark: Option<String>,
    #[arg(short = 'r', long = "root", value_name = "DIR")]
    pub(crate) store_root: Option<PathBuf>,
    #[arg(long)]
    pub(crate) json: bool,
}

#[derive(Debug, Parser)]
pub(crate) struct ListArgs {
    #[arg(short = 'c', long = "config", value_name = "PATH")]
    pub(crate) config: Option<PathBuf>,
    #[arg(long = "benchmark", value_name = "ID_OR_PATH")]
    pub(crate) benchmark: Option<String>,
    #[arg(short = 'r', long = "root", value_name = "DIR")]
    pub(crate) store_root: Option<PathBuf>,
    #[arg(long, value_enum, default_value_t = ListKind::All)]
    pub(crate) kind: ListKind,
    #[arg(long)]
    pub(crate) json: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub(crate) enum ListKind {
    All,
    TaskSets,
    Agents,
    Benchmarks,
    Tasks,
    Views,
    Datasets,
}

#[derive(Debug, Parser)]
pub(crate) struct SelectArgs {
    #[arg(short = 'c', long = "config", value_name = "PATH")]
    pub(crate) config: Option<PathBuf>,
    #[arg(long = "benchmark", value_name = "ID_OR_PATH")]
    pub(crate) benchmark: Option<String>,
    #[arg(short = 'r', long = "root", value_name = "DIR")]
    pub(crate) store_root: Option<PathBuf>,
    #[arg(long = "task-set")]
    pub(crate) task_set: Option<String>,
    #[arg(long)]
    pub(crate) agent: Option<String>,
    #[arg(long)]
    pub(crate) task: Option<String>,
    #[arg(long)]
    pub(crate) live: bool,
    #[arg(long)]
    pub(crate) json: bool,
}

#[derive(Debug, Parser)]
pub(crate) struct RunArgs {
    #[arg(short = 'c', long = "config", value_name = "PATH")]
    pub(crate) config: Option<PathBuf>,
    #[arg(long = "benchmark", value_name = "ID_OR_PATH")]
    pub(crate) benchmark: Option<String>,
    #[arg(short = 'r', long = "root", value_name = "DIR")]
    pub(crate) store_root: Option<PathBuf>,
    #[arg(long = "task-set")]
    pub(crate) task_set: Option<String>,
    #[arg(long)]
    pub(crate) agent: Option<String>,
    #[arg(long)]
    pub(crate) task: Option<String>,
    #[arg(long)]
    pub(crate) overwrite: bool,
    #[arg(long, value_name = "DIR")]
    pub(crate) output_root: Option<PathBuf>,
    #[arg(long = "include", value_name = "ITEMS")]
    pub(crate) include: Vec<String>,
    #[arg(long)]
    pub(crate) json: bool,
}

#[derive(Debug, Parser)]
pub(crate) struct ViewArgs {
    #[arg(short = 'c', long = "config", value_name = "PATH")]
    pub(crate) config: Option<PathBuf>,
    #[arg(long = "benchmark", value_name = "ID_OR_PATH")]
    pub(crate) benchmark: Option<String>,
    #[arg(long = "report", value_name = "KEY")]
    pub(crate) report: Option<String>,
    #[arg(short = 'r', long = "root", value_name = "DIR")]
    pub(crate) store_root: Option<PathBuf>,
    #[arg(long, value_name = "PATH")]
    pub(crate) path: Option<PathBuf>,
    #[arg(long = "task-set")]
    pub(crate) task_set: Option<String>,
    #[arg(long)]
    pub(crate) agent: Option<String>,
    #[arg(long)]
    pub(crate) task: Option<String>,
    #[arg(long, value_enum)]
    pub(crate) status: Option<CaseStatusFilter>,
    #[arg(long = "group-by", value_name = "ITEMS")]
    pub(crate) group_by: Vec<String>,
    #[arg(short = 'i', long = "include", value_name = "ITEMS")]
    pub(crate) include: Vec<String>,
    #[arg(long, value_enum)]
    pub(crate) format: Option<ViewFormat>,
    #[arg(short = 'o', long, value_name = "PATH", num_args = 0..=1)]
    pub(crate) output: Option<Option<PathBuf>>,
}

#[derive(Debug, Parser)]
pub(crate) struct ServeArgs {
    #[arg(short = 'c', long = "config", value_name = "PATH")]
    pub(crate) config: Option<PathBuf>,
    #[arg(long = "benchmark", value_name = "ID_OR_PATH")]
    pub(crate) benchmark: Option<String>,
    #[arg(long = "report", value_name = "KEY")]
    pub(crate) report: Option<String>,
    #[arg(short = 'r', long = "root", value_name = "DIR")]
    pub(crate) store_root: Option<PathBuf>,
    #[arg(long, value_name = "PATH")]
    pub(crate) path: Option<PathBuf>,
    #[arg(long = "task-set")]
    pub(crate) task_set: Option<String>,
    #[arg(long)]
    pub(crate) agent: Option<String>,
    #[arg(long)]
    pub(crate) task: Option<String>,
    #[arg(long, value_enum)]
    pub(crate) status: Option<CaseStatusFilter>,
    #[arg(long, default_value = "127.0.0.1")]
    pub(crate) host: std::net::IpAddr,
    #[arg(long, default_value_t = 0)]
    pub(crate) port: u16,
}

#[derive(Debug, Parser)]
pub(crate) struct DatasetImportArgs {
    #[arg(value_name = "PATH")]
    pub(crate) path: PathBuf,
    #[arg(short = 'r', long = "root", value_name = "DIR")]
    pub(crate) store_root: Option<PathBuf>,
    #[arg(long)]
    pub(crate) id: Option<String>,
    #[arg(long)]
    pub(crate) name: Option<String>,
    #[arg(long)]
    pub(crate) kind: Option<String>,
    #[arg(long)]
    pub(crate) loader: Option<String>,
    #[arg(long)]
    pub(crate) split: Option<String>,
    #[arg(long)]
    pub(crate) sample_limit: Option<usize>,
    #[arg(long)]
    pub(crate) cache_key: Option<String>,
    #[arg(long)]
    pub(crate) license: Option<String>,
    #[arg(long = "tag")]
    pub(crate) tags: Vec<String>,
    #[arg(long)]
    pub(crate) notes: Option<String>,
    #[arg(long)]
    pub(crate) json: bool,
}

pub fn run_cli_from<I, T>(args: I) -> CliOutcome
where
    I: IntoIterator<Item = T>,
    T: Into<OsString> + Clone,
{
    match Cli::try_parse_from(args) {
        Ok(cli) => {
            let json_errors = command_wants_json(&cli.command);
            match dispatch_cli(cli) {
                Ok(outcome) => outcome,
                Err(err) if json_errors => {
                    let diagnostic = EvalDiagnostic::from_error(err);
                    CliOutcome {
                        code: 1,
                        stdout: String::new(),
                        stderr: format!(
                            "{}\n",
                            serde_json::to_string_pretty(&diagnostic)
                                .unwrap_or_else(|_| "{\"code\":\"peval_error\"}".to_string())
                        ),
                    }
                }
                Err(err) => CliOutcome {
                    code: 1,
                    stdout: String::new(),
                    stderr: format!("error: {err:#}\n"),
                },
            }
        }
        Err(err) => CliOutcome {
            code: if err.use_stderr() { 2 } else { 0 },
            stdout: if err.use_stderr() {
                String::new()
            } else {
                err.to_string()
            },
            stderr: if err.use_stderr() {
                err.to_string()
            } else {
                String::new()
            },
        },
    }
}

pub(crate) fn command_wants_json(command: &Commands) -> bool {
    match command {
        Commands::Init(args) => args.json,
        Commands::Project(ProjectCommands::Add(args)) => args.json,
        Commands::Project(ProjectCommands::List(args)) => args.json,
        Commands::Project(ProjectCommands::Remove(args)) => args.json,
        Commands::Doctor(args) => args.json,
        Commands::List(args) => args.json,
        Commands::Check(args) => args.json,
        Commands::Run(args) => args.json,
        Commands::View(args) => effective_view_format(
            args.format,
            args.output.as_ref().and_then(|output| output.as_deref()),
            matches!(args.output, Some(None)),
        )
        .is_ok_and(|format| format == ViewFormat::Json),
        Commands::Serve(_) => false,
        Commands::Dataset(DatasetCommands::Import(args)) => args.json,
    }
}

pub(crate) fn dispatch_cli(cli: Cli) -> Result<CliOutcome> {
    match cli.command {
        Commands::Init(args) => run_init(args),
        Commands::Project(args) => run_project(args),
        Commands::Doctor(args) => run_doctor(args),
        Commands::List(args) => run_list(args),
        Commands::Check(args) => run_check(args),
        Commands::Run(args) => run_run(args),
        Commands::View(args) => run_view(args),
        Commands::Serve(args) => run_serve_command(args),
        Commands::Dataset(args) => run_dataset(args),
    }
}

pub(crate) fn process_service() -> Result<EvalService> {
    Ok(EvalService::new(ServiceContext::from_process()?))
}

pub(crate) fn run_init(args: InitArgs) -> Result<CliOutcome> {
    let service = process_service()?;
    let config = service
        .init(InitStoreRequest {
            root: args.root,
            make_default: args.make_default,
            force: args.force,
        })
        .map_err(anyhow::Error::new)?;
    if args.json {
        return Ok(success(serde_json::to_string_pretty(&config)?));
    }
    Ok(success(format!(
        "peval workspace: {}\ndefault workspace: {}\n",
        config.root.display(),
        config.default_workspace
    )))
}

pub(crate) fn run_project(args: ProjectCommands) -> Result<CliOutcome> {
    let wants_json = match &args {
        ProjectCommands::Add(args) => args.json,
        ProjectCommands::List(args) => args.json,
        ProjectCommands::Remove(args) => args.json,
    };
    let message = "`peval project` is removed; use `--config <eval-config.toml>`, `--benchmark <id-or-path>`, and agent/benchmark registries in eval, workspace, or user config files";
    if wants_json {
        let diagnostic = EvalDiagnostic::error("removed_command", message);
        return Ok(CliOutcome {
            code: 1,
            stdout: String::new(),
            stderr: format!("{}\n", serde_json::to_string_pretty(&diagnostic)?),
        });
    }
    bail!("{message}")
}

pub(crate) fn run_doctor(args: ProjectArgs) -> Result<CliOutcome> {
    let service = process_service()?;
    let project = service
        .load_project(
            args.config.as_deref(),
            args.benchmark.as_deref(),
            args.store_root.clone(),
        )
        .map_err(anyhow::Error::new)?;
    let store = service.store(args.store_root).map_err(anyhow::Error::new)?;
    let payload = json!({
        "schema_version": SCHEMA_VERSION,
        "eval": &project.name,
        "benchmark": &project.benchmark_id,
        "root": &project.benchmark_root,
        "eval_root": &store.root,
        "agents": project.agents.len(),
        "sets": project.task_sets.len(),
        "fake_adapter": "available",
        "command_adapter": "available",
        "acp_adapter": "wrapper",
        "psychevo_adapter": "wrapper",
        "opencode_adapter": "wrapper",
        "hermes_adapter": "wrapper",
        "views": ["html", "markdown", "json"],
    });
    if args.json {
        return Ok(success(serde_json::to_string_pretty(&payload)?));
    }
    Ok(success(format!(
        "eval: {}\nbenchmark: {}\nroot: {}\neval root: {}\nagents: {}\ntask sets: {}\nfake adapter: available\npsychevo adapter: wrapper\nopencode adapter: wrapper\nhermes adapter: wrapper\n",
        project.name,
        project.benchmark_id,
        project.benchmark_root.display(),
        store.root.display(),
        project.agents.len(),
        project.task_sets.len(),
    )))
}

pub(crate) fn run_list(args: ListArgs) -> Result<CliOutcome> {
    let service = process_service()?;
    let needs_project = matches!(
        args.kind,
        ListKind::All | ListKind::TaskSets | ListKind::Tasks
    );
    let project = if needs_project {
        Some(
            service
                .load_project(
                    args.config.as_deref(),
                    args.benchmark.as_deref(),
                    args.store_root.clone(),
                )
                .map_err(anyhow::Error::new)?,
        )
    } else {
        service
            .try_load_project(
                args.config.as_deref(),
                args.benchmark.as_deref(),
                args.store_root.clone(),
            )
            .map_err(anyhow::Error::new)?
    };
    let needs_store = matches!(args.kind, ListKind::All | ListKind::Datasets);
    let store = if needs_store {
        Some(
            service
                .store(args.store_root.clone())
                .map_err(anyhow::Error::new)?,
        )
    } else {
        None
    };
    let tasks = project
        .as_ref()
        .map(list_tasks)
        .transpose()?
        .unwrap_or_default();
    let datasets = if needs_store {
        service
            .list_datasets(args.store_root.clone())
            .map_err(anyhow::Error::new)?
    } else {
        Vec::new()
    };
    let eval_root = store.as_ref().map(|store| store.root.clone());
    let registry_agents = if let Some(project) = project.as_ref() {
        project.agents.values().cloned().collect::<Vec<_>>()
    } else {
        list_registry_agents(args.store_root.clone())?
    };
    let registry_benchmarks = list_registry_benchmarks(args.store_root.clone())?;
    let payload = json!({
        "schema_version": SCHEMA_VERSION,
        "eval_root": eval_root,
        "benchmarks": registry_benchmarks,
        "sets": project.as_ref().map(|project| project.task_sets.values().map(|task_set| json!({
            "id": &task_set.id,
            "name": &task_set.name,
            "tasks": &task_set.tasks,
        })).collect::<Vec<_>>()).unwrap_or_default(),
        "agents": registry_agents.iter().map(|agent| json!({
            "id": &agent.id,
            "name": &agent.name,
            "kind": agent.kind,
        })).collect::<Vec<_>>(),
        "tasks": tasks,
        "views": ["html", "markdown", "json"],
        "datasets": datasets,
    });
    if args.json {
        return Ok(success(serde_json::to_string_pretty(&payload)?));
    }
    let mut out = String::new();
    if matches!(args.kind, ListKind::All | ListKind::TaskSets) {
        let project = project.as_ref().context("list kind requires eval config")?;
        out.push_str("task sets\n");
        for task_set in project.task_sets.values() {
            out.push_str(&format!("- {}\n", task_set.id));
        }
    }
    if matches!(args.kind, ListKind::All | ListKind::Agents) {
        out.push_str("agents\n");
        for agent in &registry_agents {
            out.push_str(&format!("- {} ({:?})\n", agent.id, agent.kind));
        }
    }
    if matches!(args.kind, ListKind::All | ListKind::Benchmarks) {
        out.push_str("benchmarks\n");
        for benchmark in list_registry_benchmarks(args.store_root.clone())? {
            out.push_str(&format!(
                "- {} {}\n",
                benchmark["id"].as_str().unwrap_or("unknown"),
                benchmark["path"].as_str().unwrap_or("")
            ));
        }
    }
    if matches!(args.kind, ListKind::All | ListKind::Tasks) {
        let project = project.as_ref().context("list kind requires eval config")?;
        out.push_str("tasks\n");
        for task in list_tasks(project)? {
            out.push_str(&format!("- {}\n", task["id"].as_str().unwrap_or("unknown")));
        }
    }
    if matches!(args.kind, ListKind::All | ListKind::Views) {
        out.push_str("views\n- html\n- markdown\n- json\n");
    }
    if matches!(args.kind, ListKind::All | ListKind::Datasets) {
        let _store = store.as_ref().context("list kind requires peval root")?;
        out.push_str("datasets\n");
        for dataset in &datasets {
            out.push_str(&format!(
                "- {} ({}) payload={} exists={}\n",
                dataset.id,
                dataset.kind,
                dataset.payload.display(),
                dataset.payload_exists
            ));
        }
    }
    Ok(success(out))
}

pub(crate) fn run_check(args: SelectArgs) -> Result<CliOutcome> {
    let service = process_service()?;
    validate_direct_benchmark_selection(
        args.benchmark.as_deref(),
        args.agent.as_deref(),
        args.task_set.as_deref(),
        args.task.as_deref(),
    )?;
    let project = service
        .load_project(
            args.config.as_deref(),
            args.benchmark.as_deref(),
            args.store_root,
        )
        .map_err(anyhow::Error::new)?;
    let cases = service
        .check(
            &project,
            args.task_set.as_deref(),
            args.task.as_deref(),
            args.agent.as_deref(),
        )
        .map_err(anyhow::Error::new)?;
    let payload = json!({
        "schema_version": SCHEMA_VERSION,
        "eval": project.name,
        "benchmark": project.benchmark_id,
        "cases": cases.len(),
        "live": args.live,
        "status": "ok",
    });
    if args.json {
        return Ok(success(serde_json::to_string_pretty(&payload)?));
    }
    Ok(success(format!("check ok: {} case(s)\n", cases.len())))
}

pub(crate) fn run_run(args: RunArgs) -> Result<CliOutcome> {
    let service = process_service()?;
    let summary = service
        .run(RunRequest {
            config: args.config,
            benchmark: args.benchmark,
            task_set: args.task_set,
            task: args.task,
            agent: args.agent,
            overwrite: args.overwrite,
            store_root: args.store_root,
            output_root: args.output_root,
            include_artifacts: args.include,
        })
        .map_err(anyhow::Error::new)?;
    let code = if summary.status == RunStatus::Passed {
        0
    } else {
        1
    };
    if args.json {
        return Ok(CliOutcome {
            code,
            stdout: serde_json::to_string_pretty(&summary)?,
            stderr: String::new(),
        });
    }
    Ok(CliOutcome {
        code,
        stdout: format!(
            "run {:?}\nbenchmark: {}\ncells: {} selected / {} executed / {} reused / {} overwritten / {} retried\nresults: {} passed / {} failed\n",
            summary.status,
            summary.benchmark,
            summary.selected_cells,
            summary.executed_cells,
            summary.reused_cells,
            summary.overwritten_cells,
            summary.retried_cells,
            summary.passed_cells,
            summary.failed_cells,
        ),
        stderr: String::new(),
    })
}

pub(crate) fn run_view(args: ViewArgs) -> Result<CliOutcome> {
    let service = process_service()?;
    let output = args.output.clone();
    let format = effective_view_format(
        args.format,
        output.as_ref().and_then(|output| output.as_deref()),
        matches!(output, Some(None)),
    )?;
    let view = service
        .view(ViewRequest {
            config: args.config,
            benchmark: args.benchmark,
            report: args.report,
            store_root: args.store_root,
            path: args.path,
            task_set: args.task_set,
            agent: args.agent,
            task: args.task,
            status: args.status,
            group_by: parse_view_groups(&args.group_by)?,
            include: parse_view_includes(&args.include)?,
        })
        .map_err(anyhow::Error::new)?;
    let rendered = render_view(&view, format)?;
    let output = match output {
        Some(Some(path)) => Some(path),
        Some(None) => Some(default_view_output_path(&view, format)?),
        None => None,
    };
    if let Some(output) = output {
        if let Some(parent) = output
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }
        fs::write(&output, rendered.as_bytes())
            .with_context(|| format!("failed to write {}", output.display()))?;
        Ok(success(format!("wrote {}\n", output.display())))
    } else {
        Ok(success(rendered))
    }
}

pub(crate) fn run_serve_command(args: ServeArgs) -> Result<CliOutcome> {
    let service = process_service()?;
    run_serve_blocking(
        service,
        ServeOptions {
            config: args.config,
            benchmark: args.benchmark,
            report: args.report,
            store_root: args.store_root,
            path: args.path,
            task_set: args.task_set,
            agent: args.agent,
            task: args.task,
            status: args.status,
            host: args.host,
            port: args.port,
        },
    )?;
    Ok(success(String::new()))
}

pub(crate) fn run_dataset(args: DatasetCommands) -> Result<CliOutcome> {
    match args {
        DatasetCommands::Import(args) => {
            let service = process_service()?;
            let entry = service
                .dataset_import(DatasetImportRequest {
                    store_root: args.store_root,
                    path: args.path,
                    id: args.id,
                    name: args.name,
                    kind: args.kind,
                    loader: args.loader,
                    split: args.split,
                    sample_limit: args.sample_limit,
                    cache_key: args.cache_key,
                    license: args.license,
                    tags: args.tags,
                    notes: args.notes,
                })
                .map_err(anyhow::Error::new)?;
            if args.json {
                Ok(success(serde_json::to_string_pretty(&entry)?))
            } else {
                Ok(success(format!(
                    "dataset {}: {}\npayload: {}\npayload exists: {}\n",
                    entry.id,
                    entry.kind,
                    entry.payload.display(),
                    entry.payload_exists
                )))
            }
        }
    }
}

pub(crate) fn parse_view_includes(values: &[String]) -> Result<Vec<ViewInclude>> {
    let mut includes = Vec::new();
    for value in values {
        for item in value
            .split(',')
            .map(str::trim)
            .filter(|item| !item.is_empty())
        {
            if item.eq_ignore_ascii_case("all") {
                includes.extend(all_view_includes());
            } else {
                let include = ViewInclude::from_str(item, true)
                    .map_err(|err| anyhow::anyhow!("invalid view include `{item}`: {err}"))?;
                includes.push(include);
            }
        }
    }
    Ok(includes
        .into_iter()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect())
}

pub(crate) fn parse_view_groups(values: &[String]) -> Result<Vec<ViewGroupBy>> {
    let mut groups = Vec::new();
    for value in values {
        for item in value
            .split(',')
            .map(str::trim)
            .filter(|item| !item.is_empty())
        {
            let group = ViewGroupBy::from_str(item, true)
                .map_err(|err| anyhow::anyhow!("invalid view group `{item}`: {err}"))?;
            groups.push(group);
        }
    }
    Ok(groups
        .into_iter()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect())
}

pub(crate) fn effective_view_format(
    explicit: Option<ViewFormat>,
    output: Option<&Path>,
    default_output: bool,
) -> Result<ViewFormat> {
    if let Some(format) = explicit {
        return Ok(format);
    }
    let Some(output) = output else {
        return Ok(if default_output {
            ViewFormat::Html
        } else {
            ViewFormat::Markdown
        });
    };
    match output
        .extension()
        .and_then(|value| value.to_str())
        .map(|value| value.to_ascii_lowercase())
        .as_deref()
    {
        Some("json") => Ok(ViewFormat::Json),
        Some("html") | Some("htm") => Ok(ViewFormat::Html),
        Some("md") | Some("markdown") => Ok(ViewFormat::Markdown),
        _ => Ok(ViewFormat::Markdown),
    }
}

pub(crate) fn default_view_output_path(view: &ViewReport, format: ViewFormat) -> Result<PathBuf> {
    let runs_root = view.scope.workspace_root.join("runs");
    let relative_scope = view.scope.path.strip_prefix(&runs_root).with_context(|| {
        format!(
            "default view output requires a scope under {}; pass -o PATH for external paths",
            runs_root.display()
        )
    })?;
    if relative_scope.components().any(|component| {
        matches!(
            component,
            Component::ParentDir | Component::RootDir | Component::Prefix(_)
        )
    }) {
        bail!(
            "default view output requires a normalized scope under {}; pass -o PATH",
            runs_root.display()
        );
    }
    let mut output = view.scope.workspace_root.join("views");
    if !relative_scope.as_os_str().is_empty() {
        output = output.join(relative_scope);
    }
    Ok(output.join(format!("index.{}", view_format_extension(format))))
}

pub(crate) fn view_format_extension(format: ViewFormat) -> &'static str {
    match format {
        ViewFormat::Json => "json",
        ViewFormat::Markdown => "md",
        ViewFormat::Html => "html",
    }
}

pub(crate) fn success(stdout: String) -> CliOutcome {
    CliOutcome {
        code: 0,
        stdout,
        stderr: String::new(),
    }
}

pub(crate) fn list_tasks(project: &EvalProject) -> Result<Vec<Value>> {
    let mut tasks = Vec::new();
    let mut seen = BTreeSet::new();
    for task_set in project.task_sets.values() {
        for task in load_task_set_tasks(project, task_set, None)? {
            if seen.insert(task.id.clone()) {
                tasks.push(json!({
                    "id": task.id,
                    "name": task.name,
                    "kind": task.kind,
                    "manifest": task.manifest_path,
                }));
            }
        }
    }
    Ok(tasks)
}

pub(crate) fn resolved_registry_for_cli(store_root: Option<PathBuf>) -> Result<ResolvedRegistry> {
    let env_map = inherited_env();
    let cwd = env::current_dir()?;
    let home = resolve_psychevo_home(&env_map, &cwd)?;
    let store = resolve_optional_store(store_root)?;
    ResolvedRegistry::load(
        None,
        store.as_ref().map(|store| store.root.as_path()),
        &home,
    )
}

pub(crate) fn list_registry_agents(store_root: Option<PathBuf>) -> Result<Vec<AgentManifest>> {
    Ok(resolved_registry_for_cli(store_root)?
        .agents
        .into_values()
        .collect())
}

pub(crate) fn list_registry_benchmarks(store_root: Option<PathBuf>) -> Result<Vec<Value>> {
    Ok(resolved_registry_for_cli(store_root)?
        .benchmarks
        .into_values()
        .map(|benchmark| {
            json!({
                "id": benchmark.id,
                "name": benchmark.name,
                "path": benchmark.path,
                "path_exists": benchmark.path.is_file(),
            })
        })
        .collect())
}

#[cfg(test)]
pub(crate) mod tests {
    pub(crate) use super::*;

    pub(crate) mod project_lifecycle;
}
