use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
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
    pub output_root: PathBuf,
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
    pub project_root: PathBuf,
    pub suite: Option<String>,
    pub agent: Option<String>,
    pub run_id: Option<String>,
    pub output_root: Option<PathBuf>,
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

#[derive(Debug, Clone)]
struct ProjectManifest {
    schema_version: u32,
    name: String,
    output_root: PathBuf,
    allow_live: bool,
}

#[derive(Debug, Deserialize)]
struct RawProjectManifest {
    schema_version: u32,
    #[serde(default = "default_project_name")]
    name: String,
    #[serde(default = "default_output_root")]
    output_root: PathBuf,
    #[serde(default)]
    allow_live: bool,
}

fn default_project_name() -> String {
    "evaluation".to_string()
}

fn default_output_root() -> PathBuf {
    PathBuf::from("target/peval/runs")
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

    pub fn output_base(&self) -> PathBuf {
        resolve_relative(&self.root, &self.output_root)
    }
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
    let project = EvalProject::load(&request.project_root)?;
    let cases = expand_matrix(&project, request.suite.as_deref(), request.agent.as_deref())?;
    if cases.is_empty() {
        bail!("no cases selected");
    }
    for case in &cases {
        validate_case(&project, case)?;
    }

    let run_id = request.run_id.unwrap_or_else(generate_run_id);
    let output_base = request
        .output_root
        .map(|path| resolve_relative(&project.root, &path))
        .unwrap_or_else(|| project.output_base());
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
            let output = run_psychevo_agent(&case.agent, &case.task.dir, workspace, &prompt)?;
            push_event(
                events,
                &case.case_id,
                "psychevo_agent_finished",
                "Psychevo live adapter command finished",
                json!({
                    "exit_code": output.code,
                    "stdout": output.stdout,
                    "stderr": output.stderr,
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

fn write_jsonl(path: &Path, events: &[TrajectoryEvent]) -> Result<()> {
    let mut content = String::new();
    for event in events {
        content.push_str(&serde_json::to_string(event)?);
        content.push('\n');
    }
    fs::write(path, content).with_context(|| format!("failed to write {}", path.display()))
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
            out.push_str(&format!("- cases: {}\n", summary.total_cases));
            out.push_str(&format!("- passed: {}\n", summary.passed_cases));
            out.push_str(&format!("- failed: {}\n\n", summary.failed_cases));
            out.push_str("| case | suite | task | agent | status | score |\n");
            out.push_str("| --- | --- | --- | --- | --- | --- |\n");
            for case in &summary.cases {
                out.push_str(&format!(
                    "| `{}` | `{}` | `{}` | `{}` | {:?} | {} |\n",
                    case.case_id,
                    case.suite_id,
                    case.task_id,
                    case.agent_id,
                    case.status,
                    case.score.score.unwrap_or_default()
                ));
            }
            Ok(out)
        }
        ReportFormat::Html => {
            let mut rows = String::new();
            for case in &summary.cases {
                rows.push_str(&format!(
                    "<tr><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{:?}</td><td>{}</td></tr>",
                    escape_html(&case.case_id),
                    escape_html(&case.suite_id),
                    escape_html(&case.task_id),
                    escape_html(&case.agent_id),
                    case.status,
                    case.score.score.unwrap_or_default()
                ));
            }
            Ok(format!(
                "<!doctype html><html><head><meta charset=\"utf-8\"><title>peval {}</title></head><body><h1>peval report {}</h1><p>Status: {:?}</p><p>Cases: {} passed / {} failed / {} total</p><table><thead><tr><th>Case</th><th>Suite</th><th>Task</th><th>Agent</th><th>Status</th><th>Score</th></tr></thead><tbody>{}</tbody></table></body></html>",
                escape_html(&summary.run_id),
                escape_html(&summary.run_id),
                summary.status,
                summary.passed_cases,
                summary.failed_cases,
                summary.total_cases,
                rows
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

fn escape_html(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
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
}

#[derive(Debug, Parser)]
struct ProjectArgs {
    #[arg(long, value_name = "DIR", default_value = ".")]
    project: PathBuf,
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Parser)]
struct ListArgs {
    #[arg(long, value_name = "DIR", default_value = ".")]
    project: PathBuf,
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
}

#[derive(Debug, Parser)]
struct SelectArgs {
    #[arg(long, value_name = "DIR", default_value = ".")]
    project: PathBuf,
    #[arg(long)]
    suite: Option<String>,
    #[arg(long)]
    agent: Option<String>,
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Parser)]
struct RunArgs {
    #[arg(long, value_name = "DIR", default_value = ".")]
    project: PathBuf,
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
    #[arg(long, value_name = "DIR")]
    run_root: PathBuf,
    #[arg(long, value_enum, default_value_t = ReportFormat::Markdown)]
    format: ReportFormat,
    #[arg(long, value_name = "PATH")]
    output: Option<PathBuf>,
}

#[derive(Debug, Parser)]
struct CompareArgs {
    #[arg(value_name = "RUN_ROOT", required = true)]
    run_roots: Vec<PathBuf>,
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Parser)]
struct ReplayArgs {
    #[arg(long, value_name = "DIR")]
    run_root: PathBuf,
    #[arg(long)]
    case: Option<String>,
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
        Commands::Doctor(args) => run_doctor(args),
        Commands::List(args) => run_list(args),
        Commands::Check(args) => run_check(args),
        Commands::Run(args) => run_run(args),
        Commands::Report(args) => run_report(args),
        Commands::Compare(args) => run_compare(args),
        Commands::Replay(args) => run_replay(args),
    }
}

fn run_doctor(args: ProjectArgs) -> Result<CliOutcome> {
    let project = EvalProject::load(args.project)?;
    let payload = json!({
        "schema_version": SCHEMA_VERSION,
        "project": &project.name,
        "root": &project.root,
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
        "project: {}\nroot: {}\nallow_live: {}\nagents: {}\nsuites: {}\nfake adapter: available\npsychevo adapter: manifest-gated\n",
        project.name,
        project.root.display(),
        project.allow_live,
        project.agents.len(),
        project.suites.len(),
    )))
}

fn run_list(args: ListArgs) -> Result<CliOutcome> {
    let project = EvalProject::load(args.project)?;
    let tasks = list_tasks(&project)?;
    let payload = json!({
        "schema_version": SCHEMA_VERSION,
        "suites": project.suites.values().map(|suite| json!({
            "id": &suite.id,
            "name": &suite.name,
            "agents": &suite.agents,
            "tasks": &suite.tasks,
        })).collect::<Vec<_>>(),
        "agents": project.agents.values().map(|agent| json!({
            "id": &agent.id,
            "name": &agent.name,
            "kind": agent.kind,
        })).collect::<Vec<_>>(),
        "tasks": tasks,
        "reports": ["html", "markdown", "json"],
    });
    if args.json {
        return Ok(success(serde_json::to_string_pretty(&payload)?));
    }
    let mut out = String::new();
    if matches!(args.kind, ListKind::All | ListKind::Suites) {
        out.push_str("suites\n");
        for suite in project.suites.values() {
            out.push_str(&format!("- {}\n", suite.id));
        }
    }
    if matches!(args.kind, ListKind::All | ListKind::Agents) {
        out.push_str("agents\n");
        for agent in project.agents.values() {
            out.push_str(&format!("- {} ({:?})\n", agent.id, agent.kind));
        }
    }
    if matches!(args.kind, ListKind::All | ListKind::Tasks) {
        out.push_str("tasks\n");
        for task in list_tasks(&project)? {
            out.push_str(&format!("- {}\n", task["id"].as_str().unwrap_or("unknown")));
        }
    }
    if matches!(args.kind, ListKind::All | ListKind::Reports) {
        out.push_str("reports\n- html\n- markdown\n- json\n");
    }
    Ok(success(out))
}

fn run_check(args: SelectArgs) -> Result<CliOutcome> {
    let project = EvalProject::load(args.project)?;
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
        project_root: args.project,
        suite: args.suite,
        agent: args.agent,
        run_id: args.run_id,
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
    let rendered = render_report(ReportRequest {
        run_root: args.run_root,
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
    let report = compare_runs(CompareRequest {
        run_roots: args.run_roots,
    })?;
    Ok(success(render_compare(&report, args.json)?))
}

fn run_replay(args: ReplayArgs) -> Result<CliOutcome> {
    let report = replay_run(ReplayRequest {
        run_root: args.run_root,
        case_id: args.case,
    })?;
    Ok(success(render_replay(&report, args.json)?))
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
            project_root: fixture_project(),
            suite: Some("rust-swe".to_string()),
            agent: None,
            run_id: Some("fixture-one".to_string()),
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
        assert!(html.contains("<table>"));
        let json_report = render_report(ReportRequest {
            run_root: root_one.clone(),
            format: ReportFormat::Json,
        })
        .expect("json");
        assert!(json_report.contains("\"schema_version\": 1"));

        let run_two = run_evaluation(RunRequest {
            project_root: fixture_project(),
            suite: Some("rust-swe".to_string()),
            agent: Some("fake-pass".to_string()),
            run_id: Some("fixture-two".to_string()),
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
    fn scorer_failure_malformed_json_and_timeout_are_classified() {
        let temp = tempfile::tempdir().expect("temp");
        let project = create_scorer_project(temp.path());
        let summary = run_evaluation(RunRequest {
            project_root: project,
            suite: Some("scorer-suite".to_string()),
            agent: Some("fake-pass".to_string()),
            run_id: Some("scorer-cases".to_string()),
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
    fn cli_smoke_covers_all_commands() {
        let temp = tempfile::tempdir().expect("temp");
        let project = fixture_project();
        let doctor = run_cli_from([
            "peval",
            "doctor",
            "--project",
            project.to_str().unwrap(),
            "--json",
        ]);
        assert_eq!(doctor.code, 0, "{}", doctor.stderr);
        let list = run_cli_from([
            "peval",
            "list",
            "--project",
            project.to_str().unwrap(),
            "--json",
        ]);
        assert_eq!(list.code, 0, "{}", list.stderr);
        assert!(!list.stdout.to_ascii_lowercase().contains("csv"));
        let check = run_cli_from([
            "peval",
            "check",
            "--project",
            project.to_str().unwrap(),
            "--suite",
            "rust-swe",
            "--json",
        ]);
        assert_eq!(check.code, 0, "{}", check.stderr);
        let run = run_cli_from([
            "peval",
            "run",
            "--project",
            project.to_str().unwrap(),
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
            "--project",
            project.to_str().unwrap(),
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
}
