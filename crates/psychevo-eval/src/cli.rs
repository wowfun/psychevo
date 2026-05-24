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

pub(crate) fn generate_run_id() -> String {
    Uuid::now_v7().to_string()
}

pub(crate) fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

pub(crate) fn reject_unsupported(schema_version: u32, path: &Path) -> Result<()> {
    if schema_version != SCHEMA_VERSION {
        bail!(
            "{} uses unsupported schema_version {}; supported schema_version is {}",
            path.display(),
            schema_version,
            SCHEMA_VERSION
        );
    }
    Ok(())
}

pub(crate) fn reject_unsupported_result_schema(schema_version: u32) -> Result<()> {
    if schema_version != SCHEMA_VERSION {
        bail!(
            "scorer returned unsupported schema_version {}; supported schema_version is {}",
            schema_version,
            SCHEMA_VERSION
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
#[command(about = "Run local Psychevo evaluation suites")]
pub(crate) struct Cli {
    #[command(subcommand)]
    pub(crate) command: Commands,
}

#[derive(Debug, Subcommand)]
pub(crate) enum Commands {
    #[command(about = "Initialize the user-level peval store")]
    Init(InitArgs),
    #[command(about = "Inspect local evaluation readiness")]
    Doctor(ProjectArgs),
    #[command(about = "List suites, agents, tasks, and report formats")]
    List(ListArgs),
    #[command(about = "Validate evaluation manifests without executing cases")]
    Check(SelectArgs),
    #[command(about = "Run an evaluation matrix and write artifacts")]
    Run(RunArgs),
    #[command(about = "Render a report from existing run artifacts")]
    Report(ReportArgs),
    #[command(about = "Compare existing run artifact roots")]
    Compare(CompareArgs),
    #[command(about = "Replay stored trajectory events")]
    Replay(ReplayArgs),
    #[command(subcommand, about = "Manage local evaluation datasets")]
    Dataset(DatasetCommands),
}

#[derive(Debug, Parser)]
pub(crate) struct InitArgs {
    #[arg(long = "root", value_name = "DIR")]
    pub(crate) root: Option<PathBuf>,
    #[arg(long)]
    pub(crate) force: bool,
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
    #[arg(long = "root", value_name = "DIR")]
    pub(crate) store_root: Option<PathBuf>,
    #[arg(long)]
    pub(crate) json: bool,
}

#[derive(Debug, Parser)]
pub(crate) struct ListArgs {
    #[arg(short = 'c', long = "config", value_name = "PATH")]
    pub(crate) config: Option<PathBuf>,
    #[arg(long = "root", value_name = "DIR")]
    pub(crate) store_root: Option<PathBuf>,
    #[arg(long, value_enum, default_value_t = ListKind::All)]
    pub(crate) kind: ListKind,
    #[arg(long)]
    pub(crate) json: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub(crate) enum ListKind {
    All,
    Suites,
    Agents,
    Tasks,
    Reports,
    Runs,
    Datasets,
}

#[derive(Debug, Parser)]
pub(crate) struct SelectArgs {
    #[arg(short = 'c', long = "config", value_name = "PATH")]
    pub(crate) config: Option<PathBuf>,
    #[arg(long = "root", value_name = "DIR")]
    pub(crate) store_root: Option<PathBuf>,
    #[arg(long)]
    pub(crate) suite: Option<String>,
    #[arg(long)]
    pub(crate) agent: Option<String>,
    #[arg(long)]
    pub(crate) json: bool,
}

#[derive(Debug, Parser)]
pub(crate) struct RunArgs {
    #[arg(short = 'c', long = "config", value_name = "PATH")]
    pub(crate) config: Option<PathBuf>,
    #[arg(long = "root", value_name = "DIR")]
    pub(crate) store_root: Option<PathBuf>,
    #[arg(long)]
    pub(crate) suite: Option<String>,
    #[arg(long)]
    pub(crate) agent: Option<String>,
    #[arg(long)]
    pub(crate) run_id: Option<String>,
    #[arg(long, value_name = "DIR")]
    pub(crate) output_root: Option<PathBuf>,
    #[arg(long)]
    pub(crate) json: bool,
}

#[derive(Debug, Parser)]
pub(crate) struct ReportArgs {
    #[arg(short = 'c', long = "config", value_name = "PATH")]
    pub(crate) config: Option<PathBuf>,
    #[arg(long = "root", value_name = "DIR")]
    pub(crate) store_root: Option<PathBuf>,
    #[arg(long, value_name = "RUN")]
    pub(crate) run_root: PathBuf,
    #[arg(long)]
    pub(crate) suite: Option<String>,
    #[arg(long)]
    pub(crate) agent: Option<String>,
    #[arg(long, value_enum)]
    pub(crate) status: Option<RunStatusFilter>,
    #[arg(long, value_enum, default_value_t = ReportFormat::Markdown)]
    pub(crate) format: ReportFormat,
    #[arg(long, value_name = "PATH")]
    pub(crate) output: Option<PathBuf>,
}

#[derive(Debug, Parser)]
pub(crate) struct CompareArgs {
    #[arg(value_name = "RUN_ROOT", required = true)]
    pub(crate) run_roots: Vec<PathBuf>,
    #[arg(short = 'c', long = "config", value_name = "PATH")]
    pub(crate) config: Option<PathBuf>,
    #[arg(long = "root", value_name = "DIR")]
    pub(crate) store_root: Option<PathBuf>,
    #[arg(long)]
    pub(crate) suite: Option<String>,
    #[arg(long)]
    pub(crate) agent: Option<String>,
    #[arg(long, value_enum)]
    pub(crate) status: Option<RunStatusFilter>,
    #[arg(long)]
    pub(crate) json: bool,
}

#[derive(Debug, Parser)]
pub(crate) struct ReplayArgs {
    #[arg(short = 'c', long = "config", value_name = "PATH")]
    pub(crate) config: Option<PathBuf>,
    #[arg(long = "root", value_name = "DIR")]
    pub(crate) store_root: Option<PathBuf>,
    #[arg(long, value_name = "RUN")]
    pub(crate) run_root: PathBuf,
    #[arg(long)]
    pub(crate) suite: Option<String>,
    #[arg(long)]
    pub(crate) agent: Option<String>,
    #[arg(long, value_enum)]
    pub(crate) status: Option<RunStatusFilter>,
    #[arg(long)]
    pub(crate) case: Option<String>,
    #[arg(long)]
    pub(crate) json: bool,
}

#[derive(Debug, Parser)]
pub(crate) struct DatasetImportArgs {
    #[arg(value_name = "PATH")]
    pub(crate) path: PathBuf,
    #[arg(long = "root", value_name = "DIR")]
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
        Ok(cli) => match dispatch_cli(cli) {
            Ok(outcome) => outcome,
            Err(err) => CliOutcome {
                code: 1,
                stdout: String::new(),
                stderr: format!("error: {err:#}\n"),
            },
        },
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

pub(crate) fn dispatch_cli(cli: Cli) -> Result<CliOutcome> {
    match cli.command {
        Commands::Init(args) => run_init(args),
        Commands::Doctor(args) => run_doctor(args),
        Commands::List(args) => run_list(args),
        Commands::Check(args) => run_check(args),
        Commands::Run(args) => run_run(args),
        Commands::Report(args) => run_report(args),
        Commands::Compare(args) => run_compare(args),
        Commands::Replay(args) => run_replay(args),
        Commands::Dataset(args) => run_dataset(args),
    }
}

pub(crate) fn run_init(args: InitArgs) -> Result<CliOutcome> {
    let config = init_eval_store(InitStoreRequest {
        root: args.root,
        force: args.force,
    })?;
    if args.json {
        return Ok(success(serde_json::to_string_pretty(&config)?));
    }
    Ok(success(format!("peval root: {}\n", config.root.display())))
}

pub(crate) fn run_doctor(args: ProjectArgs) -> Result<CliOutcome> {
    let project = load_project_from_config(args.config.as_deref())?;
    let store = EvalStore::resolve(args.store_root)?;
    let payload = json!({
        "schema_version": SCHEMA_VERSION,
        "project": &project.name,
        "root": &project.root,
        "eval_root": &store.root,
        "allow_live": project.allow_live,
        "agents": project.agents.len(),
        "suites": project.suites.len(),
        "fake_adapter": "available",
        "psychevo_adapter": "manifest-gated",
        "reports": ["html", "markdown", "json"],
    });
    if args.json {
        return Ok(success(serde_json::to_string_pretty(&payload)?));
    }
    Ok(success(format!(
        "project: {}\nroot: {}\neval root: {}\nallow_live: {}\nagents: {}\nsuites: {}\nfake adapter: available\npsychevo adapter: manifest-gated\n",
        project.name,
        project.root.display(),
        store.root.display(),
        project.allow_live,
        project.agents.len(),
        project.suites.len(),
    )))
}

pub(crate) fn run_list(args: ListArgs) -> Result<CliOutcome> {
    let needs_project = matches!(
        args.kind,
        ListKind::All | ListKind::Suites | ListKind::Agents | ListKind::Tasks
    );
    let project = if needs_project {
        Some(load_project_from_config(args.config.as_deref())?)
    } else {
        try_load_project_from_config(args.config.as_deref())?
    };
    let needs_store = matches!(
        args.kind,
        ListKind::All | ListKind::Runs | ListKind::Datasets
    );
    let store = if needs_store {
        Some(EvalStore::resolve(args.store_root)?)
    } else {
        None
    };
    let tasks = project
        .as_ref()
        .map(list_tasks)
        .transpose()?
        .unwrap_or_default();
    let runs = store
        .as_ref()
        .map(EvalStore::list_runs)
        .transpose()?
        .unwrap_or_default();
    let datasets = store
        .as_ref()
        .map(EvalStore::list_datasets)
        .transpose()?
        .unwrap_or_default();
    let eval_root = store.as_ref().map(|store| store.root.clone());
    let payload = json!({
        "schema_version": SCHEMA_VERSION,
        "eval_root": eval_root,
        "suites": project.as_ref().map(|project| project.suites.values().map(|suite| json!({
            "id": &suite.id,
            "name": &suite.name,
            "agents": &suite.agents,
            "tasks": &suite.tasks,
        })).collect::<Vec<_>>()).unwrap_or_default(),
        "agents": project.as_ref().map(|project| project.agents.values().map(|agent| json!({
            "id": &agent.id,
            "name": &agent.name,
            "kind": agent.kind,
        })).collect::<Vec<_>>()).unwrap_or_default(),
        "tasks": tasks,
        "reports": ["html", "markdown", "json"],
        "runs": runs,
        "datasets": datasets,
    });
    if args.json {
        return Ok(success(serde_json::to_string_pretty(&payload)?));
    }
    let mut out = String::new();
    if matches!(args.kind, ListKind::All | ListKind::Suites) {
        let project = project.as_ref().context("list kind requires eval config")?;
        out.push_str("suites\n");
        for suite in project.suites.values() {
            out.push_str(&format!("- {}\n", suite.id));
        }
    }
    if matches!(args.kind, ListKind::All | ListKind::Agents) {
        let project = project.as_ref().context("list kind requires eval config")?;
        out.push_str("agents\n");
        for agent in project.agents.values() {
            out.push_str(&format!("- {} ({:?})\n", agent.id, agent.kind));
        }
    }
    if matches!(args.kind, ListKind::All | ListKind::Tasks) {
        let project = project.as_ref().context("list kind requires eval config")?;
        out.push_str("tasks\n");
        for task in list_tasks(project)? {
            out.push_str(&format!("- {}\n", task["id"].as_str().unwrap_or("unknown")));
        }
    }
    if matches!(args.kind, ListKind::All | ListKind::Reports) {
        out.push_str("reports\n- html\n- markdown\n- json\n");
    }
    if matches!(args.kind, ListKind::All | ListKind::Runs) {
        let store = store.as_ref().context("list kind requires peval root")?;
        out.push_str("runs\n");
        for run in store.list_runs()? {
            out.push_str(&format!(
                "- {}/{} {:?} {}/{} {}\n",
                run.project_slug,
                run.run_id,
                run.status,
                run.passed_cases,
                run.total_cases,
                run.artifact_root.display()
            ));
        }
    }
    if matches!(args.kind, ListKind::All | ListKind::Datasets) {
        let store = store.as_ref().context("list kind requires peval root")?;
        out.push_str("datasets\n");
        for dataset in store.list_datasets()? {
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
    let project = load_project_from_config(args.config.as_deref())?;
    let cases = check_project(&project, args.suite.as_deref(), args.agent.as_deref())?;
    let payload = json!({
        "schema_version": SCHEMA_VERSION,
        "project": project.name,
        "cases": cases.len(),
        "status": "ok",
    });
    if args.json {
        return Ok(success(serde_json::to_string_pretty(&payload)?));
    }
    Ok(success(format!("check ok: {} case(s)\n", cases.len())))
}

pub(crate) fn run_run(args: RunArgs) -> Result<CliOutcome> {
    let summary = run_evaluation(RunRequest {
        config: args.config,
        suite: args.suite,
        agent: args.agent,
        run_id: args.run_id,
        store_root: args.store_root,
        output_root: args.output_root,
    })?;
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
            "run {}: {:?}\nartifact root: {}\ncases: {} passed / {} failed / {} total\n",
            summary.run_id,
            summary.status,
            summary.artifact_root.display(),
            summary.passed_cases,
            summary.failed_cases,
            summary.total_cases,
        ),
        stderr: String::new(),
    })
}

pub(crate) fn run_report(args: ReportArgs) -> Result<CliOutcome> {
    let run_root = resolve_cli_run_selector(
        args.config.as_deref(),
        args.store_root,
        &args.run_root,
        RunSelectorFilters {
            suite: args.suite,
            agent: args.agent,
            status: args.status,
        },
    )?;
    let rendered = render_report(ReportRequest {
        run_root,
        format: args.format,
    })?;
    if let Some(output) = args.output {
        fs::write(&output, rendered.as_bytes())
            .with_context(|| format!("failed to write {}", output.display()))?;
        Ok(success(format!("wrote {}\n", output.display())))
    } else {
        Ok(success(rendered))
    }
}

pub(crate) fn run_compare(args: CompareArgs) -> Result<CliOutcome> {
    let filters = RunSelectorFilters {
        suite: args.suite,
        agent: args.agent,
        status: args.status,
    };
    let run_roots = args
        .run_roots
        .iter()
        .map(|run_root| {
            resolve_cli_run_selector(
                args.config.as_deref(),
                args.store_root.clone(),
                run_root,
                filters.clone(),
            )
        })
        .collect::<Result<Vec<_>>>()?;
    let report = compare_runs(CompareRequest { run_roots })?;
    Ok(success(render_compare(&report, args.json)?))
}

pub(crate) fn run_replay(args: ReplayArgs) -> Result<CliOutcome> {
    let run_root = resolve_cli_run_selector(
        args.config.as_deref(),
        args.store_root,
        &args.run_root,
        RunSelectorFilters {
            suite: args.suite,
            agent: args.agent,
            status: args.status,
        },
    )?;
    let report = replay_run(ReplayRequest {
        run_root,
        case_id: args.case,
    })?;
    Ok(success(render_replay(&report, args.json)?))
}

pub(crate) fn run_dataset(args: DatasetCommands) -> Result<CliOutcome> {
    match args {
        DatasetCommands::Import(args) => {
            let entry = import_dataset(DatasetImportRequest {
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
            })?;
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

pub(crate) fn resolve_cli_run_selector(
    config: Option<&Path>,
    store_root: Option<PathBuf>,
    selector: &Path,
    filters: RunSelectorFilters,
) -> Result<PathBuf> {
    let explicit_selector = resolve_cli_path(selector)?;
    if explicit_selector.join("summary.json").is_file() {
        return Ok(explicit_selector);
    }
    let project = try_load_project_from_config(config)?;
    let namespace = project.as_ref().map(EvalProject::namespace).transpose()?;
    let store = EvalStore::resolve(store_root)?;
    store.resolve_run_selector(namespace.as_deref(), selector, &filters)
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
    for suite in project.suites.values() {
        for task in load_suite_tasks(suite)? {
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

#[cfg(test)]
pub(crate) mod tests {
    pub(crate) use super::*;
    pub(crate) use std::sync::Mutex;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    struct EnvGuard {
        key: &'static str,
        previous: Option<OsString>,
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            unsafe {
                if let Some(previous) = &self.previous {
                    std::env::set_var(self.key, previous);
                } else {
                    std::env::remove_var(self.key);
                }
            }
        }
    }

    fn set_env_var(key: &'static str, value: Option<&Path>) -> EnvGuard {
        let previous = std::env::var_os(key);
        unsafe {
            if let Some(value) = value {
                std::env::set_var(key, value);
            } else {
                std::env::remove_var(key);
            }
        }
        EnvGuard { key, previous }
    }

    pub(crate) fn fixture_project() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures/local-rust-swe")
    }
    pub(crate) mod project_lifecycle;
}
