use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command as ProcessCommand;

use anyhow::{Context, Result, anyhow, bail};

use super::artifacts::{default_artifact_root, display_path};
use super::model::{
    CiEnvironmentOutput, RunOutput, StepRunOutput, StepStatus, WorkflowProfile, WorkflowStep,
    WorkflowStepAction,
};
use super::process::{create_step_log, run_logged_process};
use super::profiles::{find_profile, plan_for_profile_with_env};
use super::retention::warn_if_ci_retention_cleanup_fails;
use super::tui_capture::run_tui_vhs_demo;
use super::workbench_visual::run_workbench_visual;
use crate::live::{LiveEnvMode, run_ci_single_provider_live};

const FAILURE_TAIL_LINES: usize = 80;

pub(crate) fn execute_profile(
    root: &Path,
    id: &str,
    allow_live: bool,
    live_env: Option<LiveEnvMode>,
    artifact_root: Option<PathBuf>,
) -> Result<RunOutput> {
    let profile = find_profile(id)?;
    if live_env.is_some() && !profile.live {
        bail!("--live-env is only valid for live CI/CD profiles");
    }
    if profile.live && !allow_live {
        bail!("profile '{id}' requires explicit --live opt-in");
    }
    let live_env = live_env.unwrap_or_default();

    let use_default_artifact_root = artifact_root.is_none();
    let artifact_root = artifact_root.unwrap_or_else(|| default_artifact_root(root));
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

    let mut steps = Vec::new();
    for (index, step) in profile.steps.iter().enumerate() {
        if step.live && !allow_live {
            bail!("step '{}' requires explicit --live opt-in", step.id);
        }
        println!("ci {}: {} ...", profile.id, step.id);
        let log_path = artifact_root
            .join("logs")
            .join(format!("{:02}-{}.log", index + 1, step.id));
        let execution = match run_step(root, &artifact_root, profile, step, live_env, &log_path) {
            Ok(execution) => execution,
            Err(error) => {
                if use_default_artifact_root {
                    warn_if_ci_retention_cleanup_fails(root, &artifact_root);
                }
                return Err(error);
            }
        };
        let failed = matches!(execution.output.status, StepStatus::Failed);
        if failed {
            let summary = failure_summary(profile.id, &execution.output);
            if execution.mirrored_diagnostics == 0
                && let Ok(tail) = read_log_tail(&log_path, FAILURE_TAIL_LINES)
                && !tail.trim().is_empty()
            {
                eprintln!("last log output from {}:\n{}", log_path.display(), tail);
            }
            steps.push(execution.output);
            if use_default_artifact_root {
                warn_if_ci_retention_cleanup_fails(root, &artifact_root);
            }
            bail!("{summary}");
        }
        steps.push(execution.output);
    }

    let run = RunOutput {
        profile: super::model::profile_summary(profile),
        environment: profile
            .live
            .then_some(CiEnvironmentOutput { mode: live_env }),
        artifact_root: display_path(&artifact_root),
        steps,
    };
    fs::write(
        artifact_root.join("results.json"),
        serde_json::to_vec_pretty(&run)?,
    )
    .with_context(|| format!("write {}", artifact_root.join("results.json").display()))?;
    if use_default_artifact_root {
        warn_if_ci_retention_cleanup_fails(root, &artifact_root);
    }
    Ok(run)
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
        WorkflowStepAction::SingleProviderLive => {
            run_single_provider_live_step(root, artifact_root, profile, step, live_env, log_path)
        }
        WorkflowStepAction::TuiVhsDemo => {
            run_tui_vhs_demo_step(root, artifact_root, step, log_path)
        }
        WorkflowStepAction::WorkbenchVisual => {
            run_workbench_visual_step(root, artifact_root, step, log_path)
        }
    }
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
    let mut process = ProcessCommand::new(program);
    process
        .args(args)
        .current_dir(root)
        .env("PSYCHEVO_CI_ARTIFACT_ROOT", artifact_root);
    let outcome = run_logged_process(step.id, &mut process, log)?;

    println!(
        "ci step {}: {}",
        step.id,
        if outcome.passed { "ok" } else { "failed" }
    );
    Ok(step_execution(
        step,
        log_path,
        command,
        outcome.passed,
        outcome.exit_code,
        outcome.mirrored_diagnostics,
    ))
}

fn run_tui_vhs_demo_step(
    root: &Path,
    artifact_root: &Path,
    step: &WorkflowStep,
    log_path: &Path,
) -> Result<StepExecution> {
    let log = create_step_log(log_path)?;
    let outcome = run_tui_vhs_demo(root, artifact_root, log)?;
    println!(
        "ci step {}: {}",
        step.id,
        if outcome.passed { "ok" } else { "failed" }
    );
    Ok(step_execution(
        step,
        log_path,
        step.action.command_for_plan(),
        outcome.passed,
        outcome.exit_code,
        outcome.mirrored_diagnostics,
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
    println!(
        "ci step {}: {}",
        step.id,
        if outcome.passed { "ok" } else { "failed" }
    );
    Ok(step_execution(
        step,
        log_path,
        step.action.command_for_plan(),
        outcome.passed,
        outcome.exit_code,
        outcome.mirrored_diagnostics,
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
    println!(
        "ci step {}: {}",
        step.id,
        if outcome.passed { "ok" } else { "failed" }
    );
    Ok(step_execution(
        step,
        log_path,
        step.action.command_for_plan(),
        outcome.passed,
        outcome.exit_code,
        outcome.mirrored_diagnostics,
    ))
}

fn step_execution(
    step: &WorkflowStep,
    log_path: &Path,
    command: &'static [&'static str],
    passed: bool,
    exit_code: Option<i32>,
    mirrored_diagnostics: usize,
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
        },
        mirrored_diagnostics,
    }
}

#[derive(Debug)]
struct StepExecution {
    output: StepRunOutput,
    mirrored_diagnostics: usize,
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
    use super::*;

    #[test]
    fn live_profile_requires_explicit_opt_in() {
        let temp = std::env::temp_dir().join("psychevo-xtask-live-opt-in-test");
        let err = execute_profile(Path::new("."), "live", false, None, Some(temp))
            .expect_err("live profile should be rejected before execution");
        assert!(err.to_string().contains("requires explicit --live opt-in"));
    }

    #[test]
    fn non_live_profile_rejects_live_env_mode() {
        let temp = std::env::temp_dir().join("psychevo-xtask-live-env-non-live-test");
        let err = execute_profile(
            Path::new("."),
            "changed",
            false,
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
        };
        assert_eq!(
            failure_summary("changed", &output),
            "CI/CD profile 'changed' failed at step 'demo'; log: /tmp/demo.log"
        );
        assert_eq!(tail_lines("one\ntwo\nthree\n", 2), "two\nthree\n");
    }
}
