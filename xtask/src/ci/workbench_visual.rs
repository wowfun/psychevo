use std::fs;
use std::path::Path;
use std::process::Command as ProcessCommand;
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result};
use serde_json::Value;

use super::process::{
    ProcessOutcome, command_exists, run_logged_process, write_log_line, write_mirrored_line,
};

const PLAYWRIGHT_INSTALL_HINT: &str = "cargo xtask doctor deps install --only playwright";
const CRITICAL_JOURNEY_SPEC: &str = "apps/workbench/e2e/critical-journey.spec.ts";
const CRITICAL_JOURNEY_PROFILE_SAMPLES: usize = 20;
const WORKBENCH_VISUAL_SPECS: &[&str] = &[
    "apps/workbench/e2e/composer-visual.spec.ts",
    "apps/workbench/e2e/acp-peer-visual.spec.ts",
    "apps/workbench/e2e/new-create-visual.spec.ts",
    "apps/workbench/e2e/runtime-profile-visual.spec.ts",
    "apps/workbench/e2e/agent-application-visual.spec.ts",
    "apps/workbench/e2e/codex-plugin-authority-visual.spec.ts",
];
const REQUIRED_RUNTIME_PROFILE_PROOFS: &[&str] = &[
    "codex-plugin-authority-disabled-chromium-desktop.png",
    "codex-plugin-authority-ready-chromium-desktop.png",
    "codex-plugin-authority-needs-auth-chromium-desktop.png",
    "codex-plugin-authority-needs-trust-chromium-desktop.png",
    "codex-plugin-authority-partial-chromium-desktop.png",
    "codex-plugin-authority-incompatible-chromium-desktop.png",
    "agent-runtime-selector-native-acp-chromium-desktop.png",
    "agent-runtime-selector-native-acp-chromium-mobile.png",
    "codex-acp-common-controls-chromium-desktop.png",
    "codex-acp-common-controls-chromium-mobile.png",
    "opencode-acp-common-controls-chromium-desktop.png",
    "opencode-acp-common-controls-chromium-mobile.png",
    "active-turn-next-model-queued-chromium-desktop.png",
    "active-turn-next-model-queued-chromium-mobile.png",
    "next-turn-model-observed-chromium-desktop.png",
    "next-turn-model-observed-chromium-mobile.png",
    "managed-codex-acp-missing-chromium-desktop.png",
    "managed-codex-acp-missing-chromium-mobile.png",
    "managed-codex-acp-recovered-chromium-desktop.png",
    "managed-codex-acp-recovered-chromium-mobile.png",
    "process-ephemeral-before-restart-chromium-desktop.png",
    "process-ephemeral-before-restart-chromium-mobile.png",
    "process-ephemeral-restart-unavailable-chromium-desktop.png",
    "process-ephemeral-restart-unavailable-chromium-mobile.png",
    "channel-acp-runtime-profile-chromium-desktop.png",
    "channel-acp-runtime-profile-chromium-mobile.png",
    "acp-profile-detail-chromium-desktop.png",
    "acp-profile-detail-chromium-mobile.png",
    "acp-backend-doctor-chromium-desktop.png",
    "acp-backend-doctor-chromium-mobile.png",
    "acp-profile-editor-chromium-desktop.png",
    "acp-profile-editor-chromium-mobile.png",
    "opencode-acp-standard-timeline-chromium-desktop.png",
    "opencode-acp-standard-timeline-chromium-mobile.png",
    "acp-unsupported-direct-actions-chromium-desktop.png",
    "acp-unsupported-direct-actions-chromium-mobile.png",
    "agent-session-archive-sources-chromium-desktop.png",
    "agent-session-archive-sources-chromium-mobile.png",
    "agent-session-lifecycle-actions-chromium-desktop.png",
    "agent-session-lifecycle-actions-chromium-mobile.png",
    "agent-session-delete-confirmation-chromium-desktop.png",
    "agent-session-delete-confirmation-chromium-mobile.png",
    "acp-peer-visual/03-stable-v1-stream-desktop-chromium-desktop.png",
    "acp-peer-visual/03-stable-v1-stream-mobile-chromium-mobile.png",
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
    if !root.join(CRITICAL_JOURNEY_SPEC).is_file() {
        return failed_workbench_visual(
            log,
            &[format!(
                "Workbench critical journey spec is missing: {}",
                root.join(CRITICAL_JOURNEY_SPEC).display()
            )],
        );
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

    let journey_root = workbench_dir.join("journeys").join("first-turn");
    for pass in ["profile", "visual"] {
        let mut journey = ProcessCommand::new("pnpm");
        journey
            .args(["exec", "playwright", "test", CRITICAL_JOURNEY_SPEC])
            .args(["--project", "chromium-desktop"]);
        apply_workbench_visual_env(&mut journey, root, artifact_root, &screenshot_root);
        journey
            .env("PSYCHEVO_JOURNEY_PASS", pass)
            .env("PSYCHEVO_PLAYWRIGHT_JOURNEY_ROOT", &journey_root)
            .env(
                "PSYCHEVO_JOURNEY_PROFILE_SAMPLES",
                CRITICAL_JOURNEY_PROFILE_SAMPLES.to_string(),
            )
            .env_remove("NO_COLOR");
        let outcome = run_logged_process(
            &format!("workbench critical journey {pass}"),
            &mut journey,
            Arc::clone(&log),
        )?;
        mirrored_diagnostics += outcome.mirrored_diagnostics;
        if !outcome.passed {
            return Ok(ProcessOutcome {
                passed: false,
                exit_code: outcome.exit_code,
                mirrored_diagnostics,
            });
        }
        let errors = validate_critical_journey_evidence(
            &journey_root,
            pass,
            CRITICAL_JOURNEY_PROFILE_SAMPLES,
        );
        if !errors.is_empty() {
            for error in &errors {
                write_mirrored_line(&log, error)?;
            }
            return Ok(ProcessOutcome {
                passed: false,
                exit_code: outcome.exit_code,
                mirrored_diagnostics: mirrored_diagnostics + errors.len(),
            });
        }
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

fn validate_critical_journey_evidence(
    journey_root: &Path,
    pass: &str,
    measured_samples: usize,
) -> Vec<String> {
    let mut errors = Vec::new();
    for adapter in ["native", "acp"] {
        for scenario in ["ready-send", "pending-draft-send"] {
            let artifact_root = journey_root.join(adapter).join(scenario).join(pass);
            let manifest_path = artifact_root.join("journey.json");
            errors.extend(validate_journey_manifest(
                &manifest_path,
                &artifact_root,
                adapter,
                scenario,
                pass,
                measured_samples,
            ));
        }
    }
    errors
}

fn validate_journey_manifest(
    manifest_path: &Path,
    artifact_root: &Path,
    adapter: &str,
    scenario: &str,
    pass: &str,
    measured_samples: usize,
) -> Vec<String> {
    let label = format!("{adapter}/{scenario}/{pass}");
    let mut errors = Vec::new();
    let bytes = match fs::read(manifest_path) {
        Ok(bytes) => bytes,
        Err(error) => {
            errors.push(format!(
                "missing critical journey manifest {label}: {} ({error})",
                manifest_path.display()
            ));
            return errors;
        }
    };
    let manifest: Value = match serde_json::from_slice(&bytes) {
        Ok(value) => value,
        Err(error) => {
            errors.push(format!(
                "invalid critical journey manifest {label}: {} ({error})",
                manifest_path.display()
            ));
            return errors;
        }
    };
    if manifest.get("schemaVersion").and_then(Value::as_u64) != Some(1) {
        errors.push(format!("critical journey {label} must use schemaVersion 1"));
    }
    if manifest.get("outcome").and_then(Value::as_str) != Some("passed") {
        errors.push(format!("critical journey {label} did not pass"));
    }
    let run = manifest.get("run").and_then(Value::as_object);
    for (key, expected) in [
        ("adapter", adapter),
        ("scenario", scenario),
        ("pass", pass),
        ("surface", "workbench"),
    ] {
        if run.and_then(|value| value.get(key)).and_then(Value::as_str) != Some(expected) {
            errors.push(format!("critical journey {label} has incorrect run.{key}"));
        }
    }
    let expected_ids = match scenario {
        "pending-draft-send" => [
            "gui_ready",
            "send_clicked",
            "draft_context_ready",
            "runtime_request_dispatched",
            "first_output_visible",
            "turn_settled",
        ],
        _ => [
            "gui_ready",
            "draft_context_ready",
            "send_clicked",
            "runtime_request_dispatched",
            "first_output_visible",
            "turn_settled",
        ],
    };
    let checkpoints = manifest
        .get("checkpoints")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let actual_ids = checkpoints
        .iter()
        .filter_map(|checkpoint| checkpoint.get("id").and_then(Value::as_str))
        .collect::<Vec<_>>();
    if actual_ids != expected_ids {
        errors.push(format!(
            "critical journey {label} checkpoints were {:?}, expected {:?}",
            actual_ids, expected_ids
        ));
    }
    if pass == "visual" {
        for checkpoint in &checkpoints {
            let id = checkpoint
                .get("id")
                .and_then(Value::as_str)
                .unwrap_or("<unknown>");
            let relative = checkpoint
                .get("screenshot")
                .and_then(|value| value.get("path"))
                .and_then(Value::as_str);
            match relative {
                Some(relative) if artifact_root.join(relative).is_file() => {}
                Some(relative) => errors.push(format!(
                    "critical journey {label} checkpoint {id} screenshot is missing: {}",
                    artifact_root.join(relative).display()
                )),
                None => errors.push(format!(
                    "critical journey {label} checkpoint {id} has no screenshot"
                )),
            }
        }
    } else if pass == "profile" {
        if checkpoints
            .iter()
            .any(|checkpoint| checkpoint.get("screenshot").is_some())
        {
            errors.push(format!(
                "critical journey {label} profiling checkpoints must not contain screenshots"
            ));
        }
        if manifest
            .pointer("/profile/measuredSamples")
            .and_then(Value::as_u64)
            != Some(measured_samples as u64)
        {
            errors.push(format!(
                "critical journey {label} must contain {measured_samples} measured samples"
            ));
        }
        if manifest
            .pointer("/profile/samples")
            .and_then(Value::as_array)
            .map(Vec::len)
            != Some(measured_samples)
        {
            errors.push(format!(
                "critical journey {label} must retain {measured_samples} raw samples"
            ));
        }
        for evidence in ["cold", "summary", "traceDiagnostic", "warmup"] {
            if !manifest
                .pointer(&format!("/profile/{evidence}"))
                .is_some_and(Value::is_object)
            {
                errors.push(format!(
                    "critical journey {label} is missing profile.{evidence} evidence"
                ));
            }
        }
        let trace = manifest.pointer("/trace/path").and_then(Value::as_str);
        match trace {
            Some(relative) if artifact_root.join(relative).is_file() => {}
            Some(relative) => errors.push(format!(
                "critical journey {label} trace is missing: {}",
                artifact_root.join(relative).display()
            )),
            None => errors.push(format!("critical journey {label} has no trace reference")),
        }
    }
    errors
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn validates_visual_and_profile_journey_artifacts() {
        let root = test_root("valid");
        let checkpoint_ids = [
            "gui_ready",
            "draft_context_ready",
            "send_clicked",
            "runtime_request_dispatched",
            "first_output_visible",
            "turn_settled",
        ];
        fs::create_dir_all(&root).expect("create journey test root");
        let checkpoints = checkpoint_ids
            .iter()
            .map(|id| {
                let path = format!("{id}.png");
                fs::write(root.join(&path), b"png").expect("write screenshot");
                serde_json::json!({ "id": id, "screenshot": { "path": path } })
            })
            .collect::<Vec<_>>();
        fs::write(
            root.join("journey.json"),
            serde_json::to_vec(&serde_json::json!({
                "schemaVersion": 1,
                "outcome": "passed",
                "run": {
                    "adapter": "native",
                    "scenario": "ready-send",
                    "pass": "visual",
                    "surface": "workbench"
                },
                "checkpoints": checkpoints
            }))
            .expect("serialize visual manifest"),
        )
        .expect("write visual manifest");
        assert!(
            validate_journey_manifest(
                &root.join("journey.json"),
                &root,
                "native",
                "ready-send",
                "visual",
                20,
            )
            .is_empty()
        );

        fs::write(root.join("trace.zip"), b"trace").expect("write trace");
        fs::write(
            root.join("journey.json"),
            serde_json::to_vec(&serde_json::json!({
                "schemaVersion": 1,
                "outcome": "passed",
                "run": {
                    "adapter": "native",
                    "scenario": "ready-send",
                    "pass": "profile",
                    "surface": "workbench"
                },
                "checkpoints": checkpoint_ids.map(|id| serde_json::json!({ "id": id })),
                "profile": {
                    "cold": {},
                    "measuredSamples": 20,
                    "samples": vec![serde_json::json!({}); 20],
                    "summary": {},
                    "traceDiagnostic": {},
                    "warmup": {}
                },
                "trace": { "path": "trace.zip" }
            }))
            .expect("serialize profile manifest"),
        )
        .expect("write profile manifest");
        assert!(
            validate_journey_manifest(
                &root.join("journey.json"),
                &root,
                "native",
                "ready-send",
                "profile",
                20,
            )
            .is_empty()
        );
        fs::remove_dir_all(root).expect("remove journey test root");
    }

    #[test]
    fn reports_missing_visual_evidence() {
        let root = test_root("missing");
        fs::create_dir_all(&root).expect("create journey test root");
        fs::write(
            root.join("journey.json"),
            br#"{"schemaVersion":1,"outcome":"passed","run":{"adapter":"native","scenario":"ready-send","pass":"visual","surface":"workbench"},"checkpoints":[]}"#,
        )
        .expect("write incomplete manifest");
        let errors = validate_journey_manifest(
            &root.join("journey.json"),
            &root,
            "native",
            "ready-send",
            "visual",
            20,
        );
        assert!(
            errors
                .iter()
                .any(|error| error.contains("checkpoints were"))
        );
        fs::remove_dir_all(root).expect("remove journey test root");
    }

    fn test_root(label: &str) -> std::path::PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time after epoch")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "psychevo-workbench-journey-{label}-{}-{nonce}",
            std::process::id()
        ))
    }
}
