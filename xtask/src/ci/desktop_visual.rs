use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command as ProcessCommand;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{Context, Result};

use crate::desktop_wdio::{DesktopWdioOptions, DesktopWdioTimeouts, run_desktop_wdio};

use super::process::{
    ProcessOutcome, command_exists, run_logged_process_with_timeout, write_log_line,
    write_mirrored_line,
};

const PEVO_BUILD_TIMEOUT: Duration = Duration::from_secs(20 * 60);
const WDIO_BUILD_TIMEOUT: Duration = Duration::from_secs(45 * 60);
const WDIO_SMOKE_TIMEOUT: Duration = Duration::from_secs(10 * 60);
const GATEWAY_CLEANUP_TIMEOUT: Duration = Duration::from_secs(2 * 60);

pub(crate) fn run_desktop_visual(
    root: &Path,
    artifact_root: &Path,
    log: Arc<Mutex<fs::File>>,
) -> Result<ProcessOutcome> {
    let desktop_root = artifact_root.join("visual").join("desktop-native");
    fs::create_dir_all(&desktop_root)
        .with_context(|| format!("create {}", desktop_root.display()))?;

    if !cfg!(target_os = "linux") {
        write_capability_manifest(
            &desktop_root,
            "exempt",
            Some("native visual acceptance is required on Linux only"),
        )?;
        write_log_line(
            &log,
            "native Desktop/Floating visual acceptance is exempt on Windows and macOS",
        )?;
        return Ok(ProcessOutcome {
            passed: true,
            exit_code: Some(0),
            mirrored_diagnostics: 0,
            had_suppressed_output: true,
        });
    }

    let prerequisite_error = if !command_exists("pnpm") {
        Some("missing pnpm for native Desktop/Floating visual validation")
    } else if !command_exists("pkg-config") {
        Some("missing pkg-config for native Tauri Linux validation")
    } else if desktop_session() == "unknown" {
        Some("native Desktop/Floating visual validation requires an X11 or Wayland display")
    } else {
        None
    };
    if let Some(error) = prerequisite_error {
        write_capability_manifest(&desktop_root, "failed", Some(error))?;
        return failed_desktop_visual(log, error);
    }
    write_capability_manifest(&desktop_root, "running", None)?;

    let home = desktop_root.join("home");
    fs::create_dir_all(&home).with_context(|| format!("create {}", home.display()))?;
    let config = home.join("config.toml");
    let db = desktop_root.join("state.db");
    fs::write(&config, "model = \"lmstudio/noop\"\n")
        .with_context(|| format!("write {}", config.display()))?;

    let (pevo_bin, build_outcome) = ensure_pevo_binary(root, artifact_root, Arc::clone(&log))?;
    if !build_outcome.passed {
        write_capability_manifest(&desktop_root, "failed", Some("pevo build failed"))?;
        return Ok(build_outcome);
    }

    let wdio_root = desktop_root.join("wdio");
    let options = DesktopWdioOptions {
        root,
        artifact_root: &wdio_root,
        pevo_bin: &pevo_bin,
        floating_text: "Psychevo deterministic native visual selected text",
        provider_token: None,
        timeouts: DesktopWdioTimeouts {
            build: WDIO_BUILD_TIMEOUT,
            smoke: WDIO_SMOKE_TIMEOUT,
            cleanup: GATEWAY_CLEANUP_TIMEOUT,
        },
    };
    let run = run_desktop_wdio(&options, Arc::clone(&log), |command| {
        command
            .env("PSYCHEVO_HOME", &home)
            .env("PSYCHEVO_CONFIG", &config)
            .env("PSYCHEVO_DB", &db)
            .env("PSYCHEVO_CI_ARTIFACT_ROOT", artifact_root);
    })?;

    let outcome = ProcessOutcome {
        passed: run.outcome.passed,
        exit_code: run.outcome.exit_code,
        mirrored_diagnostics: build_outcome.mirrored_diagnostics + run.outcome.mirrored_diagnostics,
        had_suppressed_output: build_outcome.had_suppressed_output
            || run.outcome.had_suppressed_output,
    };
    if let Some(detail) = run.failure_detail {
        write_mirrored_line(&log, &detail)?;
        write_capability_manifest(&desktop_root, "failed", Some(&detail))?;
        return Ok(ProcessOutcome {
            mirrored_diagnostics: outcome.mirrored_diagnostics + 1,
            ..outcome
        });
    }
    write_capability_manifest(&desktop_root, "passed", None)?;
    write_log_line(
        &log,
        &format!("native Desktop/Floating evidence: {}", wdio_root.display()),
    )?;
    Ok(outcome)
}

fn ensure_pevo_binary(
    root: &Path,
    artifact_root: &Path,
    log: Arc<Mutex<fs::File>>,
) -> Result<(PathBuf, ProcessOutcome)> {
    let configured = env::var_os("PEVO_BIN").map(PathBuf::from);
    let pevo_bin = configured
        .map(|path| {
            if path.is_absolute() {
                path
            } else {
                root.join(path)
            }
        })
        .unwrap_or_else(|| root.join("target").join("debug").join("pevo"));
    if pevo_bin.is_file() {
        return Ok((
            pevo_bin,
            ProcessOutcome {
                passed: true,
                exit_code: Some(0),
                mirrored_diagnostics: 0,
                had_suppressed_output: false,
            },
        ));
    }

    let mut cargo = ProcessCommand::new("cargo");
    cargo
        .args(["build", "-p", "psychevo-cli", "--quiet"])
        .current_dir(root)
        .env("PSYCHEVO_CI_ARTIFACT_ROOT", artifact_root);
    let outcome = run_logged_process_with_timeout(
        "build psychevo-cli for native Desktop visual validation",
        &mut cargo,
        log,
        PEVO_BUILD_TIMEOUT,
    )?;
    if outcome.passed && !pevo_bin.is_file() {
        anyhow::bail!("built pevo binary is missing: {}", pevo_bin.display());
    }
    Ok((pevo_bin, outcome))
}

fn write_capability_manifest(root: &Path, status: &str, reason: Option<&str>) -> Result<()> {
    let value = serde_json::json!({
        "schemaVersion": 1,
        "platform": std::env::consts::OS,
        "session": desktop_session(),
        "status": status,
        "reason": reason,
    });
    fs::write(
        root.join("capabilities.json"),
        serde_json::to_vec_pretty(&value)?,
    )
    .with_context(|| format!("write {}", root.join("capabilities.json").display()))
}

fn desktop_session() -> &'static str {
    if !cfg!(target_os = "linux") {
        return "not-applicable";
    }
    match env::var("XDG_SESSION_TYPE")
        .ok()
        .map(|value| value.to_ascii_lowercase())
        .as_deref()
    {
        Some("wayland") => "wayland",
        Some("x11") => "x11",
        _ if env::var("WAYLAND_DISPLAY").is_ok_and(|value| !value.trim().is_empty()) => "wayland",
        _ if env::var("DISPLAY").is_ok_and(|value| !value.trim().is_empty()) => "x11",
        _ => "unknown",
    }
}

fn failed_desktop_visual(log: Arc<Mutex<fs::File>>, detail: &str) -> Result<ProcessOutcome> {
    write_mirrored_line(&log, detail)?;
    Ok(ProcessOutcome {
        passed: false,
        exit_code: None,
        mirrored_diagnostics: 1,
        had_suppressed_output: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capability_manifest_is_current_and_machine_readable() {
        let root = std::env::temp_dir().join(format!(
            "psychevo-desktop-visual-capability-{}",
            std::process::id()
        ));
        fs::create_dir_all(&root).expect("root");
        write_capability_manifest(&root, "failed", Some("no display")).expect("manifest");
        let value: serde_json::Value = serde_json::from_slice(
            &fs::read(root.join("capabilities.json")).expect("read manifest"),
        )
        .expect("parse manifest");
        assert_eq!(value["schemaVersion"], 1);
        assert_eq!(value["status"], "failed");
        assert_eq!(value["reason"], "no display");
        fs::remove_dir_all(root).expect("cleanup");
    }
}
