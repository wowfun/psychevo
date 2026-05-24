use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::ffi::OsString;
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand, ValueEnum};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use uuid::Uuid;

pub const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone)]
pub struct EvalProject {
    pub root: PathBuf,
    pub manifest_path: PathBuf,
    pub schema_version: u32,
    pub name: String,
    pub output_root: Option<PathBuf>,
    pub allow_live: bool,
    pub agents: BTreeMap<String, AgentManifest>,
    pub suites: BTreeMap<String, SuiteManifest>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentManifest {
    pub schema_version: u32,
    pub id: String,
    #[serde(default)]
    pub name: Option<String>,
    pub kind: AgentKind,
    #[serde(default)]
    pub fake: FakeAgentOptions,
    #[serde(default)]
    pub psychevo: PsychevoAgentOptions,
    #[serde(skip)]
    pub manifest_path: PathBuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AgentKind {
    Fake,
    Psychevo,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FakeAgentOptions {
    #[serde(default = "default_fake_behavior")]
    pub behavior: FakeBehavior,
}

impl Default for FakeAgentOptions {
    fn default() -> Self {
        Self {
            behavior: FakeBehavior::Pass,
        }
    }
}

fn default_fake_behavior() -> FakeBehavior {
    FakeBehavior::Pass
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FakeBehavior {
    Pass,
    Fail,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PsychevoAgentOptions {
    #[serde(default)]
    pub command: Option<String>,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub model: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SuiteManifest {
    pub schema_version: u32,
    pub id: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub agents: Vec<String>,
    #[serde(default)]
    pub tasks: Vec<PathBuf>,
    #[serde(skip)]
    pub manifest_path: PathBuf,
    #[serde(skip)]
    pub dir: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskManifest {
    pub schema_version: u32,
    pub id: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default = "default_task_kind")]
    pub kind: String,
    pub prompt: PromptManifest,
    pub workspace: WorkspaceManifest,
    pub scorer: CommandManifest,
    #[serde(default)]
    pub fake: FakeTaskCommands,
    #[serde(skip)]
    pub manifest_path: PathBuf,
    #[serde(skip)]
    pub dir: PathBuf,
}

fn default_task_kind() -> String {
    "swe-style".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptManifest {
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub file: Option<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceManifest {
    pub source: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandManifest {
    pub command: Vec<String>,
    #[serde(default)]
    pub timeout_seconds: Option<u64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FakeTaskCommands {
    #[serde(default)]
    pub pass: Option<CommandManifest>,
    #[serde(default)]
    pub fail: Option<CommandManifest>,
}

#[derive(Debug, Clone)]
pub struct RunRequest {
    pub config: Option<PathBuf>,
    pub suite: Option<String>,
    pub agent: Option<String>,
    pub run_id: Option<String>,
    pub store_root: Option<PathBuf>,
    pub output_root: Option<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct InitStoreRequest {
    pub root: Option<PathBuf>,
    pub force: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunSummary {
    pub schema_version: u32,
    pub run_id: String,
    pub project: String,
    pub artifact_root: PathBuf,
    pub started_at_ms: u128,
    pub finished_at_ms: u128,
    pub total_cases: usize,
    pub passed_cases: usize,
    pub failed_cases: usize,
    pub status: RunStatus,
    pub cases: Vec<CaseResult>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    Passed,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ValueEnum)]
#[serde(rename_all = "snake_case")]
#[clap(rename_all = "kebab-case")]
pub enum RunStatusFilter {
    Passed,
    Failed,
}

impl From<RunStatusFilter> for RunStatus {
    fn from(value: RunStatusFilter) -> Self {
        match value {
            RunStatusFilter::Passed => RunStatus::Passed,
            RunStatusFilter::Failed => RunStatus::Failed,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaseResult {
    pub schema_version: u32,
    pub case_id: String,
    pub suite_id: String,
    pub task_id: String,
    pub agent_id: String,
    pub status: CaseStatus,
    pub score: ScoreResult,
    pub duration_ms: u128,
    pub artifacts: CaseArtifacts,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CaseStatus {
    Passed,
    Failed,
    SetupFailed,
    RuntimeFailed,
    ScorerFailed,
    Timeout,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScoreResult {
    pub schema_version: u32,
    pub passed: bool,
    #[serde(default)]
    pub score: Option<f64>,
    pub message: String,
    #[serde(default)]
    pub details: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaseArtifacts {
    pub result: PathBuf,
    pub trajectory: PathBuf,
    pub scorer_stdout: PathBuf,
    pub scorer_stderr: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrajectoryEvent {
    pub schema_version: u32,
    pub sequence: u64,
    pub case_id: String,
    pub kind: String,
    pub message: String,
    pub timestamp_ms: u128,
    #[serde(default)]
    pub data: Value,
}

#[derive(Debug, Clone)]
pub struct ReportRequest {
    pub run_root: PathBuf,
    pub format: ReportFormat,
}

#[derive(Debug, Clone)]
pub struct CompareRequest {
    pub run_roots: Vec<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct ReplayRequest {
    pub run_root: PathBuf,
    pub case_id: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CompareReport {
    pub schema_version: u32,
    pub runs: Vec<CompareRun>,
    pub cases: Vec<CompareCase>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CompareRun {
    pub run_id: String,
    pub artifact_root: PathBuf,
    pub status: RunStatus,
}

#[derive(Debug, Clone, Serialize)]
pub struct CompareCase {
    pub key: String,
    pub statuses: BTreeMap<String, CaseStatus>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ReplayReport {
    pub schema_version: u32,
    pub run_id: String,
    pub events: Vec<TrajectoryEvent>,
}

#[derive(Debug, Clone, Default)]
pub struct RunSelectorFilters {
    pub suite: Option<String>,
    pub agent: Option<String>,
    pub status: Option<RunStatusFilter>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunIndexEntry {
    pub schema_version: u32,
    pub project: String,
    pub project_slug: String,
    #[serde(default)]
    pub namespace: PathBuf,
    pub run_id: String,
    pub artifact_root: PathBuf,
    pub report_html: PathBuf,
    pub report_markdown: PathBuf,
    pub started_at_ms: u128,
    pub finished_at_ms: u128,
    pub total_cases: usize,
    pub passed_cases: usize,
    pub failed_cases: usize,
    pub status: RunStatus,
    pub suites: Vec<String>,
    pub agents: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvalStoreIndex {
    pub schema_version: u32,
    pub generated_at_ms: u128,
    pub runs: Vec<RunIndexEntry>,
    pub datasets: Vec<DatasetEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LatestProjectIndex {
    pub schema_version: u32,
    pub generated_at_ms: u128,
    pub project: String,
    pub project_slug: String,
    pub latest: Option<RunIndexEntry>,
    pub runs: Vec<RunIndexEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatasetManifest {
    pub schema_version: u32,
    pub id: String,
    pub name: String,
    pub kind: String,
    pub source: String,
    pub payload: PathBuf,
    #[serde(default)]
    pub loader: Option<String>,
    #[serde(default)]
    pub split: Option<String>,
    #[serde(default)]
    pub sample_limit: Option<usize>,
    #[serde(default)]
    pub cache_key: Option<String>,
    #[serde(default)]
    pub license: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatasetEntry {
    pub schema_version: u32,
    pub id: String,
    pub name: String,
    pub kind: String,
    pub source: String,
    pub payload: PathBuf,
    pub payload_exists: bool,
    pub manifest_path: PathBuf,
    #[serde(default)]
    pub loader: Option<String>,
    #[serde(default)]
    pub split: Option<String>,
    #[serde(default)]
    pub sample_limit: Option<usize>,
    #[serde(default)]
    pub cache_key: Option<String>,
    #[serde(default)]
    pub license: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub notes: Option<String>,
}

#[derive(Debug, Clone)]
pub struct DatasetImportRequest {
    pub store_root: Option<PathBuf>,
    pub path: PathBuf,
    pub id: Option<String>,
    pub name: Option<String>,
    pub kind: Option<String>,
    pub loader: Option<String>,
    pub split: Option<String>,
    pub sample_limit: Option<usize>,
    pub cache_key: Option<String>,
    pub license: Option<String>,
    pub tags: Vec<String>,
    pub notes: Option<String>,
}

#[derive(Debug, Clone)]
pub struct EvalStore {
    pub root: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PevalConfig {
    pub schema_version: u32,
    pub root: PathBuf,
}

#[derive(Debug, Clone)]
struct ProjectManifest {
    schema_version: u32,
    name: String,
    output_root: Option<PathBuf>,
    allow_live: bool,
}

#[derive(Debug, Deserialize)]
struct RawProjectManifest {
    schema_version: u32,
    #[serde(default = "default_project_name")]
    name: String,
    #[serde(default)]
    output_root: Option<PathBuf>,
    #[serde(default)]
    allow_live: bool,
}

fn default_project_name() -> String {
    "evaluation".to_string()
}

#[derive(Debug, Clone)]
pub struct CasePlan {
    pub case_id: String,
    pub suite: SuiteManifest,
    pub task: TaskManifest,
    pub agent: AgentManifest,
}

#[derive(Debug)]
struct ProcessOutcome {
    success: bool,
    code: Option<i32>,
    timed_out: bool,
    stdout: String,
    stderr: String,
}

impl EvalProject {
    pub fn load(start: impl AsRef<Path>) -> Result<Self> {
        let manifest_path = discover_manifest(start.as_ref())?;
        let root = manifest_path
            .parent()
            .context("eval.toml has no parent directory")?
            .to_path_buf();
        let manifest = read_project_manifest(&manifest_path)?;
        let agents = load_agent_manifests(&root)?;
        let suites = load_suite_manifests(&root)?;
        Ok(Self {
            root,
            manifest_path,
            schema_version: manifest.schema_version,
            name: manifest.name,
            output_root: manifest.output_root,
            allow_live: manifest.allow_live,
            agents,
            suites,
        })
    }

    pub fn namespace(&self) -> Result<PathBuf> {
        self.output_root
            .as_ref()
            .map(|path| validate_store_namespace(path))
            .transpose()?
            .map(Ok)
            .unwrap_or_else(|| Ok(PathBuf::from("runs").join(self.slug())))
    }

    pub fn slug(&self) -> String {
        slugify(&self.name)
    }
}

impl EvalStore {
    pub fn resolve(store_root: Option<PathBuf>) -> Result<Self> {
        Ok(Self {
            root: resolve_store_root(store_root)?,
        })
    }

    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    pub fn run_output_base(&self, project: &EvalProject) -> Result<PathBuf> {
        Ok(self.root.join(project.namespace()?))
    }

    pub fn ensure_layout(&self) -> Result<()> {
        fs::create_dir_all(self.root.join("runs"))
            .with_context(|| format!("failed to create {}", self.root.join("runs").display()))?;
        fs::create_dir_all(self.root.join("datasets")).with_context(|| {
            format!("failed to create {}", self.root.join("datasets").display())
        })?;
        self.refresh_indexes()?;
        self.write_dashboard()
    }

    pub fn resolve_run_selector(
        &self,
        namespace: Option<&Path>,
        selector: &Path,
        filters: &RunSelectorFilters,
    ) -> Result<PathBuf> {
        let explicit_selector = resolve_cli_path(selector)?;
        if explicit_selector.join("summary.json").is_file() {
            return Ok(explicit_selector);
        }

        let selector_text = selector.to_string_lossy();
        if selector_text == "latest" {
            return self
                .latest_run(namespace, filters)?
                .map(|entry| entry.artifact_root)
                .with_context(|| {
                    let scope = namespace
                        .map(|path| path.display().to_string())
                        .unwrap_or_else(|| "global".to_string());
                    format!(
                        "no latest run found for {scope} under {}",
                        self.root.display()
                    )
                });
        }

        if let Some(namespace) = namespace
            && selector.components().count() == 1
        {
            let run_root = self.root.join(namespace).join(selector);
            if run_root.join("summary.json").is_file() {
                return Ok(run_root);
            }
        }

        let store_root = self.root.join(selector);
        if store_root.join("summary.json").is_file() {
            return Ok(store_root);
        }

        let legacy_runs_root = self.root.join("runs").join(selector);
        if legacy_runs_root.join("summary.json").is_file() {
            return Ok(legacy_runs_root);
        }

        bail!(
            "could not resolve run selector `{}` under {}",
            selector.display(),
            self.root.display()
        )
    }

    pub fn list_runs(&self) -> Result<Vec<RunIndexEntry>> {
        match self.read_index() {
            Ok(index) => Ok(index.runs),
            Err(_) => self.scan_runs(),
        }
    }

    pub fn list_datasets(&self) -> Result<Vec<DatasetEntry>> {
        let mut entries = Vec::new();
        let datasets_dir = self.root.join("datasets");
        if !datasets_dir.is_dir() {
            return Ok(entries);
        }
        for entry in fs::read_dir(&datasets_dir)
            .with_context(|| format!("failed to read {}", datasets_dir.display()))?
        {
            let path = entry?.path().join("dataset.toml");
            if path.is_file() {
                entries.push(read_dataset_entry(&path)?);
            }
        }
        entries.sort_by(|left, right| left.id.cmp(&right.id));
        Ok(entries)
    }

    pub fn register_run(&self, _summary: &RunSummary) -> Result<()> {
        self.refresh_indexes()?;
        self.write_dashboard().with_context(|| {
            format!(
                "failed to write {}",
                self.root.join("dashboard.html").display()
            )
        })
    }

    pub fn refresh_after_dataset_change(&self) -> Result<()> {
        self.refresh_indexes()?;
        self.write_dashboard()
    }

    fn latest_run(
        &self,
        namespace: Option<&Path>,
        filters: &RunSelectorFilters,
    ) -> Result<Option<RunIndexEntry>> {
        let status = filters.status.map(RunStatus::from);
        let mut runs = self.list_runs()?;
        runs.retain(|entry| {
            namespace.is_none_or(|expected| entry.namespace == expected)
                && filters
                    .suite
                    .as_ref()
                    .is_none_or(|suite| entry.suites.iter().any(|value| value == suite))
                && filters
                    .agent
                    .as_ref()
                    .is_none_or(|agent| entry.agents.iter().any(|value| value == agent))
                && status.is_none_or(|expected| entry.status == expected)
        });
        runs.sort_by(|left, right| {
            right
                .started_at_ms
                .cmp(&left.started_at_ms)
                .then_with(|| right.run_id.cmp(&left.run_id))
        });
        Ok(runs.into_iter().next())
    }

    fn read_index(&self) -> Result<EvalStoreIndex> {
        let path = self.root.join("index.json");
        let index: EvalStoreIndex = serde_json::from_str(
            &fs::read_to_string(&path)
                .with_context(|| format!("failed to read {}", path.display()))?,
        )
        .with_context(|| format!("failed to parse {}", path.display()))?;
        reject_unsupported(index.schema_version, &path)?;
        Ok(index)
    }

    fn refresh_indexes(&self) -> Result<()> {
        fs::create_dir_all(&self.root)
            .with_context(|| format!("failed to create {}", self.root.display()))?;
        let runs = self.scan_runs()?;
        let datasets = self.list_datasets()?;
        let index = EvalStoreIndex {
            schema_version: SCHEMA_VERSION,
            generated_at_ms: now_ms(),
            runs: runs.clone(),
            datasets,
        };
        write_json_pretty(&self.root.join("index.json"), &index)?;

        let mut by_namespace: BTreeMap<PathBuf, (String, String, Vec<RunIndexEntry>)> =
            BTreeMap::new();
        for run in runs {
            by_namespace
                .entry(run.namespace.clone())
                .or_insert_with(|| (run.project.clone(), run.project_slug.clone(), Vec::new()))
                .2
                .push(run);
        }
        for (namespace, (project, project_slug, runs)) in by_namespace {
            self.write_latest_for_namespace(&namespace, &project, &project_slug, &runs)?;
        }
        Ok(())
    }

    fn write_latest_for_namespace(
        &self,
        namespace: &Path,
        project: &str,
        project_slug: &str,
        runs: &[RunIndexEntry],
    ) -> Result<()> {
        let mut sorted = runs.to_vec();
        sorted.sort_by(|left, right| {
            right
                .started_at_ms
                .cmp(&left.started_at_ms)
                .then_with(|| right.run_id.cmp(&left.run_id))
        });
        let latest = sorted.first().cloned();
        let latest_index = LatestProjectIndex {
            schema_version: SCHEMA_VERSION,
            generated_at_ms: now_ms(),
            project: project.to_string(),
            project_slug: project_slug.to_string(),
            latest,
            runs: sorted,
        };
        let dir = self.root.join(namespace);
        fs::create_dir_all(&dir).with_context(|| format!("failed to create {}", dir.display()))?;
        write_json_pretty(&dir.join("latest.json"), &latest_index)
    }

    fn scan_runs(&self) -> Result<Vec<RunIndexEntry>> {
        let mut runs = Vec::new();
        if !self.root.is_dir() {
            return Ok(runs);
        }
        self.scan_runs_in(&self.root, &mut runs)?;
        runs.sort_by(|left, right| {
            right
                .started_at_ms
                .cmp(&left.started_at_ms)
                .then_with(|| right.run_id.cmp(&left.run_id))
        });
        Ok(runs)
    }

    fn scan_runs_in(&self, dir: &Path, runs: &mut Vec<RunIndexEntry>) -> Result<()> {
        if dir == self.root.join("datasets") {
            return Ok(());
        }
        if dir.join("summary.json").is_file() {
            let summary = read_run_summary(dir)?;
            runs.push(run_index_entry(&summary, dir, &self.root));
            return Ok(());
        }
        for entry in
            fs::read_dir(dir).with_context(|| format!("failed to read {}", dir.display()))?
        {
            let path = entry?.path();
            if path.is_dir() {
                self.scan_runs_in(&path, runs)?;
            }
        }
        Ok(())
    }

    fn write_dashboard(&self) -> Result<()> {
        fs::create_dir_all(&self.root)
            .with_context(|| format!("failed to create {}", self.root.display()))?;
        let runs = self.list_runs()?;
        let datasets = self.list_datasets()?;
        let html = render_store_dashboard(self, &runs, &datasets);
        fs::write(self.root.join("dashboard.html"), html).with_context(|| {
            format!(
                "failed to write {}",
                self.root.join("dashboard.html").display()
            )
        })
    }
}

pub fn init_eval_store(request: InitStoreRequest) -> Result<PevalConfig> {
    let env_map = inherited_env();
    let cwd = env::current_dir()?;
    let home = resolve_psychevo_home(&env_map, &cwd)?;
    let config_path = home.join("peval.toml");
    let root = if let Some(path) = request.root {
        resolve_explicit_path(&path, &env_map, &cwd)?
    } else {
        home_path(&env_map)?.join(".local/evals")
    };
    let config = PevalConfig {
        schema_version: SCHEMA_VERSION,
        root,
    };

    if config_path.is_file() {
        let existing = read_peval_config(&config_path)?;
        if existing.root == config.root {
            EvalStore::new(existing.root.clone()).ensure_layout()?;
            return Ok(existing);
        }
        if !request.force {
            bail!(
                "{} already points to {}; rerun `peval init --force --root {}` to replace it",
                config_path.display(),
                existing.root.display(),
                config.root.display()
            );
        }
    }

    fs::create_dir_all(&home).with_context(|| format!("failed to create {}", home.display()))?;
    write_toml_pretty(&config_path, &config)?;
    EvalStore::new(config.root.clone()).ensure_layout()?;
    Ok(config)
}

pub fn check_project(
    project: &EvalProject,
    suite_filter: Option<&str>,
    agent_filter: Option<&str>,
) -> Result<Vec<CaseResult>> {
    let cases = expand_matrix(project, suite_filter, agent_filter)?;
    for case in &cases {
        validate_case(project, case)?;
    }
    Ok(cases
        .into_iter()
        .map(|case| CaseResult {
            schema_version: SCHEMA_VERSION,
            case_id: case.case_id,
            suite_id: case.suite.id,
            task_id: case.task.id,
            agent_id: case.agent.id,
            status: CaseStatus::Passed,
            score: ScoreResult {
                schema_version: SCHEMA_VERSION,
                passed: true,
                score: None,
                message: "validated".to_string(),
                details: Value::Null,
            },
            duration_ms: 0,
            artifacts: CaseArtifacts {
                result: PathBuf::new(),
                trajectory: PathBuf::new(),
                scorer_stdout: PathBuf::new(),
                scorer_stderr: PathBuf::new(),
            },
        })
        .collect())
}

pub fn expand_matrix(
    project: &EvalProject,
    suite_filter: Option<&str>,
    agent_filter: Option<&str>,
) -> Result<Vec<CasePlan>> {
    let mut plans = Vec::new();
    let suites = selected_suites(project, suite_filter)?;
    for suite in suites {
        let agent_ids = selected_agent_ids(project, &suite, agent_filter)?;
        let tasks = load_suite_tasks(&suite)?;
        for task in tasks {
            for agent_id in &agent_ids {
                let agent = project
                    .agents
                    .get(agent_id)
                    .with_context(|| format!("unknown agent `{agent_id}` in suite `{}`", suite.id))?
                    .clone();
                let case_id = sanitize_id(&format!("{}__{}__{}", suite.id, task.id, agent.id));
                plans.push(CasePlan {
                    case_id,
                    suite: suite.clone(),
                    task: task.clone(),
                    agent,
                });
            }
        }
    }
    Ok(plans)
}

pub fn run_evaluation(request: RunRequest) -> Result<RunSummary> {
    let project = load_project_from_config(request.config.as_deref())?;
    let cases = expand_matrix(&project, request.suite.as_deref(), request.agent.as_deref())?;
    if cases.is_empty() {
        bail!("no cases selected");
    }
    for case in &cases {
        validate_case(&project, case)?;
    }

    let run_id = request.run_id.unwrap_or_else(generate_run_id);
    let explicit_output = request.output_root.is_some();
    let store = if explicit_output {
        None
    } else {
        Some(EvalStore::resolve(request.store_root)?)
    };
    let output_base = if let Some(path) = request.output_root {
        resolve_cli_path(&path)?
    } else if let Some(store) = &store {
        store.run_output_base(&project)?
    } else {
        unreachable!("explicit output-root is the only non-store run path")
    };
    let artifact_root = output_base.join(&run_id);
    fs::create_dir_all(&artifact_root)
        .with_context(|| format!("failed to create {}", artifact_root.display()))?;

    let started_at_ms = now_ms();
    let mut results = Vec::new();
    for case in cases {
        let result = run_case(&project, &artifact_root, case)?;
        results.push(result);
    }
    let finished_at_ms = now_ms();
    let passed_cases = results
        .iter()
        .filter(|case| case.status == CaseStatus::Passed)
        .count();
    let failed_cases = results.len().saturating_sub(passed_cases);
    let status = if failed_cases == 0 {
        RunStatus::Passed
    } else {
        RunStatus::Failed
    };
    let summary = RunSummary {
        schema_version: SCHEMA_VERSION,
        run_id,
        project: project.name,
        artifact_root: artifact_root.clone(),
        started_at_ms,
        finished_at_ms,
        total_cases: results.len(),
        passed_cases,
        failed_cases,
        status,
        cases: results,
    };
    write_json_pretty(&artifact_root.join("summary.json"), &summary)?;
    write_run_reports(&summary)?;
    if let Some(store) = store {
        store.register_run(&summary)?;
    }
    Ok(summary)
}

pub fn render_report(request: ReportRequest) -> Result<String> {
    let summary = read_run_summary(&request.run_root)?;
    render_summary_report(&summary, request.format)
}

pub fn compare_runs(request: CompareRequest) -> Result<CompareReport> {
    if request.run_roots.len() < 2 {
        bail!("compare requires at least two run artifact roots");
    }
    let summaries = request
        .run_roots
        .iter()
        .map(|root| read_run_summary(root))
        .collect::<Result<Vec<_>>>()?;
    let runs = summaries
        .iter()
        .map(|summary| CompareRun {
            run_id: summary.run_id.clone(),
            artifact_root: summary.artifact_root.clone(),
            status: summary.status,
        })
        .collect::<Vec<_>>();
    let mut keys = BTreeSet::new();
    for summary in &summaries {
        for case in &summary.cases {
            keys.insert(compare_key(case));
        }
    }
    let mut cases = Vec::new();
    for key in keys {
        let mut statuses = BTreeMap::new();
        for summary in &summaries {
            if let Some(case) = summary.cases.iter().find(|case| compare_key(case) == key) {
                statuses.insert(summary.run_id.clone(), case.status);
            }
        }
        cases.push(CompareCase { key, statuses });
    }
    Ok(CompareReport {
        schema_version: SCHEMA_VERSION,
        runs,
        cases,
    })
}

pub fn replay_run(request: ReplayRequest) -> Result<ReplayReport> {
    let summary = read_run_summary(&request.run_root)?;
    let mut events = Vec::new();
    for case in &summary.cases {
        if request
            .case_id
            .as_deref()
            .is_some_and(|id| id != case.case_id)
        {
            continue;
        }
        let path = request.run_root.join(&case.artifacts.trajectory);
        let file =
            fs::File::open(&path).with_context(|| format!("failed to open {}", path.display()))?;
        for line in BufReader::new(file).lines() {
            let line = line.with_context(|| format!("failed to read {}", path.display()))?;
            if line.trim().is_empty() {
                continue;
            }
            let event: TrajectoryEvent = serde_json::from_str(&line)
                .with_context(|| format!("invalid trajectory event in {}", path.display()))?;
            reject_unsupported(event.schema_version, &path)?;
            events.push(event);
        }
    }
    if events.is_empty() && request.case_id.is_some() {
        bail!("no trajectory events matched requested case");
    }
    Ok(ReplayReport {
        schema_version: SCHEMA_VERSION,
        run_id: summary.run_id,
        events,
    })
}

pub fn import_dataset(request: DatasetImportRequest) -> Result<DatasetEntry> {
    let store = EvalStore::resolve(request.store_root)?;
    let input = resolve_cli_path(&request.path)?;
    let source = fs::canonicalize(&input)
        .with_context(|| format!("failed to resolve dataset path {}", input.display()))?;
    if !source.exists() {
        bail!("dataset path does not exist: {}", source.display());
    }

    let id = request.id.unwrap_or_else(|| {
        source
            .file_stem()
            .or_else(|| source.file_name())
            .and_then(|value| value.to_str())
            .map(slugify)
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| "dataset".to_string())
    });
    let id = slugify(&id);
    let dataset_dir = store.root.join("datasets").join(&id);
    fs::create_dir_all(&dataset_dir)
        .with_context(|| format!("failed to create {}", dataset_dir.display()))?;

    let payload_link = dataset_dir.join("payload");
    let payload = if link_dataset_payload(&source, &payload_link)? {
        PathBuf::from("payload")
    } else {
        source.clone()
    };
    let manifest = DatasetManifest {
        schema_version: SCHEMA_VERSION,
        id: id.clone(),
        name: request.name.unwrap_or_else(|| id.clone()),
        kind: request.kind.unwrap_or_else(|| "local".to_string()),
        source: source.display().to_string(),
        payload,
        loader: request.loader,
        split: request.split,
        sample_limit: request.sample_limit,
        cache_key: request.cache_key,
        license: request.license,
        tags: request.tags,
        notes: request.notes,
    };
    write_toml_pretty(&dataset_dir.join("dataset.toml"), &manifest)?;
    store.refresh_after_dataset_change()?;
    read_dataset_entry(&dataset_dir.join("dataset.toml"))
}

fn run_case(project: &EvalProject, artifact_root: &Path, case: CasePlan) -> Result<CaseResult> {
    let started = Instant::now();
    let case_dir = artifact_root.join("cases").join(&case.case_id);
    fs::create_dir_all(&case_dir)
        .with_context(|| format!("failed to create {}", case_dir.display()))?;
    let result_rel = PathBuf::from("cases")
        .join(&case.case_id)
        .join("result.json");
    let trajectory_rel = PathBuf::from("cases")
        .join(&case.case_id)
        .join("trajectory.jsonl");
    let stdout_rel = PathBuf::from("cases")
        .join(&case.case_id)
        .join("scorer.stdout");
    let stderr_rel = PathBuf::from("cases")
        .join(&case.case_id)
        .join("scorer.stderr");

    let mut events = Vec::new();
    push_event(
        &mut events,
        &case.case_id,
        "case_started",
        "case execution started",
        json!({
            "suite": case.suite.id,
            "task": case.task.id,
            "agent": case.agent.id,
        }),
    );

    let workspace_source = resolve_relative(&case.task.dir, &case.task.workspace.source);
    let workspace_temp = tempfile::Builder::new()
        .prefix("peval-workspace-")
        .tempdir()
        .context("failed to create temporary case workspace")?;
    copy_dir(&workspace_source, workspace_temp.path()).with_context(|| {
        format!(
            "failed to copy task workspace {}",
            workspace_source.display()
        )
    })?;
    push_event(
        &mut events,
        &case.case_id,
        "workspace_prepared",
        "temporary workspace prepared",
        json!({ "workspace_source": workspace_source }),
    );

    let (status, score) =
        if let Err(err) = run_agent(project, &case, workspace_temp.path(), &mut events) {
            let score = ScoreResult {
                schema_version: SCHEMA_VERSION,
                passed: false,
                score: None,
                message: format!("{err:#}"),
                details: Value::Null,
            };
            push_event(
                &mut events,
                &case.case_id,
                "agent_failed",
                &score.message,
                Value::Null,
            );
            fs::write(artifact_root.join(&stdout_rel), "").with_context(|| {
                format!(
                    "failed to write {}",
                    artifact_root.join(&stdout_rel).display()
                )
            })?;
            fs::write(artifact_root.join(&stderr_rel), score.message.as_bytes()).with_context(
                || {
                    format!(
                        "failed to write {}",
                        artifact_root.join(&stderr_rel).display()
                    )
                },
            )?;
            (CaseStatus::RuntimeFailed, score)
        } else {
            let scorer = run_process(&case.task.scorer, &case.task.dir, workspace_temp.path())
                .context("failed to run scorer")?;
            fs::write(artifact_root.join(&stdout_rel), scorer.stdout.as_bytes()).with_context(
                || {
                    format!(
                        "failed to write {}",
                        artifact_root.join(&stdout_rel).display()
                    )
                },
            )?;
            fs::write(artifact_root.join(&stderr_rel), scorer.stderr.as_bytes()).with_context(
                || {
                    format!(
                        "failed to write {}",
                        artifact_root.join(&stderr_rel).display()
                    )
                },
            )?;
            let (case_status, scorer_score) = parse_scorer_result(&scorer);
            push_event(
                &mut events,
                &case.case_id,
                "scorer_finished",
                &scorer_score.message,
                json!({
                    "status": case_status,
                    "passed": scorer_score.passed,
                    "exit_code": scorer.code,
                    "timed_out": scorer.timed_out,
                }),
            );
            (case_status, scorer_score)
        };

    push_event(
        &mut events,
        &case.case_id,
        "case_finished",
        "case execution finished",
        json!({ "status": status }),
    );
    write_jsonl(&artifact_root.join(&trajectory_rel), &events)?;

    let result = CaseResult {
        schema_version: SCHEMA_VERSION,
        case_id: case.case_id,
        suite_id: case.suite.id,
        task_id: case.task.id,
        agent_id: case.agent.id,
        status,
        score,
        duration_ms: started.elapsed().as_millis(),
        artifacts: CaseArtifacts {
            result: result_rel.clone(),
            trajectory: trajectory_rel,
            scorer_stdout: stdout_rel,
            scorer_stderr: stderr_rel,
        },
    };
    write_json_pretty(&artifact_root.join(&result_rel), &result)?;
    Ok(result)
}

fn run_agent(
    project: &EvalProject,
    case: &CasePlan,
    workspace: &Path,
    events: &mut Vec<TrajectoryEvent>,
) -> Result<()> {
    match case.agent.kind {
        AgentKind::Fake => {
            let spec = match case.agent.fake.behavior {
                FakeBehavior::Pass => case.task.fake.pass.as_ref(),
                FakeBehavior::Fail => case.task.fake.fail.as_ref(),
            };
            if let Some(command) = spec {
                let output = run_process(command, &case.task.dir, workspace)?;
                let kind = if output.success {
                    "fake_agent_finished"
                } else {
                    "fake_agent_failed"
                };
                push_event(
                    events,
                    &case.case_id,
                    kind,
                    "fake agent command finished",
                    json!({
                        "behavior": case.agent.fake.behavior,
                        "exit_code": output.code,
                        "stdout": output.stdout,
                        "stderr": output.stderr,
                        "timed_out": output.timed_out,
                    }),
                );
                if !output.success {
                    bail!("fake agent `{}` failed", case.agent.id);
                }
            } else {
                push_event(
                    events,
                    &case.case_id,
                    "fake_agent_noop",
                    "fake agent made no workspace changes",
                    json!({ "behavior": case.agent.fake.behavior }),
                );
            }
            Ok(())
        }
        AgentKind::Psychevo => {
            if !project.allow_live {
                bail!(
                    "agent `{}` uses the Psychevo live adapter, but allow_live is false",
                    case.agent.id
                );
            }
            let prompt = task_prompt(&case.task)?;
            push_event(
                events,
                &case.case_id,
                "psychevo_agent_started",
                "Psychevo live adapter command started",
                json!({
                    "agent": case.agent.id,
                    "task": case.task.id,
                }),
            );
            let output = run_psychevo_agent(&case.agent, &case.task.dir, workspace, &prompt)?;
            let observation = collect_psychevo_observation_output(workspace, &output);
            append_psychevo_process_events(events, &case.case_id, &observation);
            push_event(
                events,
                &case.case_id,
                "psychevo_agent_finished",
                "Psychevo live adapter command finished",
                json!({
                    "exit_code": output.code,
                    "stdout_bytes": output.stdout.len(),
                    "stderr_bytes": output.stderr.len(),
                    "timed_out": output.timed_out,
                }),
            );
            if output.success {
                Ok(())
            } else {
                bail!("Psychevo agent `{}` failed", case.agent.id)
            }
        }
    }
}

fn run_psychevo_agent(
    agent: &AgentManifest,
    task_dir: &Path,
    workspace: &Path,
    prompt: &str,
) -> Result<ProcessOutcome> {
    let command = agent
        .psychevo
        .command
        .clone()
        .unwrap_or_else(|| "pevo".to_string());
    let mut args = if agent.psychevo.args.is_empty() {
        let mut args = vec![
            "run".to_string(),
            "--dir".to_string(),
            workspace.display().to_string(),
            "--format".to_string(),
            "json".to_string(),
            "--dangerously-skip-permissions".to_string(),
            "--no-skills".to_string(),
            "--no-agents".to_string(),
        ];
        if let Some(model) = &agent.psychevo.model {
            args.push("--model".to_string());
            args.push(model.clone());
        }
        args.push(prompt.to_string());
        args
    } else {
        agent
            .psychevo
            .args
            .iter()
            .map(|arg| {
                arg.replace("{workspace}", &workspace.display().to_string())
                    .replace("{prompt}", prompt)
            })
            .collect()
    };
    let spec = CommandManifest {
        command: {
            let mut command_and_args = vec![command];
            command_and_args.append(&mut args);
            command_and_args
        },
        timeout_seconds: Some(600),
    };
    run_process(&spec, task_dir, workspace)
}

#[derive(Debug)]
struct PsychevoObservationOutput {
    stdout: String,
    stderr: String,
}

fn collect_psychevo_observation_output(
    workspace: &Path,
    output: &ProcessOutcome,
) -> PsychevoObservationOutput {
    let stdout = read_optional_string(&workspace.join("pevo-live.stdout"))
        .unwrap_or_else(|| output.stdout.clone());
    let stderr = match read_optional_string(&workspace.join("pevo-live.stderr")) {
        Some(file_stderr) if output.stderr.trim().is_empty() => file_stderr,
        Some(file_stderr) if file_stderr.trim().is_empty() => output.stderr.clone(),
        Some(file_stderr) => format!("{}\n{}", file_stderr.trim_end(), output.stderr.trim_end()),
        None => output.stderr.clone(),
    };
    PsychevoObservationOutput { stdout, stderr }
}

fn read_optional_string(path: &Path) -> Option<String> {
    fs::read_to_string(path).ok()
}

fn append_psychevo_process_events(
    events: &mut Vec<TrajectoryEvent>,
    case_id: &str,
    output: &PsychevoObservationOutput,
) {
    for line in output.stdout.lines().filter(|line| !line.trim().is_empty()) {
        match serde_json::from_str::<Value>(line) {
            Ok(raw_event) => {
                let event_type = raw_event
                    .get("type")
                    .and_then(Value::as_str)
                    .unwrap_or("event");
                let kind = format!("psychevo_{}", event_kind_suffix(event_type));
                push_event(
                    events,
                    case_id,
                    &kind,
                    &format!("Psychevo runtime event: {event_type}"),
                    json!({ "raw_event": raw_event }),
                );
            }
            Err(err) => push_event(
                events,
                case_id,
                "psychevo_stdout_line",
                "Psychevo adapter stdout line",
                json!({
                    "line": line,
                    "parse_error": err.to_string(),
                }),
            ),
        }
    }
    for line in output.stderr.lines().filter(|line| !line.trim().is_empty()) {
        push_event(
            events,
            case_id,
            "psychevo_stderr_line",
            "Psychevo adapter stderr line",
            json!({ "line": line }),
        );
    }
}

fn event_kind_suffix(value: &str) -> String {
    let normalized = sanitize_id(&value.to_ascii_lowercase());
    let trimmed = normalized.trim_matches('_');
    if trimmed.is_empty() {
        "event".to_string()
    } else {
        trimmed.to_string()
    }
}

fn parse_scorer_result(outcome: &ProcessOutcome) -> (CaseStatus, ScoreResult) {
    if outcome.timed_out {
        return (
            CaseStatus::Timeout,
            ScoreResult {
                schema_version: SCHEMA_VERSION,
                passed: false,
                score: Some(0.0),
                message: "scorer timed out".to_string(),
                details: json!({ "stderr": outcome.stderr }),
            },
        );
    }
    if !outcome.success {
        return (
            CaseStatus::ScorerFailed,
            ScoreResult {
                schema_version: SCHEMA_VERSION,
                passed: false,
                score: Some(0.0),
                message: format!("scorer exited with code {:?}", outcome.code),
                details: json!({
                    "stdout": outcome.stdout,
                    "stderr": outcome.stderr,
                }),
            },
        );
    }
    match serde_json::from_str::<ScoreResult>(&outcome.stdout) {
        Ok(mut score) => {
            if let Err(err) = reject_unsupported_result_schema(score.schema_version) {
                score.passed = false;
                score.score = Some(0.0);
                score.message = err.to_string();
                return (CaseStatus::ScorerFailed, score);
            }
            let status = if score.passed {
                CaseStatus::Passed
            } else {
                CaseStatus::Failed
            };
            (status, score)
        }
        Err(err) => (
            CaseStatus::ScorerFailed,
            ScoreResult {
                schema_version: SCHEMA_VERSION,
                passed: false,
                score: Some(0.0),
                message: format!("malformed scorer JSON: {err}"),
                details: json!({ "stdout": outcome.stdout }),
            },
        ),
    }
}

fn run_process(
    spec: &CommandManifest,
    task_dir: &Path,
    workspace: &Path,
) -> Result<ProcessOutcome> {
    if spec.command.is_empty() {
        bail!("command declaration is empty");
    }
    let program = resolve_command_part(&spec.command[0], task_dir);
    let mut command = Command::new(program);
    for arg in &spec.command[1..] {
        command.arg(resolve_command_part(arg, task_dir));
    }
    command
        .current_dir(workspace)
        .env("PEVAL_WORKSPACE", workspace)
        .env("PEVAL_TASK_DIR", task_dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = command.spawn().with_context(|| {
        format!(
            "failed to spawn command `{}` in {}",
            spec.command.join(" "),
            workspace.display()
        )
    })?;
    let timeout = spec.timeout_seconds.map(Duration::from_secs);
    if let Some(timeout) = timeout {
        let started = Instant::now();
        loop {
            if child.try_wait()?.is_some() {
                let output = child.wait_with_output()?;
                return Ok(ProcessOutcome {
                    success: output.status.success(),
                    code: output.status.code(),
                    timed_out: false,
                    stdout: String::from_utf8_lossy(&output.stdout).to_string(),
                    stderr: String::from_utf8_lossy(&output.stderr).to_string(),
                });
            }
            if started.elapsed() >= timeout {
                let _ = child.kill();
                let output = child.wait_with_output()?;
                return Ok(ProcessOutcome {
                    success: false,
                    code: output.status.code(),
                    timed_out: true,
                    stdout: String::from_utf8_lossy(&output.stdout).to_string(),
                    stderr: String::from_utf8_lossy(&output.stderr).to_string(),
                });
            }
            thread::sleep(Duration::from_millis(20));
        }
    }
    let output = child.wait_with_output()?;
    Ok(ProcessOutcome {
        success: output.status.success(),
        code: output.status.code(),
        timed_out: false,
        stdout: String::from_utf8_lossy(&output.stdout).to_string(),
        stderr: String::from_utf8_lossy(&output.stderr).to_string(),
    })
}

fn validate_case(project: &EvalProject, case: &CasePlan) -> Result<()> {
    reject_unsupported(case.suite.schema_version, &case.suite.manifest_path)?;
    reject_unsupported(case.agent.schema_version, &case.agent.manifest_path)?;
    reject_unsupported(case.task.schema_version, &case.task.manifest_path)?;
    if case.agent.kind == AgentKind::Psychevo && !project.allow_live {
        bail!(
            "agent `{}` uses Psychevo live execution, but {} has allow_live = false",
            case.agent.id,
            project.manifest_path.display()
        );
    }
    let workspace_source = resolve_relative(&case.task.dir, &case.task.workspace.source);
    if !workspace_source.is_dir() {
        bail!(
            "task `{}` workspace source does not exist: {}",
            case.task.id,
            workspace_source.display()
        );
    }
    validate_command(&case.task.scorer, &case.task.dir, "scorer")?;
    if let Some(command) = &case.task.fake.pass {
        validate_command(command, &case.task.dir, "fake pass command")?;
    }
    if let Some(command) = &case.task.fake.fail {
        validate_command(command, &case.task.dir, "fake fail command")?;
    }
    Ok(())
}

fn validate_command(command: &CommandManifest, dir: &Path, label: &str) -> Result<()> {
    if command.command.is_empty() {
        bail!("{label} declaration is empty");
    }
    let program = &command.command[0];
    if is_declared_path(program, dir) {
        let path = resolve_relative(dir, Path::new(program));
        if !path.exists() {
            bail!("{label} path does not exist: {}", path.display());
        }
    }
    for arg in &command.command[1..] {
        if is_declared_path(arg, dir) {
            let path = resolve_relative(dir, Path::new(arg));
            if !path.exists() {
                bail!("{label} argument path does not exist: {}", path.display());
            }
        }
    }
    Ok(())
}

fn selected_suites(
    project: &EvalProject,
    suite_filter: Option<&str>,
) -> Result<Vec<SuiteManifest>> {
    if let Some(id) = suite_filter {
        return Ok(vec![
            project
                .suites
                .get(id)
                .with_context(|| format!("unknown suite `{id}`"))?
                .clone(),
        ]);
    }
    Ok(project.suites.values().cloned().collect())
}

fn selected_agent_ids(
    project: &EvalProject,
    suite: &SuiteManifest,
    agent_filter: Option<&str>,
) -> Result<Vec<String>> {
    if let Some(agent_id) = agent_filter {
        if !project.agents.contains_key(agent_id) {
            bail!("unknown agent `{agent_id}`");
        }
        return Ok(vec![agent_id.to_string()]);
    }
    if suite.agents.is_empty() {
        return Ok(project.agents.keys().cloned().collect());
    }
    Ok(suite.agents.clone())
}

fn load_suite_tasks(suite: &SuiteManifest) -> Result<Vec<TaskManifest>> {
    if suite.tasks.is_empty() {
        bail!("suite `{}` does not declare any tasks", suite.id);
    }
    suite
        .tasks
        .iter()
        .map(|path| {
            let path = resolve_relative(&suite.dir, path);
            read_task_manifest(&path)
        })
        .collect()
}

fn read_project_manifest(path: &Path) -> Result<ProjectManifest> {
    let raw: RawProjectManifest = read_toml(path)?;
    reject_unsupported(raw.schema_version, path)?;
    if let Some(output_root) = raw.output_root.as_ref() {
        validate_store_namespace(output_root)
            .with_context(|| format!("invalid output_root in {}", path.display()))?;
    }
    Ok(ProjectManifest {
        schema_version: raw.schema_version,
        name: raw.name,
        output_root: raw.output_root,
        allow_live: raw.allow_live,
    })
}

fn load_agent_manifests(root: &Path) -> Result<BTreeMap<String, AgentManifest>> {
    let mut agents = BTreeMap::new();
    for path in sorted_toml_files(&root.join("agents"))? {
        let mut manifest: AgentManifest = read_toml(&path)?;
        reject_unsupported(manifest.schema_version, &path)?;
        manifest.manifest_path = path;
        if agents.insert(manifest.id.clone(), manifest).is_some() {
            bail!("duplicate agent id");
        }
    }
    if agents.is_empty() {
        bail!(
            "no agent manifests found under {}",
            root.join("agents").display()
        );
    }
    Ok(agents)
}

fn load_suite_manifests(root: &Path) -> Result<BTreeMap<String, SuiteManifest>> {
    let mut suites = BTreeMap::new();
    for path in sorted_toml_files(&root.join("suites"))? {
        let mut manifest: SuiteManifest = read_toml(&path)?;
        reject_unsupported(manifest.schema_version, &path)?;
        manifest.dir = path
            .parent()
            .context("suite manifest has no parent")?
            .to_path_buf();
        manifest.manifest_path = path;
        if suites.insert(manifest.id.clone(), manifest).is_some() {
            bail!("duplicate suite id");
        }
    }
    if suites.is_empty() {
        bail!(
            "no suite manifests found under {}",
            root.join("suites").display()
        );
    }
    Ok(suites)
}

fn read_task_manifest(path: &Path) -> Result<TaskManifest> {
    let mut manifest: TaskManifest = read_toml(path)?;
    reject_unsupported(manifest.schema_version, path)?;
    manifest.dir = path
        .parent()
        .context("task manifest has no parent")?
        .to_path_buf();
    manifest.manifest_path = path.to_path_buf();
    Ok(manifest)
}

fn sorted_toml_files(dir: &Path) -> Result<Vec<PathBuf>> {
    if !dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut paths = fs::read_dir(dir)?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<std::io::Result<Vec<_>>>()?;
    paths.retain(|path| path.extension().is_some_and(|ext| ext == "toml"));
    paths.sort();
    Ok(paths)
}

fn read_toml<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<T> {
    let raw =
        fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))?;
    toml::from_str(&raw).with_context(|| format!("failed to parse {}", path.display()))
}

fn discover_manifest(start: &Path) -> Result<PathBuf> {
    let mut current = if start.is_file() {
        if start.file_name().is_some_and(|name| name == "eval.toml") {
            return Ok(start.to_path_buf());
        }
        start.parent().unwrap_or(start).to_path_buf()
    } else {
        start.to_path_buf()
    };
    loop {
        let candidate = current.join("eval.toml");
        if candidate.is_file() {
            return Ok(candidate);
        }
        if !current.pop() {
            break;
        }
    }
    bail!("could not find eval.toml from {}", start.display())
}

fn load_project_from_config(config: Option<&Path>) -> Result<EvalProject> {
    match config {
        Some(path) => EvalProject::load(resolve_cli_path(path)?),
        None => EvalProject::load(env::current_dir()?),
    }
}

fn try_load_project_from_config(config: Option<&Path>) -> Result<Option<EvalProject>> {
    match config {
        Some(path) => Ok(Some(EvalProject::load(resolve_cli_path(path)?)?)),
        None => match discover_manifest(&env::current_dir()?) {
            Ok(path) => Ok(Some(EvalProject::load(path)?)),
            Err(_) => Ok(None),
        },
    }
}

fn task_prompt(task: &TaskManifest) -> Result<String> {
    if let Some(path) = &task.prompt.file {
        let path = resolve_relative(&task.dir, path);
        return fs::read_to_string(&path)
            .with_context(|| format!("failed to read prompt {}", path.display()));
    }
    Ok(task.prompt.text.clone())
}

fn copy_dir(from: &Path, to: &Path) -> Result<()> {
    fs::create_dir_all(to)?;
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        let source = entry.path();
        let target = to.join(entry.file_name());
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            copy_dir(&source, &target)?;
        } else if file_type.is_file() {
            fs::copy(&source, &target).with_context(|| {
                format!(
                    "failed to copy {} to {}",
                    source.display(),
                    target.display()
                )
            })?;
        }
    }
    Ok(())
}

fn push_event(
    events: &mut Vec<TrajectoryEvent>,
    case_id: &str,
    kind: &str,
    message: &str,
    data: Value,
) {
    events.push(TrajectoryEvent {
        schema_version: SCHEMA_VERSION,
        sequence: events.len() as u64,
        case_id: case_id.to_string(),
        kind: kind.to_string(),
        message: message.to_string(),
        timestamp_ms: now_ms(),
        data,
    });
}

fn write_json_pretty<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    let content = serde_json::to_string_pretty(value)?;
    fs::write(path, content).with_context(|| format!("failed to write {}", path.display()))
}

fn write_toml_pretty<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    let content = toml::to_string_pretty(value)?;
    fs::write(path, content).with_context(|| format!("failed to write {}", path.display()))
}

fn write_jsonl(path: &Path, events: &[TrajectoryEvent]) -> Result<()> {
    let mut content = String::new();
    for event in events {
        content.push_str(&serde_json::to_string(event)?);
        content.push('\n');
    }
    fs::write(path, content).with_context(|| format!("failed to write {}", path.display()))
}

fn link_dataset_payload(source: &Path, link: &Path) -> Result<bool> {
    if fs::symlink_metadata(link).is_ok() {
        if link.is_dir() && !link.is_symlink() {
            fs::remove_dir_all(link)
                .with_context(|| format!("failed to remove {}", link.display()))?;
        } else {
            fs::remove_file(link)
                .with_context(|| format!("failed to remove {}", link.display()))?;
        }
    }
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(source, link).with_context(|| {
            format!(
                "failed to link dataset payload {} to {}",
                source.display(),
                link.display()
            )
        })?;
        Ok(true)
    }
    #[cfg(not(unix))]
    {
        let _ = (source, link);
        Ok(false)
    }
}

fn read_run_summary(root: &Path) -> Result<RunSummary> {
    let path = root.join("summary.json");
    let summary: RunSummary = serde_json::from_str(
        &fs::read_to_string(&path)
            .with_context(|| format!("failed to read run summary {}", path.display()))?,
    )
    .with_context(|| format!("failed to parse run summary {}", path.display()))?;
    reject_unsupported(summary.schema_version, &path)?;
    Ok(summary)
}

fn write_run_reports(summary: &RunSummary) -> Result<()> {
    fs::write(
        summary.artifact_root.join("report.md"),
        render_summary_report(summary, ReportFormat::Markdown)?,
    )
    .with_context(|| {
        format!(
            "failed to write {}",
            summary.artifact_root.join("report.md").display()
        )
    })?;
    fs::write(
        summary.artifact_root.join("report.html"),
        render_summary_report(summary, ReportFormat::Html)?,
    )
    .with_context(|| {
        format!(
            "failed to write {}",
            summary.artifact_root.join("report.html").display()
        )
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
#[clap(rename_all = "kebab-case")]
pub enum ReportFormat {
    Html,
    Markdown,
    Json,
}

fn render_summary_report(summary: &RunSummary, format: ReportFormat) -> Result<String> {
    match format {
        ReportFormat::Json => Ok(serde_json::to_string_pretty(summary)?),
        ReportFormat::Markdown => {
            let mut out = String::new();
            out.push_str(&format!("# peval report `{}`\n\n", summary.run_id));
            out.push_str(&format!("- status: {:?}\n", summary.status));
            out.push_str(&format!("- project: `{}`\n", summary.project));
            out.push_str(&format!(
                "- artifact root: `{}`\n",
                summary.artifact_root.display()
            ));
            out.push_str(&format!("- cases: {}\n", summary.total_cases));
            out.push_str(&format!("- passed: {}\n", summary.passed_cases));
            out.push_str(&format!("- failed: {}\n\n", summary.failed_cases));
            out.push_str("| case | suite | task | agent | status | score | artifacts |\n");
            out.push_str("| --- | --- | --- | --- | --- | --- | --- |\n");
            for case in &summary.cases {
                out.push_str(&format!(
                    "| `{}` | `{}` | `{}` | `{}` | {:?} | {} | [result]({}) [trajectory]({}) [stdout]({}) [stderr]({}) |\n",
                    case.case_id,
                    case.suite_id,
                    case.task_id,
                    case.agent_id,
                    case.status,
                    case.score.score.unwrap_or_default(),
                    case.artifacts.result.display(),
                    case.artifacts.trajectory.display(),
                    case.artifacts.scorer_stdout.display(),
                    case.artifacts.scorer_stderr.display(),
                ));
            }
            Ok(out)
        }
        ReportFormat::Html => {
            let mut rows = String::new();
            for case in &summary.cases {
                rows.push_str(&format!(
                    "<tr data-status=\"{}\" data-suite=\"{}\" data-agent=\"{}\"><td><button class=\"case-toggle\" type=\"button\" aria-expanded=\"false\">{}</button><div class=\"case-note\" hidden>{}</div></td><td>{}</td><td>{}</td><td>{}</td><td><span class=\"stamp {}\">{:?}</span></td><td class=\"num\">{}</td><td class=\"links\"><a href=\"{}\">result</a><a href=\"{}\">trajectory</a><a href=\"{}\">stdout</a><a href=\"{}\">stderr</a></td></tr>",
                    status_filter_value(case.status),
                    escape_html(&case.suite_id),
                    escape_html(&case.agent_id),
                    escape_html(&case.case_id),
                    escape_html(&truncate_text(&case.score.message, 220)),
                    escape_html(&case.suite_id),
                    escape_html(&case.task_id),
                    escape_html(&case.agent_id),
                    status_class(case.status),
                    case.status,
                    case.score.score.unwrap_or_default(),
                    escape_html(&case.artifacts.result.to_string_lossy()),
                    escape_html(&case.artifacts.trajectory.to_string_lossy()),
                    escape_html(&case.artifacts.scorer_stdout.to_string_lossy()),
                    escape_html(&case.artifacts.scorer_stderr.to_string_lossy()),
                ));
            }
            Ok(format!(
                "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\"><title>peval {}</title>{}</head><body><main class=\"page\"><section class=\"mast\"><div><p class=\"eyebrow\">Psychevo evaluation report</p><h1>{}</h1><p class=\"subline\">{} · artifacts at <code>{}</code></p></div><div class=\"verdict {}\">{:?}</div></section><section class=\"metrics\"><div><span>{}</span><strong>{}</strong></div><div><span>passed</span><strong>{}</strong></div><div><span>failed</span><strong>{}</strong></div><div><span>total</span><strong>{}</strong></div></section><section class=\"toolbar\"><button type=\"button\" data-filter=\"all\">all</button><button type=\"button\" data-filter=\"failed\">exceptions</button><input type=\"search\" id=\"caseSearch\" placeholder=\"filter cases\"></section><section class=\"ledger\"><table id=\"caseTable\"><thead><tr><th>case</th><th>suite</th><th>task</th><th>agent</th><th>status</th><th>score</th><th>artifacts</th></tr></thead><tbody>{}</tbody></table></section></main>{}</body></html>",
                escape_html(&summary.run_id),
                report_css(),
                escape_html(&summary.run_id),
                escape_html(&summary.project),
                escape_html(&summary.artifact_root.to_string_lossy()),
                status_class_for_run(summary.status),
                summary.status,
                "status",
                format!("{:?}", summary.status),
                summary.passed_cases,
                summary.failed_cases,
                summary.total_cases,
                rows,
                report_js(),
            ))
        }
    }
}

fn compare_key(case: &CaseResult) -> String {
    format!("{}/{}/{}", case.suite_id, case.task_id, case.agent_id)
}

fn render_compare(report: &CompareReport, json_output: bool) -> Result<String> {
    if json_output {
        return Ok(serde_json::to_string_pretty(report)?);
    }
    let mut out = String::new();
    out.push_str("peval compare\n");
    for run in &report.runs {
        out.push_str(&format!("- {}: {:?}\n", run.run_id, run.status));
    }
    for case in &report.cases {
        out.push_str(&format!("{}:", case.key));
        for (run, status) in &case.statuses {
            out.push_str(&format!(" {}={:?}", run, status));
        }
        out.push('\n');
    }
    Ok(out)
}

fn render_replay(report: &ReplayReport, json_output: bool) -> Result<String> {
    if json_output {
        return Ok(serde_json::to_string_pretty(report)?);
    }
    let mut out = String::new();
    out.push_str(&format!("peval replay {}\n", report.run_id));
    for event in &report.events {
        out.push_str(&format!(
            "{:04} {} {} - {}\n",
            event.sequence, event.case_id, event.kind, event.message
        ));
    }
    Ok(out)
}

fn render_store_dashboard(
    store: &EvalStore,
    runs: &[RunIndexEntry],
    datasets: &[DatasetEntry],
) -> String {
    let latest = runs.first();
    let mut run_rows = String::new();
    for run in runs {
        let report_link = store_link(store, &run.report_html);
        run_rows.push_str(&format!(
            "<tr data-status=\"{}\" data-suite=\"{}\" data-agent=\"{}\" data-dataset=\"{}\"><td><a href=\"{}\">{}</a><div class=\"muted\">{}</div></td><td>{}</td><td><span class=\"stamp {}\">{:?}</span></td><td class=\"num\">{}/{}</td><td>{}</td><td>{}</td><td><label class=\"pick\"><input type=\"checkbox\" value=\"{}\"> compare</label></td></tr>",
            status_filter_value_for_run(run.status),
            escape_html(&run.suites.join(" ")),
            escape_html(&run.agents.join(" ")),
            escape_html(&run.suites.join(" ")),
            escape_html(&report_link),
            escape_html(&run.run_id),
            escape_html(&run.project),
            escape_html(&run.suites.join(", ")),
            status_class_for_run(run.status),
            run.status,
            run.passed_cases,
            run.total_cases,
            escape_html(&run.agents.join(", ")),
            escape_html(&store_link(store, &run.artifact_root)),
            escape_html(&run.run_id),
        ));
    }

    let mut dataset_rows = String::new();
    for dataset in datasets {
        let exists = if dataset.payload_exists {
            "present"
        } else {
            "missing"
        };
        dataset_rows.push_str(&format!(
            "<tr data-dataset=\"{}\"><td><button class=\"dataset-filter\" type=\"button\" data-dataset=\"{}\">{}</button><div class=\"muted\">{}</div></td><td>{}</td><td>{}</td><td><span class=\"stamp {}\">{}</span></td><td>{}</td></tr>",
            escape_html(&dataset.id),
            escape_html(&dataset.id),
            escape_html(&dataset.name),
            escape_html(&dataset.id),
            escape_html(&dataset.kind),
            escape_html(dataset.split.as_deref().unwrap_or("")),
            exists,
            exists,
            escape_html(&dataset.payload.to_string_lossy()),
        ));
    }

    let latest_line = latest
        .map(|run| {
            format!(
                "<a href=\"{}\">latest: {}</a>",
                escape_html(&store_link(store, &run.report_html)),
                escape_html(&run.run_id)
            )
        })
        .unwrap_or_else(|| "latest: none".to_string());
    let failed_runs = runs
        .iter()
        .filter(|run| run.status == RunStatus::Failed)
        .count();
    let passed_runs = runs.len().saturating_sub(failed_runs);
    format!(
        "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\"><title>peval dashboard</title>{}</head><body><main class=\"page\"><section class=\"mast\"><div><p class=\"eyebrow\">Psychevo evaluation ledger</p><h1>Evaluation results center</h1><p class=\"subline\">{} · root <code>{}</code></p></div><div class=\"verdict\">{}</div></section><section class=\"metrics\"><div><span>runs</span><strong>{}</strong></div><div><span>passed</span><strong>{}</strong></div><div><span>exceptions</span><strong>{}</strong></div><div><span>datasets</span><strong>{}</strong></div></section><section class=\"toolbar\"><button type=\"button\" data-filter=\"all\">all runs</button><button type=\"button\" data-filter=\"failed\">exceptions</button><button type=\"button\" id=\"comparePicked\">compare picked</button><input type=\"search\" id=\"caseSearch\" placeholder=\"filter ledger\"></section><section class=\"ledger\"><h2>Run ledger</h2><table id=\"caseTable\"><thead><tr><th>run</th><th>suites</th><th>status</th><th>pass</th><th>agents</th><th>artifact root</th><th>compare</th></tr></thead><tbody>{}</tbody></table></section><section class=\"ledger\"><h2>Dataset inventory</h2><table id=\"datasetTable\"><thead><tr><th>dataset</th><th>kind</th><th>split</th><th>payload</th><th>path</th></tr></thead><tbody>{}</tbody></table></section></main>{}</body></html>",
        report_css(),
        latest_line,
        escape_html(&store.root.to_string_lossy()),
        "Editorial Lab Report",
        runs.len(),
        passed_runs,
        failed_runs,
        datasets.len(),
        run_rows,
        dataset_rows,
        report_js(),
    )
}

fn run_index_entry(summary: &RunSummary, artifact_root: &Path, store_root: &Path) -> RunIndexEntry {
    let suites = sorted_unique(summary.cases.iter().map(|case| case.suite_id.clone()));
    let agents = sorted_unique(summary.cases.iter().map(|case| case.agent_id.clone()));
    let namespace = artifact_root
        .strip_prefix(store_root)
        .ok()
        .and_then(|relative| relative.parent())
        .filter(|parent| !parent.as_os_str().is_empty())
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("runs").join(slugify(&summary.project)));
    RunIndexEntry {
        schema_version: SCHEMA_VERSION,
        project: summary.project.clone(),
        project_slug: slugify(&summary.project),
        namespace,
        run_id: summary.run_id.clone(),
        artifact_root: artifact_root.to_path_buf(),
        report_html: artifact_root.join("report.html"),
        report_markdown: artifact_root.join("report.md"),
        started_at_ms: summary.started_at_ms,
        finished_at_ms: summary.finished_at_ms,
        total_cases: summary.total_cases,
        passed_cases: summary.passed_cases,
        failed_cases: summary.failed_cases,
        status: summary.status,
        suites,
        agents,
    }
}

fn read_dataset_entry(path: &Path) -> Result<DatasetEntry> {
    let manifest: DatasetManifest = read_toml(path)?;
    reject_unsupported(manifest.schema_version, path)?;
    let dataset_dir = path
        .parent()
        .with_context(|| format!("dataset manifest has no parent: {}", path.display()))?;
    let payload = resolve_relative(dataset_dir, &manifest.payload);
    Ok(DatasetEntry {
        schema_version: manifest.schema_version,
        id: manifest.id,
        name: manifest.name,
        kind: manifest.kind,
        source: manifest.source,
        payload_exists: payload.exists(),
        payload,
        manifest_path: path.to_path_buf(),
        loader: manifest.loader,
        split: manifest.split,
        sample_limit: manifest.sample_limit,
        cache_key: manifest.cache_key,
        license: manifest.license,
        tags: manifest.tags,
        notes: manifest.notes,
    })
}

fn sorted_unique<I>(values: I) -> Vec<String>
where
    I: IntoIterator<Item = String>,
{
    values
        .into_iter()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn status_filter_value(status: CaseStatus) -> &'static str {
    match status {
        CaseStatus::Passed => "passed",
        CaseStatus::Failed
        | CaseStatus::SetupFailed
        | CaseStatus::RuntimeFailed
        | CaseStatus::ScorerFailed
        | CaseStatus::Timeout => "failed",
    }
}

fn status_filter_value_for_run(status: RunStatus) -> &'static str {
    match status {
        RunStatus::Passed => "passed",
        RunStatus::Failed => "failed",
    }
}

fn status_class(status: CaseStatus) -> &'static str {
    match status {
        CaseStatus::Passed => "present",
        CaseStatus::Failed => "missing",
        CaseStatus::SetupFailed
        | CaseStatus::RuntimeFailed
        | CaseStatus::ScorerFailed
        | CaseStatus::Timeout => "failed",
    }
}

fn status_class_for_run(status: RunStatus) -> &'static str {
    match status {
        RunStatus::Passed => "present",
        RunStatus::Failed => "failed",
    }
}

fn store_link(store: &EvalStore, path: &Path) -> String {
    path.strip_prefix(&store.root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

fn truncate_text(value: &str, limit: usize) -> String {
    if value.chars().count() <= limit {
        return value.to_string();
    }
    let mut out = value
        .chars()
        .take(limit.saturating_sub(1))
        .collect::<String>();
    out.push_str("...");
    out
}

fn report_css() -> &'static str {
    r#"<style>
:root{color-scheme:light;--ink:#201d1a;--muted:#706960;--paper:#f4efe6;--panel:#fffaf0;--line:rgba(32,29,26,.12);--accent:#7f2f22;--ok:#2f6b43;--bad:#9b2d24;--warn:#8a5d14}
*{box-sizing:border-box}body{margin:0;background:var(--paper);color:var(--ink);font:14px/1.5 ui-sans-serif,system-ui,-apple-system,BlinkMacSystemFont,"Segoe UI",sans-serif;-webkit-font-smoothing:antialiased}button,input{font:inherit}a{color:var(--accent);text-decoration:none}a:hover{text-decoration:underline}.page{max-width:1180px;margin:0 auto;padding:32px 20px 56px}.mast{display:flex;align-items:flex-end;justify-content:space-between;gap:24px;padding:24px 0 18px;border-bottom:2px solid var(--ink)}.eyebrow{margin:0 0 8px;color:var(--accent);font-weight:700}.mast h1{margin:0;font-family:Georgia,"Times New Roman",serif;font-size:clamp(34px,6vw,72px);line-height:.95;letter-spacing:0}.subline{margin:12px 0 0;color:var(--muted);max-width:820px}.verdict{min-width:150px;padding:14px 16px;border-radius:8px;background:var(--ink);color:var(--panel);text-align:center;font-family:Georgia,"Times New Roman",serif;font-size:20px}.metrics{display:grid;grid-template-columns:repeat(4,minmax(0,1fr));gap:14px;margin:18px 0}.metrics div{background:var(--panel);border-radius:8px;padding:16px 18px;box-shadow:0 1px 4px rgba(32,29,26,.14)}.metrics span{display:block;color:var(--muted)}.metrics strong{display:block;margin-top:8px;font:700 28px/1 Georgia,"Times New Roman",serif;font-variant-numeric:tabular-nums}.toolbar{display:flex;gap:10px;align-items:center;margin:22px 0;flex-wrap:wrap}.toolbar button,.case-toggle,.dataset-filter{min-height:40px;border:0;border-radius:8px;background:var(--ink);color:var(--panel);padding:9px 13px;cursor:pointer;transition:transform .14s ease,opacity .14s ease}.toolbar button:active,.case-toggle:active,.dataset-filter:active{transform:scale(.96)}.toolbar input{min-height:40px;min-width:240px;border:0;border-radius:8px;background:var(--panel);padding:9px 12px;box-shadow:inset 0 0 0 1px var(--line)}.ledger{margin-top:22px;background:var(--panel);border-radius:8px;padding:16px;box-shadow:0 1px 4px rgba(32,29,26,.14);overflow:auto}.ledger h2{font-family:Georgia,"Times New Roman",serif;margin:0 0 12px;font-size:24px;letter-spacing:0}table{width:100%;border-collapse:collapse;min-width:760px}th,td{padding:11px 12px;border-bottom:1px solid var(--line);text-align:left;vertical-align:top}th{color:var(--muted);font-weight:700}td.num{text-align:right;font-variant-numeric:tabular-nums}.muted{color:var(--muted);font-size:12px}.stamp{display:inline-flex;align-items:center;min-height:26px;border-radius:6px;padding:3px 8px;font-weight:700}.stamp.present{background:rgba(47,107,67,.12);color:var(--ok)}.stamp.missing,.stamp.failed{background:rgba(155,45,36,.12);color:var(--bad)}.links{white-space:nowrap}.links a{margin-right:10px}.case-note{margin-top:8px;color:var(--muted);max-width:420px}.pick{white-space:nowrap;color:var(--muted)}@media(max-width:760px){.mast{display:block}.verdict{margin-top:18px}.metrics{grid-template-columns:repeat(2,minmax(0,1fr))}.page{padding:18px 12px 36px}.toolbar input{width:100%;min-width:0}}
</style>"#
}

fn report_js() -> &'static str {
    r#"<script>
(function(){
  const table=document.getElementById('caseTable');
  const search=document.getElementById('caseSearch');
  const apply=()=>{if(!table)return;const q=(search&&search.value||'').toLowerCase();const active=document.body.dataset.filter||'all';table.querySelectorAll('tbody tr').forEach(row=>{const text=row.textContent.toLowerCase();const status=row.dataset.status||'';row.hidden=(active==='failed'&&status!=='failed')||(q&&!text.includes(q));});};
  document.querySelectorAll('[data-filter]').forEach(btn=>btn.addEventListener('click',()=>{document.body.dataset.filter=btn.dataset.filter;apply();}));
  if(search)search.addEventListener('input',apply);
  document.querySelectorAll('.case-toggle').forEach(btn=>btn.addEventListener('click',()=>{const note=btn.parentElement.querySelector('.case-note');const open=btn.getAttribute('aria-expanded')==='true';btn.setAttribute('aria-expanded',String(!open));if(note)note.hidden=open;}));
  document.querySelectorAll('.dataset-filter').forEach(btn=>btn.addEventListener('click',()=>{if(search){search.value=btn.dataset.dataset||'';apply();}}));
  const compare=document.getElementById('comparePicked');
  if(compare)compare.addEventListener('click',()=>{const picks=[...document.querySelectorAll('.pick input:checked')].map(input=>input.value); if(picks.length>=2){compare.textContent='compare: '+picks.slice(0,2).join(' vs ');} else {compare.textContent='pick two runs';}});
  apply();
})();
</script>"#
}

fn escape_html(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn resolve_store_root(explicit: Option<PathBuf>) -> Result<PathBuf> {
    let env_map = inherited_env();
    let cwd = env::current_dir()?;
    if let Some(path) = explicit {
        return resolve_explicit_path(&path, &env_map, &cwd);
    }
    if let Some(value) = env_value("PEVAL_ROOT", &env_map) {
        return resolve_explicit_path(Path::new(&value), &env_map, &cwd);
    }

    let config_path = resolve_psychevo_home(&env_map, &cwd)?.join("peval.toml");
    if !config_path.is_file() {
        bail!(
            "peval is not initialized; run `peval init` to create {} or pass --root/PEVAL_ROOT",
            config_path.display()
        );
    }
    let config = read_peval_config(&config_path)?;
    if config.root.is_absolute() {
        Ok(config.root)
    } else {
        let base = config_path.parent().unwrap_or_else(|| Path::new("."));
        Ok(base.join(config.root))
    }
}

fn read_peval_config(path: &Path) -> Result<PevalConfig> {
    let config: PevalConfig = read_toml(path)?;
    reject_unsupported(config.schema_version, path)?;
    Ok(config)
}

fn resolve_psychevo_home(env_map: &BTreeMap<String, String>, cwd: &Path) -> Result<PathBuf> {
    if let Some(value) = env_value("PSYCHEVO_HOME", env_map) {
        resolve_explicit_path(Path::new(&value), env_map, cwd)
    } else {
        resolve_explicit_path(Path::new("~/.psychevo"), env_map, cwd)
    }
}

fn resolve_cli_path(path: &Path) -> Result<PathBuf> {
    let env_map = inherited_env();
    resolve_explicit_path(path, &env_map, &env::current_dir()?)
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
        .ok_or_else(|| anyhow::anyhow!("HOME is required to expand ~"))
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

fn validate_store_namespace(path: &Path) -> Result<PathBuf> {
    if path.as_os_str().is_empty() {
        bail!("output_root must not be empty");
    }
    if path.is_absolute() {
        bail!("output_root must be relative to the peval root");
    }
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Normal(part) => out.push(part),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                bail!("output_root must not escape the peval root")
            }
        }
    }
    if out.as_os_str().is_empty() {
        bail!("output_root must name a store namespace");
    }
    Ok(out)
}

fn resolve_relative(base: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        base.join(path)
    }
}

fn resolve_command_part(part: &str, task_dir: &Path) -> OsString {
    let candidate = task_dir.join(part);
    if candidate.exists() {
        return absolute_path(&candidate).into_os_string();
    }
    if looks_like_relative_path(part) {
        absolute_path(&resolve_relative(task_dir, Path::new(part))).into_os_string()
    } else {
        OsString::from(part)
    }
}

fn absolute_path(path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(path)
    }
}

fn looks_like_relative_path(value: &str) -> bool {
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

fn is_declared_path(value: &str, task_dir: &Path) -> bool {
    task_dir.join(value).exists() || looks_like_relative_path(value)
}

fn sanitize_id(value: &str) -> String {
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

fn slugify(value: &str) -> String {
    let slug = sanitize_id(&value.to_ascii_lowercase())
        .trim_matches('_')
        .to_string();
    if slug.is_empty() {
        "evaluation".to_string()
    } else {
        slug
    }
}

fn generate_run_id() -> String {
    Uuid::now_v7().to_string()
}

fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

fn reject_unsupported(schema_version: u32, path: &Path) -> Result<()> {
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

fn reject_unsupported_result_schema(schema_version: u32) -> Result<()> {
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
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
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
struct InitArgs {
    #[arg(long = "root", value_name = "DIR")]
    root: Option<PathBuf>,
    #[arg(long)]
    force: bool,
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Subcommand)]
enum DatasetCommands {
    #[command(about = "Register a local dataset payload")]
    Import(DatasetImportArgs),
}

#[derive(Debug, Parser)]
struct ProjectArgs {
    #[arg(short = 'c', long = "config", value_name = "PATH")]
    config: Option<PathBuf>,
    #[arg(long = "root", value_name = "DIR")]
    store_root: Option<PathBuf>,
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Parser)]
struct ListArgs {
    #[arg(short = 'c', long = "config", value_name = "PATH")]
    config: Option<PathBuf>,
    #[arg(long = "root", value_name = "DIR")]
    store_root: Option<PathBuf>,
    #[arg(long, value_enum, default_value_t = ListKind::All)]
    kind: ListKind,
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum ListKind {
    All,
    Suites,
    Agents,
    Tasks,
    Reports,
    Runs,
    Datasets,
}

#[derive(Debug, Parser)]
struct SelectArgs {
    #[arg(short = 'c', long = "config", value_name = "PATH")]
    config: Option<PathBuf>,
    #[arg(long = "root", value_name = "DIR")]
    store_root: Option<PathBuf>,
    #[arg(long)]
    suite: Option<String>,
    #[arg(long)]
    agent: Option<String>,
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Parser)]
struct RunArgs {
    #[arg(short = 'c', long = "config", value_name = "PATH")]
    config: Option<PathBuf>,
    #[arg(long = "root", value_name = "DIR")]
    store_root: Option<PathBuf>,
    #[arg(long)]
    suite: Option<String>,
    #[arg(long)]
    agent: Option<String>,
    #[arg(long)]
    run_id: Option<String>,
    #[arg(long, value_name = "DIR")]
    output_root: Option<PathBuf>,
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Parser)]
struct ReportArgs {
    #[arg(short = 'c', long = "config", value_name = "PATH")]
    config: Option<PathBuf>,
    #[arg(long = "root", value_name = "DIR")]
    store_root: Option<PathBuf>,
    #[arg(long, value_name = "RUN")]
    run_root: PathBuf,
    #[arg(long)]
    suite: Option<String>,
    #[arg(long)]
    agent: Option<String>,
    #[arg(long, value_enum)]
    status: Option<RunStatusFilter>,
    #[arg(long, value_enum, default_value_t = ReportFormat::Markdown)]
    format: ReportFormat,
    #[arg(long, value_name = "PATH")]
    output: Option<PathBuf>,
}

#[derive(Debug, Parser)]
struct CompareArgs {
    #[arg(value_name = "RUN_ROOT", required = true)]
    run_roots: Vec<PathBuf>,
    #[arg(short = 'c', long = "config", value_name = "PATH")]
    config: Option<PathBuf>,
    #[arg(long = "root", value_name = "DIR")]
    store_root: Option<PathBuf>,
    #[arg(long)]
    suite: Option<String>,
    #[arg(long)]
    agent: Option<String>,
    #[arg(long, value_enum)]
    status: Option<RunStatusFilter>,
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Parser)]
struct ReplayArgs {
    #[arg(short = 'c', long = "config", value_name = "PATH")]
    config: Option<PathBuf>,
    #[arg(long = "root", value_name = "DIR")]
    store_root: Option<PathBuf>,
    #[arg(long, value_name = "RUN")]
    run_root: PathBuf,
    #[arg(long)]
    suite: Option<String>,
    #[arg(long)]
    agent: Option<String>,
    #[arg(long, value_enum)]
    status: Option<RunStatusFilter>,
    #[arg(long)]
    case: Option<String>,
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Parser)]
struct DatasetImportArgs {
    #[arg(value_name = "PATH")]
    path: PathBuf,
    #[arg(long = "root", value_name = "DIR")]
    store_root: Option<PathBuf>,
    #[arg(long)]
    id: Option<String>,
    #[arg(long)]
    name: Option<String>,
    #[arg(long)]
    kind: Option<String>,
    #[arg(long)]
    loader: Option<String>,
    #[arg(long)]
    split: Option<String>,
    #[arg(long)]
    sample_limit: Option<usize>,
    #[arg(long)]
    cache_key: Option<String>,
    #[arg(long)]
    license: Option<String>,
    #[arg(long = "tag")]
    tags: Vec<String>,
    #[arg(long)]
    notes: Option<String>,
    #[arg(long)]
    json: bool,
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

fn dispatch_cli(cli: Cli) -> Result<CliOutcome> {
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

fn run_init(args: InitArgs) -> Result<CliOutcome> {
    let config = init_eval_store(InitStoreRequest {
        root: args.root,
        force: args.force,
    })?;
    if args.json {
        return Ok(success(serde_json::to_string_pretty(&config)?));
    }
    Ok(success(format!("peval root: {}\n", config.root.display())))
}

fn run_doctor(args: ProjectArgs) -> Result<CliOutcome> {
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

fn run_list(args: ListArgs) -> Result<CliOutcome> {
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

fn run_check(args: SelectArgs) -> Result<CliOutcome> {
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

fn run_run(args: RunArgs) -> Result<CliOutcome> {
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

fn run_report(args: ReportArgs) -> Result<CliOutcome> {
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

fn run_compare(args: CompareArgs) -> Result<CliOutcome> {
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

fn run_replay(args: ReplayArgs) -> Result<CliOutcome> {
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

fn run_dataset(args: DatasetCommands) -> Result<CliOutcome> {
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

fn resolve_cli_run_selector(
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

fn success(stdout: String) -> CliOutcome {
    CliOutcome {
        code: 0,
        stdout,
        stderr: String::new(),
    }
}

fn list_tasks(project: &EvalProject) -> Result<Vec<Value>> {
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
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use std::sync::Mutex;

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

    fn fixture_project() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures/local-rust-swe")
    }

    #[test]
    fn project_discovery_validation_and_matrix_are_deterministic() {
        let project =
            EvalProject::load(fixture_project().join("tasks/rust-swe-add")).expect("project load");
        assert_eq!(project.name, "local-rust-swe");
        let cases = check_project(&project, Some("rust-swe"), None).expect("check");
        let ids = cases
            .iter()
            .map(|case| case.case_id.as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            ids,
            [
                "rust-swe__rust-swe-add__fake-pass",
                "rust-swe__rust-swe-add__fake-fail"
            ]
        );
    }

    #[test]
    fn unsupported_schema_is_rejected() {
        let temp = tempfile::tempdir().expect("temp");
        write_minimal_project(temp.path(), 99, r#"agents = ["fake-pass"]"#);
        let err = EvalProject::load(temp.path()).expect_err("unsupported schema");
        assert!(
            err.to_string().contains("unsupported schema_version 99"),
            "{err:#}"
        );
    }

    #[test]
    fn psychevo_live_agent_requires_manifest_opt_in() {
        let temp = tempfile::tempdir().expect("temp");
        write_minimal_project(temp.path(), 1, r#"agents = ["psychevo-live"]"#);
        fs::write(
            temp.path().join("agents/psychevo-live.toml"),
            "schema_version = 1\nid = \"psychevo-live\"\nkind = \"psychevo\"\n[psychevo]\ncommand = \"pevo\"\n",
        )
        .expect("psychevo agent");
        let project = EvalProject::load(temp.path()).expect("project");
        let err = check_project(&project, Some("suite"), Some("psychevo-live"))
            .expect_err("live agent should be gated");
        assert!(err.to_string().contains("allow_live = false"), "{err:#}");
    }

    #[test]
    fn fake_agents_write_artifacts_reports_compare_and_replay() {
        let temp = tempfile::tempdir().expect("temp");
        let run_one = run_evaluation(RunRequest {
            config: Some(fixture_project().join("eval.toml")),
            suite: Some("rust-swe".to_string()),
            agent: None,
            run_id: Some("fixture-one".to_string()),
            store_root: None,
            output_root: Some(temp.path().to_path_buf()),
        })
        .expect("run");
        assert_eq!(run_one.status, RunStatus::Failed);
        assert_eq!(run_one.passed_cases, 1);
        assert_eq!(run_one.failed_cases, 1);
        let root_one = temp.path().join("fixture-one");
        assert!(root_one.join("summary.json").is_file());
        assert!(
            root_one
                .join("cases/rust-swe__rust-swe-add__fake-pass/result.json")
                .is_file()
        );
        assert!(
            !root_one
                .join("cases/rust-swe__rust-swe-add__fake-pass/workspace")
                .exists(),
            "case workspaces should not be retained as artifacts"
        );
        let markdown = render_report(ReportRequest {
            run_root: root_one.clone(),
            format: ReportFormat::Markdown,
        })
        .expect("markdown");
        assert!(markdown.contains("fake-pass"));
        let html = render_report(ReportRequest {
            run_root: root_one.clone(),
            format: ReportFormat::Html,
        })
        .expect("html");
        assert!(html.contains("id=\"caseTable\""));
        let json_report = render_report(ReportRequest {
            run_root: root_one.clone(),
            format: ReportFormat::Json,
        })
        .expect("json");
        assert!(json_report.contains("\"schema_version\": 1"));

        let run_two = run_evaluation(RunRequest {
            config: Some(fixture_project().join("eval.toml")),
            suite: Some("rust-swe".to_string()),
            agent: Some("fake-pass".to_string()),
            run_id: Some("fixture-two".to_string()),
            store_root: None,
            output_root: Some(temp.path().to_path_buf()),
        })
        .expect("run two");
        assert_eq!(run_two.status, RunStatus::Passed);
        let compare = compare_runs(CompareRequest {
            run_roots: vec![root_one.clone(), temp.path().join("fixture-two")],
        })
        .expect("compare");
        assert_eq!(compare.runs.len(), 2);
        assert!(
            compare
                .cases
                .iter()
                .any(|case| case.key == "rust-swe/rust-swe-add/fake-pass")
        );
        let replay = replay_run(ReplayRequest {
            run_root: root_one,
            case_id: Some("rust-swe__rust-swe-add__fake-pass".to_string()),
        })
        .expect("replay");
        assert!(
            replay
                .events
                .iter()
                .any(|event| event.kind == "scorer_finished")
        );
    }

    #[test]
    fn eval_store_init_default_root_env_root_output_bypass_and_manifest_namespace() {
        let _lock = ENV_LOCK.lock().expect("env lock");
        let _env = set_env_var("PEVAL_ROOT", None);
        let temp = tempfile::tempdir().expect("temp");
        let psychevo_home = temp.path().join("psychevo-home");
        let user_home = temp.path().join("user-home");
        fs::create_dir_all(&user_home).expect("user home");
        let _psychevo_home = set_env_var("PSYCHEVO_HOME", Some(&psychevo_home));
        let _home = set_env_var("HOME", Some(&user_home));

        let project_root = temp.path().join("project");
        fs::create_dir_all(&project_root).expect("project dir");
        write_minimal_project(&project_root, 1, r#"agents = ["fake-pass"]"#);

        let uninitialized = run_evaluation(RunRequest {
            config: Some(project_root.join("eval.toml")),
            suite: Some("suite".to_string()),
            agent: Some("fake-pass".to_string()),
            run_id: Some("uninitialized".to_string()),
            store_root: None,
            output_root: None,
        })
        .expect_err("uninitialized store should fail");
        assert!(uninitialized.to_string().contains("peval init"));

        let initialized = init_eval_store(InitStoreRequest {
            root: None,
            force: false,
        })
        .expect("init default root");
        assert_eq!(initialized.root, user_home.join(".local/evals"));
        assert!(psychevo_home.join("peval.toml").is_file());
        assert!(initialized.root.join("runs").is_dir());
        assert!(initialized.root.join("datasets").is_dir());
        assert!(initialized.root.join("index.json").is_file());
        assert!(initialized.root.join("dashboard.html").is_file());

        let same_init = init_eval_store(InitStoreRequest {
            root: None,
            force: false,
        })
        .expect("idempotent init");
        assert_eq!(same_init.root, initialized.root);

        let replacement_root = temp.path().join("replacement-root");
        let replace_without_force = init_eval_store(InitStoreRequest {
            root: Some(replacement_root),
            force: false,
        })
        .expect_err("changing root requires force");
        assert!(replace_without_force.to_string().contains("--force"));

        let default = run_evaluation(RunRequest {
            config: Some(project_root.join("eval.toml")),
            suite: Some("suite".to_string()),
            agent: Some("fake-pass".to_string()),
            run_id: Some("default-root".to_string()),
            store_root: None,
            output_root: None,
        })
        .expect("default root");
        assert_eq!(
            default.artifact_root,
            initialized.root.join("runs/bad").join("default-root")
        );
        assert!(initialized.root.join("index.json").is_file());
        assert!(default.artifact_root.join("report.html").is_file());
        assert!(default.artifact_root.join("report.md").is_file());

        let flag_root = temp.path().join("flag-root");
        let by_flag = run_evaluation(RunRequest {
            config: Some(project_root.join("eval.toml")),
            suite: Some("suite".to_string()),
            agent: Some("fake-pass".to_string()),
            run_id: Some("flag-root".to_string()),
            store_root: Some(flag_root.clone()),
            output_root: None,
        })
        .expect("flag root");
        assert_eq!(
            by_flag.artifact_root,
            flag_root.join("runs/bad").join("flag-root")
        );

        let env_root = temp.path().join("env-root");
        {
            let _env = set_env_var("PEVAL_ROOT", Some(&env_root));
            let by_env = run_evaluation(RunRequest {
                config: Some(project_root.join("eval.toml")),
                suite: Some("suite".to_string()),
                agent: Some("fake-pass".to_string()),
                run_id: Some("env-root".to_string()),
                store_root: None,
                output_root: None,
            })
            .expect("env root");
            assert_eq!(by_env.artifact_root, env_root.join("runs/bad/env-root"));
        }

        let bypass_root = temp.path().join("bypass-store");
        let external = temp.path().join("external");
        let bypass = run_evaluation(RunRequest {
            config: Some(project_root.join("eval.toml")),
            suite: Some("suite".to_string()),
            agent: Some("fake-pass".to_string()),
            run_id: Some("external-run".to_string()),
            store_root: Some(bypass_root.clone()),
            output_root: Some(external.clone()),
        })
        .expect("external run");
        assert_eq!(bypass.artifact_root, external.join("external-run"));
        assert!(
            !bypass_root.join("index.json").exists(),
            "explicit output-root should not register in EvalStore"
        );

        let legacy_root = temp.path().join("legacy-project");
        fs::create_dir_all(&legacy_root).expect("legacy dir");
        write_minimal_project(&legacy_root, 1, r#"agents = ["fake-pass"]"#);
        fs::write(
            legacy_root.join("eval.toml"),
            "schema_version = 1\nname = \"legacy\"\noutput_root = \"legacy-runs\"\n",
        )
        .expect("legacy manifest");
        let namespace_store = temp.path().join("namespace-store");
        let legacy = run_evaluation(RunRequest {
            config: Some(legacy_root.join("eval.toml")),
            suite: Some("suite".to_string()),
            agent: Some("fake-pass".to_string()),
            run_id: Some("legacy-run".to_string()),
            store_root: Some(namespace_store.clone()),
            output_root: None,
        })
        .expect("legacy run");
        assert_eq!(
            legacy.artifact_root,
            namespace_store.join("legacy-runs/legacy-run")
        );
        assert!(namespace_store.join("legacy-runs/latest.json").is_file());

        fs::write(
            legacy_root.join("eval.toml"),
            "schema_version = 1\nname = \"legacy\"\noutput_root = \"../outside\"\n",
        )
        .expect("invalid namespace manifest");
        let invalid = run_evaluation(RunRequest {
            config: Some(legacy_root.join("eval.toml")),
            suite: Some("suite".to_string()),
            agent: Some("fake-pass".to_string()),
            run_id: Some("invalid".to_string()),
            store_root: Some(namespace_store),
            output_root: None,
        })
        .expect_err("invalid namespace");
        assert!(invalid.to_string().contains("output_root"));
    }

    #[test]
    fn eval_store_index_fallback_latest_dataset_import_and_dashboard() {
        let _lock = ENV_LOCK.lock().expect("env lock");
        let _env = set_env_var("PEVAL_ROOT", None);
        let temp = tempfile::tempdir().expect("temp");
        let store_root = temp.path().join("store");
        let project = fixture_project();

        let failed = run_evaluation(RunRequest {
            config: Some(project.join("eval.toml")),
            suite: Some("rust-swe".to_string()),
            agent: None,
            run_id: Some("store-failed".to_string()),
            store_root: Some(store_root.clone()),
            output_root: None,
        })
        .expect("failed run");
        assert_eq!(failed.status, RunStatus::Failed);
        let passed = run_evaluation(RunRequest {
            config: Some(project.join("eval.toml")),
            suite: Some("rust-swe".to_string()),
            agent: Some("fake-pass".to_string()),
            run_id: Some("store-passed".to_string()),
            store_root: Some(store_root.clone()),
            output_root: None,
        })
        .expect("passed run");
        assert_eq!(passed.status, RunStatus::Passed);

        let loaded = EvalProject::load(&project).expect("project");
        let store = EvalStore::new(store_root.clone());
        let namespace = loaded.namespace().expect("namespace");
        let failed_latest = store
            .resolve_run_selector(
                Some(&namespace),
                Path::new("latest"),
                &RunSelectorFilters {
                    suite: Some("rust-swe".to_string()),
                    agent: None,
                    status: Some(RunStatusFilter::Failed),
                },
            )
            .expect("latest failed");
        assert_eq!(failed_latest, failed.artifact_root);

        fs::write(store_root.join("index.json"), "{not-json").expect("corrupt index");
        let fallback_runs = store.list_runs().expect("fallback scan");
        assert!(fallback_runs.iter().any(|run| run.run_id == "store-failed"));
        assert!(fallback_runs.iter().any(|run| run.run_id == "store-passed"));

        let payload = temp.path().join("tasks.jsonl");
        fs::write(&payload, "{\"prompt\":\"x\"}\n").expect("payload");
        let dataset = import_dataset(DatasetImportRequest {
            store_root: Some(store_root.clone()),
            path: payload.clone(),
            id: Some("GDPVal Mini".to_string()),
            name: None,
            kind: Some("gdpval".to_string()),
            loader: Some("jsonl".to_string()),
            split: Some("mini".to_string()),
            sample_limit: Some(1),
            cache_key: Some("gdpval-mini".to_string()),
            license: Some("local".to_string()),
            tags: vec!["fixture".to_string()],
            notes: Some("local reference".to_string()),
        })
        .expect("dataset import");
        assert_eq!(dataset.id, "gdpval_mini");
        assert!(dataset.payload_exists);
        assert!(
            store_root
                .join("datasets/gdpval_mini/dataset.toml")
                .is_file()
        );

        let dashboard = fs::read_to_string(store_root.join("dashboard.html")).expect("dashboard");
        assert!(dashboard.contains("Evaluation results center"));
        assert!(dashboard.contains("gdpval_mini"));
        assert!(!dashboard.contains("scorer_finished"));

        let report = fs::read_to_string(passed.artifact_root.join("report.html")).expect("report");
        assert!(report.contains("Psychevo evaluation report"));
        assert!(report.contains("trajectory"));
        assert!(!report.contains("case execution started"));
    }

    #[test]
    fn scorer_failure_malformed_json_and_timeout_are_classified() {
        let temp = tempfile::tempdir().expect("temp");
        let project = create_scorer_project(temp.path());
        let summary = run_evaluation(RunRequest {
            config: Some(project.join("eval.toml")),
            suite: Some("scorer-suite".to_string()),
            agent: Some("fake-pass".to_string()),
            run_id: Some("scorer-cases".to_string()),
            store_root: None,
            output_root: Some(temp.path().join("runs")),
        })
        .expect("run");
        let statuses = summary
            .cases
            .iter()
            .map(|case| (case.task_id.as_str(), case.status))
            .collect::<BTreeMap<_, _>>();
        assert_eq!(statuses["scorer-success"], CaseStatus::Passed);
        assert_eq!(statuses["scorer-failure"], CaseStatus::ScorerFailed);
        assert_eq!(statuses["scorer-malformed"], CaseStatus::ScorerFailed);
        assert_eq!(statuses["scorer-timeout"], CaseStatus::Timeout);
    }

    #[test]
    fn psychevo_adapter_preserves_runtime_observation_events_in_trajectory() {
        let temp = tempfile::tempdir().expect("temp");
        let project = create_psychevo_trace_project(temp.path());
        let summary = run_evaluation(RunRequest {
            config: Some(project.join("eval.toml")),
            suite: Some("trace-suite".to_string()),
            agent: Some("psychevo-trace".to_string()),
            run_id: Some("trace-run".to_string()),
            store_root: Some(temp.path().join("store")),
            output_root: None,
        })
        .expect("run");
        assert_eq!(summary.status, RunStatus::Passed);
        let trajectory = fs::read_to_string(
            summary
                .artifact_root
                .join("cases/trace-suite__trace-task__psychevo-trace/trajectory.jsonl"),
        )
        .expect("trajectory");
        assert!(trajectory.contains("\"kind\":\"psychevo_run_start\""));
        assert!(trajectory.contains("\"kind\":\"psychevo_tool_execution_start\""));
        assert!(trajectory.contains("\"raw_event\":{\"session_id\":\"trace-session\""));
        assert!(trajectory.contains("agent stderr line"));
    }

    #[test]
    fn cli_smoke_covers_all_commands() {
        let _lock = ENV_LOCK.lock().expect("env lock");
        let _env = set_env_var("PEVAL_ROOT", None);
        let temp = tempfile::tempdir().expect("temp");
        let psychevo_home = temp.path().join("psychevo-home");
        let user_home = temp.path().join("user-home");
        fs::create_dir_all(&user_home).expect("user home");
        let _psychevo_home = set_env_var("PSYCHEVO_HOME", Some(&psychevo_home));
        let _home = set_env_var("HOME", Some(&user_home));

        let project = fixture_project();
        let config = project.join("eval.toml");
        let store_root = temp.path().join("cli-store");

        let init = run_cli_from([
            "peval",
            "init",
            "--root",
            store_root.to_str().unwrap(),
            "--json",
        ]);
        assert_eq!(init.code, 0, "{}", init.stderr);
        assert!(init.stdout.contains("cli-store"));

        let removed_project =
            run_cli_from(["peval", "check", "--project", project.to_str().unwrap()]);
        assert_eq!(removed_project.code, 2);

        let doctor = run_cli_from(["peval", "doctor", "-c", config.to_str().unwrap(), "--json"]);
        assert_eq!(doctor.code, 0, "{}", doctor.stderr);
        let list = run_cli_from(["peval", "list", "-c", config.to_str().unwrap(), "--json"]);
        assert_eq!(list.code, 0, "{}", list.stderr);
        assert!(!list.stdout.to_ascii_lowercase().contains("csv"));
        let check = run_cli_from([
            "peval",
            "check",
            "-c",
            config.to_str().unwrap(),
            "--suite",
            "rust-swe",
            "--json",
        ]);
        assert_eq!(check.code, 0, "{}", check.stderr);
        let run = run_cli_from([
            "peval",
            "run",
            "-c",
            config.to_str().unwrap(),
            "--suite",
            "rust-swe",
            "--agent",
            "fake-pass",
            "--run-id",
            "cli-smoke",
            "--output-root",
            temp.path().to_str().unwrap(),
            "--json",
        ]);
        assert_eq!(run.code, 0, "{}", run.stderr);
        let run_root = temp.path().join("cli-smoke");
        let failing_run = run_cli_from([
            "peval",
            "run",
            "-c",
            config.to_str().unwrap(),
            "--suite",
            "rust-swe",
            "--run-id",
            "cli-smoke-failing-suite",
            "--output-root",
            temp.path().to_str().unwrap(),
            "--json",
        ]);
        assert_eq!(failing_run.code, 1);
        assert!(failing_run.stdout.contains("\"failed_cases\": 1"));
        let report = run_cli_from([
            "peval",
            "report",
            "--run-root",
            run_root.to_str().unwrap(),
            "--format",
            "json",
        ]);
        assert_eq!(report.code, 0, "{}", report.stderr);
        let compare = run_cli_from([
            "peval",
            "compare",
            run_root.to_str().unwrap(),
            run_root.to_str().unwrap(),
            "--json",
        ]);
        assert_eq!(compare.code, 0, "{}", compare.stderr);
        let replay = run_cli_from([
            "peval",
            "replay",
            "--run-root",
            run_root.to_str().unwrap(),
            "--json",
        ]);
        assert_eq!(replay.code, 0, "{}", replay.stderr);

        let payload = temp.path().join("dataset.jsonl");
        fs::write(&payload, "{\"prompt\":\"hello\"}\n").expect("dataset payload");
        let dataset = run_cli_from([
            "peval",
            "dataset",
            "import",
            payload.to_str().unwrap(),
            "--id",
            "cli-data",
            "--kind",
            "jsonl",
            "--json",
        ]);
        assert_eq!(dataset.code, 0, "{}", dataset.stderr);
        assert!(dataset.stdout.contains("\"id\": \"cli-data\""));

        let store_run = run_cli_from([
            "peval",
            "run",
            "-c",
            config.to_str().unwrap(),
            "--suite",
            "rust-swe",
            "--agent",
            "fake-pass",
            "--run-id",
            "cli-store-run",
            "--json",
        ]);
        assert_eq!(store_run.code, 0, "{}", store_run.stderr);
        assert!(
            store_root
                .join("runs/local-rust-swe/cli-store-run/report.html")
                .is_file()
        );
        assert!(store_root.join("dashboard.html").is_file());

        let list_runs = run_cli_from(["peval", "list", "--kind", "runs", "--json"]);
        assert_eq!(list_runs.code, 0, "{}", list_runs.stderr);
        assert!(list_runs.stdout.contains("cli-store-run"));

        let list_datasets = run_cli_from(["peval", "list", "--kind", "datasets", "--json"]);
        assert_eq!(list_datasets.code, 0, "{}", list_datasets.stderr);
        assert!(list_datasets.stdout.contains("cli-data"));

        let latest_report = run_cli_from([
            "peval",
            "report",
            "-c",
            config.to_str().unwrap(),
            "--run-root",
            "latest",
            "--agent",
            "fake-pass",
            "--status",
            "passed",
            "--format",
            "json",
        ]);
        assert_eq!(latest_report.code, 0, "{}", latest_report.stderr);
        assert!(
            latest_report
                .stdout
                .contains("\"run_id\": \"cli-store-run\"")
        );

        let latest_compare = run_cli_from([
            "peval",
            "compare",
            "latest",
            "local-rust-swe/cli-store-run",
            "-c",
            config.to_str().unwrap(),
            "--agent",
            "fake-pass",
            "--status",
            "passed",
            "--json",
        ]);
        assert_eq!(latest_compare.code, 0, "{}", latest_compare.stderr);

        let latest_replay = run_cli_from([
            "peval",
            "replay",
            "--run-root",
            "latest",
            "--agent",
            "fake-pass",
            "--status",
            "passed",
            "--json",
        ]);
        assert_eq!(latest_replay.code, 0, "{}", latest_replay.stderr);
    }

    fn write_minimal_project(root: &Path, schema_version: u32, suite_extra: &str) {
        fs::create_dir_all(root.join("agents")).expect("agents");
        fs::create_dir_all(root.join("suites")).expect("suites");
        fs::create_dir_all(root.join("tasks/task/workspace")).expect("workspace");
        fs::write(
            root.join("eval.toml"),
            format!("schema_version = {schema_version}\nname = \"bad\"\n"),
        )
        .expect("project");
        fs::write(
            root.join("agents/fake-pass.toml"),
            "schema_version = 1\nid = \"fake-pass\"\nkind = \"fake\"\n[fake]\nbehavior = \"pass\"\n",
        )
        .expect("agent");
        fs::write(
            root.join("suites/suite.toml"),
            format!(
                "schema_version = 1\nid = \"suite\"\n{}\ntasks = [\"../tasks/task/task.toml\"]\n",
                suite_extra
            ),
        )
        .expect("suite");
        fs::write(
            root.join("tasks/task/task.toml"),
            "schema_version = 1\nid = \"task\"\n[prompt]\ntext = \"fix\"\n[workspace]\nsource = \"workspace\"\n[scorer]\ncommand = [\"sh\", \"score.sh\"]\n",
        )
        .expect("task");
        fs::write(
            root.join("tasks/task/score.sh"),
            "echo '{\"schema_version\":1,\"passed\":true,\"score\":1.0,\"message\":\"ok\"}'\n",
        )
        .expect("score");
    }

    fn create_scorer_project(root: &Path) -> PathBuf {
        fs::create_dir_all(root.join("agents")).expect("agents");
        fs::create_dir_all(root.join("suites")).expect("suites");
        fs::write(
            root.join("eval.toml"),
            "schema_version = 1\nname = \"scorer-project\"\n",
        )
        .expect("project");
        fs::write(
            root.join("agents/fake-pass.toml"),
            "schema_version = 1\nid = \"fake-pass\"\nkind = \"fake\"\n[fake]\nbehavior = \"pass\"\n",
        )
        .expect("agent");
        let tasks = [
            (
                "scorer-success",
                "echo '{\"schema_version\":1,\"passed\":true,\"score\":1.0,\"message\":\"ok\"}'\n",
            ),
            ("scorer-failure", "echo scorer failed >&2\nexit 7\n"),
            ("scorer-malformed", "echo not-json\n"),
            ("scorer-timeout", "sleep 2\n"),
        ];
        let mut task_paths = Vec::new();
        for (id, script) in tasks {
            let dir = root.join("tasks").join(id);
            fs::create_dir_all(dir.join("workspace")).expect("workspace");
            fs::write(dir.join("workspace/README.md"), id).expect("readme");
            fs::write(dir.join("score.sh"), script).expect("score");
            let timeout = if id == "scorer-timeout" {
                "timeout_seconds = 1\n"
            } else {
                ""
            };
            fs::write(
                dir.join("task.toml"),
                format!(
                    "schema_version = 1\nid = \"{id}\"\n[prompt]\ntext = \"score\"\n[workspace]\nsource = \"workspace\"\n[scorer]\ncommand = [\"sh\", \"score.sh\"]\n{timeout}"
                ),
            )
            .expect("task");
            task_paths.push(format!("\"../tasks/{id}/task.toml\""));
        }
        fs::write(
            root.join("suites/scorer.toml"),
            format!(
                "schema_version = 1\nid = \"scorer-suite\"\nagents = [\"fake-pass\"]\ntasks = [{}]\n",
                task_paths.join(", ")
            ),
        )
        .expect("suite");
        root.to_path_buf()
    }

    fn create_psychevo_trace_project(root: &Path) -> PathBuf {
        fs::create_dir_all(root.join("agents")).expect("agents");
        fs::create_dir_all(root.join("suites")).expect("suites");
        let task_dir = root.join("tasks/trace-task");
        fs::create_dir_all(task_dir.join("workspace")).expect("workspace");
        fs::write(
            root.join("eval.toml"),
            "schema_version = 1\nname = \"trace-project\"\nallow_live = true\n",
        )
        .expect("project");
        fs::write(
            root.join("agents/psychevo-trace.toml"),
            "schema_version = 1\nid = \"psychevo-trace\"\nkind = \"psychevo\"\n[psychevo]\ncommand = \"sh\"\nargs = [\"agent.sh\", \"{workspace}\", \"{prompt}\"]\n",
        )
        .expect("agent");
        fs::write(
            root.join("suites/trace.toml"),
            "schema_version = 1\nid = \"trace-suite\"\nagents = [\"psychevo-trace\"]\ntasks = [\"../tasks/trace-task/task.toml\"]\n",
        )
        .expect("suite");
        fs::write(
            task_dir.join("task.toml"),
            "schema_version = 1\nid = \"trace-task\"\n[prompt]\ntext = \"trace\"\n[workspace]\nsource = \"workspace\"\n[scorer]\ncommand = [\"sh\", \"score.sh\"]\n",
        )
        .expect("task");
        fs::write(
            task_dir.join("score.sh"),
            "echo '{\"schema_version\":1,\"passed\":true,\"score\":1.0,\"message\":\"ok\"}'\n",
        )
        .expect("score");
        fs::write(
            task_dir.join("agent.sh"),
            r#"#!/bin/sh
printf '%s\n' '{"type":"run_start","session_id":"trace-session","workdir":"workspace"}'
printf '%s\n' '{"type":"message_update","role":"assistant","delta":"editing"}'
printf '%s\n' '{"type":"tool_execution_start","tool_name":"write","tool_call_id":"call-1"}'
printf '%s\n' '{"type":"agent_end","outcome":"normal","final_answer":"done"}'
echo "agent stderr line" >&2
"#,
        )
        .expect("agent script");
        root.to_path_buf()
    }
}
