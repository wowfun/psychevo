use std::fs;
use std::path::Path;
use std::process::Command as ProcessCommand;
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result};
use serde_json::Value;

use crate::host_command;

use super::process::{
    ProcessOutcome, command_exists, run_logged_process, write_log_line, write_mirrored_line,
};

const SPEC: &str = "apps/workbench/e2e/surface-comparison.spec.ts";
const DEFAULT_MEASURED_SAMPLES: usize = 20;
const DEFAULT_TRACKED_DIRTY_FILES: usize = 200;
const PLAYWRIGHT_INSTALL_HINT: &str = "pnpm exec playwright install chromium";

pub(crate) fn run_surface_profile(
    root: &Path,
    artifact_root: &Path,
    log: Arc<Mutex<fs::File>>,
) -> Result<ProcessOutcome> {
    if !command_exists("pnpm") {
        return failed(
            log,
            &[
                "missing pnpm for surface profiling".to_string(),
                format!("run: {PLAYWRIGHT_INSTALL_HINT}"),
            ],
        );
    }
    if !command_exists("python3") {
        return failed(
            log,
            &["missing python3 for fullscreen TUI PTY profiling".to_string()],
        );
    }
    let spec = root.join(SPEC);
    if !spec.is_file() {
        return failed(
            log,
            &[format!(
                "surface comparison spec is missing: {}",
                spec.display()
            )],
        );
    }
    let pevo =
        root.join("target")
            .join("debug")
            .join(if cfg!(windows) { "pevo.exe" } else { "pevo" });
    if !pevo.is_file() {
        return failed(
            log,
            &[format!(
                "surface profile pevo binary is missing: {}",
                pevo.display()
            )],
        );
    }

    let output = artifact_root.join("profile").join("surface-comparison");
    if output.exists() {
        fs::remove_dir_all(&output)
            .with_context(|| format!("remove stale surface profile {}", output.display()))?;
    }
    fs::create_dir_all(&output)
        .with_context(|| format!("create surface profile {}", output.display()))?;
    write_log_line(
        &log,
        &format!("surface profile artifacts: {}", output.display()),
    )?;

    let mut version = host_command::pnpm(["exec", "playwright", "--version"])?;
    apply_env(&mut version, root, artifact_root, &output, &pevo);
    let version_outcome = run_logged_process("playwright version", &mut version, Arc::clone(&log))?;
    if !version_outcome.passed {
        write_mirrored_line(&log, &format!("run: {PLAYWRIGHT_INSTALL_HINT}"))?;
        return Ok(ProcessOutcome {
            passed: false,
            exit_code: version_outcome.exit_code,
            mirrored_diagnostics: version_outcome.mirrored_diagnostics + 1,
            had_suppressed_output: version_outcome.had_suppressed_output,
        });
    }

    let mut profile = host_command::pnpm([
        "exec",
        "playwright",
        "test",
        SPEC,
        "--project",
        "chromium-desktop",
    ])?;
    apply_env(&mut profile, root, artifact_root, &output, &pevo);
    profile.env_remove("NO_COLOR");
    let outcome = run_logged_process(
        "TUI versus Workbench profile",
        &mut profile,
        Arc::clone(&log),
    )?;
    let mut mirrored_diagnostics =
        version_outcome.mirrored_diagnostics + outcome.mirrored_diagnostics;
    let had_suppressed_output =
        version_outcome.had_suppressed_output || outcome.had_suppressed_output;
    if !outcome.passed {
        return Ok(ProcessOutcome {
            passed: false,
            exit_code: outcome.exit_code,
            mirrored_diagnostics,
            had_suppressed_output,
        });
    }

    let errors = validate_surface_profile(&output, DEFAULT_MEASURED_SAMPLES);
    for error in &errors {
        write_mirrored_line(&log, error)?;
    }
    mirrored_diagnostics += errors.len();
    Ok(ProcessOutcome {
        passed: errors.is_empty(),
        exit_code: outcome.exit_code,
        mirrored_diagnostics,
        had_suppressed_output,
    })
}

fn apply_env(
    command: &mut ProcessCommand,
    root: &Path,
    artifact_root: &Path,
    output: &Path,
    pevo: &Path,
) {
    command
        .current_dir(root)
        .env("PSYCHEVO_CI_ARTIFACT_ROOT", artifact_root)
        .env("PSYCHEVO_SURFACE_PROFILE", "1")
        .env("PSYCHEVO_SURFACE_PROFILE_ROOT", output)
        .env(
            "PSYCHEVO_SURFACE_PROFILE_SAMPLES",
            DEFAULT_MEASURED_SAMPLES.to_string(),
        )
        .env(
            "PSYCHEVO_SURFACE_PROFILE_DIRTY_FILES",
            DEFAULT_TRACKED_DIRTY_FILES.to_string(),
        )
        .env("PSYCHEVO_PEVO_BIN", pevo);
}

fn validate_surface_profile(root: &Path, measured_samples: usize) -> Vec<String> {
    let manifest_path = root.join("comparison.json");
    let bytes = match fs::read(&manifest_path) {
        Ok(bytes) => bytes,
        Err(error) => {
            return vec![format!(
                "missing surface comparison manifest: {} ({error})",
                manifest_path.display()
            )];
        }
    };
    let manifest: Value = match serde_json::from_slice(&bytes) {
        Ok(value) => value,
        Err(error) => {
            return vec![format!(
                "invalid surface comparison manifest: {} ({error})",
                manifest_path.display()
            )];
        }
    };
    let mut errors = Vec::new();
    if manifest.get("schemaVersion").and_then(Value::as_u64) != Some(3) {
        errors.push("surface comparison must use schemaVersion 3".to_string());
    }
    if manifest.get("outcome").and_then(Value::as_str) != Some("passed") {
        errors.push("surface comparison did not pass".to_string());
    }
    if manifest
        .pointer("/contract/measuredSamples")
        .and_then(Value::as_u64)
        != Some(measured_samples as u64)
    {
        errors.push(format!(
            "surface comparison must contain {measured_samples} measured samples"
        ));
    }
    if manifest
        .pointer("/contract/trackedDirtyFiles")
        .and_then(Value::as_u64)
        != Some(DEFAULT_TRACKED_DIRTY_FILES as u64)
    {
        errors.push(format!(
            "surface comparison must contain {DEFAULT_TRACKED_DIRTY_FILES} deterministic dirty tracked files"
        ));
    }
    for surface in ["tui", "workbench"] {
        let samples = manifest
            .pointer(&format!("/surfaces/{surface}/samples"))
            .and_then(Value::as_array);
        if samples.map(Vec::len) != Some(measured_samples) {
            errors.push(format!(
                "surface comparison {surface} must contain {measured_samples} raw samples"
            ));
        }
        if let Some(samples) = samples {
            for (index, sample) in samples.iter().enumerate() {
                if surface == "workbench" {
                    if sample
                        .pointer("/workbenchGateway/structure/turnStarted")
                        .and_then(Value::as_u64)
                        != Some(1)
                    {
                        errors.push(format!(
                            "surface comparison Workbench sample {index} must contain exactly one public turnStarted"
                        ));
                    }
                    if sample
                        .pointer("/workbenchGateway/structure/reviewScans")
                        .and_then(Value::as_u64)
                        != Some(0)
                    {
                        errors.push(format!(
                            "surface comparison Workbench sample {index} must contain zero synchronous review scans"
                        ));
                    }
                } else if sample.get("workbenchGateway").is_some() {
                    errors.push(format!(
                        "surface comparison TUI sample {index} must not synthesize a Gateway Turn"
                    ));
                }
            }
        }
        validate_summary(
            &manifest,
            surface,
            "summary",
            &[
                "sendToFeedbackCommitMs",
                "sendToRequestMs",
                "requestToFirstSurfaceCommitMs",
                "firstSurfaceCommitToSettledCommitMs",
                "sendToSettledCommitMs",
            ],
            measured_samples,
            &mut errors,
        );
        validate_summary(
            &manifest,
            surface,
            "surfaceSummary",
            &[
                "assistantReceivedToControllerAppliedMs",
                "assistantAppliedToSurfaceCommitMs",
                "completionReceivedToControllerAppliedMs",
                "completionAppliedToSettledCommitMs",
            ],
            measured_samples,
            &mut errors,
        );
        for phase in ["cold", "warmup", "traceDiagnostic"] {
            if !manifest
                .pointer(&format!("/surfaces/{surface}/{phase}"))
                .is_some_and(Value::is_object)
            {
                errors.push(format!(
                    "surface comparison {surface} is missing {phase} evidence"
                ));
            }
        }
    }
    validate_top_level_summary(
        &manifest,
        "workbenchGatewaySummary",
        &[
            "turnStartReceivedToAdmittedMs",
            "turnAdmittedToAcceptedMs",
            "turnAcceptedToExecutionStartedMs",
            "executionStartedToUserEntryProjectedMs",
            "userEntryProjectedToFirstAssistantMs",
            "firstAssistantToTurnCompletedMs",
        ],
        measured_samples,
        &mut errors,
    );
    for reference in [
        "/artifacts/providerEvents",
        "/artifacts/report",
        "/artifacts/tuiTrace",
        "/artifacts/workbenchBrowserMarks",
        "/artifacts/workbenchGatewayTrace",
        "/artifacts/workbenchTrace",
    ] {
        match manifest.pointer(reference).and_then(Value::as_str) {
            Some(relative)
                if root
                    .join(relative)
                    .metadata()
                    .is_ok_and(|metadata| metadata.is_file() && metadata.len() > 0) => {}
            Some(relative) => errors.push(format!(
                "surface comparison artifact is missing: {}",
                root.join(relative).display()
            )),
            None => errors.push(format!(
                "surface comparison is missing artifact ref {reference}"
            )),
        }
    }
    for delta in ["delta", "surfaceDelta"] {
        if !manifest.get(delta).is_some_and(Value::is_object) {
            errors.push(format!(
                "surface comparison is missing GUI-minus-TUI {delta} data"
            ));
        }
    }
    errors
}

fn validate_top_level_summary(
    manifest: &Value,
    summary: &str,
    metrics: &[&str],
    measured_samples: usize,
    errors: &mut Vec<String>,
) {
    for metric in metrics {
        let base = format!("/{summary}/{metric}");
        let observed = manifest
            .pointer(&format!("{base}/observedSamples"))
            .and_then(Value::as_u64);
        let missing = manifest
            .pointer(&format!("{base}/missingSamples"))
            .and_then(Value::as_u64);
        if observed
            .zip(missing)
            .map(|(observed, missing)| observed + missing)
            != Some(measured_samples as u64)
        {
            errors.push(format!(
                "surface comparison {summary}.{metric} has invalid sample accounting"
            ));
        }
        for percentile in ["p50", "p95"] {
            if manifest
                .pointer(&format!("{base}/{percentile}"))
                .and_then(Value::as_f64)
                .is_none()
            {
                errors.push(format!(
                    "surface comparison is missing {summary}.{metric}.{percentile}"
                ));
            }
        }
    }
}

fn validate_summary(
    manifest: &Value,
    surface: &str,
    summary: &str,
    metrics: &[&str],
    measured_samples: usize,
    errors: &mut Vec<String>,
) {
    for metric in metrics {
        let base = format!("/surfaces/{surface}/{summary}/{metric}");
        let observed = manifest
            .pointer(&format!("{base}/observedSamples"))
            .and_then(Value::as_u64);
        let missing = manifest
            .pointer(&format!("{base}/missingSamples"))
            .and_then(Value::as_u64);
        if observed
            .zip(missing)
            .map(|(observed, missing)| observed + missing)
            != Some(measured_samples as u64)
        {
            errors.push(format!(
                "surface comparison {surface} {summary}.{metric} has invalid sample accounting"
            ));
        }
        for percentile in ["p50", "p95"] {
            let value = manifest.pointer(&format!("{base}/{percentile}"));
            let allows_missing = summary == "summary" && *metric == "sendToFeedbackCommitMs";
            if value.and_then(Value::as_f64).is_none()
                && !(allows_missing && value.is_some_and(Value::is_null))
            {
                errors.push(format!(
                    "surface comparison {surface} is missing {summary}.{metric}.{percentile}"
                ));
            }
        }
    }
}

fn failed(log: Arc<Mutex<fs::File>>, lines: &[String]) -> Result<ProcessOutcome> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn validator_requires_raw_samples_summaries_and_artifacts() {
        let root = test_root("valid");
        fs::create_dir_all(&root).expect("create test root");
        for artifact in [
            "provider.jsonl",
            "report.md",
            "tui.jsonl",
            "browser.jsonl",
            "workbench-gateway.jsonl",
            "trace.zip",
        ] {
            fs::write(root.join(artifact), b"artifact").expect("write artifact");
        }
        let metric = serde_json::json!({
            "missingSamples": 0,
            "observedSamples": 2,
            "p50": 1.0,
            "p95": 2.0
        });
        let summary = serde_json::json!({
            "sendToFeedbackCommitMs": metric,
            "sendToRequestMs": metric,
            "requestToFirstSurfaceCommitMs": metric,
            "firstSurfaceCommitToSettledCommitMs": metric,
            "sendToSettledCommitMs": metric
        });
        let gateway_summary = serde_json::json!({
            "turnStartReceivedToAdmittedMs": metric,
            "turnAdmittedToAcceptedMs": metric,
            "turnAcceptedToExecutionStartedMs": metric,
            "executionStartedToUserEntryProjectedMs": metric,
            "userEntryProjectedToFirstAssistantMs": metric,
            "firstAssistantToTurnCompletedMs": metric
        });
        let surface_summary = serde_json::json!({
            "assistantReceivedToControllerAppliedMs": metric,
            "assistantAppliedToSurfaceCommitMs": metric,
            "completionReceivedToControllerAppliedMs": metric,
            "completionAppliedToSettledCommitMs": metric
        });
        let tui_surface = serde_json::json!({
            "cold": {},
            "samples": [
                {},
                {}
            ],
            "summary": summary,
            "surfaceSummary": surface_summary,
            "traceDiagnostic": {},
            "warmup": {}
        });
        let workbench_surface = serde_json::json!({
            "cold": {},
            "samples": [
                {"workbenchGateway": {"structure": {"reviewScans": 0, "turnStarted": 1}}},
                {"workbenchGateway": {"structure": {"reviewScans": 0, "turnStarted": 1}}}
            ],
            "summary": summary,
            "surfaceSummary": surface_summary,
            "traceDiagnostic": {},
            "warmup": {}
        });
        fs::write(
            root.join("comparison.json"),
            serde_json::to_vec(&serde_json::json!({
                "schemaVersion": 3,
                "outcome": "passed",
                "contract": {
                    "measuredSamples": 2,
                    "trackedDirtyFiles": DEFAULT_TRACKED_DIRTY_FILES
                },
                "surfaces": {
                    "tui": tui_surface,
                    "workbench": workbench_surface
                },
                "delta": {},
                "surfaceDelta": {},
                "workbenchGatewaySummary": gateway_summary,
                "artifacts": {
                    "providerEvents": "provider.jsonl",
                    "report": "report.md",
                    "tuiTrace": "tui.jsonl",
                    "workbenchBrowserMarks": "browser.jsonl",
                    "workbenchGatewayTrace": "workbench-gateway.jsonl",
                    "workbenchTrace": "trace.zip"
                }
            }))
            .expect("manifest json"),
        )
        .expect("write manifest");
        assert!(validate_surface_profile(&root, 2).is_empty());
        fs::remove_dir_all(root).expect("remove test root");
    }

    #[test]
    fn validator_reports_partial_evidence() {
        let root = test_root("partial");
        fs::create_dir_all(&root).expect("create test root");
        fs::write(
            root.join("comparison.json"),
            br#"{"schemaVersion":3,"outcome":"failed"}"#,
        )
        .expect("write partial manifest");
        let errors = validate_surface_profile(&root, 20);
        assert!(errors.iter().any(|error| error.contains("did not pass")));
        assert!(errors.iter().any(|error| error.contains("raw samples")));
        fs::remove_dir_all(root).expect("remove test root");
    }

    fn test_root(label: &str) -> std::path::PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time after epoch")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "psychevo-surface-profile-{label}-{}-{nonce}",
            std::process::id()
        ))
    }
}
