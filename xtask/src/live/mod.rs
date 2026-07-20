mod environment;
mod registry;
mod verifier;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command as ProcessCommand;
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result, bail};
use clap::{Args, Subcommand};
use serde::Serialize;

use crate::ci::artifacts::{default_artifact_root, display_path};
use crate::ci::process::{
    ProcessOutcome, command_exists, create_step_log, run_logged_process, write_log_line,
    write_mirrored_line,
};
use crate::ci::retention::warn_if_ci_retention_cleanup_fails;

use self::registry::{
    DEFAULT_SUITE, LIVE_CHECKS, LIVE_SUITES, LiveCheck, LiveCheckAction, LiveProvider,
    LiveSelection, command_for_plan, resolve_providers, select_checks,
};
pub(crate) use environment::LiveEnvMode;
use environment::{
    LiveEnvironment, LiveEnvironmentPathsOutput, LiveEnvironmentPlanOutput, LivePrerequisites,
};

#[derive(Debug, Subcommand)]
pub(crate) enum LiveCommand {
    List {
        #[arg(long)]
        json: bool,
    },
    Plan {
        #[command(flatten)]
        selection: LiveSelectionArgs,
        #[arg(long = "env", value_enum, default_value_t = LiveEnvMode::default())]
        env_mode: LiveEnvMode,
        #[arg(long)]
        json: bool,
    },
    Run {
        #[command(flatten)]
        selection: LiveSelectionArgs,
        #[arg(long = "env", value_enum, default_value_t = LiveEnvMode::default())]
        env_mode: LiveEnvMode,
        #[arg(long)]
        json: bool,
        #[arg(long)]
        artifact_root: Option<PathBuf>,
    },
}

#[derive(Clone, Debug, Args)]
pub(crate) struct LiveSelectionArgs {
    #[arg(long = "check")]
    checks: Vec<String>,
    #[arg(long = "suite")]
    suites: Vec<String>,
    #[arg(long)]
    all: bool,
    #[arg(long = "provider")]
    providers: Vec<String>,
}

#[derive(Debug, Serialize)]
struct LiveListOutput {
    default_suite: &'static str,
    providers: Vec<ProviderOutput>,
    suites: Vec<SuiteOutput>,
    checks: Vec<CheckOutput>,
}

#[derive(Debug, Serialize)]
struct LivePlanOutput {
    default_suite: &'static str,
    environment: LiveEnvironmentPlanOutput,
    artifact_root: Option<String>,
    providers: Vec<ProviderOutput>,
    checks: Vec<CheckPlanOutput>,
}

#[derive(Debug, Serialize)]
struct LiveRunOutput {
    environment: LiveEnvironmentPlanOutput,
    artifact_root: String,
    providers: Vec<ProviderOutput>,
    checks: Vec<CheckRunOutput>,
}

#[derive(Debug, Serialize)]
struct ProviderOutput {
    id: &'static str,
    model: &'static str,
}

#[derive(Debug, Serialize)]
struct SuiteOutput {
    id: &'static str,
    description: &'static str,
}

#[derive(Debug, Serialize)]
struct CheckOutput {
    id: &'static str,
    description: &'static str,
    suites: Vec<&'static str>,
}

#[derive(Debug, Serialize)]
struct CheckPlanOutput {
    id: &'static str,
    description: &'static str,
    suites: Vec<&'static str>,
    command: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
struct CheckRunOutput {
    id: &'static str,
    description: &'static str,
    status: LiveStatus,
    artifact_path: String,
    log_path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    home_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    config_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    db_path: Option<String>,
    detail: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum LiveStatus {
    Passed,
    Failed,
    Blocked,
    Skipped,
}

#[derive(Debug)]
struct CheckResult {
    status: LiveStatus,
    detail: Option<String>,
    environment: Option<LiveEnvironmentPathsOutput>,
    had_suppressed_output: bool,
}

impl CheckResult {
    fn include_suppressed_output(mut self, had_suppressed_output: bool) -> Self {
        self.had_suppressed_output |= had_suppressed_output;
        self
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PlaywrightLiveContext {
    check_id: &'static str,
    provider: &'static str,
    model: &'static str,
    env_mode: LiveEnvMode,
    config_path: String,
    home: String,
    db_path: String,
    pevo_bin: String,
    cwd: Option<String>,
    artifact_root: String,
    timeout_ms: u64,
    interval_ms: u64,
    prompt: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct DesktopLiveCapabilitySnapshot {
    display_variables: Vec<String>,
    native_runtime_available: bool,
    os: &'static str,
    provider_required: bool,
    reason: Option<String>,
    session: &'static str,
}

pub(crate) fn run(command: LiveCommand, root: &Path) -> Result<()> {
    match command {
        LiveCommand::List { json } => {
            let output = list_output();
            if json {
                print_json(&output)
            } else {
                for check in output.checks {
                    println!("{}\t{}", check.id, check.description);
                }
                Ok(())
            }
        }
        LiveCommand::Plan {
            selection,
            env_mode,
            json,
        } => {
            let plan = plan_output(&selection.into_selection(), env_mode, None)?;
            if json {
                print_json(&plan)
            } else {
                print_plan(&plan);
                Ok(())
            }
        }
        LiveCommand::Run {
            selection,
            env_mode,
            json,
            artifact_root,
        } => {
            let run = execute_live(root, &selection.into_selection(), env_mode, artifact_root)?;
            if json {
                print_json(&run)?;
            } else {
                print_run_summary(&run);
            }
            if let Some(non_success) = run.checks.iter().find(|check| {
                check.status != LiveStatus::Passed && check.status != LiveStatus::Skipped
            }) {
                bail!(
                    "live check '{}' ended as {:?}; artifacts: {}",
                    non_success.id,
                    non_success.status,
                    run.artifact_root
                );
            }
            Ok(())
        }
    }
}

pub(crate) fn run_ci_single_provider_live(
    root: &Path,
    artifact_root: &Path,
    env_mode: LiveEnvMode,
    log: Arc<Mutex<fs::File>>,
) -> Result<ProcessOutcome> {
    let providers = vec![registry::XIAOMI_TOKEN_PLAN];
    let check = registry::check_by_id("provider-smoke").context("provider-smoke check")?;
    let check_dir = artifact_root.join("live").join(check.id);
    fs::create_dir_all(check_dir.join("logs"))
        .with_context(|| format!("create {}", check_dir.display()))?;
    let result = run_check(
        root,
        artifact_root,
        &check_dir,
        check,
        &providers,
        env_mode,
        Arc::clone(&log),
    )?;
    if result.status == LiveStatus::Passed {
        write_log_line(&log, "single-provider-live: passed")?;
        return Ok(ProcessOutcome {
            passed: true,
            exit_code: Some(0),
            mirrored_diagnostics: 0,
            had_suppressed_output: result.had_suppressed_output,
        });
    }

    let status_code = match result.status {
        LiveStatus::Passed => 0,
        LiveStatus::Failed => 1,
        LiveStatus::Blocked => 2,
        LiveStatus::Skipped => 0,
    };
    let detail = result
        .detail
        .unwrap_or_else(|| "live provider smoke did not pass".to_string());
    write_mirrored_line(&log, &format!("single-provider-live: {detail}"))?;
    Ok(ProcessOutcome {
        passed: false,
        exit_code: Some(status_code),
        mirrored_diagnostics: 1,
        had_suppressed_output: result.had_suppressed_output,
    })
}

impl LiveSelectionArgs {
    fn into_selection(self) -> LiveSelection {
        LiveSelection {
            checks: self.checks,
            suites: self.suites,
            all: self.all,
            providers: self.providers,
        }
    }
}

fn list_output() -> LiveListOutput {
    LiveListOutput {
        default_suite: DEFAULT_SUITE,
        providers: provider_outputs(&[registry::XIAOMI_TOKEN_PLAN, registry::DEEPSEEK]),
        suites: LIVE_SUITES
            .iter()
            .map(|suite| SuiteOutput {
                id: suite.id,
                description: suite.description,
            })
            .collect(),
        checks: LIVE_CHECKS
            .iter()
            .map(|check| CheckOutput {
                id: check.id,
                description: check.description,
                suites: check.suites.to_vec(),
            })
            .collect(),
    }
}

fn plan_output(
    selection: &LiveSelection,
    env_mode: LiveEnvMode,
    artifact_root: Option<&Path>,
) -> Result<LivePlanOutput> {
    let checks = select_checks(selection)?;
    let providers = providers_for_checks(&checks, &selection.providers)?;
    Ok(LivePlanOutput {
        default_suite: DEFAULT_SUITE,
        environment: LiveEnvironmentPlanOutput { mode: env_mode },
        artifact_root: artifact_root.map(display_path),
        providers: provider_outputs(&providers),
        checks: checks
            .into_iter()
            .map(|check| CheckPlanOutput {
                id: check.id,
                description: check.description,
                suites: check.suites.to_vec(),
                command: command_for_plan(check),
            })
            .collect(),
    })
}

fn execute_live(
    root: &Path,
    selection: &LiveSelection,
    env_mode: LiveEnvMode,
    artifact_root: Option<PathBuf>,
) -> Result<LiveRunOutput> {
    let use_default_artifact_root = artifact_root.is_none();
    let invocation_cwd = std::env::current_dir().context("read xtask invocation directory")?;
    let artifact_root = resolve_live_artifact_root(root, &invocation_cwd, artifact_root);
    fs::create_dir_all(artifact_root.join("live"))
        .with_context(|| format!("create artifact root {}", artifact_root.display()))?;
    let plan = plan_output(selection, env_mode, Some(&artifact_root))?;
    fs::write(
        artifact_root.join("live-plan.json"),
        serde_json::to_vec_pretty(&plan)?,
    )
    .with_context(|| format!("write {}", artifact_root.join("live-plan.json").display()))?;

    let checks = select_checks(selection)?;
    let providers = providers_for_checks(&checks, &selection.providers)?;
    let mut outputs = Vec::new();
    for check in checks {
        println!("live {} ...", check.id);
        let check_dir = artifact_root.join("live").join(check.id);
        let log_path = check_dir.join("logs").join("check.log");
        fs::create_dir_all(check_dir.join("logs"))
            .with_context(|| format!("create {}", check_dir.display()))?;
        let log = create_step_log(&log_path)?;
        let result = run_check(
            root,
            &artifact_root,
            &check_dir,
            check,
            &providers,
            env_mode,
            Arc::clone(&log),
        )?;
        let environment = result.environment.clone();
        let output = CheckRunOutput {
            id: check.id,
            description: check.description,
            status: result.status.clone(),
            artifact_path: display_path(&check_dir),
            log_path: display_path(&log_path),
            home_path: environment.as_ref().map(|env| env.home_path.clone()),
            config_path: environment.as_ref().map(|env| env.config_path.clone()),
            db_path: environment.as_ref().map(|env| env.db_path.clone()),
            detail: result.detail.clone(),
        };
        fs::write(
            check_dir.join("result.json"),
            serde_json::to_vec_pretty(&output)?,
        )
        .with_context(|| format!("write {}", check_dir.join("result.json").display()))?;
        println!("live {}: {:?}", check.id, output.status);
        outputs.push(output);
    }

    let run = LiveRunOutput {
        environment: LiveEnvironmentPlanOutput { mode: env_mode },
        artifact_root: display_path(&artifact_root),
        providers: provider_outputs(&providers),
        checks: outputs,
    };
    fs::write(
        artifact_root.join("live-results.json"),
        serde_json::to_vec_pretty(&run)?,
    )
    .with_context(|| {
        format!(
            "write {}",
            artifact_root.join("live-results.json").display()
        )
    })?;
    if use_default_artifact_root {
        warn_if_ci_retention_cleanup_fails(root, &artifact_root);
    }
    Ok(run)
}

fn resolve_live_artifact_root(
    root: &Path,
    invocation_cwd: &Path,
    artifact_root: Option<PathBuf>,
) -> PathBuf {
    match artifact_root {
        Some(path) if path.is_absolute() => path,
        Some(path) => invocation_cwd.join(path),
        None => default_artifact_root(root),
    }
}

fn providers_for_checks(
    checks: &[&LiveCheck],
    provider_args: &[String],
) -> Result<Vec<LiveProvider>> {
    if checks.iter().any(|check| check_requires_provider(check)) {
        resolve_providers(provider_args)
    } else {
        if !provider_args.is_empty() {
            let _ = resolve_providers(provider_args)?;
        }
        Ok(Vec::new())
    }
}

fn check_requires_provider(check: &LiveCheck) -> bool {
    match check.action {
        LiveCheckAction::DesktopNativeSmoke { provider_required } => provider_required,
        LiveCheckAction::CargoIgnoredTest {
            provider_required, ..
        } => provider_required,
        LiveCheckAction::ProviderSmoke
        | LiveCheckAction::PevoDoctorLive
        | LiveCheckAction::Playwright { .. } => true,
        LiveCheckAction::DeterministicPlaywright { .. } => false,
    }
}

fn run_check(
    root: &Path,
    artifact_root: &Path,
    check_dir: &Path,
    check: &'static LiveCheck,
    providers: &[LiveProvider],
    env_mode: LiveEnvMode,
    log: Arc<Mutex<fs::File>>,
) -> Result<CheckResult> {
    match check.action {
        LiveCheckAction::DesktopNativeSmoke { provider_required } => {
            run_desktop_native_smoke_check(
                root,
                check_dir,
                providers,
                env_mode,
                provider_required,
                log,
            )
        }
        LiveCheckAction::ProviderSmoke => {
            run_provider_smoke_check(root, check_dir, providers, env_mode, log)
        }
        LiveCheckAction::PevoDoctorLive => {
            run_pevo_doctor_live_check(root, check_dir, providers, env_mode, log)
        }
        LiveCheckAction::CargoIgnoredTest { package, test, .. } => {
            run_cargo_ignored_live_check(root, check_dir, providers, env_mode, package, test, log)
        }
        LiveCheckAction::DeterministicPlaywright { spec, grep } => {
            run_deterministic_playwright_check(
                root,
                artifact_root,
                check_dir,
                check,
                env_mode,
                spec,
                grep,
                log,
            )
        }
        LiveCheckAction::Playwright {
            spec,
            grep,
            needs_opencode,
            needs_skill_cwd,
        } => run_playwright_live_check(
            root,
            artifact_root,
            check_dir,
            check,
            providers,
            env_mode,
            spec,
            grep,
            needs_opencode,
            needs_skill_cwd,
            log,
        ),
    }
}

#[allow(clippy::too_many_arguments)]
fn run_deterministic_playwright_check(
    root: &Path,
    artifact_root: &Path,
    check_dir: &Path,
    check: &'static LiveCheck,
    _env_mode: LiveEnvMode,
    spec: &'static str,
    grep: &'static str,
    log: Arc<Mutex<fs::File>>,
) -> Result<CheckResult> {
    if !command_exists("pnpm") {
        return blocked(
            log,
            "missing pnpm; run: cargo xtask doctor deps install --only playwright".to_string(),
        );
    }
    let spec_path = root.join(spec);
    if !spec_path.is_file() {
        return failed_result(
            log,
            format!(
                "deterministic Agent Playwright spec is missing: {}",
                spec_path.display()
            ),
            None,
        );
    }

    let home = check_dir.join("home");
    let cwd = check_dir.join("cwd");
    let config_path = home.join("config.toml");
    let db_path = check_dir.join("state.db");
    fs::create_dir_all(&home).with_context(|| format!("create {}", home.display()))?;
    fs::create_dir_all(&cwd).with_context(|| format!("create {}", cwd.display()))?;
    fs::write(&config_path, "model = \"lmstudio/noop\"\n")
        .with_context(|| format!("write {}", config_path.display()))?;
    if db_path.is_file() {
        fs::remove_file(&db_path).with_context(|| format!("remove {}", db_path.display()))?;
    }
    let environment = LiveEnvironmentPathsOutput {
        mode: LiveEnvMode::Isolated,
        home_path: home.display().to_string(),
        config_path: config_path.display().to_string(),
        db_path: db_path.display().to_string(),
    };

    let (pevo_bin, mut had_suppressed_output) = match ensure_pevo_built(root, Arc::clone(&log))? {
        Ok(value) => value,
        Err(mut result) => {
            result.environment = Some(environment);
            return Ok(result);
        }
    };
    let context_path = check_dir.join("xtask-live-context.json");
    let context = PlaywrightLiveContext {
        check_id: check.id,
        provider: "deterministic-fake",
        model: "runtime-owned",
        env_mode: LiveEnvMode::Isolated,
        config_path: config_path.display().to_string(),
        home: home.display().to_string(),
        db_path: db_path.display().to_string(),
        pevo_bin: pevo_bin.display().to_string(),
        cwd: Some(cwd.display().to_string()),
        artifact_root: check_dir.display().to_string(),
        timeout_ms: playwright_timeout_ms(check.id),
        interval_ms: 100,
        prompt: None,
    };
    fs::write(&context_path, serde_json::to_vec_pretty(&context)?)
        .with_context(|| format!("write {}", context_path.display()))?;

    let mut build = ProcessCommand::new("pnpm");
    build
        .args(["--filter", "@psychevo/workbench", "build"])
        .current_dir(root)
        .env("PSYCHEVO_XTASK_LIVE_CONTEXT", &context_path);
    let build_outcome = run_logged_process(
        "workbench deterministic Agent build",
        &mut build,
        Arc::clone(&log),
    )?;
    if !build_outcome.passed {
        return Ok(check_result_from_outcome(
            build_outcome,
            "Workbench build failed",
            Some(environment),
        )?
        .include_suppressed_output(had_suppressed_output));
    }
    had_suppressed_output |= build_outcome.had_suppressed_output;

    let mut test = ProcessCommand::new("pnpm");
    test.args([
        "exec",
        "playwright",
        "test",
        spec,
        "--grep",
        grep,
        "--project",
        "chromium-desktop",
    ])
    .current_dir(root)
    .env("PSYCHEVO_XTASK_LIVE_CONTEXT", &context_path)
    .env("PSYCHEVO_CI_ARTIFACT_ROOT", artifact_root)
    .env("PSYCHEVO_RUNTIME_LIVE_FAKE", "1")
    .env_remove("NO_COLOR");
    let outcome = run_logged_process(check.id, &mut test, log)?;
    Ok(check_result_from_outcome(
        outcome,
        &format!("deterministic Agent Playwright check {} failed", check.id),
        Some(environment),
    )?
    .include_suppressed_output(had_suppressed_output))
}

fn run_desktop_native_smoke_check(
    root: &Path,
    check_dir: &Path,
    providers: &[LiveProvider],
    env_mode: LiveEnvMode,
    provider_required: bool,
    log: Arc<Mutex<fs::File>>,
) -> Result<CheckResult> {
    let prerequisites = match LivePrerequisites::load(root) {
        Ok(prerequisites) => prerequisites,
        Err(reason) => return blocked(log, reason),
    };
    let live_env = match prerequisites.resolve(env_mode, check_dir) {
        Ok(live_env) => live_env,
        Err(error) => return failed_result(log, format!("{error:#}"), None),
    };
    let environment = Some(live_env.to_output());
    let live_environment = Some(live_env);
    let mut provider = None;
    if provider_required {
        let Some(selected_provider) = providers.first().copied() else {
            return blocked_with_env(log, "no live provider selected".to_string(), environment);
        };
        if !prerequisites.provider_credentials_available(&selected_provider) {
            return blocked_with_env(
                log,
                format!(
                    "{} credentials missing from .local/.psychevo-dev/.env",
                    selected_provider.id
                ),
                environment,
            );
        }
        provider = Some(selected_provider);
    }

    let skip_reason = desktop_native_skip_reason();
    write_desktop_capability_snapshot(check_dir, provider_required, skip_reason.clone())?;
    if let Some(reason) = skip_reason {
        return skipped_with_env(log, reason, environment);
    }
    if !command_exists("pnpm") {
        return blocked_with_env(
            log,
            "missing pnpm; run: cargo xtask doctor deps install --only playwright".to_string(),
            environment,
        );
    }
    let (pevo_bin, mut had_suppressed_output) = match ensure_pevo_built(root, Arc::clone(&log))? {
        Ok(value) => value,
        Err(mut result) => {
            result.environment = environment;
            return Ok(result);
        }
    };

    let wdio_artifact_root = check_dir.join("wdio");
    fs::create_dir_all(&wdio_artifact_root)
        .with_context(|| format!("create {}", wdio_artifact_root.display()))?;
    let provider_token = provider.map(desktop_provider_live_token);
    let floating_text = desktop_floating_live_text(provider_token.as_deref());

    let mut build = ProcessCommand::new("pnpm");
    build
        .args(["--filter", "@psychevo/desktop", "tauri:wdio-build"])
        .current_dir(root);
    configure_desktop_wdio_command(
        &mut build,
        &wdio_artifact_root,
        &floating_text,
        live_environment.as_ref(),
        provider,
        Some(pevo_bin.as_path()),
        provider_token.as_deref(),
    );
    let outcome = run_logged_process("desktop native WDIO build", &mut build, Arc::clone(&log))?;
    if !outcome.passed {
        return Ok(check_result_from_outcome(
            outcome,
            "Desktop native WDIO build failed",
            environment,
        )?
        .include_suppressed_output(had_suppressed_output));
    }
    had_suppressed_output |= outcome.had_suppressed_output;

    let mut wdio = ProcessCommand::new("pnpm");
    wdio.args(["--filter", "@psychevo/desktop", "wdio"])
        .current_dir(root);
    configure_desktop_wdio_command(
        &mut wdio,
        &wdio_artifact_root,
        &floating_text,
        live_environment.as_ref(),
        provider,
        Some(pevo_bin.as_path()),
        provider_token.as_deref(),
    );
    let outcome = run_logged_process("desktop native WDIO smoke", &mut wdio, Arc::clone(&log))?;
    let outcome_had_suppressed_output = outcome.had_suppressed_output;
    if outcome.passed
        && let Err(error) = validate_desktop_startup_artifacts(&wdio_artifact_root)
    {
        return Ok(failed_result(
            log,
            format!("Desktop native startup evidence is invalid: {error:#}"),
            environment,
        )?
        .include_suppressed_output(had_suppressed_output || outcome_had_suppressed_output));
    }
    Ok(
        check_result_from_outcome(outcome, "Desktop native WDIO smoke failed", environment)?
            .include_suppressed_output(had_suppressed_output),
    )
}

fn validate_desktop_startup_artifacts(wdio_artifact_root: &Path) -> Result<()> {
    const REQUIRED_CHECKPOINTS: &[&str] = &[
        "process_start",
        "window_ready",
        "managed_gateway_ready",
        "bridge_connected",
        "gui_ready",
        "draft_context_ready",
    ];
    const SCREENSHOT_CHECKPOINTS: &[&str] = &["gui_ready", "draft_context_ready"];

    let manifest_path = wdio_artifact_root.join("desktop-startup-journey.json");
    let manifest: serde_json::Value = serde_json::from_slice(
        &fs::read(&manifest_path).with_context(|| format!("read {}", manifest_path.display()))?,
    )
    .with_context(|| format!("parse {}", manifest_path.display()))?;
    if manifest
        .get("schemaVersion")
        .and_then(serde_json::Value::as_u64)
        != Some(1)
    {
        bail!(
            "{} has an unsupported schemaVersion",
            manifest_path.display()
        );
    }
    if manifest
        .pointer("/run/outcome")
        .and_then(serde_json::Value::as_str)
        != Some("passed")
    {
        bail!("{} does not describe a passed run", manifest_path.display());
    }
    let checkpoints = manifest
        .get("checkpoints")
        .and_then(serde_json::Value::as_array)
        .with_context(|| format!("{} is missing checkpoints", manifest_path.display()))?;
    for id in REQUIRED_CHECKPOINTS {
        let matching = checkpoints
            .iter()
            .filter(|checkpoint| {
                checkpoint.get("id").and_then(serde_json::Value::as_str) == Some(id)
            })
            .collect::<Vec<_>>();
        if matching.len() != 1 {
            bail!(
                "{} contains {} '{}' checkpoints; expected exactly one",
                manifest_path.display(),
                matching.len(),
                id
            );
        }
        let checkpoint = matching[0];
        if checkpoint.get("status").and_then(serde_json::Value::as_str) != Some("complete") {
            bail!(
                "{} checkpoint '{}' is incomplete",
                manifest_path.display(),
                id
            );
        }
        if SCREENSHOT_CHECKPOINTS.contains(id) {
            let screenshot_path = checkpoint
                .pointer("/screenshot/path")
                .and_then(serde_json::Value::as_str)
                .with_context(|| {
                    format!(
                        "{} checkpoint '{}' has no screenshot path",
                        manifest_path.display(),
                        id
                    )
                })?;
            let relative = Path::new(screenshot_path);
            if relative.is_absolute()
                || relative.components().any(|component| {
                    matches!(
                        component,
                        std::path::Component::ParentDir
                            | std::path::Component::RootDir
                            | std::path::Component::Prefix(_)
                    )
                })
            {
                bail!(
                    "{} checkpoint '{}' has an unsafe screenshot path",
                    manifest_path.display(),
                    id
                );
            }
            let screenshot = wdio_artifact_root.join(relative);
            if !screenshot.is_file() {
                bail!(
                    "{} checkpoint '{}' screenshot is missing: {}",
                    manifest_path.display(),
                    id,
                    screenshot.display()
                );
            }
        }
    }
    let rust_trace = wdio_artifact_root.join("desktop-startup-rust.jsonl");
    if !rust_trace.is_file() {
        bail!(
            "Desktop Rust startup trace is missing: {}",
            rust_trace.display()
        );
    }
    Ok(())
}

fn configure_desktop_wdio_command(
    command: &mut ProcessCommand,
    wdio_artifact_root: &Path,
    floating_text: &str,
    live_env: Option<&LiveEnvironment>,
    provider: Option<LiveProvider>,
    pevo_bin: Option<&Path>,
    provider_token: Option<&str>,
) {
    if let Some(live_env) = live_env {
        live_env.apply_to_command(command, provider);
    }
    if let Some(pevo_bin) = pevo_bin {
        command.env("PSYCHEVO_PEVO_BIN", pevo_bin);
    }
    command
        .env("PSYCHEVO_WDIO_ARTIFACT_ROOT", wdio_artifact_root)
        .env("PSYCHEVO_FLOATING_TEXT", floating_text);
    if let Some(token) = provider_token {
        command
            .env("PSYCHEVO_DESKTOP_PROVIDER_LIVE", "1")
            .env("PSYCHEVO_FLOATING_PROVIDER_TOKEN", token);
    }
}

fn desktop_provider_live_token(provider: LiveProvider) -> String {
    format!(
        "PEVO_DF_{}_OK",
        provider.id.replace('-', "_").to_ascii_uppercase()
    )
}

fn desktop_floating_live_text(provider_token: Option<&str>) -> String {
    provider_token.map_or_else(
        || "Psychevo floating live smoke selected text".to_string(),
        |token| format!("Floating provider live token: {token}"),
    )
}

fn run_provider_smoke_check(
    root: &Path,
    check_dir: &Path,
    providers: &[LiveProvider],
    env_mode: LiveEnvMode,
    log: Arc<Mutex<fs::File>>,
) -> Result<CheckResult> {
    let prerequisites = match LivePrerequisites::load(root) {
        Ok(prerequisites) => prerequisites,
        Err(reason) => return blocked(log, reason),
    };
    let live_env = match prerequisites.resolve(env_mode, check_dir) {
        Ok(live_env) => live_env,
        Err(error) => return failed_result(log, format!("{error:#}"), None),
    };
    let environment = live_env.to_output();
    let (pevo_bin, had_suppressed_output) = match ensure_pevo_built(root, Arc::clone(&log))? {
        Ok(value) => value,
        Err(mut result) => {
            result.environment = Some(environment.clone());
            return Ok(result);
        }
    };

    let mut failed = None;
    let mut blocked_reason = None;
    let mut verifications = Vec::new();
    for provider in providers {
        if !prerequisites.provider_credentials_available(provider) {
            blocked_reason = Some(format!(
                "{} credentials missing from {}",
                provider.id,
                root.join(".local/.psychevo-dev/.env").display()
            ));
            continue;
        }
        match run_provider_smoke(root, check_dir, &pevo_bin, &live_env, *provider, &log)? {
            Ok(summary) => verifications.push(summary),
            Err(detail) => failed = Some(detail),
        }
    }

    if let Some(detail) = failed {
        return Ok(failed_result(log, detail, Some(environment))?
            .include_suppressed_output(had_suppressed_output));
    }
    if let Some(reason) = blocked_reason {
        return Ok(blocked_with_env(log, reason, Some(environment))?
            .include_suppressed_output(had_suppressed_output));
    }
    Ok(CheckResult {
        status: LiveStatus::Passed,
        detail: Some(format!(
            "{} provider smoke run(s) passed",
            verifications.len()
        )),
        environment: Some(environment),
        had_suppressed_output,
    })
}

fn run_pevo_doctor_live_check(
    root: &Path,
    check_dir: &Path,
    providers: &[LiveProvider],
    env_mode: LiveEnvMode,
    log: Arc<Mutex<fs::File>>,
) -> Result<CheckResult> {
    let prerequisites = match LivePrerequisites::load(root) {
        Ok(prerequisites) => prerequisites,
        Err(reason) => return blocked(log, reason),
    };
    let live_env = match prerequisites.resolve(env_mode, check_dir) {
        Ok(live_env) => live_env,
        Err(error) => return failed_result(log, format!("{error:#}"), None),
    };
    let environment = live_env.to_output();
    for provider in providers {
        if !prerequisites.provider_credentials_available(provider) {
            return blocked_with_env(
                log,
                format!(
                    "{} credentials missing from .local/.psychevo-dev/.env",
                    provider.id
                ),
                Some(environment),
            );
        }
    }
    let (pevo_bin, had_suppressed_output) = match ensure_pevo_built(root, Arc::clone(&log))? {
        Ok(value) => value,
        Err(mut result) => {
            result.environment = Some(environment.clone());
            return Ok(result);
        }
    };
    let mut command = ProcessCommand::new(pevo_bin);
    command
        .args(["doctor", "--live", "--json"])
        .current_dir(root);
    live_env.apply_to_command(&mut command, None);
    let outcome = run_logged_process("pevo-doctor-live", &mut command, log)?;
    Ok(check_result_from_outcome(
        outcome,
        "pevo doctor --live failed",
        Some(live_env.to_output()),
    )?
    .include_suppressed_output(had_suppressed_output))
}

fn run_cargo_ignored_live_check(
    root: &Path,
    check_dir: &Path,
    providers: &[LiveProvider],
    env_mode: LiveEnvMode,
    package: &'static str,
    test: &'static str,
    log: Arc<Mutex<fs::File>>,
) -> Result<CheckResult> {
    let prerequisites = match LivePrerequisites::load(root) {
        Ok(prerequisites) => prerequisites,
        Err(reason) => return blocked(log, reason),
    };
    let live_env = match prerequisites.resolve(env_mode, check_dir) {
        Ok(live_env) => live_env,
        Err(error) => return failed_result(log, format!("{error:#}"), None),
    };
    let environment = live_env.to_output();
    for provider in providers {
        if !prerequisites.provider_credentials_available(provider) {
            return blocked_with_env(
                log,
                format!(
                    "{} credentials missing from .local/.psychevo-dev/.env",
                    provider.id
                ),
                Some(environment),
            );
        }
    }
    let mut command = ProcessCommand::new("cargo");
    command
        .args(["test", "-p", package, test, "--", "--ignored", "--exact"])
        .current_dir(root);
    live_env.apply_to_command(&mut command, providers.first().copied());
    let outcome = run_logged_process(test, &mut command, log)?;
    check_result_from_outcome(
        outcome,
        &format!("{test} failed"),
        Some(live_env.to_output()),
    )
}

#[allow(clippy::too_many_arguments)]
fn run_playwright_live_check(
    root: &Path,
    artifact_root: &Path,
    check_dir: &Path,
    check: &'static LiveCheck,
    providers: &[LiveProvider],
    env_mode: LiveEnvMode,
    spec: &'static str,
    grep: &'static str,
    needs_opencode: bool,
    needs_skill_cwd: bool,
    log: Arc<Mutex<fs::File>>,
) -> Result<CheckResult> {
    let prerequisites = match LivePrerequisites::load(root) {
        Ok(prerequisites) => prerequisites,
        Err(reason) => return blocked(log, reason),
    };
    let live_env = match prerequisites.resolve(env_mode, check_dir) {
        Ok(live_env) => live_env,
        Err(error) => return failed_result(log, format!("{error:#}"), None),
    };
    let environment = live_env.to_output();
    let Some(provider) = providers.first().copied() else {
        return blocked_with_env(
            log,
            "no live provider selected".to_string(),
            Some(environment),
        );
    };
    if !prerequisites.provider_credentials_available(&provider) {
        return blocked_with_env(
            log,
            format!(
                "{} credentials missing from .local/.psychevo-dev/.env",
                provider.id
            ),
            Some(environment),
        );
    }
    if !command_exists("pnpm") {
        return blocked_with_env(
            log,
            "missing pnpm; run: cargo xtask doctor deps install --only playwright".to_string(),
            Some(environment),
        );
    }
    if needs_opencode && !command_exists("opencode") {
        return blocked_with_env(
            log,
            "missing opencode command for ACP live validation".to_string(),
            Some(environment),
        );
    }

    let skill_cwd = if needs_skill_cwd {
        let path = root
            .parent()
            .map(|parent| parent.join("feedgarden"))
            .unwrap_or_else(|| root.join("../feedgarden"));
        if !path.is_dir() {
            return blocked_with_env(
                log,
                format!("live skill cwd not found: {}", path.display()),
                Some(environment),
            );
        }
        Some(path)
    } else {
        None
    };

    let (pevo_bin, mut had_suppressed_output) = match ensure_pevo_built(root, Arc::clone(&log))? {
        Ok(value) => value,
        Err(mut result) => {
            result.environment = Some(environment.clone());
            return Ok(result);
        }
    };
    let cwd = match check.id {
        "web-automation-live" => Some(prepare_automation_cwd(check_dir)?),
        "web-subagent-live" => Some(prepare_subagent_cwd(check_dir)?),
        _ => skill_cwd,
    };
    let context_path = check_dir.join("xtask-live-context.json");
    let context = PlaywrightLiveContext {
        check_id: check.id,
        provider: provider.id,
        model: provider.model,
        env_mode,
        config_path: live_env.config_path().display().to_string(),
        home: live_env.home_path().display().to_string(),
        db_path: live_env.db_path().display().to_string(),
        pevo_bin: pevo_bin.display().to_string(),
        cwd: cwd.as_ref().map(|path| path.display().to_string()),
        artifact_root: check_dir.display().to_string(),
        timeout_ms: playwright_timeout_ms(check.id),
        interval_ms: 3_000,
        prompt: (check.id == "web-skill-live").then(|| "$x-daily".to_string()),
    };
    fs::write(&context_path, serde_json::to_vec_pretty(&context)?)
        .with_context(|| format!("write {}", context_path.display()))?;

    let mut build = ProcessCommand::new("pnpm");
    build
        .args(["--filter", "@psychevo/workbench", "build"])
        .current_dir(root);
    live_env.apply_to_command(&mut build, Some(provider));
    build.env("PSYCHEVO_XTASK_LIVE_CONTEXT", &context_path);
    let build_outcome = run_logged_process("workbench live build", &mut build, Arc::clone(&log))?;
    if !build_outcome.passed {
        return Ok(check_result_from_outcome(
            build_outcome,
            "Workbench build failed",
            Some(live_env.to_output()),
        )?
        .include_suppressed_output(had_suppressed_output));
    }
    had_suppressed_output |= build_outcome.had_suppressed_output;

    let mut test = ProcessCommand::new("pnpm");
    test.args([
        "exec",
        "playwright",
        "test",
        spec,
        "--grep",
        grep,
        "--project",
        "chromium-desktop",
    ])
    .current_dir(root);
    live_env.apply_to_command(&mut test, Some(provider));
    test.env("PSYCHEVO_XTASK_LIVE_CONTEXT", &context_path)
        .env("PSYCHEVO_CI_ARTIFACT_ROOT", artifact_root)
        .env_remove("NO_COLOR");
    let outcome = run_logged_process(check.id, &mut test, log)?;
    Ok(check_result_from_outcome(
        outcome,
        &format!("Playwright live check {} failed", check.id),
        Some(live_env.to_output()),
    )?
    .include_suppressed_output(had_suppressed_output))
}

fn run_provider_smoke(
    root: &Path,
    check_dir: &Path,
    pevo_bin: &Path,
    live_env: &LiveEnvironment,
    provider: LiveProvider,
    log: &Arc<Mutex<fs::File>>,
) -> Result<Result<verifier::ProviderSmokeVerification, String>> {
    let provider_dir = check_dir.join(provider.id);
    let cwd = provider_dir.join("cwd");
    fs::create_dir_all(&cwd).with_context(|| format!("create {}", cwd.display()))?;
    let token = format!(
        "PEVO_LIVE_{}_{}",
        provider.id.replace('-', "_").to_ascii_uppercase(),
        provider_dir
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("PROVIDER")
    );
    fs::write(
        cwd.join("pevo_live_probe.txt"),
        format!("probe token: {token}\n"),
    )
    .with_context(|| format!("write probe in {}", cwd.display()))?;
    let first_log = provider_dir.join("first.ndjson");
    let second_log = provider_dir.join("second.ndjson");

    let first = run_pevo_json_turn(
        root,
        pevo_bin,
        live_env,
        &provider_dir,
        provider,
        &cwd,
        &first_log,
        false,
        "There is a file named pevo_live_probe.txt in this workspace. Inspect the workspace and report the probe token it contains.",
        log,
    )?;
    if !first.passed {
        return Ok(Err(format!("{} first pevo run failed", provider.id)));
    }
    let second = run_pevo_json_turn(
        root,
        pevo_bin,
        live_env,
        &provider_dir,
        provider,
        &cwd,
        &second_log,
        true,
        "Continue the same session and report the same probe token again.",
        log,
    )?;
    if !second.passed {
        return Ok(Err(format!("{} continue pevo run failed", provider.id)));
    }

    match verifier::verify_provider_smoke(provider.id, &token, &first_log, &second_log) {
        Ok(summary) => Ok(Ok(summary)),
        Err(error) => Ok(Err(error.to_string())),
    }
}

#[allow(clippy::too_many_arguments)]
fn run_pevo_json_turn(
    root: &Path,
    pevo_bin: &Path,
    live_env: &LiveEnvironment,
    provider_dir: &Path,
    provider: LiveProvider,
    cwd: &Path,
    stdout_path: &Path,
    continue_latest: bool,
    prompt: &str,
    log: &Arc<Mutex<fs::File>>,
) -> Result<ProcessOutcome> {
    fs::create_dir_all(provider_dir)
        .with_context(|| format!("create {}", provider_dir.display()))?;
    let stderr_path = stdout_path.with_extension("stderr.log");
    let mut command = ProcessCommand::new(pevo_bin);
    command
        .arg("run")
        .arg("--dir")
        .arg(cwd)
        .args(["--format", "json", "--include-reasoning"])
        .arg("-m")
        .arg(provider.model);
    if continue_latest {
        command.arg("--continue");
    }
    command.arg(prompt).current_dir(root);
    live_env.apply_to_command(&mut command, Some(provider));

    let output = command
        .output()
        .with_context(|| format!("run live provider {}", provider.id))?;
    fs::write(stdout_path, &output.stdout)
        .with_context(|| format!("write {}", stdout_path.display()))?;
    fs::write(&stderr_path, &output.stderr)
        .with_context(|| format!("write {}", stderr_path.display()))?;
    write_log_line(
        log,
        &format!(
            "{} stdout: {}; stderr: {}",
            provider.id,
            stdout_path.display(),
            stderr_path.display()
        ),
    )?;
    let mut mirrored = 0;
    if !output.stderr.is_empty() {
        let text = String::from_utf8_lossy(&output.stderr);
        for line in text.lines() {
            write_mirrored_line(log, line)?;
            mirrored += 1;
        }
    }
    Ok(ProcessOutcome {
        passed: output.status.success(),
        exit_code: output.status.code(),
        mirrored_diagnostics: mirrored,
        had_suppressed_output: false,
    })
}

fn ensure_pevo_built(
    root: &Path,
    log: Arc<Mutex<fs::File>>,
) -> Result<Result<(PathBuf, bool), CheckResult>> {
    let mut command = ProcessCommand::new("cargo");
    command
        .args(["build", "-p", "psychevo-cli", "--quiet"])
        .current_dir(root);
    let outcome = run_logged_process("build psychevo-cli", &mut command, log)?;
    if !outcome.passed {
        return Ok(Err(CheckResult {
            status: LiveStatus::Failed,
            detail: Some("cargo build -p psychevo-cli failed".to_string()),
            environment: None,
            had_suppressed_output: outcome.had_suppressed_output,
        }));
    }
    let pevo_bin = root.join("target").join("debug").join(binary_name("pevo"));
    if !pevo_bin.is_file() {
        return Ok(Err(CheckResult {
            status: LiveStatus::Failed,
            detail: Some(format!(
                "built pevo binary is missing: {}",
                pevo_bin.display()
            )),
            environment: None,
            had_suppressed_output: outcome.had_suppressed_output,
        }));
    }
    Ok(Ok((pevo_bin, outcome.had_suppressed_output)))
}

fn prepare_automation_cwd(check_dir: &Path) -> Result<PathBuf> {
    let cwd = check_dir.join("cwd");
    fs::create_dir_all(&cwd).with_context(|| format!("create {}", cwd.display()))?;
    fs::write(
        cwd.join("README.md"),
        "Live GUI automation validation workspace.\n",
    )
    .with_context(|| format!("write {}", cwd.join("README.md").display()))?;
    Ok(cwd)
}

fn prepare_subagent_cwd(check_dir: &Path) -> Result<PathBuf> {
    let cwd = check_dir.join("cwd");
    let agent_dir = cwd.join(".psychevo").join("agents");
    fs::create_dir_all(&agent_dir).with_context(|| format!("create {}", agent_dir.display()))?;
    fs::write(
        agent_dir.join("translate.md"),
        r#"---
description: Translate between Chinese and English.
---
Translate the assigned text between Chinese and English. Return only the translation and direction.
"#,
    )
    .with_context(|| format!("write {}", agent_dir.join("translate.md").display()))?;
    Ok(cwd)
}

fn check_result_from_outcome(
    outcome: ProcessOutcome,
    failure: &str,
    environment: Option<LiveEnvironmentPathsOutput>,
) -> Result<CheckResult> {
    Ok(CheckResult {
        status: if outcome.passed {
            LiveStatus::Passed
        } else {
            LiveStatus::Failed
        },
        detail: (!outcome.passed).then(|| failure.to_string()),
        environment,
        had_suppressed_output: outcome.had_suppressed_output,
    })
}

fn blocked(log: Arc<Mutex<fs::File>>, reason: String) -> Result<CheckResult> {
    blocked_with_env(log, reason, None)
}

fn blocked_with_env(
    log: Arc<Mutex<fs::File>>,
    reason: String,
    environment: Option<LiveEnvironmentPathsOutput>,
) -> Result<CheckResult> {
    write_mirrored_line(&log, &format!("blocked: {reason}"))?;
    Ok(CheckResult {
        status: LiveStatus::Blocked,
        detail: Some(reason),
        environment,
        had_suppressed_output: false,
    })
}

fn failed_result(
    log: Arc<Mutex<fs::File>>,
    detail: String,
    environment: Option<LiveEnvironmentPathsOutput>,
) -> Result<CheckResult> {
    write_mirrored_line(&log, &format!("failed: {detail}"))?;
    Ok(CheckResult {
        status: LiveStatus::Failed,
        detail: Some(detail),
        environment,
        had_suppressed_output: false,
    })
}

fn skipped_with_env(
    log: Arc<Mutex<fs::File>>,
    reason: String,
    environment: Option<LiveEnvironmentPathsOutput>,
) -> Result<CheckResult> {
    write_log_line(&log, &format!("skipped: {reason}"))?;
    Ok(CheckResult {
        status: LiveStatus::Skipped,
        detail: Some(reason),
        environment,
        had_suppressed_output: true,
    })
}

fn write_desktop_capability_snapshot(
    check_dir: &Path,
    provider_required: bool,
    reason: Option<String>,
) -> Result<()> {
    let snapshot = DesktopLiveCapabilitySnapshot {
        display_variables: observed_display_variables(),
        native_runtime_available: reason.is_none(),
        os: desktop_os(),
        provider_required,
        reason,
        session: desktop_session(),
    };
    fs::write(
        check_dir.join("capabilities.json"),
        serde_json::to_vec_pretty(&snapshot)?,
    )
    .with_context(|| format!("write {}", check_dir.join("capabilities.json").display()))
}

fn desktop_native_skip_reason() -> Option<String> {
    if desktop_os() == "linux" && !command_exists("pkg-config") {
        return Some(
            "native Tauri Linux prerequisites are unavailable: missing pkg-config".to_string(),
        );
    }
    if desktop_os() == "linux" && desktop_session() == "unknown" {
        return Some("native Desktop smoke requires an X11 or Wayland display session".to_string());
    }
    None
}

fn desktop_os() -> &'static str {
    if cfg!(target_os = "macos") {
        "macos"
    } else if cfg!(target_os = "windows") {
        "windows"
    } else {
        "linux"
    }
}

fn desktop_session() -> &'static str {
    if desktop_os() != "linux" {
        return "unknown";
    }
    match std::env::var("XDG_SESSION_TYPE")
        .ok()
        .map(|value| value.to_ascii_lowercase())
        .as_deref()
    {
        Some("wayland") => "wayland",
        Some("x11") => "x11",
        _ if std::env::var("WAYLAND_DISPLAY").is_ok_and(|value| !value.trim().is_empty()) => {
            "wayland"
        }
        _ if std::env::var("DISPLAY").is_ok_and(|value| !value.trim().is_empty()) => "x11",
        _ => "unknown",
    }
}

fn observed_display_variables() -> Vec<String> {
    [
        "DISPLAY",
        "WAYLAND_DISPLAY",
        "XDG_SESSION_TYPE",
        "XDG_CURRENT_DESKTOP",
        "DESKTOP_SESSION",
        "WSL_DISTRO_NAME",
        "WSL_INTEROP",
    ]
    .into_iter()
    .filter(|name| std::env::var(name).is_ok_and(|value| !value.trim().is_empty()))
    .map(str::to_string)
    .collect()
}

fn provider_outputs(providers: &[LiveProvider]) -> Vec<ProviderOutput> {
    providers
        .iter()
        .map(|provider| ProviderOutput {
            id: provider.id,
            model: provider.model,
        })
        .collect()
}

fn print_plan(plan: &LivePlanOutput) {
    println!("live\tdefault-suite={}", plan.default_suite);
    for check in &plan.checks {
        println!("  {}\t{}", check.id, check.command.join(" "));
    }
}

fn print_run_summary(run: &LiveRunOutput) {
    println!("artifacts: {}", run.artifact_root);
    for check in &run.checks {
        println!("{}\t{:?}\t{}", check.id, check.status, check.artifact_path);
    }
}

fn print_json<T: Serialize>(value: &T) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}

fn binary_name(base: &str) -> String {
    if cfg!(windows) {
        format!("{base}.exe")
    } else {
        base.to_string()
    }
}

fn playwright_timeout_ms(check_id: &str) -> u64 {
    match check_id {
        "web-skill-live" => 900_000,
        "opencode-acp-delegate-live" => 540_000,
        "web-subagent-live" => 420_000,
        "web-automation-live" | "opencode-acp-gui-live" => 360_000,
        _ => 240_000,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_json_shape_contains_registry() {
        let value = serde_json::to_value(list_output()).expect("json");
        assert_eq!(value["default_suite"], "smoke");
        assert!(
            value["checks"]
                .as_array()
                .unwrap()
                .iter()
                .any(|check| { check["id"] == "provider-smoke" })
        );
        assert!(
            value["providers"]
                .as_array()
                .unwrap()
                .iter()
                .any(|provider| { provider["id"] == "deepseek" })
        );
    }

    #[test]
    fn plan_defaults_to_smoke() {
        let plan = plan_output(
            &LiveSelection {
                checks: Vec::new(),
                suites: Vec::new(),
                all: false,
                providers: Vec::new(),
            },
            LiveEnvMode::default(),
            None,
        )
        .expect("plan");
        assert_eq!(plan.checks.len(), 1);
        assert_eq!(plan.checks[0].id, "provider-smoke");
        assert_eq!(plan.providers[0].id, "xiaomi-token-plan");
        assert_eq!(plan.environment.mode, LiveEnvMode::Shared);
    }

    #[test]
    fn plan_accepts_isolated_environment_mode() {
        let plan = plan_output(
            &LiveSelection {
                checks: Vec::new(),
                suites: Vec::new(),
                all: false,
                providers: Vec::new(),
            },
            LiveEnvMode::Isolated,
            None,
        )
        .expect("plan");
        assert_eq!(plan.environment.mode, LiveEnvMode::Isolated);
    }

    #[test]
    fn plan_expands_repeated_suite_and_provider_flags() {
        let plan = plan_output(
            &LiveSelection {
                checks: Vec::new(),
                suites: vec!["web".to_string(), "skill".to_string()],
                all: false,
                providers: vec!["deepseek".to_string()],
            },
            LiveEnvMode::Shared,
            Some(Path::new("/tmp/artifacts")),
        )
        .expect("plan");
        let ids = plan.checks.iter().map(|check| check.id).collect::<Vec<_>>();
        assert_eq!(
            ids,
            vec![
                "web-composer-draft-open-first-send",
                "web-composer-live",
                "web-automation-live",
                "web-subagent-live",
                "web-skill-live"
            ]
        );
        assert_eq!(plan.providers[0].id, "deepseek");
        assert_eq!(plan.artifact_root.as_deref(), Some("/tmp/artifacts"));
    }

    #[test]
    fn relative_explicit_artifact_root_makes_deterministic_context_paths_absolute() {
        let repo_root = std::env::temp_dir().join("psychevo-repo");
        let invocation_cwd = repo_root.join("nested");
        let artifact_root = resolve_live_artifact_root(
            &repo_root,
            &invocation_cwd,
            Some(PathBuf::from("artifacts/agent-live")),
        );
        let check_dir = artifact_root
            .join("live")
            .join("agent-managed-codex-offline");

        assert_eq!(artifact_root, invocation_cwd.join("artifacts/agent-live"));
        for path in [
            check_dir.clone(),
            check_dir.join("home"),
            check_dir.join("home/config.toml"),
            check_dir.join("state.db"),
            check_dir.join("cwd"),
        ] {
            assert!(
                path.is_absolute(),
                "context path must be absolute: {path:?}"
            );
        }
    }

    #[test]
    fn absolute_explicit_artifact_root_is_preserved() {
        let repo_root = std::env::temp_dir().join("psychevo-repo");
        let invocation_cwd = repo_root.join("nested");
        let explicit = repo_root.join("review-artifacts");

        assert_eq!(
            resolve_live_artifact_root(&repo_root, &invocation_cwd, Some(explicit.clone())),
            explicit
        );
    }

    #[test]
    fn plan_expands_desktop_suite_with_provider_live_check() {
        let plan = plan_output(
            &LiveSelection {
                checks: Vec::new(),
                suites: vec!["desktop".to_string()],
                all: false,
                providers: Vec::new(),
            },
            LiveEnvMode::Shared,
            None,
        )
        .expect("plan");
        let planned = plan
            .checks
            .iter()
            .map(|check| (check.id, check.command.clone()))
            .collect::<Vec<_>>();
        assert_eq!(
            planned,
            vec![
                (
                    "desktop-native-smoke-live",
                    vec![
                        "xtask-internal".to_string(),
                        "desktop-native-smoke".to_string(),
                        "provider-required=false".to_string(),
                    ],
                ),
                (
                    "desktop-floating-provider-live",
                    vec![
                        "xtask-internal".to_string(),
                        "desktop-native-smoke".to_string(),
                        "provider-required=true".to_string(),
                    ],
                ),
            ]
        );
    }

    #[test]
    fn agent_suite_plan_uses_only_deterministic_fakes_and_no_provider() {
        let plan = plan_output(
            &LiveSelection {
                checks: Vec::new(),
                suites: vec!["agents".to_string()],
                all: false,
                providers: Vec::new(),
            },
            LiveEnvMode::Shared,
            None,
        )
        .expect("plan");
        assert!(plan.providers.is_empty());
        assert!(!plan.checks.is_empty());
        assert!(plan.checks.iter().all(|check| {
            check.command.get(1).map(String::as_str) == Some("playwright-deterministic")
        }));
    }

    #[test]
    fn desktop_provider_live_probe_text_exposes_token_in_preview() {
        let token = desktop_provider_live_token(registry::XIAOMI_TOKEN_PLAN);
        let text = desktop_floating_live_text(Some(&token));
        assert!(text.contains(&token));
        assert!(
            text.chars().count() <= 80,
            "Desktop activation preview truncates after 80 characters: {text}"
        );
    }

    #[test]
    fn desktop_native_wdio_command_uses_live_environment_without_provider() {
        let root = std::env::temp_dir().join(format!(
            "psychevo-xtask-desktop-live-env-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("time")
                .as_nanos()
        ));
        let dev_home = root.join(".local").join(".psychevo-dev");
        fs::create_dir_all(&dev_home).expect("dev home");
        fs::write(dev_home.join("config.toml"), "").expect("config");
        fs::write(dev_home.join(".env"), "DUMMY_ENV=1\n").expect("env");

        let prerequisites = LivePrerequisites::load(&root).expect("prerequisites");
        let check_dir = root.join("check");
        let live_env = prerequisites
            .resolve(LiveEnvMode::Shared, &check_dir)
            .expect("live env");
        let mut command = ProcessCommand::new("pnpm");
        configure_desktop_wdio_command(
            &mut command,
            Path::new("/tmp/wdio-artifacts"),
            "selected text",
            Some(&live_env),
            None,
            Some(Path::new("/tmp/pevo")),
            None,
        );

        assert_eq!(
            command_env(&command, "PSYCHEVO_HOME").as_deref(),
            Some(dev_home.to_string_lossy().as_ref())
        );
        assert_eq!(
            command_env(&command, "PSYCHEVO_CONFIG").as_deref(),
            Some(dev_home.join("config.toml").to_string_lossy().as_ref())
        );
        assert_eq!(
            command_env(&command, "PSYCHEVO_DB").as_deref(),
            Some(dev_home.join("state.db").to_string_lossy().as_ref())
        );
        assert_eq!(command_env(&command, "DUMMY_ENV").as_deref(), Some("1"));
        assert!(command_env(&command, "PSYCHEVO_INFERENCE_PROVIDER").is_none());
        assert!(command_env(&command, "PSYCHEVO_DESKTOP_PROVIDER_LIVE").is_none());

        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn desktop_startup_artifact_validation_requires_complete_screenshot_evidence() {
        let root = std::env::temp_dir().join(format!(
            "psychevo-xtask-desktop-startup-artifacts-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("time")
                .as_nanos()
        ));
        let screenshots = root.join("screenshots");
        fs::create_dir_all(&screenshots).expect("screenshots");
        for filename in ["gui-ready.png", "draft-ready.png"] {
            fs::write(screenshots.join(filename), "proof").expect("screenshot");
        }
        fs::write(root.join("desktop-startup-rust.jsonl"), "{}\n").expect("rust trace");
        let checkpoints = [
            "process_start",
            "window_ready",
            "managed_gateway_ready",
            "bridge_connected",
            "gui_ready",
            "draft_context_ready",
        ]
        .into_iter()
        .map(|id| {
            let screenshot = match id {
                "gui_ready" => serde_json::json!({ "path": "screenshots/gui-ready.png" }),
                "draft_context_ready" => {
                    serde_json::json!({ "path": "screenshots/draft-ready.png" })
                }
                _ => serde_json::Value::Null,
            };
            serde_json::json!({ "id": id, "screenshot": screenshot, "status": "complete" })
        })
        .collect::<Vec<_>>();
        fs::write(
            root.join("desktop-startup-journey.json"),
            serde_json::to_vec(&serde_json::json!({
                "schemaVersion": 1,
                "run": { "outcome": "passed" },
                "checkpoints": checkpoints,
            }))
            .expect("manifest json"),
        )
        .expect("manifest");

        validate_desktop_startup_artifacts(&root).expect("valid startup evidence");
        fs::remove_file(screenshots.join("draft-ready.png")).expect("remove screenshot");
        let error = validate_desktop_startup_artifacts(&root)
            .expect_err("missing screenshot must fail")
            .to_string();
        assert!(error.contains("draft_context_ready"));

        fs::remove_dir_all(root).expect("cleanup");
    }

    fn command_env(command: &ProcessCommand, key: &str) -> Option<String> {
        command.get_envs().find_map(|(env_key, value)| {
            (env_key == key).then(|| value.map(|value| value.to_string_lossy().into_owned()))?
        })
    }
}
