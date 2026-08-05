use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command as ProcessCommand;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{Context, Result};
use serde::Serialize;
use serde_json::Value;

use crate::host_command;

use super::process::{
    ProcessOutcome, command_exists, run_logged_process_with_timeout, write_log_line,
    write_mirrored_line,
};

const PLAYWRIGHT_INSTALL_HINT: &str = "pnpm exec playwright install chromium";
const CRITICAL_JOURNEY_SPEC: &str = "apps/workbench/e2e/critical-journey.spec.ts";
const CRITICAL_JOURNEY_PROFILE_SAMPLES: usize = 20;
const PLAYWRIGHT_VERSION_TIMEOUT: Duration = Duration::from_secs(2 * 60);
const WORKBENCH_BUILD_TIMEOUT: Duration = Duration::from_secs(15 * 60);
const CRITICAL_JOURNEY_TIMEOUT: Duration = Duration::from_secs(30 * 60);
const WORKBENCH_VISUAL_TIMEOUT: Duration = Duration::from_secs(90 * 60);

pub(crate) fn run_workbench_critical_journey(
    root: &Path,
    artifact_root: &Path,
    log: Arc<Mutex<fs::File>>,
) -> Result<ProcessOutcome> {
    if !command_exists("pnpm") {
        return failed_workbench_visual(
            log,
            &[
                "missing pnpm for Workbench critical browser validation".to_string(),
                format!("run: {PLAYWRIGHT_INSTALL_HINT}"),
            ],
        );
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

    let output_root = artifact_root.join("critical-browser");
    if output_root.exists() {
        fs::remove_dir_all(&output_root)
            .with_context(|| format!("remove stale {}", output_root.display()))?;
    }
    let screenshot_root = output_root.join("screenshots");
    let journey_root = output_root.join("journeys").join("first-turn");
    fs::create_dir_all(&screenshot_root)
        .with_context(|| format!("create {}", screenshot_root.display()))?;
    write_log_line(
        &log,
        &format!("critical browser artifacts: {}", output_root.display()),
    )?;

    run_critical_journey_pass(
        root,
        artifact_root,
        &screenshot_root,
        &journey_root,
        "visual",
        log,
    )
}

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
    let screenshot_root = workbench_dir.join("screenshots");
    let playwright_root = workbench_dir.join("playwright");
    fs::create_dir_all(&screenshot_root)
        .with_context(|| format!("create {}", screenshot_root.display()))?;
    write_log_line(
        &log,
        &format!(
            "Workbench visual screenshots: {}",
            screenshot_root.display()
        ),
    )?;

    let result = run_workbench_visual_steps(
        root,
        artifact_root,
        &workbench_dir,
        &screenshot_root,
        &playwright_root,
        Arc::clone(&log),
    );
    match result {
        Ok(mut outcome) => {
            let evidence = inspect_workbench_visual_evidence(&workbench_dir, &playwright_root);
            let evidence_errors = if outcome.passed {
                evidence.validation_errors.clone()
            } else {
                Vec::new()
            };
            for error in &evidence_errors {
                write_mirrored_line(&log, error)?;
            }
            if !evidence_errors.is_empty() {
                outcome.passed = false;
                outcome.mirrored_diagnostics += evidence_errors.len();
            }
            write_workbench_visual_manifest(
                &workbench_dir,
                outcome.passed,
                &evidence,
                &evidence_errors,
            )?;
            Ok(outcome)
        }
        Err(error) => {
            let evidence = inspect_workbench_visual_evidence(&workbench_dir, &playwright_root);
            write_workbench_visual_manifest(
                &workbench_dir,
                false,
                &evidence,
                &[format!("visual runner error: {error:#}")],
            )?;
            Err(error)
        }
    }
}

fn run_workbench_visual_steps(
    root: &Path,
    artifact_root: &Path,
    workbench_dir: &Path,
    screenshot_root: &Path,
    playwright_root: &Path,
    log: Arc<Mutex<fs::File>>,
) -> Result<ProcessOutcome> {
    let mut mirrored_diagnostics = 0;
    let mut had_suppressed_output = false;
    let mut version = host_command::pnpm(["exec", "playwright", "--version"])?;
    apply_workbench_visual_env(&mut version, root, artifact_root, screenshot_root);
    let outcome = run_logged_process_with_timeout(
        "playwright version",
        &mut version,
        Arc::clone(&log),
        PLAYWRIGHT_VERSION_TIMEOUT,
    )?;
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

    let mut build = host_command::pnpm(["--filter", "@psychevo/workbench", "build"])?;
    apply_workbench_visual_env(&mut build, root, artifact_root, screenshot_root);
    let outcome = run_logged_process_with_timeout(
        "workbench visual build",
        &mut build,
        Arc::clone(&log),
        WORKBENCH_BUILD_TIMEOUT,
    )?;
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

    let journey_root = workbench_dir.join("journeys").join("first-turn");
    for pass in ["profile", "visual"] {
        let outcome = run_critical_journey_pass(
            root,
            artifact_root,
            screenshot_root,
            &journey_root,
            pass,
            Arc::clone(&log),
        )?;
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
    }

    let mut test = host_command::pnpm([
        "exec",
        "playwright",
        "test",
        "--project",
        "chromium-desktop",
        "--project",
        "chromium-mobile",
    ])?;
    apply_workbench_visual_env(&mut test, root, artifact_root, screenshot_root);
    test.env("PSYCHEVO_PLAYWRIGHT_OUTPUT_ROOT", playwright_root)
        .env("PSYCHEVO_PLAYWRIGHT_CAPTURE_EVERY_TEST", "1")
        .env_remove("NO_COLOR")
        .env_remove("PSYCHEVO_XTASK_LIVE_CONTEXT")
        .env_remove("PSYCHEVO_JOURNEY_PASS")
        .env_remove("PSYCHEVO_SURFACE_PROFILE_ROOT")
        .env_remove("PSYCHEVO_RUNTIME_LIVE_FAKE");
    let outcome = run_logged_process_with_timeout(
        "complete deterministic Workbench Playwright inventory",
        &mut test,
        Arc::clone(&log),
        WORKBENCH_VISUAL_TIMEOUT,
    )?;
    mirrored_diagnostics += outcome.mirrored_diagnostics;
    had_suppressed_output |= outcome.had_suppressed_output;
    Ok(ProcessOutcome {
        passed: outcome.passed,
        exit_code: outcome.exit_code,
        mirrored_diagnostics,
        had_suppressed_output,
    })
}

fn run_critical_journey_pass(
    root: &Path,
    artifact_root: &Path,
    screenshot_root: &Path,
    journey_root: &Path,
    pass: &str,
    log: Arc<Mutex<fs::File>>,
) -> Result<ProcessOutcome> {
    let mut journey = host_command::pnpm([
        "exec",
        "playwright",
        "test",
        CRITICAL_JOURNEY_SPEC,
        "--project",
        "chromium-desktop",
    ])?;
    apply_workbench_visual_env(&mut journey, root, artifact_root, screenshot_root);
    journey
        .env("PSYCHEVO_JOURNEY_PASS", pass)
        .env("PSYCHEVO_PLAYWRIGHT_JOURNEY_ROOT", journey_root)
        .env(
            "PSYCHEVO_JOURNEY_PROFILE_SAMPLES",
            CRITICAL_JOURNEY_PROFILE_SAMPLES.to_string(),
        )
        .env_remove("NO_COLOR");
    let outcome = run_logged_process_with_timeout(
        &format!("workbench critical journey {pass}"),
        &mut journey,
        Arc::clone(&log),
        CRITICAL_JOURNEY_TIMEOUT,
    )?;
    if !outcome.passed {
        return Ok(outcome);
    }
    let errors =
        validate_critical_journey_evidence(journey_root, pass, CRITICAL_JOURNEY_PROFILE_SAMPLES);
    if errors.is_empty() {
        return Ok(outcome);
    }
    for error in &errors {
        write_mirrored_line(&log, error)?;
    }
    Ok(ProcessOutcome {
        passed: false,
        exit_code: outcome.exit_code,
        mirrored_diagnostics: outcome.mirrored_diagnostics + errors.len(),
        had_suppressed_output: outcome.had_suppressed_output,
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

#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
struct WorkbenchVisualEvidence {
    artifacts: Vec<VisualArtifact>,
    desktop_executed: usize,
    desktop_screenshots: usize,
    mobile_executed: usize,
    mobile_screenshots: usize,
    executed_results: usize,
    screenshot_results: usize,
    manual_screenshots: usize,
    validation_errors: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct VisualArtifact {
    path: String,
    bytes: u64,
    kind: &'static str,
}

fn inspect_workbench_visual_evidence(
    workbench_root: &Path,
    playwright_root: &Path,
) -> WorkbenchVisualEvidence {
    let mut evidence = WorkbenchVisualEvidence::default();
    let files = match collect_files(workbench_root) {
        Ok(files) => files,
        Err(error) => {
            evidence.validation_errors.push(format!(
                "failed to inventory Workbench visual artifacts: {error:#}"
            ));
            return evidence;
        }
    };
    for path in files {
        let Ok(metadata) = fs::metadata(&path) else {
            evidence.validation_errors.push(format!(
                "Workbench visual artifact disappeared during inventory: {}",
                path.display()
            ));
            continue;
        };
        let kind = artifact_kind(&path);
        if kind == "screenshot" {
            if metadata.len() == 0 {
                evidence.validation_errors.push(format!(
                    "Workbench visual screenshot is empty: {}",
                    path.display()
                ));
            }
            if !path.starts_with(playwright_root) {
                evidence.manual_screenshots += 1;
            }
        }
        evidence.artifacts.push(VisualArtifact {
            path: portable_relative_path(workbench_root, &path),
            bytes: metadata.len(),
            kind,
        });
    }
    evidence
        .artifacts
        .sort_by(|left, right| left.path.cmp(&right.path));

    let report_path = playwright_root.join("report.json");
    let report: Value = match fs::read(&report_path)
        .with_context(|| format!("read {}", report_path.display()))
        .and_then(|bytes| {
            serde_json::from_slice(&bytes)
                .with_context(|| format!("parse {}", report_path.display()))
        }) {
        Ok(report) => report,
        Err(error) => {
            evidence.validation_errors.push(format!(
                "missing or invalid current-run Playwright report: {error:#}"
            ));
            return evidence;
        }
    };
    inspect_playwright_report_node(&report, playwright_root, &mut evidence);
    if evidence.executed_results == 0 {
        evidence
            .validation_errors
            .push("complete deterministic Playwright inventory executed no tests".to_string());
    }
    if evidence.desktop_executed == 0 {
        evidence.validation_errors.push(
            "complete deterministic Playwright inventory executed no desktop tests".to_string(),
        );
    }
    if evidence.mobile_executed == 0 {
        evidence.validation_errors.push(
            "complete deterministic Playwright inventory executed no mobile tests".to_string(),
        );
    }
    if evidence.screenshot_results == 0 {
        evidence.validation_errors.push(format!(
            "Playwright retained no automatic screenshots for {} executed results",
            evidence.executed_results
        ));
    }
    if evidence.desktop_screenshots == 0 {
        evidence
            .validation_errors
            .push("Playwright retained no automatic desktop screenshots".to_string());
    }
    if evidence.mobile_screenshots == 0 {
        evidence
            .validation_errors
            .push("Playwright retained no automatic mobile screenshots".to_string());
    }
    if evidence.manual_screenshots == 0 {
        evidence
            .validation_errors
            .push("Workbench visual run retained no manual or journey screenshots".to_string());
    }
    evidence
}

fn inspect_playwright_report_node(
    value: &Value,
    playwright_root: &Path,
    evidence: &mut WorkbenchVisualEvidence,
) {
    match value {
        Value::Object(object) => {
            if let (Some(project), Some(results)) = (
                object.get("projectName").and_then(Value::as_str),
                object.get("results").and_then(Value::as_array),
            ) {
                for result in results {
                    if result.get("status").and_then(Value::as_str) == Some("skipped") {
                        continue;
                    }
                    evidence.executed_results += 1;
                    match project {
                        "chromium-desktop" => evidence.desktop_executed += 1,
                        "chromium-mobile" => evidence.mobile_executed += 1,
                        unexpected => evidence.validation_errors.push(format!(
                            "unexpected Playwright project in visual report: {unexpected}"
                        )),
                    }
                    if valid_result_screenshot(
                        result,
                        playwright_root,
                        &mut evidence.validation_errors,
                    ) {
                        evidence.screenshot_results += 1;
                        match project {
                            "chromium-desktop" => evidence.desktop_screenshots += 1,
                            "chromium-mobile" => evidence.mobile_screenshots += 1,
                            _ => {}
                        }
                    }
                }
            }
            for child in object.values() {
                inspect_playwright_report_node(child, playwright_root, evidence);
            }
        }
        Value::Array(values) => {
            for child in values {
                inspect_playwright_report_node(child, playwright_root, evidence);
            }
        }
        _ => {}
    }
}

fn valid_result_screenshot(
    result: &Value,
    playwright_root: &Path,
    errors: &mut Vec<String>,
) -> bool {
    let Some(attachments) = result.get("attachments").and_then(Value::as_array) else {
        return false;
    };
    for attachment in attachments {
        if attachment.get("contentType").and_then(Value::as_str) != Some("image/png") {
            continue;
        }
        let Some(path) = attachment.get("path").and_then(Value::as_str) else {
            continue;
        };
        let path = PathBuf::from(path);
        let path = if path.is_absolute() {
            path
        } else {
            playwright_root.join(path)
        };
        let canonical_root = fs::canonicalize(playwright_root).ok();
        let canonical_path = fs::canonicalize(&path).ok();
        if canonical_root
            .as_ref()
            .zip(canonical_path.as_ref())
            .is_none_or(|(root, path)| !path.starts_with(root))
        {
            errors.push(format!(
                "Playwright screenshot escaped or is missing from the artifact root: {}",
                path.display()
            ));
            return false;
        }
        if fs::metadata(&path)
            .map(|metadata| metadata.len() > 0)
            .unwrap_or(false)
        {
            return true;
        }
        errors.push(format!(
            "Playwright screenshot is missing or empty: {}",
            path.display()
        ));
        return false;
    }
    false
}

fn collect_files(root: &Path) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(&directory)
            .with_context(|| format!("read artifact directory {}", directory.display()))?
        {
            let entry = entry.with_context(|| format!("read entry in {}", directory.display()))?;
            let file_type = entry
                .file_type()
                .with_context(|| format!("read file type for {}", entry.path().display()))?;
            if file_type.is_dir() {
                pending.push(entry.path());
            } else if file_type.is_file() {
                files.push(entry.path());
            }
        }
    }
    Ok(files)
}

fn artifact_kind(path: &Path) -> &'static str {
    match path.extension().and_then(|extension| extension.to_str()) {
        Some("png") => "screenshot",
        Some("zip") => "trace",
        Some("webm") => "video",
        Some("json") | Some("jsonl") => "structured",
        _ => "other",
    }
}

fn portable_relative_path(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

fn write_workbench_visual_manifest(
    workbench_root: &Path,
    passed: bool,
    evidence: &WorkbenchVisualEvidence,
    runner_errors: &[String],
) -> Result<()> {
    let manifest = serde_json::json!({
        "schemaVersion": 1,
        "outcome": if passed { "passed" } else { "failed" },
        "selection": {
            "mode": "all-non-live",
            "projects": ["chromium-desktop", "chromium-mobile"]
        },
        "evidence": evidence,
        "runnerErrors": runner_errors,
    });
    let path = workbench_root.join("visual-manifest.json");
    fs::write(&path, serde_json::to_vec_pretty(&manifest)?)
        .with_context(|| format!("write {}", path.display()))
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
        had_suppressed_output: false,
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
    fn validates_dynamic_playwright_inventory_by_executed_result() {
        let root = test_root("playwright-inventory");
        let playwright = root.join("playwright");
        let desktop = playwright.join("desktop.png");
        let mobile = playwright.join("mobile.png");
        fs::create_dir_all(&playwright).expect("playwright root");
        fs::create_dir_all(root.join("screenshots")).expect("screenshot roots");
        fs::write(root.join("screenshots/manual.png"), b"manual").expect("manual screenshot");
        fs::write(&desktop, b"desktop").expect("desktop screenshot");
        fs::write(&mobile, b"mobile").expect("mobile screenshot");
        fs::write(
            playwright.join("report.json"),
            serde_json::to_vec(&serde_json::json!({
                "suites": [{
                    "specs": [{
                        "tests": [
                            {
                                "projectName": "chromium-desktop",
                                "results": [{
                                    "status": "passed",
                                    "attachments": [{
                                        "contentType": "image/png",
                                        "path": desktop
                                    }]
                                }]
                            },
                            {
                                "projectName": "chromium-mobile",
                                "results": [{
                                    "status": "passed",
                                    "attachments": [{
                                        "contentType": "image/png",
                                        "path": mobile
                                    }]
                                }]
                            },
                            {
                                "projectName": "chromium-mobile",
                                "results": [{ "status": "skipped", "attachments": [] }]
                            }
                        ]
                    }]
                }]
            }))
            .expect("serialize report"),
        )
        .expect("report");

        let evidence = inspect_workbench_visual_evidence(&root, &playwright);
        assert!(evidence.validation_errors.is_empty());
        assert_eq!(evidence.executed_results, 2);
        assert_eq!(evidence.screenshot_results, 2);
        assert_eq!(evidence.desktop_executed, 1);
        assert_eq!(evidence.desktop_screenshots, 1);
        assert_eq!(evidence.mobile_executed, 1);
        assert_eq!(evidence.mobile_screenshots, 1);
        assert_eq!(evidence.manual_screenshots, 1);

        fs::remove_dir_all(root).expect("remove inventory root");
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
