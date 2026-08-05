use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command as ProcessCommand;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow, bail};

use super::artifacts::{default_artifact_root, display_path};
use super::desktop_manifest_parity::check_desktop_manifest_parity;
use super::desktop_visual::run_desktop_visual;
use super::model::{
    CiEnvironmentOutput, RunOutput, StepRunOutput, StepStatus, WorkflowProfile, WorkflowStep,
    WorkflowStepAction,
};
use super::process::{create_step_log, run_logged_process};
use super::profiles::{find_profile, plan_for_profile_with_env};
use super::retention::warn_if_ci_retention_cleanup_fails;
use super::sdk_architecture::check_sdk_architecture;
use super::surface_profile::run_surface_profile;
use super::tui_capture::run_tui_vhs_demo;
use super::workbench_visual::{run_workbench_critical_journey, run_workbench_visual};
use crate::host_command;
use crate::live::{LiveEnvMode, run_ci_single_provider_live};

const FAILURE_TAIL_LINES: usize = 80;

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct RunOptIns {
    pub(crate) live: bool,
    pub(crate) package: bool,
}

pub(crate) fn execute_profile(
    root: &Path,
    id: &str,
    opt_ins: RunOptIns,
    live_env: Option<LiveEnvMode>,
    artifact_root: Option<PathBuf>,
) -> Result<RunOutput> {
    let profile = find_profile(id)?;
    if live_env.is_some() && !profile.live {
        bail!("--live-env is only valid for live CI/CD profiles");
    }
    if profile.live && !opt_ins.live {
        bail!("profile '{id}' requires explicit --live opt-in");
    }
    if profile.artifact_only && !opt_ins.package {
        bail!("profile '{id}' requires explicit --package opt-in");
    }
    let live_env = live_env.unwrap_or_default();

    let use_default_artifact_root = artifact_root.is_none();
    let invocation_cwd = std::env::current_dir().context("read xtask invocation directory")?;
    let artifact_root = resolve_ci_artifact_root(root, &invocation_cwd, artifact_root);
    prepare_profile_artifacts(profile, &artifact_root)?;
    fs::create_dir_all(artifact_root.join("logs"))
        .with_context(|| format!("create artifact root {}", artifact_root.display()))?;

    let plan = plan_for_profile_with_env(
        profile,
        Some(&artifact_root),
        profile.live.then_some(live_env),
    )?;
    fs::write(
        artifact_root.join("plan.json"),
        serde_json::to_vec_pretty(&plan)?,
    )
    .with_context(|| format!("write {}", artifact_root.join("plan.json").display()))?;

    let run_started = Instant::now();
    let mut steps = Vec::new();
    for (index, step) in profile.steps.iter().enumerate() {
        if step.live && !opt_ins.live {
            bail!("step '{}' requires explicit --live opt-in", step.id);
        }
        println!("ci {}: {} ...", profile.id, step.id);
        let log_path = artifact_root
            .join("logs")
            .join(format!("{:02}-{}.log", index + 1, step.id));
        let step_started = Instant::now();
        let mut execution = match run_step(root, &artifact_root, profile, step, live_env, &log_path)
        {
            Ok(execution) => execution,
            Err(error) => {
                let mut output = step_execution(
                    step,
                    &log_path,
                    step.action.command_for_plan(),
                    false,
                    None,
                    false,
                )
                .output;
                output.duration_ms = elapsed_ms(step_started.elapsed());
                print_step_completion(profile.id, &output);
                steps.push(output);
                write_run_output(
                    profile,
                    live_env,
                    &artifact_root,
                    elapsed_ms(run_started.elapsed()),
                    steps,
                )?;
                if use_default_artifact_root {
                    warn_if_ci_retention_cleanup_fails(root, &artifact_root);
                }
                return Err(error);
            }
        };
        execution.output.duration_ms = elapsed_ms(step_started.elapsed());
        print_step_completion(profile.id, &execution.output);
        let failed = matches!(execution.output.status, StepStatus::Failed);
        if failed {
            let summary = failure_summary(profile.id, &execution.output);
            if let Some(tail) = failure_log_tail(&log_path, execution.had_suppressed_output) {
                eprintln!("last log output from {}:\n{}", log_path.display(), tail);
            }
            steps.push(execution.output);
            write_run_output(
                profile,
                live_env,
                &artifact_root,
                elapsed_ms(run_started.elapsed()),
                steps,
            )?;
            if use_default_artifact_root {
                warn_if_ci_retention_cleanup_fails(root, &artifact_root);
            }
            bail!("{summary}");
        }
        steps.push(execution.output);
    }

    let run = write_run_output(
        profile,
        live_env,
        &artifact_root,
        elapsed_ms(run_started.elapsed()),
        steps,
    )?;
    if use_default_artifact_root {
        warn_if_ci_retention_cleanup_fails(root, &artifact_root);
    }
    Ok(run)
}

fn prepare_profile_artifacts(profile: &WorkflowProfile, artifact_root: &Path) -> Result<()> {
    if profile.id != "visual" {
        return Ok(());
    }
    let visual_root = artifact_root.join("visual");
    if visual_root.exists() {
        fs::remove_dir_all(&visual_root)
            .with_context(|| format!("remove stale visual root {}", visual_root.display()))?;
    }
    Ok(())
}

fn write_run_output(
    profile: &WorkflowProfile,
    live_env: LiveEnvMode,
    artifact_root: &Path,
    duration_ms: u64,
    steps: Vec<StepRunOutput>,
) -> Result<RunOutput> {
    let run = RunOutput {
        profile: super::model::profile_summary(profile),
        environment: profile
            .live
            .then_some(CiEnvironmentOutput { mode: live_env }),
        artifact_root: display_path(artifact_root),
        duration_ms,
        steps,
    };
    fs::write(
        artifact_root.join("results.json"),
        serde_json::to_vec_pretty(&run)?,
    )
    .with_context(|| format!("write {}", artifact_root.join("results.json").display()))?;
    Ok(run)
}

fn elapsed_ms(duration: Duration) -> u64 {
    duration.as_millis().try_into().unwrap_or(u64::MAX)
}

fn print_step_completion(profile_id: &str, output: &StepRunOutput) {
    let status = match output.status {
        StepStatus::Passed => "passed",
        StepStatus::Failed => "failed",
    };
    println!(
        "ci {}: {} {} ({} ms)",
        profile_id, output.id, status, output.duration_ms
    );
}

fn resolve_ci_artifact_root(
    root: &Path,
    invocation_cwd: &Path,
    artifact_root: Option<PathBuf>,
) -> PathBuf {
    let path = artifact_root.unwrap_or_else(|| default_artifact_root(root));
    if path.is_absolute() {
        path
    } else {
        invocation_cwd.join(path)
    }
}

fn run_step(
    root: &Path,
    artifact_root: &Path,
    profile: &WorkflowProfile,
    step: &WorkflowStep,
    live_env: LiveEnvMode,
    log_path: &Path,
) -> Result<StepExecution> {
    match step.action {
        WorkflowStepAction::Command(command) => {
            run_command_step(root, artifact_root, step, command, log_path)
        }
        WorkflowStepAction::DesktopManifestParity => {
            create_step_log(log_path)?;
            check_desktop_manifest_parity(root)?;
            Ok(step_execution(
                step,
                log_path,
                step.action.command_for_plan(),
                true,
                Some(0),
                false,
            ))
        }
        WorkflowStepAction::ArtifactCommand {
            command,
            target_dir,
        } => run_artifact_command_step(root, artifact_root, step, command, target_dir, log_path),
        WorkflowStepAction::SdkArchitecture => {
            create_step_log(log_path)?;
            check_sdk_architecture(root)?;
            Ok(step_execution(
                step,
                log_path,
                step.action.command_for_plan(),
                true,
                Some(0),
                false,
            ))
        }
        WorkflowStepAction::SingleProviderLive => {
            run_single_provider_live_step(root, artifact_root, profile, step, live_env, log_path)
        }
        WorkflowStepAction::WorkbenchCriticalJourney => {
            run_workbench_critical_journey_step(root, artifact_root, step, log_path)
        }
        WorkflowStepAction::DesktopVisual => {
            run_desktop_visual_step(root, artifact_root, step, log_path)
        }
        WorkflowStepAction::SurfaceProfile => {
            run_surface_profile_step(root, artifact_root, step, log_path)
        }
        WorkflowStepAction::TuiVhsDemo => {
            run_tui_vhs_demo_step(root, artifact_root, step, log_path)
        }
        WorkflowStepAction::WorkbenchVisual => {
            run_workbench_visual_step(root, artifact_root, step, log_path)
        }
    }
}

fn run_artifact_command_step(
    root: &Path,
    artifact_root: &Path,
    step: &WorkflowStep,
    command: &'static [&'static str],
    target_dir: &str,
    log_path: &Path,
) -> Result<StepExecution> {
    let (program, args) = command
        .split_first()
        .ok_or_else(|| anyhow!("step '{}' has an empty command", step.id))?;
    let log = create_step_log(log_path)?;
    let target_dir = artifact_root.join("package").join(target_dir);
    fs::create_dir_all(&target_dir)
        .with_context(|| format!("create Desktop target root {}", target_dir.display()))?;
    let mut process = step_process(program, args)?;
    process
        .current_dir(root)
        .env("PSYCHEVO_CI_ARTIFACT_ROOT", artifact_root)
        .env("CARGO_TARGET_DIR", target_dir);
    if cfg!(target_os = "linux") && is_desktop_bundle_command(command) {
        process.env("APPIMAGE_EXTRACT_AND_RUN", "1");
    }
    let outcome = run_logged_process(step.id, &mut process, log)?;
    Ok(step_execution(
        step,
        log_path,
        command,
        outcome.passed,
        outcome.exit_code,
        outcome.had_suppressed_output,
    ))
}

fn is_desktop_bundle_command(command: &[&str]) -> bool {
    command
        .windows(2)
        .any(|arguments| arguments == ["@psychevo/desktop", "tauri:build"])
}

fn run_surface_profile_step(
    root: &Path,
    artifact_root: &Path,
    step: &WorkflowStep,
    log_path: &Path,
) -> Result<StepExecution> {
    let log = create_step_log(log_path)?;
    let outcome = run_surface_profile(root, artifact_root, log)?;
    Ok(step_execution(
        step,
        log_path,
        step.action.command_for_plan(),
        outcome.passed,
        outcome.exit_code,
        outcome.had_suppressed_output,
    ))
}

fn run_command_step(
    root: &Path,
    artifact_root: &Path,
    step: &WorkflowStep,
    command: &'static [&'static str],
    log_path: &Path,
) -> Result<StepExecution> {
    let (program, args) = command
        .split_first()
        .ok_or_else(|| anyhow!("step '{}' has an empty command", step.id))?;
    let log = create_step_log(log_path)?;
    let mut process = step_process(program, args)?;
    process
        .current_dir(root)
        .env("PSYCHEVO_CI_ARTIFACT_ROOT", artifact_root);
    let outcome = run_logged_process(step.id, &mut process, log)?;

    Ok(step_execution(
        step,
        log_path,
        command,
        outcome.passed,
        outcome.exit_code,
        outcome.had_suppressed_output,
    ))
}

fn step_process(program: &str, args: &[&str]) -> Result<ProcessCommand> {
    if program == "pnpm" {
        host_command::pnpm(args)
    } else {
        let mut process = ProcessCommand::new(program);
        process.args(args);
        Ok(process)
    }
}

fn run_tui_vhs_demo_step(
    root: &Path,
    artifact_root: &Path,
    step: &WorkflowStep,
    log_path: &Path,
) -> Result<StepExecution> {
    let log = create_step_log(log_path)?;
    let outcome = run_tui_vhs_demo(root, artifact_root, log)?;
    Ok(step_execution(
        step,
        log_path,
        step.action.command_for_plan(),
        outcome.passed,
        outcome.exit_code,
        outcome.had_suppressed_output,
    ))
}

fn run_desktop_visual_step(
    root: &Path,
    artifact_root: &Path,
    step: &WorkflowStep,
    log_path: &Path,
) -> Result<StepExecution> {
    let log = create_step_log(log_path)?;
    let outcome = run_desktop_visual(root, artifact_root, log)?;
    Ok(step_execution(
        step,
        log_path,
        step.action.command_for_plan(),
        outcome.passed,
        outcome.exit_code,
        outcome.had_suppressed_output,
    ))
}

fn run_workbench_visual_step(
    root: &Path,
    artifact_root: &Path,
    step: &WorkflowStep,
    log_path: &Path,
) -> Result<StepExecution> {
    let log = create_step_log(log_path)?;
    let outcome = run_workbench_visual(root, artifact_root, log)?;
    Ok(step_execution(
        step,
        log_path,
        step.action.command_for_plan(),
        outcome.passed,
        outcome.exit_code,
        outcome.had_suppressed_output,
    ))
}

fn run_workbench_critical_journey_step(
    root: &Path,
    artifact_root: &Path,
    step: &WorkflowStep,
    log_path: &Path,
) -> Result<StepExecution> {
    let log = create_step_log(log_path)?;
    let outcome = run_workbench_critical_journey(root, artifact_root, log)?;
    Ok(step_execution(
        step,
        log_path,
        step.action.command_for_plan(),
        outcome.passed,
        outcome.exit_code,
        outcome.had_suppressed_output,
    ))
}

fn run_single_provider_live_step(
    root: &Path,
    artifact_root: &Path,
    _profile: &WorkflowProfile,
    step: &WorkflowStep,
    live_env: LiveEnvMode,
    log_path: &Path,
) -> Result<StepExecution> {
    let log = create_step_log(log_path)?;
    let outcome = run_ci_single_provider_live(root, artifact_root, live_env, log)?;
    Ok(step_execution(
        step,
        log_path,
        step.action.command_for_plan(),
        outcome.passed,
        outcome.exit_code,
        outcome.had_suppressed_output,
    ))
}

fn step_execution(
    step: &WorkflowStep,
    log_path: &Path,
    command: &'static [&'static str],
    passed: bool,
    exit_code: Option<i32>,
    had_suppressed_output: bool,
) -> StepExecution {
    StepExecution {
        output: StepRunOutput {
            id: step.id,
            description: step.description,
            command: command.iter().map(|part| (*part).to_string()).collect(),
            live: step.live,
            status: if passed {
                StepStatus::Passed
            } else {
                StepStatus::Failed
            },
            exit_code,
            log_path: display_path(log_path),
            duration_ms: 0,
        },
        had_suppressed_output,
    }
}

#[derive(Debug)]
struct StepExecution {
    output: StepRunOutput,
    had_suppressed_output: bool,
}

fn failure_summary(profile_id: &str, output: &StepRunOutput) -> String {
    format!(
        "CI/CD profile '{}' failed at step '{}'; log: {}",
        profile_id, output.id, output.log_path
    )
}

fn read_log_tail(path: &Path, max_lines: usize) -> Result<String> {
    let bytes = fs::read(path).with_context(|| format!("read {}", path.display()))?;
    Ok(tail_lines(&String::from_utf8_lossy(&bytes), max_lines))
}

fn failure_log_tail(path: &Path, had_suppressed_output: bool) -> Option<String> {
    if !had_suppressed_output {
        return None;
    }
    read_log_tail(path, FAILURE_TAIL_LINES)
        .ok()
        .filter(|tail| !tail.trim().is_empty())
}

fn tail_lines(contents: &str, max_lines: usize) -> String {
    let lines: Vec<_> = contents.lines().collect();
    let start = lines.len().saturating_sub(max_lines);
    let mut output = lines[start..].join("\n");
    if !output.is_empty() && contents.ends_with('\n') {
        output.push('\n');
    }
    output
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    static NEXT_TEST_LOG: AtomicUsize = AtomicUsize::new(0);

    fn test_log_path(label: &str) -> PathBuf {
        let id = NEXT_TEST_LOG.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "psychevo-xtask-runner-{label}-{}-{id}.log",
            std::process::id()
        ))
    }

    #[test]
    fn live_profile_requires_explicit_opt_in() {
        let temp = std::env::temp_dir().join("psychevo-xtask-live-opt-in-test");
        let err = execute_profile(
            Path::new("."),
            "live",
            RunOptIns::default(),
            None,
            Some(temp),
        )
        .expect_err("live profile should be rejected before execution");
        assert!(err.to_string().contains("requires explicit --live opt-in"));
    }

    #[test]
    fn appimage_extraction_follows_the_desktop_bundle_command_not_a_step_id() {
        assert!(is_desktop_bundle_command(&[
            "pnpm",
            "--filter",
            "@psychevo/desktop",
            "tauri:build",
        ]));
        assert!(!is_desktop_bundle_command(&[
            "cargo",
            "build",
            "--release",
            "--bin",
            "pevo",
        ]));
    }

    #[test]
    fn package_profile_requires_explicit_opt_in_before_creating_artifacts() {
        let temp = test_log_path("package-opt-in").with_extension("");
        assert!(!temp.exists());
        let err = execute_profile(
            Path::new("."),
            "package",
            RunOptIns::default(),
            None,
            Some(temp.clone()),
        )
        .expect_err("package profile should be rejected before execution");
        assert!(
            err.to_string()
                .contains("requires explicit --package opt-in")
        );
        assert!(!temp.exists());
    }

    #[test]
    fn visual_profile_cleans_its_complete_subtree_before_the_first_step() {
        let artifact_root = test_log_path("visual-clean-root").with_extension("");
        let stale = artifact_root.join("visual/desktop-native/stale.png");
        let retained = artifact_root.join("logs/previous.log");
        fs::create_dir_all(stale.parent().expect("stale parent")).expect("visual root");
        fs::create_dir_all(retained.parent().expect("log parent")).expect("log root");
        fs::write(&stale, b"stale").expect("stale visual evidence");
        fs::write(&retained, b"log").expect("retained log");

        prepare_profile_artifacts(
            find_profile("visual").expect("visual profile"),
            &artifact_root,
        )
        .expect("clean visual artifacts");

        assert!(!artifact_root.join("visual").exists());
        assert!(retained.is_file());
        fs::remove_dir_all(artifact_root).expect("cleanup");
    }

    #[test]
    fn non_live_profile_rejects_live_env_mode() {
        let temp = std::env::temp_dir().join("psychevo-xtask-live-env-non-live-test");
        let err = execute_profile(
            Path::new("."),
            "changed",
            RunOptIns::default(),
            Some(LiveEnvMode::Isolated),
            Some(temp),
        )
        .expect_err("non-live profile should reject --live-env");
        assert!(err.to_string().contains("--live-env"));
    }

    #[test]
    fn failure_summary_includes_log_path_and_tail_is_bounded() {
        let output = StepRunOutput {
            id: "demo",
            description: "Demo",
            command: vec!["false".to_string()],
            live: false,
            status: StepStatus::Failed,
            exit_code: Some(1),
            log_path: "/tmp/demo.log".to_string(),
            duration_ms: 0,
        };
        assert_eq!(
            failure_summary("changed", &output),
            "CI/CD profile 'changed' failed at step 'demo'; log: /tmp/demo.log"
        );
        assert_eq!(tail_lines("one\ntwo\nthree\n", 2), "two\nthree\n");
    }

    #[test]
    fn successful_and_failed_results_include_durations() {
        let profile = find_profile("changed").expect("profile");

        for (label, status) in [
            ("passed", StepStatus::Passed),
            ("failed", StepStatus::Failed),
        ] {
            let artifact_root = test_log_path(label).with_extension("");
            fs::create_dir_all(&artifact_root).expect("artifact root");
            let output = StepRunOutput {
                id: "demo",
                description: "Demo",
                command: vec![label.to_string()],
                live: false,
                status,
                exit_code: Some(if label == "passed" { 0 } else { 1 }),
                log_path: "/tmp/demo.log".to_string(),
                duration_ms: 0,
            };

            write_run_output(
                profile,
                LiveEnvMode::Isolated,
                &artifact_root,
                0,
                vec![output],
            )
            .expect("write result");
            let result: serde_json::Value = serde_json::from_slice(
                &fs::read(artifact_root.join("results.json")).expect("read result"),
            )
            .expect("decode result");
            assert_eq!(result["steps"][0]["status"], label);
            assert!(result["duration_ms"].is_u64());
            assert!(result["steps"][0]["duration_ms"].is_u64());
            fs::remove_dir_all(artifact_root).expect("remove artifact root");
        }
    }

    #[test]
    fn internal_step_errors_are_preserved_with_durations() {
        let artifact_root = test_log_path("internal-error-result").with_extension("");
        let missing_root = artifact_root.with_extension("missing-root");
        assert!(!missing_root.exists());

        execute_profile(
            &missing_root,
            "changed",
            RunOptIns::default(),
            None,
            Some(artifact_root.clone()),
        )
        .expect_err("missing execution root should fail the step internally");

        let result: serde_json::Value = serde_json::from_slice(
            &fs::read(artifact_root.join("results.json")).expect("read result"),
        )
        .expect("decode result");
        assert_eq!(result["steps"][0]["status"], "failed");
        assert!(result["duration_ms"].is_u64());
        assert!(result["steps"][0]["duration_ms"].is_u64());
        fs::remove_dir_all(artifact_root).expect("remove artifact root");
    }

    #[test]
    fn suppressed_stdout_triggers_tail_even_with_mirrored_stderr() {
        let path = test_log_path("mixed-output");
        fs::write(
            &path,
            "assertion failed: left == right\nerror: test failed, to rerun pass `-p demo`\n",
        )
        .expect("write mixed output log");

        assert_eq!(
            failure_log_tail(&path, true).as_deref(),
            Some("assertion failed: left == right\nerror: test failed, to rerun pass `-p demo`\n")
        );
        fs::remove_file(path).expect("remove mixed output log");
    }

    #[test]
    fn fully_mirrored_failure_does_not_repeat_log_tail() {
        let path = test_log_path("stderr-only");
        fs::write(&path, "error: command failed\n").expect("write stderr-only log");

        assert_eq!(failure_log_tail(&path, false), None);
        fs::remove_file(path).expect("remove stderr-only log");
    }

    #[test]
    fn failure_tail_is_empty_for_empty_or_unreadable_logs() {
        let empty_path = test_log_path("empty");
        fs::write(&empty_path, "").expect("write empty log");
        assert_eq!(failure_log_tail(&empty_path, true), None);
        fs::remove_file(empty_path).expect("remove empty log");

        let missing_path = test_log_path("missing");
        assert_eq!(failure_log_tail(&missing_path, true), None);
    }

    #[test]
    fn failure_tail_keeps_only_last_eighty_lines_without_adding_a_newline() {
        let path = test_log_path("bounded");
        let contents = (1..=81)
            .map(|line| format!("line {line}"))
            .collect::<Vec<_>>()
            .join("\n");
        fs::write(&path, contents).expect("write bounded log");

        let tail = failure_log_tail(&path, true).expect("suppressed log tail");
        assert!(!tail.contains("line 1\n"));
        assert!(tail.starts_with("line 2\n"));
        assert!(tail.ends_with("line 81"));
        assert_eq!(tail.lines().count(), FAILURE_TAIL_LINES);
        fs::remove_file(path).expect("remove bounded log");
    }

    #[test]
    fn relative_artifact_root_is_resolved_before_steps_change_directory() {
        let invocation_cwd = Path::new("/tmp/psychevo-caller");
        assert_eq!(
            resolve_ci_artifact_root(
                Path::new("/tmp/psychevo-repo"),
                invocation_cwd,
                Some(PathBuf::from("artifacts/visual")),
            ),
            invocation_cwd.join("artifacts/visual")
        );
        assert_eq!(
            resolve_ci_artifact_root(
                Path::new("/tmp/psychevo-repo"),
                invocation_cwd,
                Some(PathBuf::from("/tmp/absolute-artifacts")),
            ),
            PathBuf::from("/tmp/absolute-artifacts")
        );
    }
}
