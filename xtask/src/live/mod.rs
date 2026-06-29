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
}

#[derive(Debug)]
struct CheckResult {
    status: LiveStatus,
    detail: Option<String>,
    environment: Option<LiveEnvironmentPathsOutput>,
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
            if let Some(non_success) = run
                .checks
                .iter()
                .find(|check| check.status != LiveStatus::Passed)
            {
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
        });
    }

    let status_code = match result.status {
        LiveStatus::Passed => 0,
        LiveStatus::Failed => 1,
        LiveStatus::Blocked => 2,
    };
    let detail = result
        .detail
        .unwrap_or_else(|| "live provider smoke did not pass".to_string());
    write_mirrored_line(&log, &format!("single-provider-live: {detail}"))?;
    Ok(ProcessOutcome {
        passed: false,
        exit_code: Some(status_code),
        mirrored_diagnostics: 1,
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
    let providers = resolve_providers(&selection.providers)?;
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
    let artifact_root = artifact_root.unwrap_or_else(|| default_artifact_root(root));
    fs::create_dir_all(artifact_root.join("live"))
        .with_context(|| format!("create artifact root {}", artifact_root.display()))?;
    let plan = plan_output(selection, env_mode, Some(&artifact_root))?;
    fs::write(
        artifact_root.join("live-plan.json"),
        serde_json::to_vec_pretty(&plan)?,
    )
    .with_context(|| format!("write {}", artifact_root.join("live-plan.json").display()))?;

    let checks = select_checks(selection)?;
    let providers = resolve_providers(&selection.providers)?;
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
        LiveCheckAction::ProviderSmoke => {
            run_provider_smoke_check(root, check_dir, providers, env_mode, log)
        }
        LiveCheckAction::PevoDoctorLive => {
            run_pevo_doctor_live_check(root, check_dir, providers, env_mode, log)
        }
        LiveCheckAction::CargoIgnoredTest { package, test } => {
            run_cargo_ignored_live_check(root, check_dir, providers, env_mode, package, test, log)
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
    let pevo_bin = match ensure_pevo_built(root, Arc::clone(&log))? {
        Ok(path) => path,
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
        return failed_result(log, detail, Some(environment));
    }
    if let Some(reason) = blocked_reason {
        return blocked_with_env(log, reason, Some(environment));
    }
    Ok(CheckResult {
        status: LiveStatus::Passed,
        detail: Some(format!(
            "{} provider smoke run(s) passed",
            verifications.len()
        )),
        environment: Some(environment),
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
    let pevo_bin = match ensure_pevo_built(root, Arc::clone(&log))? {
        Ok(path) => path,
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
    check_result_from_outcome(
        outcome,
        "pevo doctor --live failed",
        Some(live_env.to_output()),
    )
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

    let pevo_bin = match ensure_pevo_built(root, Arc::clone(&log))? {
        Ok(path) => path,
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
        return check_result_from_outcome(
            build_outcome,
            "Workbench build failed",
            Some(live_env.to_output()),
        );
    }

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
        .env("PSYCHEVO_CI_ARTIFACT_ROOT", artifact_root);
    let outcome = run_logged_process(check.id, &mut test, log)?;
    check_result_from_outcome(
        outcome,
        &format!("Playwright live check {} failed", check.id),
        Some(live_env.to_output()),
    )
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
    })
}

fn ensure_pevo_built(
    root: &Path,
    log: Arc<Mutex<fs::File>>,
) -> Result<Result<PathBuf, CheckResult>> {
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
        }));
    }
    Ok(Ok(pevo_bin))
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
    })
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
                "web-composer-live",
                "web-automation-live",
                "web-subagent-live",
                "web-skill-live"
            ]
        );
        assert_eq!(plan.providers[0].id, "deepseek");
        assert_eq!(plan.artifact_root.as_deref(), Some("/tmp/artifacts"));
    }
}
