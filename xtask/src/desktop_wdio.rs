use std::fs;
use std::path::{Component, Path};
use std::process::Command as ProcessCommand;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{Context, Result, bail};

use crate::ci::process::{ProcessOutcome, run_logged_process_with_timeout, write_mirrored_line};
use crate::host_command;

pub(crate) struct DesktopWdioOptions<'a> {
    pub(crate) root: &'a Path,
    pub(crate) artifact_root: &'a Path,
    pub(crate) pevo_bin: &'a Path,
    pub(crate) floating_text: &'a str,
    pub(crate) provider_token: Option<&'a str>,
    pub(crate) timeouts: DesktopWdioTimeouts,
}

#[derive(Clone, Copy)]
pub(crate) struct DesktopWdioTimeouts {
    pub(crate) build: Duration,
    pub(crate) smoke: Duration,
    pub(crate) cleanup: Duration,
}

pub(crate) struct DesktopWdioRun {
    pub(crate) outcome: ProcessOutcome,
    pub(crate) failure_detail: Option<String>,
}

pub(crate) fn run_desktop_wdio<F>(
    options: &DesktopWdioOptions<'_>,
    log: Arc<Mutex<fs::File>>,
    configure_environment: F,
) -> Result<DesktopWdioRun>
where
    F: Fn(&mut ProcessCommand),
{
    if options.artifact_root.exists() {
        fs::remove_dir_all(options.artifact_root).with_context(|| {
            format!(
                "remove stale Desktop WDIO artifact root {}",
                options.artifact_root.display()
            )
        })?;
    }
    fs::create_dir_all(options.artifact_root)
        .with_context(|| format!("create {}", options.artifact_root.display()))?;

    let mut aggregate = ProcessAggregate::default();
    let mut build = host_command::pnpm(["--filter", "@psychevo/desktop", "tauri:wdio-build"])?;
    build.current_dir(options.root);
    configure_desktop_wdio_command(&mut build, options, &configure_environment);
    let build = match run_logged_process_with_timeout(
        "desktop native WDIO build",
        &mut build,
        Arc::clone(&log),
        options.timeouts.build,
    ) {
        Ok(outcome) => outcome,
        Err(error) => {
            return command_error(
                log,
                aggregate,
                format!("Desktop native WDIO build: {error:#}"),
            );
        }
    };
    aggregate.merge(&build);
    if !build.passed {
        return Ok(aggregate.finish(
            false,
            build.exit_code,
            Some("Desktop native WDIO build failed".to_string()),
        ));
    }

    let mut smoke = host_command::pnpm(["--filter", "@psychevo/desktop", "wdio"])?;
    smoke.current_dir(options.root);
    configure_desktop_wdio_command(&mut smoke, options, &configure_environment);
    let smoke_result = run_logged_process_with_timeout(
        "desktop native WDIO smoke",
        &mut smoke,
        Arc::clone(&log),
        options.timeouts.smoke,
    );

    let mut cleanup = ProcessCommand::new(options.pevo_bin);
    cleanup.args(["gateway", "stop"]).current_dir(options.root);
    configure_environment(&mut cleanup);
    let cleanup_result = run_logged_process_with_timeout(
        "desktop native managed Gateway cleanup",
        &mut cleanup,
        Arc::clone(&log),
        options.timeouts.cleanup,
    );

    let smoke = match smoke_result {
        Ok(outcome) => {
            aggregate.merge(&outcome);
            Some(outcome)
        }
        Err(error) => {
            write_mirrored_line(
                &log,
                &format!("Desktop native WDIO smoke failed: {error:#}"),
            )?;
            None
        }
    };
    let cleanup = match cleanup_result {
        Ok(outcome) => {
            aggregate.merge(&outcome);
            Some(outcome)
        }
        Err(error) => {
            write_mirrored_line(
                &log,
                &format!("Desktop native managed Gateway cleanup failed: {error:#}"),
            )?;
            None
        }
    };

    let cleanup_passed = cleanup.as_ref().is_some_and(|outcome| outcome.passed);
    let smoke_passed = smoke.as_ref().is_some_and(|outcome| outcome.passed);
    if !cleanup_passed {
        return Ok(aggregate.finish(
            false,
            cleanup.as_ref().and_then(|outcome| outcome.exit_code),
            Some(if smoke_passed {
                "Desktop native WDIO Gateway cleanup failed".to_string()
            } else {
                "Desktop native WDIO smoke and Gateway cleanup failed".to_string()
            }),
        ));
    }
    if !smoke_passed {
        return Ok(aggregate.finish(
            false,
            smoke.as_ref().and_then(|outcome| outcome.exit_code),
            Some("Desktop native WDIO smoke failed".to_string()),
        ));
    }

    if let Err(error) = validate_desktop_startup_artifacts(options.artifact_root) {
        let detail = format!("Desktop native startup evidence is invalid: {error:#}");
        write_mirrored_line(&log, &detail)?;
        aggregate.mirrored_diagnostics += 1;
        return Ok(aggregate.finish(false, Some(0), Some(detail)));
    }

    Ok(aggregate.finish(true, Some(0), None))
}

pub(crate) fn configure_desktop_wdio_command<F>(
    command: &mut ProcessCommand,
    options: &DesktopWdioOptions<'_>,
    configure_environment: &F,
) where
    F: Fn(&mut ProcessCommand),
{
    configure_environment(command);
    command
        .env("PSYCHEVO_PEVO_BIN", options.pevo_bin)
        .env("PSYCHEVO_WDIO_ARTIFACT_ROOT", options.artifact_root)
        .env("PSYCHEVO_FLOATING_TEXT", options.floating_text);
    if let Some(token) = options.provider_token {
        command
            .env("PSYCHEVO_DESKTOP_PROVIDER_LIVE", "1")
            .env("PSYCHEVO_FLOATING_PROVIDER_TOKEN", token);
    }
}

pub(crate) fn validate_desktop_startup_artifacts(artifact_root: &Path) -> Result<()> {
    const REQUIRED_CHECKPOINTS: &[&str] = &[
        "process_start",
        "window_ready",
        "managed_gateway_ready",
        "bridge_connected",
        "gui_ready",
        "draft_context_ready",
    ];
    const SCREENSHOT_CHECKPOINTS: &[&str] = &["gui_ready", "draft_context_ready"];

    let manifest_path = artifact_root.join("desktop-startup-journey.json");
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
                        Component::ParentDir | Component::RootDir | Component::Prefix(_)
                    )
                })
            {
                bail!(
                    "{} checkpoint '{}' has an unsafe screenshot path",
                    manifest_path.display(),
                    id
                );
            }
            let screenshot = artifact_root.join(relative);
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
    let rust_trace = artifact_root.join("desktop-startup-rust.jsonl");
    if !rust_trace.is_file() {
        bail!(
            "Desktop Rust startup trace is missing: {}",
            rust_trace.display()
        );
    }
    Ok(())
}

fn command_error(
    log: Arc<Mutex<fs::File>>,
    aggregate: ProcessAggregate,
    detail: String,
) -> Result<DesktopWdioRun> {
    write_mirrored_line(&log, &detail)?;
    Ok(aggregate.finish(false, None, Some(detail)))
}

#[derive(Default)]
struct ProcessAggregate {
    mirrored_diagnostics: usize,
    had_suppressed_output: bool,
}

impl ProcessAggregate {
    fn merge(&mut self, outcome: &ProcessOutcome) {
        self.mirrored_diagnostics += outcome.mirrored_diagnostics;
        self.had_suppressed_output |= outcome.had_suppressed_output;
    }

    fn finish(
        self,
        passed: bool,
        exit_code: Option<i32>,
        failure_detail: Option<String>,
    ) -> DesktopWdioRun {
        DesktopWdioRun {
            outcome: ProcessOutcome {
                passed,
                exit_code,
                mirrored_diagnostics: self.mirrored_diagnostics,
                had_suppressed_output: self.had_suppressed_output,
            },
            failure_detail,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;

    #[test]
    fn command_configuration_uses_artifact_root_and_injected_environment() {
        let options = DesktopWdioOptions {
            root: Path::new("/repo"),
            artifact_root: Path::new("/artifacts/wdio"),
            pevo_bin: Path::new("/repo/target/debug/pevo"),
            floating_text: "selected text",
            provider_token: None,
            timeouts: DesktopWdioTimeouts {
                build: Duration::from_secs(1),
                smoke: Duration::from_secs(1),
                cleanup: Duration::from_secs(1),
            },
        };
        let mut command = ProcessCommand::new("unused-test-command");
        configure_desktop_wdio_command(&mut command, &options, &|command| {
            command.env("PSYCHEVO_HOME", "/artifacts/home");
        });
        let env = command
            .get_envs()
            .map(|(key, value)| {
                (
                    key.to_string_lossy().into_owned(),
                    value.map(|value| value.to_string_lossy().into_owned()),
                )
            })
            .collect::<BTreeMap<_, _>>();

        assert_eq!(
            env.get("PSYCHEVO_WDIO_ARTIFACT_ROOT"),
            Some(&Some("/artifacts/wdio".to_string()))
        );
        assert_eq!(
            env.get("PSYCHEVO_PEVO_BIN"),
            Some(&Some("/repo/target/debug/pevo".to_string()))
        );
        assert_eq!(
            env.get("PSYCHEVO_HOME"),
            Some(&Some("/artifacts/home".to_string()))
        );
        assert!(!env.contains_key("PSYCHEVO_DESKTOP_PROVIDER_LIVE"));
    }

    #[test]
    fn startup_validation_requires_complete_screenshot_evidence() {
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
}
