use std::fs;
use std::path::Path;
use std::process::Command as ProcessCommand;
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result};

use super::process::{
    ProcessOutcome, command_exists, run_logged_process, write_log_line, write_mirrored_line,
};

const PLAYWRIGHT_INSTALL_HINT: &str = "cargo xtask doctor deps install --only playwright";
const DESKTOP_VISUAL_SPEC: &str = "apps/workbench/e2e/desktop-visual.spec.ts";

pub(crate) fn run_desktop_visual(
    root: &Path,
    artifact_root: &Path,
    log: Arc<Mutex<fs::File>>,
) -> Result<ProcessOutcome> {
    if !command_exists("pnpm") {
        return failed_desktop_visual(
            log,
            &[
                "missing pnpm for Desktop visual validation".to_string(),
                format!("run: {PLAYWRIGHT_INSTALL_HINT}"),
            ],
        );
    }
    if !root.join(DESKTOP_VISUAL_SPEC).is_file() {
        return failed_desktop_visual(
            log,
            &[format!(
                "Desktop visual spec is missing: {}",
                root.join(DESKTOP_VISUAL_SPEC).display()
            )],
        );
    }

    let desktop_dir = artifact_root.join("visual").join("desktop");
    if desktop_dir.exists() {
        fs::remove_dir_all(&desktop_dir).with_context(|| {
            format!("remove stale Desktop visual dir {}", desktop_dir.display())
        })?;
    }
    let screenshot_root = desktop_dir.join("screenshots");
    fs::create_dir_all(&screenshot_root)
        .with_context(|| format!("create {}", screenshot_root.display()))?;
    write_log_line(
        &log,
        &format!("Desktop visual screenshots: {}", screenshot_root.display()),
    )?;

    let mut mirrored_diagnostics = 0;
    let mut had_suppressed_output = false;
    let mut version = ProcessCommand::new("pnpm");
    version.args(["exec", "playwright", "--version"]);
    apply_desktop_visual_env(&mut version, root, artifact_root, &screenshot_root);
    let outcome = run_logged_process("playwright version", &mut version, Arc::clone(&log))?;
    mirrored_diagnostics += outcome.mirrored_diagnostics;
    had_suppressed_output |= outcome.had_suppressed_output;
    if !outcome.passed {
        write_mirrored_line(&log, &format!("run: {PLAYWRIGHT_INSTALL_HINT}"))?;
        return Ok(ProcessOutcome {
            passed: false,
            exit_code: outcome.exit_code,
            mirrored_diagnostics: mirrored_diagnostics + 1,
            had_suppressed_output,
        });
    }

    let mut build = ProcessCommand::new("pnpm");
    build.args(["--filter", "@psychevo/desktop", "build"]);
    apply_desktop_visual_env(&mut build, root, artifact_root, &screenshot_root);
    let outcome = run_logged_process("desktop visual build", &mut build, Arc::clone(&log))?;
    mirrored_diagnostics += outcome.mirrored_diagnostics;
    had_suppressed_output |= outcome.had_suppressed_output;
    if !outcome.passed {
        return Ok(ProcessOutcome {
            passed: false,
            exit_code: outcome.exit_code,
            mirrored_diagnostics,
            had_suppressed_output,
        });
    }

    let mut test = ProcessCommand::new("pnpm");
    test.args([
        "exec",
        "playwright",
        "test",
        DESKTOP_VISUAL_SPEC,
        "--project",
        "chromium-desktop",
    ]);
    apply_desktop_visual_env(&mut test, root, artifact_root, &screenshot_root);
    test.env_remove("NO_COLOR");
    let outcome = run_logged_process("desktop visual playwright", &mut test, log)?;
    mirrored_diagnostics += outcome.mirrored_diagnostics;
    had_suppressed_output |= outcome.had_suppressed_output;
    Ok(ProcessOutcome {
        passed: outcome.passed,
        exit_code: outcome.exit_code,
        mirrored_diagnostics,
        had_suppressed_output,
    })
}

fn apply_desktop_visual_env(
    command: &mut ProcessCommand,
    root: &Path,
    artifact_root: &Path,
    screenshot_root: &Path,
) {
    command
        .current_dir(root)
        .env("PSYCHEVO_CI_ARTIFACT_ROOT", artifact_root)
        .env("PSYCHEVO_PLAYWRIGHT_SCREENSHOT_ROOT", screenshot_root);
}

fn failed_desktop_visual(log: Arc<Mutex<fs::File>>, lines: &[String]) -> Result<ProcessOutcome> {
    for line in lines {
        write_mirrored_line(&log, line)?;
    }
    Ok(ProcessOutcome {
        passed: false,
        exit_code: None,
        mirrored_diagnostics: lines.len(),
        had_suppressed_output: false,
    })
}
