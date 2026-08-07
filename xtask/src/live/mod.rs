mod environment;
mod registry;
mod verifier;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command as ProcessCommand;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{Context, Result, bail};
use clap::{Args, Subcommand};
use serde::Serialize;

use crate::ci::artifacts::{default_artifact_root, display_path};
use crate::ci::process::{
    ProcessOutcome, command_exists, create_step_log, run_logged_process, write_log_line,
    write_mirrored_line,
};
use crate::ci::retention::warn_if_ci_retention_cleanup_fails;
use crate::desktop_wdio::{DesktopWdioOptions, DesktopWdioTimeouts, run_desktop_wdio};
use crate::host_command;

use self::registry::{
    DEFAULT_SUITE, LIVE_CHECKS, LIVE_SUITES, LiveCheck, LiveCheckAction, LiveProvider,
    LiveProviderSupport, LiveSelection, command_for_plan, resolve_providers, select_checks,
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

#[derive(Clone, Copy, Debug, Serialize)]
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
    id: String,
    check_id: &'static str,
    description: &'static str,
    suites: Vec<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    provider: Option<ProviderOutput>,
    #[serde(skip_serializing_if = "Option::is_none")]
    skip_reason: Option<String>,
    command: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
struct CheckRunOutput {
    id: String,
    check_id: &'static str,
    description: &'static str,
    status: LiveStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    provider: Option<ProviderOutput>,
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

#[derive(Clone, Copy, Debug)]
struct PlannedLiveCheck {
    check: &'static LiveCheck,
    provider: Option<LiveProvider>,
}

impl PlannedLiveCheck {
    fn id(self) -> String {
        self.provider.map_or_else(
            || self.check.id.to_string(),
            |provider| format!("{}@{}", self.check.id, provider.id),
        )
    }

    fn artifact_path(self, artifact_root: &Path) -> PathBuf {
        let root = artifact_root.join("live").join(self.check.id);
        self.provider
            .map_or(root.clone(), |provider| root.join(provider.id))
    }

    fn provider_output(self) -> Option<ProviderOutput> {
        self.provider.map(provider_output)
    }

    fn unsupported_reason(self) -> Option<String> {
        let provider = self.provider?;
        match provider_support(self.check) {
            LiveProviderSupport::Only(allowed) if !allowed.contains(&provider.id) => Some(format!(
                "check '{}' supports provider(s) [{}], not '{}'",
                self.check.id,
                allowed.join(", "),
                provider.id
            )),
            _ => None,
        }
    }
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
    let check_dir = artifact_root
        .join("live")
        .join(check.id)
        .join(providers[0].id);
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
    let checks = planned_checks(&checks, &providers);
    Ok(LivePlanOutput {
        default_suite: DEFAULT_SUITE,
        environment: LiveEnvironmentPlanOutput { mode: env_mode },
        artifact_root: artifact_root.map(display_path),
        providers: provider_outputs(&providers),
        checks: checks
            .into_iter()
            .map(|check| CheckPlanOutput {
                id: check.id(),
                check_id: check.check.id,
                description: check.check.description,
                suites: check.check.suites.to_vec(),
                provider: check.provider_output(),
                skip_reason: check.unsupported_reason(),
                command: command_for_plan(check.check),
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
    let checks = planned_checks(&checks, &providers);
    let mut outputs = Vec::new();
    for check in checks {
        let id = check.id();
        println!("live {id} ...");
        let check_dir = check.artifact_path(&artifact_root);
        let log_path = check_dir.join("logs").join("check.log");
        fs::create_dir_all(check_dir.join("logs"))
            .with_context(|| format!("create {}", check_dir.display()))?;
        let log = create_step_log(&log_path)?;
        let result = if let Some(reason) = check.unsupported_reason() {
            skipped_with_env(Arc::clone(&log), reason, None)?
        } else {
            let scoped_providers = check.provider.as_slice();
            run_check(
                root,
                &artifact_root,
                &check_dir,
                check.check,
                scoped_providers,
                env_mode,
                Arc::clone(&log),
            )?
        };
        let output = check_run_output(check, result, &check_dir, &log_path);
        fs::write(
            check_dir.join("result.json"),
            serde_json::to_vec_pretty(&output)?,
        )
        .with_context(|| format!("write {}", check_dir.join("result.json").display()))?;
        println!("live {}: {:?}", output.id, output.status);
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

fn check_run_output(
    check: PlannedLiveCheck,
    result: CheckResult,
    check_dir: &Path,
    log_path: &Path,
) -> CheckRunOutput {
    let environment = result.environment.as_ref();
    CheckRunOutput {
        id: check.id(),
        check_id: check.check.id,
        description: check.check.description,
        status: result.status,
        provider: check.provider_output(),
        artifact_path: display_path(check_dir),
        log_path: display_path(log_path),
        home_path: environment.map(|env| env.home_path.clone()),
        config_path: environment.map(|env| env.config_path.clone()),
        db_path: environment.map(|env| env.db_path.clone()),
        detail: result.detail,
    }
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

fn planned_checks(
    checks: &[&'static LiveCheck],
    providers: &[LiveProvider],
) -> Vec<PlannedLiveCheck> {
    checks
        .iter()
        .flat_map(|check| match provider_support(check) {
            LiveProviderSupport::None => vec![PlannedLiveCheck {
                check,
                provider: None,
            }],
            LiveProviderSupport::Any | LiveProviderSupport::Only(_) => providers
                .iter()
                .copied()
                .map(|provider| PlannedLiveCheck {
                    check,
                    provider: Some(provider),
                })
                .collect(),
        })
        .collect()
}

fn provider_support(check: &LiveCheck) -> LiveProviderSupport {
    match check.action {
        LiveCheckAction::DesktopNativeSmoke { provider_required } => {
            if provider_required {
                LiveProviderSupport::Any
            } else {
                LiveProviderSupport::None
            }
        }
        LiveCheckAction::CargoIgnoredTest {
            provider_support, ..
        } => provider_support,
        LiveCheckAction::ProviderSmoke
        | LiveCheckAction::PevoDoctorLive
        | LiveCheckAction::Playwright { .. } => LiveProviderSupport::Any,
        LiveCheckAction::DeterministicPlaywright { .. } => LiveProviderSupport::None,
    }
}

fn check_requires_provider(check: &LiveCheck) -> bool {
    provider_support(check) != LiveProviderSupport::None
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
        LiveCheckAction::CargoIgnoredTest {
            package,
            test,
            features,
            ..
        } => run_cargo_ignored_live_check_with_command_resolver(
            root,
            check_dir,
            providers,
            env_mode,
            (package, test, features),
            log,
            resolve_live_command_path,
        ),
        LiveCheckAction::DeterministicPlaywright {
            spec,
            grep,
            channels,
        } => run_deterministic_playwright_check(
            root,
            artifact_root,
            check_dir,
            check,
            env_mode,
            spec,
            grep,
            channels,
            log,
        ),
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
    channels: &'static [&'static str],
    log: Arc<Mutex<fs::File>>,
) -> Result<CheckResult> {
    if !command_exists("pnpm") {
        return blocked(
            log,
            "missing pnpm; run: pnpm exec playwright install chromium".to_string(),
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
    reset_test_owned_directory(&home)?;
    reset_test_owned_directory(&cwd)?;
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
    match prepare_deterministic_channel_extensions(
        root,
        check_dir,
        &home,
        &cwd,
        &pevo_bin,
        channels,
        Arc::clone(&log),
    )? {
        Ok(suppressed) => had_suppressed_output |= suppressed,
        Err(mut result) => {
            result.environment = Some(environment);
            return Ok(result.include_suppressed_output(had_suppressed_output));
        }
    }
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

    let mut build = host_command::pnpm(["--filter", "@psychevo/workbench", "build"])?;
    build
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

    let mut test = host_command::pnpm([
        "exec",
        "playwright",
        "test",
        spec,
        "--grep",
        grep,
        "--project",
        "chromium-desktop",
    ])?;
    test.current_dir(root)
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
    let mut live_env =
        match prerequisites.resolve(desktop_live_environment_mode(env_mode), check_dir) {
            Ok(live_env) => live_env,
            Err(error) => return failed_result(log, format!("{error:#}"), None),
        };
    let mut provider = None;
    if provider_required {
        let Some(selected_provider) = providers.first().copied() else {
            return blocked_with_env(
                log,
                "no live provider selected".to_string(),
                Some(live_env.to_output()),
            );
        };
        if !prerequisites.provider_credentials_available(&selected_provider) {
            return blocked_with_env(
                log,
                format!(
                    "{} credentials missing from .local/.psychevo-dev/.env",
                    selected_provider.id
                ),
                Some(live_env.to_output()),
            );
        }
        let config_path = check_dir.join("desktop-provider-config.toml");
        write_desktop_provider_live_config(&config_path, selected_provider)?;
        live_env = live_env.with_config_path(config_path);
        provider = Some(selected_provider);
    }
    let environment = Some(live_env.to_output());

    let skip_reason = desktop_native_skip_reason();
    write_desktop_capability_snapshot(check_dir, provider_required, skip_reason.clone())?;
    if let Some(reason) = skip_reason {
        return skipped_with_env(log, reason, environment);
    }
    if !command_exists("pnpm") {
        return blocked_with_env(
            log,
            "missing pnpm; run: pnpm exec playwright install chromium".to_string(),
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
    let provider_token = provider.map(|_| desktop_provider_live_sentinel());
    let floating_text = desktop_floating_live_text(provider_token.as_deref());
    let options = DesktopWdioOptions {
        root,
        artifact_root: &wdio_artifact_root,
        pevo_bin: &pevo_bin,
        floating_text: &floating_text,
        provider_token: provider_token.as_deref(),
        timeouts: DesktopWdioTimeouts {
            build: Duration::from_secs(45 * 60),
            smoke: Duration::from_secs(15 * 60),
            cleanup: Duration::from_secs(2 * 60),
        },
    };
    let run = run_desktop_wdio(&options, Arc::clone(&log), |command| {
        live_env.apply_to_command(command, provider);
    })?;
    had_suppressed_output |= run.outcome.had_suppressed_output;
    if let Some(detail) = run.failure_detail {
        return Ok(failed_result(log, detail, environment)?
            .include_suppressed_output(had_suppressed_output));
    }
    Ok(
        check_result_from_outcome(run.outcome, "Desktop native WDIO smoke failed", environment)?
            .include_suppressed_output(had_suppressed_output),
    )
}

fn desktop_live_environment_mode(_requested: LiveEnvMode) -> LiveEnvMode {
    LiveEnvMode::Isolated
}

fn desktop_provider_live_sentinel() -> String {
    "psychevo desktop live response ok".to_string()
}

fn write_desktop_provider_live_config(config: &Path, provider: LiveProvider) -> Result<()> {
    let mut root = toml::map::Map::new();
    root.insert(
        "model".to_string(),
        toml::Value::String(provider.model.to_string()),
    );
    fs::write(config, toml::to_string(&toml::Value::Table(root))?)
        .with_context(|| format!("write Desktop provider live config {}", config.display()))
}

fn desktop_floating_live_text(provider_token: Option<&str>) -> String {
    provider_token.map_or_else(
        || "Psychevo floating live smoke selected text".to_string(),
        |token| format!("Selected text for live check: {token}"),
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
    live_env.apply_to_command(&mut command, providers.first().copied());
    let outcome = run_logged_process("pevo-doctor-live", &mut command, log)?;
    Ok(check_result_from_outcome(
        outcome,
        "pevo doctor --live failed",
        Some(live_env.to_output()),
    )?
    .include_suppressed_output(had_suppressed_output))
}

fn run_cargo_ignored_live_check_with_command_resolver<F>(
    root: &Path,
    check_dir: &Path,
    providers: &[LiveProvider],
    env_mode: LiveEnvMode,
    cargo_test: (&'static str, &'static str, &'static [&'static str]),
    log: Arc<Mutex<fs::File>>,
    resolve_command: F,
) -> Result<CheckResult>
where
    F: Fn(&str) -> Option<PathBuf>,
{
    let (package, test, features) = cargo_test;
    let prerequisites = match LivePrerequisites::load(root) {
        Ok(prerequisites) => prerequisites,
        Err(reason) => return blocked(log, reason),
    };
    let live_env = match prerequisites.resolve(env_mode, check_dir) {
        Ok(live_env) => live_env,
        Err(error) => return failed_result(log, format!("{error:#}"), None),
    };
    let mut environment = live_env.to_output();
    let codex_broker_profile = if test
        == "server::codex_capability_broker::tests::live_codex_plugin_broker_lists_installed_plugins"
    {
        let Some(binary) = resolve_command("codex") else {
            return blocked_with_env(
                log,
                "codex-plugin-broker-live requires a Codex executable on PATH".to_string(),
                Some(environment),
            );
        };
        match prepare_codex_broker_live_profile(check_dir, binary) {
            Ok(profile) => Some(profile),
            Err(error) => {
                return failed_result(
                    log,
                    format!("failed to prepare codex-plugin-broker-live profile: {error:#}"),
                    Some(environment),
                );
            }
        }
    } else {
        None
    };
    if let Some(profile) = codex_broker_profile.as_ref() {
        environment.mode = LiveEnvMode::Isolated;
        environment.home_path = profile.home.display().to_string();
        environment.config_path = profile.config.display().to_string();
        environment.db_path = profile.db.display().to_string();
    }
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
    command.args(["test", "-p", package]);
    if !features.is_empty() {
        command.args(["--features", &features.join(",")]);
    }
    command
        .args([test, "--", "--ignored", "--exact"])
        .current_dir(root);
    live_env.apply_to_command(&mut command, providers.first().copied());
    if let Some(profile) = codex_broker_profile {
        command
            .env("PSYCHEVO_HOME", profile.home)
            .env("PSYCHEVO_CONFIG", profile.config)
            .env("PSYCHEVO_DB", profile.db)
            .env("PSYCHEVO_CODEX_BIN", profile.binary);
    }
    let outcome = run_logged_process(test, &mut command, log)?;
    check_result_from_outcome(outcome, &format!("{test} failed"), Some(environment))
}

struct CodexBrokerLiveProfile {
    home: PathBuf,
    config: PathBuf,
    db: PathBuf,
    binary: PathBuf,
}

fn prepare_codex_broker_live_profile(
    check_dir: &Path,
    binary: PathBuf,
) -> Result<CodexBrokerLiveProfile> {
    let home = check_dir.join("codex-authority-home");
    let config = home.join("config.toml");
    let db = check_dir.join("codex-authority-state.db");
    write_codex_broker_live_profile(&config, &binary)?;
    Ok(CodexBrokerLiveProfile {
        home,
        config,
        db,
        binary,
    })
}

fn write_codex_broker_live_profile(config: &Path, binary: &Path) -> Result<()> {
    let mut authority = toml::map::Map::new();
    authority.insert("enabled".to_string(), toml::Value::Boolean(true));
    authority.insert(
        "binary".to_string(),
        toml::Value::String(binary.display().to_string()),
    );
    let mut profile = toml::map::Map::new();
    profile.insert("codex_plugins".to_string(), toml::Value::Table(authority));
    let parent = config
        .parent()
        .context("Codex broker live profile has no parent directory")?;
    fs::create_dir_all(parent)
        .with_context(|| format!("create Codex broker live home {}", parent.display()))?;
    fs::write(config, toml::to_string(&toml::Value::Table(profile))?)
        .with_context(|| format!("write {}", config.display()))
}

fn resolve_live_command_path(command: &str) -> Option<PathBuf> {
    crate::host_command::resolve(command)
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
            "missing pnpm; run: pnpm exec playwright install chromium".to_string(),
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

    let mut build = host_command::pnpm(["--filter", "@psychevo/workbench", "build"])?;
    build.current_dir(root);
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

    let mut test = host_command::pnpm([
        "exec",
        "playwright",
        "test",
        spec,
        "--grep",
        grep,
        "--project",
        "chromium-desktop",
    ])?;
    test.current_dir(root);
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

fn prepare_deterministic_channel_extensions(
    root: &Path,
    check_dir: &Path,
    home: &Path,
    cwd: &Path,
    pevo_bin: &Path,
    channels: &[&str],
    log: Arc<Mutex<fs::File>>,
) -> Result<Result<bool, CheckResult>> {
    let mut had_suppressed_output = false;
    for channel in channels {
        let (package, binary) = channel_extension_source(channel)?;
        let mut build = ProcessCommand::new("cargo");
        build
            .args([
                "build",
                "--quiet",
                "--release",
                "-p",
                package,
                "--bin",
                binary,
            ])
            .current_dir(root);
        let outcome = run_logged_process(
            &format!("build deterministic {channel} Channel Extension"),
            &mut build,
            Arc::clone(&log),
        )?;
        had_suppressed_output |= outcome.had_suppressed_output;
        if !outcome.passed {
            return Ok(Err(CheckResult {
                status: LiveStatus::Failed,
                detail: Some(format!(
                    "failed to build deterministic {channel} Channel Extension"
                )),
                environment: None,
                had_suppressed_output,
            }));
        }

        let manifest = root
            .join("crates")
            .join(package)
            .join("psychevo.extension.json");
        let executable = root
            .join("target")
            .join("release")
            .join(binary_name(binary));
        let package_root = check_dir.join("extensions").join(package);
        if let Err(error) = materialize_local_channel_extension(
            &manifest,
            &executable,
            &package_root,
            &binary_name(binary),
        ) {
            return Ok(Err(failed_result(
                Arc::clone(&log),
                format!(
                    "failed to materialize deterministic {channel} Channel Extension: {error:#}"
                ),
                None,
            )?));
        }

        let mut install = ProcessCommand::new(pevo_bin);
        install
            .arg("install")
            .arg(&package_root)
            .arg("--json")
            .current_dir(cwd)
            .env("PSYCHEVO_HOME", home)
            .env("PSYCHEVO_CONFIG", home.join("config.toml"));
        let outcome = run_logged_process(
            &format!("install deterministic {channel} Channel Extension"),
            &mut install,
            Arc::clone(&log),
        )?;
        had_suppressed_output |= outcome.had_suppressed_output;
        if !outcome.passed {
            return Ok(Err(CheckResult {
                status: LiveStatus::Failed,
                detail: Some(format!(
                    "failed to install deterministic {channel} Channel Extension"
                )),
                environment: None,
                had_suppressed_output,
            }));
        }
    }
    Ok(Ok(had_suppressed_output))
}

fn channel_extension_source(channel: &str) -> Result<(&'static str, &'static str)> {
    match channel {
        "wechat" => Ok((
            "psychevo-extension-channel-wechat",
            "psychevo-channel-wechat",
        )),
        "telegram" => Ok((
            "psychevo-extension-channel-telegram",
            "psychevo-channel-telegram",
        )),
        "feishu" | "lark" => Ok((
            "psychevo-extension-channel-feishu-lark",
            "psychevo-channel-feishu-lark",
        )),
        _ => bail!("unsupported deterministic Channel Extension `{channel}`"),
    }
}

fn materialize_local_channel_extension(
    source_manifest: &Path,
    source_executable: &Path,
    package_root: &Path,
    executable_name: &str,
) -> Result<()> {
    let mut manifest: serde_json::Value = serde_json::from_slice(
        &fs::read(source_manifest)
            .with_context(|| format!("read {}", source_manifest.display()))?,
    )
    .with_context(|| format!("parse {}", source_manifest.display()))?;
    manifest["version"] = serde_json::Value::String("local".to_string());
    manifest["runtime"]["executable"] = serde_json::Value::String(format!("./{executable_name}"));
    fs::create_dir_all(package_root)
        .with_context(|| format!("create {}", package_root.display()))?;
    let mut manifest_bytes = serde_json::to_vec_pretty(&manifest)?;
    manifest_bytes.push(b'\n');
    fs::write(package_root.join("psychevo.extension.json"), manifest_bytes).with_context(|| {
        format!(
            "write local Extension manifest in {}",
            package_root.display()
        )
    })?;
    fs::copy(source_executable, package_root.join(executable_name)).with_context(|| {
        format!(
            "copy Channel Extension executable {} into {}",
            source_executable.display(),
            package_root.display()
        )
    })?;
    Ok(())
}

fn reset_test_owned_directory(path: &Path) -> Result<()> {
    if path.exists() {
        fs::remove_dir_all(path).with_context(|| format!("reset {}", path.display()))?;
    }
    fs::create_dir_all(path).with_context(|| format!("create {}", path.display()))
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
    providers.iter().copied().map(provider_output).collect()
}

fn provider_output(provider: LiveProvider) -> ProviderOutput {
    ProviderOutput {
        id: provider.id,
        model: provider.model,
    }
}

fn print_plan(plan: &LivePlanOutput) {
    println!("live\tdefault-suite={}", plan.default_suite);
    for check in &plan.checks {
        if let Some(reason) = &check.skip_reason {
            println!("  {}\tskip: {reason}", check.id);
        } else {
            println!("  {}\t{}", check.id, check.command.join(" "));
        }
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
        "web-automation-live" | "opencode-acp-gui-lifecycle-live" => 360_000,
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
        assert_eq!(plan.checks[0].id, "provider-smoke@xiaomi-token-plan");
        assert_eq!(plan.checks[0].check_id, "provider-smoke");
        assert_eq!(
            plan.checks[0].provider.map(|provider| provider.id),
            Some("xiaomi-token-plan")
        );
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
        let ids = plan
            .checks
            .iter()
            .map(|check| check.id.as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            ids,
            vec![
                "web-composer-draft-open-first-send",
                "web-composer-live@deepseek",
                "web-automation-live@deepseek",
                "web-subagent-live@deepseek",
                "web-skill-live@deepseek"
            ]
        );
        assert_eq!(plan.providers[0].id, "deepseek");
        assert_eq!(plan.artifact_root.as_deref(), Some("/tmp/artifacts"));
    }

    #[test]
    fn multi_provider_plan_expands_checks_and_exposes_allowlisted_skips() {
        let plan = plan_output(
            &LiveSelection {
                checks: vec![
                    "runtime-provider-read".to_string(),
                    "web-composer-live".to_string(),
                ],
                suites: Vec::new(),
                all: false,
                providers: vec!["xiaomi-token-plan".to_string(), "deepseek".to_string()],
            },
            LiveEnvMode::Isolated,
            Some(Path::new("/tmp/live-evidence")),
        )
        .expect("multi-provider plan");

        assert_eq!(
            plan.checks
                .iter()
                .map(|check| check.id.as_str())
                .collect::<Vec<_>>(),
            vec![
                "runtime-provider-read@xiaomi-token-plan",
                "runtime-provider-read@deepseek",
                "web-composer-live@xiaomi-token-plan",
                "web-composer-live@deepseek",
            ]
        );
        assert!(plan.checks[0].skip_reason.is_none());
        assert!(
            plan.checks[1]
                .skip_reason
                .as_deref()
                .is_some_and(|reason| reason.contains("not 'deepseek'"))
        );
        assert!(
            plan.checks[2..]
                .iter()
                .all(|check| check.skip_reason.is_none())
        );
        assert_eq!(
            plan.checks
                .iter()
                .map(|check| check.provider.map(|provider| provider.id))
                .collect::<Vec<_>>(),
            vec![
                Some("xiaomi-token-plan"),
                Some("deepseek"),
                Some("xiaomi-token-plan"),
                Some("deepseek"),
            ]
        );
    }

    #[test]
    fn provider_scoped_result_names_provider_in_identity_and_artifact_path() {
        let check = PlannedLiveCheck {
            check: registry::check_by_id("web-composer-live").expect("live check"),
            provider: Some(registry::DEEPSEEK),
        };
        let check_dir = check.artifact_path(Path::new("/tmp/live-evidence"));
        let output = check_run_output(
            check,
            CheckResult {
                status: LiveStatus::Passed,
                detail: None,
                environment: None,
                had_suppressed_output: false,
            },
            &check_dir,
            &check_dir.join("logs/check.log"),
        );
        let json = serde_json::to_value(output).expect("result JSON");

        assert_eq!(json["id"], "web-composer-live@deepseek");
        assert_eq!(json["check_id"], "web-composer-live");
        assert_eq!(json["provider"]["id"], "deepseek");
        assert!(
            json["artifact_path"]
                .as_str()
                .is_some_and(|path| path.ends_with("/live/web-composer-live/deepseek"))
        );
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
    fn deterministic_channel_package_uses_local_manifest_and_built_executable() {
        let root = std::env::temp_dir().join(format!(
            "psychevo-channel-live-package-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("time")
                .as_nanos()
        ));
        let source_manifest = root.join("source/psychevo.extension.json");
        let source_executable = root.join("source/telegram-fixture");
        let package_root = root.join("materialized");
        fs::create_dir_all(source_manifest.parent().expect("source parent")).expect("source");
        fs::write(
            &source_manifest,
            br#"{
  "schemaVersion": 1,
  "id": "psychevo.channel.telegram",
  "version": "0.1.0",
  "runtime": {
    "protocol": "psychevo-extension/1",
    "executable": "./release-name"
  }
}"#,
        )
        .expect("manifest");
        fs::write(&source_executable, b"sidecar fixture").expect("executable");

        materialize_local_channel_extension(
            &source_manifest,
            &source_executable,
            &package_root,
            "telegram-fixture",
        )
        .expect("materialize local Channel Extension");

        let manifest: serde_json::Value = serde_json::from_slice(
            &fs::read(package_root.join("psychevo.extension.json")).expect("read manifest"),
        )
        .expect("parse manifest");
        assert_eq!(manifest["id"], "psychevo.channel.telegram");
        assert_eq!(manifest["version"], "local");
        assert_eq!(manifest["runtime"]["executable"], "./telegram-fixture");
        assert_eq!(
            fs::read(package_root.join("telegram-fixture")).expect("read executable"),
            b"sidecar fixture"
        );
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn deterministic_check_directory_reset_removes_prior_runtime_state() {
        let root = std::env::temp_dir().join(format!(
            "psychevo-live-state-reset-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("time")
                .as_nanos()
        ));
        fs::create_dir_all(root.join("runtime-adapters/codex-acp")).expect("old state");
        fs::write(root.join("runtime-adapters/codex-acp/seal.json"), b"stale").expect("old seal");

        reset_test_owned_directory(&root).expect("reset test directory");

        assert!(root.is_dir());
        assert_eq!(fs::read_dir(&root).expect("read reset root").count(), 0);
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn codex_broker_live_profile_is_enabled_only_in_the_check_home() {
        let root = std::env::temp_dir().join(format!(
            "psychevo-codex-broker-live-profile-{}",
            std::process::id()
        ));
        let config = root.join("home/config.toml");
        let binary = root.join("bin/codex");

        write_codex_broker_live_profile(&config, &binary).expect("write live profile");

        let parsed = fs::read_to_string(&config)
            .expect("read live profile")
            .parse::<toml::Value>()
            .expect("parse live profile");
        assert_eq!(parsed["codex_plugins"]["enabled"].as_bool(), Some(true));
        assert_eq!(
            parsed["codex_plugins"]["binary"].as_str(),
            Some(binary.to_string_lossy().as_ref())
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn missing_codex_executable_blocks_only_the_broker_live_check() {
        let root = std::env::temp_dir().join(format!(
            "psychevo-xtask-missing-codex-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("time")
                .as_nanos()
        ));
        let dev_home = root.join(".local").join(".psychevo-dev");
        fs::create_dir_all(&dev_home).expect("dev home");
        fs::write(dev_home.join("config.toml"), "").expect("config");
        fs::write(dev_home.join(".env"), "").expect("env");
        let check_dir = root.join("check");
        fs::create_dir_all(&check_dir).expect("check dir");
        let log = Arc::new(Mutex::new(
            fs::File::create(check_dir.join("check.log")).expect("log"),
        ));

        let result = run_cargo_ignored_live_check_with_command_resolver(
            &root,
            &check_dir,
            &[],
            LiveEnvMode::Shared,
            (
                "psychevo-gateway",
                "server::codex_capability_broker::tests::live_codex_plugin_broker_lists_installed_plugins",
                &[],
            ),
            log,
            |_| None,
        )
        .expect("check result");

        assert_eq!(result.status, LiveStatus::Blocked);
        assert!(
            result
                .detail
                .as_deref()
                .is_some_and(|detail| detail.contains("Codex executable"))
        );
        assert!(result.environment.is_some());
        fs::remove_dir_all(root).expect("cleanup");
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
            .map(|check| (check.id.as_str(), check.command.clone()))
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
                    "desktop-floating-provider-live@xiaomi-token-plan",
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
    fn desktop_live_checks_are_isolated_from_a_shared_live_run() {
        assert_eq!(
            desktop_live_environment_mode(LiveEnvMode::Shared),
            LiveEnvMode::Isolated
        );
        assert_eq!(
            desktop_live_environment_mode(LiveEnvMode::Isolated),
            LiveEnvMode::Isolated
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
    fn desktop_provider_live_probe_text_exposes_benign_sentinel_in_preview() {
        let sentinel = desktop_provider_live_sentinel();
        let text = desktop_floating_live_text(Some(&sentinel));
        assert!(text.contains(&sentinel));
        assert_eq!(sentinel, "psychevo desktop live response ok");
        assert!(!text.to_ascii_lowercase().contains("token:"));
        assert!(
            text.chars().count() <= 80,
            "Desktop activation preview truncates after 80 characters: {text}"
        );
    }

    #[test]
    fn desktop_provider_live_config_selects_the_claimed_model() {
        let root = std::env::temp_dir().join(format!(
            "psychevo-desktop-provider-live-config-{}",
            std::process::id()
        ));
        let config = root.join("config.toml");
        fs::create_dir_all(&root).expect("config parent");

        write_desktop_provider_live_config(&config, registry::XIAOMI_TOKEN_PLAN)
            .expect("write provider config");

        let parsed = fs::read_to_string(&config)
            .expect("read provider config")
            .parse::<toml::Value>()
            .expect("parse provider config");
        assert_eq!(
            parsed["model"].as_str(),
            Some(registry::XIAOMI_TOKEN_PLAN.model)
        );
        let _ = fs::remove_dir_all(root);
    }
}
