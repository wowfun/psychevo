use std::fs;
use std::path::Path;
use std::process::Command as ProcessCommand;
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result};

use super::process::{
    ProcessOutcome, command_exists, run_logged_process, write_log_line, write_mirrored_line,
};

const PLAYWRIGHT_INSTALL_HINT: &str = "cargo xtask doctor deps install --only playwright";
const WORKBENCH_VISUAL_SPECS: &[&str] = &[
    "apps/workbench/e2e/composer-visual.spec.ts",
    "apps/workbench/e2e/acp-peer-visual.spec.ts",
    "apps/workbench/e2e/new-create-visual.spec.ts",
    "apps/workbench/e2e/runtime-profile-visual.spec.ts",
];
const REQUIRED_RUNTIME_PROFILE_PROOFS: &[&str] = &[
    "runtime-profile-provenance-chromium-desktop.png",
    "runtime-profile-provenance-chromium-mobile.png",
    "runtime-child-partial-history-chromium-desktop.png",
    "runtime-child-partial-history-chromium-mobile.png",
    "runtime-profile-detail-chromium-desktop.png",
    "runtime-profile-detail-chromium-mobile.png",
    "runtime-profile-editor-chromium-desktop.png",
    "runtime-profile-editor-chromium-mobile.png",
    "runtime-native-sessions-chromium-desktop.png",
    "runtime-native-sessions-chromium-mobile.png",
    "runtime-shared-attention-chromium-desktop.png",
    "runtime-shared-attention-chromium-mobile.png",
    "runtime-opencode-timeline-chromium-desktop.png",
    "runtime-opencode-timeline-chromium-mobile.png",
    "runtime-opencode-revert-chromium-desktop.png",
    "runtime-opencode-revert-chromium-mobile.png",
];

pub(crate) fn run_workbench_visual(
    root: &Path,
    artifact_root: &Path,
    log: Arc<Mutex<fs::File>>,
) -> Result<ProcessOutcome> {
    if !command_exists("pnpm") {
        return failed_workbench_visual(
            log,
            &[
                "missing pnpm for Workbench Playwright visual validation".to_string(),
                format!("run: {PLAYWRIGHT_INSTALL_HINT}"),
            ],
        );
    }
    for spec in WORKBENCH_VISUAL_SPECS {
        let path = root.join(spec);
        if !path.is_file() {
            return failed_workbench_visual(
                log,
                &[format!(
                    "Workbench visual spec is missing: {}",
                    path.display()
                )],
            );
        }
    }

    let workbench_dir = artifact_root.join("visual").join("workbench");
    if workbench_dir.exists() {
        fs::remove_dir_all(&workbench_dir).with_context(|| {
            format!(
                "remove stale Workbench visual dir {}",
                workbench_dir.display()
            )
        })?;
    }
    let screenshot_root = workbench_dir.join("screenshots");
    fs::create_dir_all(&screenshot_root)
        .with_context(|| format!("create {}", screenshot_root.display()))?;
    write_log_line(
        &log,
        &format!(
            "Workbench visual screenshots: {}",
            screenshot_root.display()
        ),
    )?;

    let mut mirrored_diagnostics = 0;
    let mut version = ProcessCommand::new("pnpm");
    version.args(["exec", "playwright", "--version"]);
    apply_workbench_visual_env(&mut version, root, artifact_root, &screenshot_root);
    let outcome = run_logged_process("playwright version", &mut version, Arc::clone(&log))?;
    mirrored_diagnostics += outcome.mirrored_diagnostics;
    if !outcome.passed {
        write_mirrored_line(&log, &format!("run: {PLAYWRIGHT_INSTALL_HINT}"))?;
        return Ok(ProcessOutcome {
            passed: false,
            exit_code: outcome.exit_code,
            mirrored_diagnostics: mirrored_diagnostics + 1,
        });
    }

    let mut build = ProcessCommand::new("pnpm");
    build.args(["--filter", "@psychevo/workbench", "build"]);
    apply_workbench_visual_env(&mut build, root, artifact_root, &screenshot_root);
    let outcome = run_logged_process("workbench visual build", &mut build, Arc::clone(&log))?;
    mirrored_diagnostics += outcome.mirrored_diagnostics;
    if !outcome.passed {
        return Ok(ProcessOutcome {
            passed: false,
            exit_code: outcome.exit_code,
            mirrored_diagnostics,
        });
    }

    let mut test = ProcessCommand::new("pnpm");
    test.arg("exec")
        .arg("playwright")
        .arg("test")
        .args(WORKBENCH_VISUAL_SPECS)
        .args([
            "--project",
            "chromium-desktop",
            "--project",
            "chromium-mobile",
        ]);
    apply_workbench_visual_env(&mut test, root, artifact_root, &screenshot_root);
    test.env_remove("NO_COLOR");
    let outcome = run_logged_process("workbench visual playwright", &mut test, Arc::clone(&log))?;
    mirrored_diagnostics += outcome.mirrored_diagnostics;
    if outcome.passed {
        let missing = REQUIRED_RUNTIME_PROFILE_PROOFS
            .iter()
            .filter(|name| !screenshot_root.join(name).is_file())
            .copied()
            .collect::<Vec<_>>();
        if !missing.is_empty() {
            for name in &missing {
                write_mirrored_line(
                    &log,
                    &format!(
                        "missing targeted Runtime Profile visual proof: {}",
                        screenshot_root.join(name).display()
                    ),
                )?;
            }
            return Ok(ProcessOutcome {
                passed: false,
                exit_code: outcome.exit_code,
                mirrored_diagnostics: mirrored_diagnostics + missing.len(),
            });
        }
    }
    Ok(ProcessOutcome {
        passed: outcome.passed,
        exit_code: outcome.exit_code,
        mirrored_diagnostics,
    })
}

fn apply_workbench_visual_env(
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

fn failed_workbench_visual(log: Arc<Mutex<fs::File>>, lines: &[String]) -> Result<ProcessOutcome> {
    for line in lines {
        write_mirrored_line(&log, line)?;
    }
    Ok(ProcessOutcome {
        passed: false,
        exit_code: None,
        mirrored_diagnostics: lines.len(),
    })
}
