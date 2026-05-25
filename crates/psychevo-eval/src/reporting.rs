#[allow(unused_imports)]
pub(crate) use super::*;

#[allow(unused_imports)]
use anyhow::{Context, Result, bail};
#[allow(unused_imports)]
use clap::{Parser, Subcommand, ValueEnum};
#[allow(unused_imports)]
use serde_json::{Value, json};
#[allow(unused_imports)]
use std::collections::{BTreeMap, BTreeSet};
#[allow(unused_imports)]
use std::env;
#[allow(unused_imports)]
use std::ffi::OsString;
#[allow(unused_imports)]
use std::fs;
#[allow(unused_imports)]
use std::io::{BufRead, BufReader};
#[allow(unused_imports)]
use std::path::{Component, Path, PathBuf};
#[allow(unused_imports)]
use std::process::{Command, Stdio};
#[allow(unused_imports)]
use std::thread;
#[allow(unused_imports)]
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
#[allow(unused_imports)]
use uuid::Uuid;

pub(crate) fn load_suite_manifests(root: &Path) -> Result<BTreeMap<String, SuiteManifest>> {
    let mut suites = BTreeMap::new();
    for path in sorted_toml_files(&root.join("suites"))? {
        let mut manifest: SuiteManifest = read_toml(&path)?;
        reject_unsupported(manifest.schema_version, &path)?;
        manifest.dir = path
            .parent()
            .context("suite manifest has no parent")?
            .to_path_buf();
        manifest.manifest_path = path;
        if suites.insert(manifest.id.clone(), manifest).is_some() {
            bail!("duplicate suite id");
        }
    }
    if suites.is_empty() {
        bail!(
            "no suite manifests found under {}",
            root.join("suites").display()
        );
    }
    Ok(suites)
}

pub(crate) fn read_task_manifest(path: &Path) -> Result<TaskManifest> {
    let mut manifest: TaskManifest = read_toml(path)?;
    reject_unsupported(manifest.schema_version, path)?;
    manifest.dir = path
        .parent()
        .context("task manifest has no parent")?
        .to_path_buf();
    manifest.manifest_path = path.to_path_buf();
    Ok(manifest)
}

pub(crate) fn sorted_toml_files(dir: &Path) -> Result<Vec<PathBuf>> {
    if !dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut paths = fs::read_dir(dir)?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<std::io::Result<Vec<_>>>()?;
    paths.retain(|path| path.extension().is_some_and(|ext| ext == "toml"));
    paths.sort();
    Ok(paths)
}

pub(crate) fn read_toml<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<T> {
    let raw =
        fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))?;
    toml::from_str(&raw).with_context(|| format!("failed to parse {}", path.display()))
}

pub(crate) fn discover_manifest(start: &Path) -> Result<PathBuf> {
    let mut current = if start.is_file() {
        if start.file_name().is_some_and(|name| name == "eval.toml") {
            return Ok(start.to_path_buf());
        }
        start.parent().unwrap_or(start).to_path_buf()
    } else {
        start.to_path_buf()
    };
    loop {
        let candidate = current.join("eval.toml");
        if candidate.is_file() {
            return Ok(candidate);
        }
        if !current.pop() {
            break;
        }
    }
    bail!("could not find eval.toml from {}", start.display())
}

pub(crate) fn load_project_from_config(config: Option<&Path>) -> Result<EvalProject> {
    match config {
        Some(path) => EvalProject::load(resolve_cli_path(path)?),
        None => EvalProject::load(env::current_dir()?),
    }
}

pub(crate) fn try_load_project_from_config(config: Option<&Path>) -> Result<Option<EvalProject>> {
    match config {
        Some(path) => Ok(Some(EvalProject::load(resolve_cli_path(path)?)?)),
        None => match discover_manifest(&env::current_dir()?) {
            Ok(path) => Ok(Some(EvalProject::load(path)?)),
            Err(_) => Ok(None),
        },
    }
}

pub(crate) fn task_prompt(task: &TaskManifest) -> Result<String> {
    if let Some(path) = &task.prompt.file {
        let path = resolve_relative(&task.dir, path);
        return fs::read_to_string(&path)
            .with_context(|| format!("failed to read prompt {}", path.display()));
    }
    Ok(task.prompt.text.clone())
}

pub(crate) fn copy_dir(from: &Path, to: &Path) -> Result<()> {
    fs::create_dir_all(to)?;
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        let source = entry.path();
        let target = to.join(entry.file_name());
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            copy_dir(&source, &target)?;
        } else if file_type.is_file() {
            fs::copy(&source, &target).with_context(|| {
                format!(
                    "failed to copy {} to {}",
                    source.display(),
                    target.display()
                )
            })?;
        }
    }
    Ok(())
}

pub(crate) fn push_event(
    events: &mut Vec<TrajectoryEvent>,
    case_id: &str,
    kind: &str,
    message: &str,
    data: Value,
) {
    events.push(TrajectoryEvent {
        schema_version: SCHEMA_VERSION,
        sequence: events.len() as u64,
        case_id: case_id.to_string(),
        kind: kind.to_string(),
        message: message.to_string(),
        timestamp_ms: now_ms(),
        data,
    });
}

pub(crate) fn write_json_pretty<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    let content = serde_json::to_string_pretty(value)?;
    fs::write(path, content).with_context(|| format!("failed to write {}", path.display()))
}

pub(crate) fn write_toml_pretty<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    let content = toml::to_string_pretty(value)?;
    fs::write(path, content).with_context(|| format!("failed to write {}", path.display()))
}

pub(crate) fn write_jsonl(path: &Path, events: &[TrajectoryEvent]) -> Result<()> {
    let mut content = String::new();
    for event in events {
        content.push_str(&serde_json::to_string(event)?);
        content.push('\n');
    }
    fs::write(path, content).with_context(|| format!("failed to write {}", path.display()))
}

pub(crate) fn link_dataset_payload(source: &Path, link: &Path) -> Result<bool> {
    if fs::symlink_metadata(link).is_ok() {
        if link.is_dir() && !link.is_symlink() {
            fs::remove_dir_all(link)
                .with_context(|| format!("failed to remove {}", link.display()))?;
        } else {
            fs::remove_file(link)
                .with_context(|| format!("failed to remove {}", link.display()))?;
        }
    }
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(source, link).with_context(|| {
            format!(
                "failed to link dataset payload {} to {}",
                source.display(),
                link.display()
            )
        })?;
        Ok(true)
    }
    #[cfg(not(unix))]
    {
        let _ = (source, link);
        Ok(false)
    }
}

pub(crate) fn read_run_summary(root: &Path) -> Result<RunSummary> {
    let path = root.join("summary.json");
    let summary: RunSummary = serde_json::from_str(
        &fs::read_to_string(&path)
            .with_context(|| format!("failed to read run summary {}", path.display()))?,
    )
    .with_context(|| format!("failed to parse run summary {}", path.display()))?;
    reject_unsupported(summary.schema_version, &path)?;
    Ok(summary)
}

pub(crate) fn write_run_reports(summary: &RunSummary) -> Result<()> {
    fs::write(
        summary.artifact_root.join("report.md"),
        render_summary_report(summary, ReportFormat::Markdown)?,
    )
    .with_context(|| {
        format!(
            "failed to write {}",
            summary.artifact_root.join("report.md").display()
        )
    })?;
    fs::write(
        summary.artifact_root.join("report.html"),
        render_summary_report(summary, ReportFormat::Html)?,
    )
    .with_context(|| {
        format!(
            "failed to write {}",
            summary.artifact_root.join("report.html").display()
        )
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
#[clap(rename_all = "kebab-case")]
pub enum ReportFormat {
    Html,
    Markdown,
    Json,
}

pub(crate) fn render_summary_report(summary: &RunSummary, format: ReportFormat) -> Result<String> {
    match format {
        ReportFormat::Json => Ok(serde_json::to_string_pretty(summary)?),
        ReportFormat::Markdown => {
            let mut out = String::new();
            out.push_str(&format!("# peval report `{}`\n\n", summary.run_id));
            out.push_str(&format!("- status: {:?}\n", summary.status));
            out.push_str(&format!("- project: `{}`\n", summary.project));
            out.push_str(&format!(
                "- artifact root: `{}`\n",
                summary.artifact_root.display()
            ));
            out.push_str(&format!("- cases: {}\n", summary.total_cases));
            out.push_str(&format!("- passed: {}\n", summary.passed_cases));
            out.push_str(&format!("- failed: {}\n\n", summary.failed_cases));
            out.push_str("| case | suite | task | family | agent | status | failure | score | scorer details | artifacts |\n");
            out.push_str("| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |\n");
            for case in &summary.cases {
                out.push_str(&format!(
                    "| `{}` | `{}` | `{}` | `{}` | `{}` | {:?} | `{}` | {} | `{}` | [result]({}) [trajectory]({}) [stdout]({}) [stderr]({}) |\n",
                    case.case_id,
                    case.suite_id,
                    case.task_id,
                    case.task_family,
                    case.agent_id,
                    case.status,
                    case.failure_class.as_deref().unwrap_or("-"),
                    case.score.score.unwrap_or_default(),
                    scorer_details_label(&case.score.details),
                    case.artifacts.result.display(),
                    case.artifacts.trajectory.display(),
                    case.artifacts.scorer_stdout.display(),
                    case.artifacts.scorer_stderr.display(),
                ));
            }
            Ok(out)
        }
        ReportFormat::Html => {
            let mut rows = String::new();
            for case in &summary.cases {
                rows.push_str(&format!(
                    "<tr data-status=\"{}\" data-suite=\"{}\" data-agent=\"{}\"><td><button class=\"case-toggle\" type=\"button\" aria-expanded=\"false\">{}</button><div class=\"case-note\" hidden>{}</div></td><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td><span class=\"stamp {}\">{:?}</span></td><td>{}</td><td class=\"num\">{}</td><td>{}</td><td class=\"links\"><a href=\"{}\">result</a><a href=\"{}\">trajectory</a><a href=\"{}\">stdout</a><a href=\"{}\">stderr</a></td></tr>",
                    status_filter_value(case.status),
                    escape_html(&case.suite_id),
                    escape_html(&case.agent_id),
                    escape_html(&case.case_id),
                    escape_html(&truncate_text(&case.score.message, 220)),
                    escape_html(&case.suite_id),
                    escape_html(&case.task_id),
                    escape_html(&case.task_family),
                    escape_html(&case.agent_id),
                    status_class(case.status),
                    case.status,
                    escape_html(case.failure_class.as_deref().unwrap_or("-")),
                    case.score.score.unwrap_or_default(),
                    escape_html(&scorer_details_label(&case.score.details)),
                    escape_html(&case.artifacts.result.to_string_lossy()),
                    escape_html(&case.artifacts.trajectory.to_string_lossy()),
                    escape_html(&case.artifacts.scorer_stdout.to_string_lossy()),
                    escape_html(&case.artifacts.scorer_stderr.to_string_lossy()),
                ));
            }
            Ok(format!(
                "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\"><title>peval {}</title>{}</head><body><main class=\"page\"><section class=\"mast\"><div><p class=\"eyebrow\">Psychevo evaluation report</p><h1>{}</h1><p class=\"subline\">{} · artifacts at <code>{}</code></p></div><div class=\"verdict {}\">{:?}</div></section><section class=\"metrics\"><div><span>{}</span><strong>{:?}</strong></div><div><span>passed</span><strong>{}</strong></div><div><span>failed</span><strong>{}</strong></div><div><span>total</span><strong>{}</strong></div></section><section class=\"toolbar\"><button type=\"button\" data-filter=\"all\">all</button><button type=\"button\" data-filter=\"failed\">exceptions</button><input type=\"search\" id=\"caseSearch\" placeholder=\"filter cases\"></section><section class=\"ledger\"><table id=\"caseTable\"><thead><tr><th>case</th><th>suite</th><th>task</th><th>family</th><th>agent</th><th>status</th><th>failure</th><th>score</th><th>scorer</th><th>artifacts</th></tr></thead><tbody>{}</tbody></table></section></main>{}</body></html>",
                escape_html(&summary.run_id),
                report_css(),
                escape_html(&summary.run_id),
                escape_html(&summary.project),
                escape_html(&summary.artifact_root.to_string_lossy()),
                status_class_for_run(summary.status),
                summary.status,
                "status",
                summary.status,
                summary.passed_cases,
                summary.failed_cases,
                summary.total_cases,
                rows,
                report_js(),
            ))
        }
    }
}

pub(crate) fn compare_key(case: &CaseResult) -> String {
    format!("{}/{}/{}", case.suite_id, case.task_id, case.agent_id)
}

pub(crate) fn render_compare(report: &CompareReport, json_output: bool) -> Result<String> {
    if json_output {
        return Ok(serde_json::to_string_pretty(report)?);
    }
    let mut out = String::new();
    out.push_str("peval compare\n");
    for run in &report.runs {
        out.push_str(&format!("- {}: {:?}\n", run.run_id, run.status));
    }
    for case in &report.cases {
        out.push_str(&format!("{}:", case.key));
        for (run, status) in &case.statuses {
            out.push_str(&format!(" {}={:?}", run, status));
        }
        out.push('\n');
    }
    Ok(out)
}

pub(crate) fn render_replay(report: &ReplayReport, json_output: bool) -> Result<String> {
    if json_output {
        return Ok(serde_json::to_string_pretty(report)?);
    }
    let mut out = String::new();
    out.push_str(&format!("peval replay {}\n", report.run_id));
    for event in &report.events {
        out.push_str(&format!(
            "{:04} {} {} - {}\n",
            event.sequence, event.case_id, event.kind, event.message
        ));
    }
    Ok(out)
}

pub(crate) fn render_store_dashboard(
    store: &EvalStore,
    runs: &[RunIndexEntry],
    datasets: &[DatasetEntry],
) -> String {
    let latest = runs.first();
    let mut run_rows = String::new();
    for run in runs {
        let report_link = store_link(store, &run.report_html);
        run_rows.push_str(&format!(
            "<tr data-status=\"{}\" data-suite=\"{}\" data-agent=\"{}\" data-dataset=\"{}\"><td><a href=\"{}\">{}</a><div class=\"muted\">{}</div></td><td>{}</td><td><span class=\"stamp {}\">{:?}</span></td><td class=\"num\">{}/{}</td><td>{}</td><td>{}</td><td><label class=\"pick\"><input type=\"checkbox\" value=\"{}\"> compare</label></td></tr>",
            status_filter_value_for_run(run.status),
            escape_html(&run.suites.join(" ")),
            escape_html(&run.agents.join(" ")),
            escape_html(&run.suites.join(" ")),
            escape_html(&report_link),
            escape_html(&run.run_id),
            escape_html(&run.project),
            escape_html(&run.suites.join(", ")),
            status_class_for_run(run.status),
            run.status,
            run.passed_cases,
            run.total_cases,
            escape_html(&run.agents.join(", ")),
            escape_html(&store_link(store, &run.artifact_root)),
            escape_html(&run.run_id),
        ));
    }

    let mut dataset_rows = String::new();
    for dataset in datasets {
        let exists = if dataset.payload_exists {
            "present"
        } else {
            "missing"
        };
        dataset_rows.push_str(&format!(
            "<tr data-dataset=\"{}\"><td><button class=\"dataset-filter\" type=\"button\" data-dataset=\"{}\">{}</button><div class=\"muted\">{}</div></td><td>{}</td><td>{}</td><td><span class=\"stamp {}\">{}</span></td><td>{}</td></tr>",
            escape_html(&dataset.id),
            escape_html(&dataset.id),
            escape_html(&dataset.name),
            escape_html(&dataset.id),
            escape_html(&dataset.kind),
            escape_html(dataset.split.as_deref().unwrap_or("")),
            exists,
            exists,
            escape_html(&dataset.payload.to_string_lossy()),
        ));
    }

    let latest_line = latest
        .map(|run| {
            format!(
                "<a href=\"{}\">latest: {}</a>",
                escape_html(&store_link(store, &run.report_html)),
                escape_html(&run.run_id)
            )
        })
        .unwrap_or_else(|| "latest: none".to_string());
    let failed_runs = runs
        .iter()
        .filter(|run| run.status == RunStatus::Failed)
        .count();
    let passed_runs = runs.len().saturating_sub(failed_runs);
    format!(
        "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\"><title>peval dashboard</title>{}</head><body><main class=\"page\"><section class=\"mast\"><div><p class=\"eyebrow\">Psychevo evaluation ledger</p><h1>Evaluation results center</h1><p class=\"subline\">{} · root <code>{}</code></p></div><div class=\"verdict\">{}</div></section><section class=\"metrics\"><div><span>runs</span><strong>{}</strong></div><div><span>passed</span><strong>{}</strong></div><div><span>exceptions</span><strong>{}</strong></div><div><span>datasets</span><strong>{}</strong></div></section><section class=\"toolbar\"><button type=\"button\" data-filter=\"all\">all runs</button><button type=\"button\" data-filter=\"failed\">exceptions</button><button type=\"button\" id=\"comparePicked\">compare picked</button><input type=\"search\" id=\"caseSearch\" placeholder=\"filter ledger\"></section><section class=\"ledger\"><h2>Run ledger</h2><table id=\"caseTable\"><thead><tr><th>run</th><th>suites</th><th>status</th><th>pass</th><th>agents</th><th>artifact root</th><th>compare</th></tr></thead><tbody>{}</tbody></table></section><section class=\"ledger\"><h2>Dataset inventory</h2><table id=\"datasetTable\"><thead><tr><th>dataset</th><th>kind</th><th>split</th><th>payload</th><th>path</th></tr></thead><tbody>{}</tbody></table></section></main>{}</body></html>",
        report_css(),
        latest_line,
        escape_html(&store.root.to_string_lossy()),
        "Editorial Lab Report",
        runs.len(),
        passed_runs,
        failed_runs,
        datasets.len(),
        run_rows,
        dataset_rows,
        report_js(),
    )
}

pub(crate) fn run_index_entry(
    summary: &RunSummary,
    artifact_root: &Path,
    store_root: &Path,
) -> RunIndexEntry {
    let suites = sorted_unique(summary.cases.iter().map(|case| case.suite_id.clone()));
    let agents = sorted_unique(summary.cases.iter().map(|case| case.agent_id.clone()));
    let namespace = artifact_root
        .strip_prefix(store_root)
        .ok()
        .and_then(|relative| relative.parent())
        .filter(|parent| !parent.as_os_str().is_empty())
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("runs").join(slugify(&summary.project)));
    RunIndexEntry {
        schema_version: SCHEMA_VERSION,
        project: summary.project.clone(),
        project_slug: slugify(&summary.project),
        namespace,
        run_id: summary.run_id.clone(),
        artifact_root: artifact_root.to_path_buf(),
        report_html: artifact_root.join("report.html"),
        report_markdown: artifact_root.join("report.md"),
        started_at_ms: summary.started_at_ms,
        finished_at_ms: summary.finished_at_ms,
        total_cases: summary.total_cases,
        passed_cases: summary.passed_cases,
        failed_cases: summary.failed_cases,
        status: summary.status,
        suites,
        agents,
    }
}

pub(crate) fn read_dataset_entry(path: &Path) -> Result<DatasetEntry> {
    let manifest: DatasetManifest = read_toml(path)?;
    reject_unsupported(manifest.schema_version, path)?;
    let dataset_dir = path
        .parent()
        .with_context(|| format!("dataset manifest has no parent: {}", path.display()))?;
    let payload = resolve_relative(dataset_dir, &manifest.payload);
    Ok(DatasetEntry {
        schema_version: manifest.schema_version,
        id: manifest.id,
        name: manifest.name,
        kind: manifest.kind,
        source: manifest.source,
        payload_exists: payload.exists(),
        payload,
        manifest_path: path.to_path_buf(),
        loader: manifest.loader,
        split: manifest.split,
        sample_limit: manifest.sample_limit,
        cache_key: manifest.cache_key,
        license: manifest.license,
        tags: manifest.tags,
        notes: manifest.notes,
    })
}

pub(crate) fn sorted_unique<I>(values: I) -> Vec<String>
where
    I: IntoIterator<Item = String>,
{
    values
        .into_iter()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

pub(crate) fn status_filter_value(status: CaseStatus) -> &'static str {
    match status {
        CaseStatus::Passed => "passed",
        CaseStatus::Failed
        | CaseStatus::SetupFailed
        | CaseStatus::RuntimeFailed
        | CaseStatus::ScorerFailed
        | CaseStatus::Timeout => "failed",
    }
}

pub(crate) fn status_filter_value_for_run(status: RunStatus) -> &'static str {
    match status {
        RunStatus::Passed => "passed",
        RunStatus::Failed => "failed",
    }
}

pub(crate) fn status_class(status: CaseStatus) -> &'static str {
    match status {
        CaseStatus::Passed => "present",
        CaseStatus::Failed => "missing",
        CaseStatus::SetupFailed
        | CaseStatus::RuntimeFailed
        | CaseStatus::ScorerFailed
        | CaseStatus::Timeout => "failed",
    }
}

pub(crate) fn status_class_for_run(status: RunStatus) -> &'static str {
    match status {
        RunStatus::Passed => "present",
        RunStatus::Failed => "failed",
    }
}

pub(crate) fn store_link(store: &EvalStore, path: &Path) -> String {
    path.strip_prefix(&store.root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

pub(crate) fn truncate_text(value: &str, limit: usize) -> String {
    if value.chars().count() <= limit {
        return value.to_string();
    }
    let mut out = value
        .chars()
        .take(limit.saturating_sub(1))
        .collect::<String>();
    out.push_str("...");
    out
}

pub(crate) fn scorer_details_label(details: &Value) -> String {
    if details.is_null() {
        return "-".to_string();
    }
    if let Some(scorer) = details.get("scorer").and_then(Value::as_str) {
        return scorer.to_string();
    }
    truncate_text(&details.to_string(), 80)
}

pub(crate) fn report_css() -> &'static str {
    r#"<style>
:root{color-scheme:light;--ink:#201d1a;--muted:#706960;--paper:#f4efe6;--panel:#fffaf0;--line:rgba(32,29,26,.12);--accent:#7f2f22;--ok:#2f6b43;--bad:#9b2d24;--warn:#8a5d14}
*{box-sizing:border-box}body{margin:0;background:var(--paper);color:var(--ink);font:14px/1.5 ui-sans-serif,system-ui,-apple-system,BlinkMacSystemFont,"Segoe UI",sans-serif;-webkit-font-smoothing:antialiased}button,input{font:inherit}a{color:var(--accent);text-decoration:none}a:hover{text-decoration:underline}.page{max-width:1180px;margin:0 auto;padding:32px 20px 56px}.mast{display:flex;align-items:flex-end;justify-content:space-between;gap:24px;padding:24px 0 18px;border-bottom:2px solid var(--ink)}.eyebrow{margin:0 0 8px;color:var(--accent);font-weight:700}.mast h1{margin:0;font-family:Georgia,"Times New Roman",serif;font-size:clamp(34px,6vw,72px);line-height:.95;letter-spacing:0}.subline{margin:12px 0 0;color:var(--muted);max-width:820px}.verdict{min-width:150px;padding:14px 16px;border-radius:8px;background:var(--ink);color:var(--panel);text-align:center;font-family:Georgia,"Times New Roman",serif;font-size:20px}.metrics{display:grid;grid-template-columns:repeat(4,minmax(0,1fr));gap:14px;margin:18px 0}.metrics div{background:var(--panel);border-radius:8px;padding:16px 18px;box-shadow:0 1px 4px rgba(32,29,26,.14)}.metrics span{display:block;color:var(--muted)}.metrics strong{display:block;margin-top:8px;font:700 28px/1 Georgia,"Times New Roman",serif;font-variant-numeric:tabular-nums}.toolbar{display:flex;gap:10px;align-items:center;margin:22px 0;flex-wrap:wrap}.toolbar button,.case-toggle,.dataset-filter{min-height:40px;border:0;border-radius:8px;background:var(--ink);color:var(--panel);padding:9px 13px;cursor:pointer;transition:transform .14s ease,opacity .14s ease}.toolbar button:active,.case-toggle:active,.dataset-filter:active{transform:scale(.96)}.toolbar input{min-height:40px;min-width:240px;border:0;border-radius:8px;background:var(--panel);padding:9px 12px;box-shadow:inset 0 0 0 1px var(--line)}.ledger{margin-top:22px;background:var(--panel);border-radius:8px;padding:16px;box-shadow:0 1px 4px rgba(32,29,26,.14);overflow:auto}.ledger h2{font-family:Georgia,"Times New Roman",serif;margin:0 0 12px;font-size:24px;letter-spacing:0}table{width:100%;border-collapse:collapse;min-width:760px}th,td{padding:11px 12px;border-bottom:1px solid var(--line);text-align:left;vertical-align:top}th{color:var(--muted);font-weight:700}td.num{text-align:right;font-variant-numeric:tabular-nums}.muted{color:var(--muted);font-size:12px}.stamp{display:inline-flex;align-items:center;min-height:26px;border-radius:6px;padding:3px 8px;font-weight:700}.stamp.present{background:rgba(47,107,67,.12);color:var(--ok)}.stamp.missing,.stamp.failed{background:rgba(155,45,36,.12);color:var(--bad)}.links{white-space:nowrap}.links a{margin-right:10px}.case-note{margin-top:8px;color:var(--muted);max-width:420px}.pick{white-space:nowrap;color:var(--muted)}@media(max-width:760px){.mast{display:block}.verdict{margin-top:18px}.metrics{grid-template-columns:repeat(2,minmax(0,1fr))}.page{padding:18px 12px 36px}.toolbar input{width:100%;min-width:0}}
</style>"#
}

pub(crate) fn report_js() -> &'static str {
    r#"<script>
(function(){
  const table=document.getElementById('caseTable');
  const search=document.getElementById('caseSearch');
  const apply=()=>{if(!table)return;const q=(search&&search.value||'').toLowerCase();const active=document.body.dataset.filter||'all';table.querySelectorAll('tbody tr').forEach(row=>{const text=row.textContent.toLowerCase();const status=row.dataset.status||'';row.hidden=(active==='failed'&&status!=='failed')||(q&&!text.includes(q));});};
  document.querySelectorAll('[data-filter]').forEach(btn=>btn.addEventListener('click',()=>{document.body.dataset.filter=btn.dataset.filter;apply();}));
  if(search)search.addEventListener('input',apply);
  document.querySelectorAll('.case-toggle').forEach(btn=>btn.addEventListener('click',()=>{const note=btn.parentElement.querySelector('.case-note');const open=btn.getAttribute('aria-expanded')==='true';btn.setAttribute('aria-expanded',String(!open));if(note)note.hidden=open;}));
  document.querySelectorAll('.dataset-filter').forEach(btn=>btn.addEventListener('click',()=>{if(search){search.value=btn.dataset.dataset||'';apply();}}));
  const compare=document.getElementById('comparePicked');
  if(compare)compare.addEventListener('click',()=>{const picks=[...document.querySelectorAll('.pick input:checked')].map(input=>input.value); if(picks.length>=2){compare.textContent='compare: '+picks.slice(0,2).join(' vs ');} else {compare.textContent='pick two runs';}});
  apply();
})();
</script>"#
}

pub(crate) fn escape_html(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

pub(crate) fn resolve_store_root(explicit: Option<PathBuf>) -> Result<PathBuf> {
    let env_map = inherited_env();
    let cwd = env::current_dir()?;
    if let Some(path) = explicit {
        return resolve_explicit_path(&path, &env_map, &cwd);
    }
    if let Some(value) = env_value("PEVAL_ROOT", &env_map) {
        return resolve_explicit_path(Path::new(&value), &env_map, &cwd);
    }

    let config_path = resolve_psychevo_home(&env_map, &cwd)?.join("peval.toml");
    if !config_path.is_file() {
        bail!(
            "peval is not initialized; run `peval init` to create {} or pass --root/PEVAL_ROOT",
            config_path.display()
        );
    }
    let config = read_peval_config(&config_path)?;
    if config.root.is_absolute() {
        Ok(config.root)
    } else {
        let base = config_path.parent().unwrap_or_else(|| Path::new("."));
        Ok(base.join(config.root))
    }
}

pub(crate) fn read_peval_config(path: &Path) -> Result<PevalConfig> {
    let config: PevalConfig = read_toml(path)?;
    reject_unsupported(config.schema_version, path)?;
    Ok(config)
}

pub(crate) fn resolve_psychevo_home(
    env_map: &BTreeMap<String, String>,
    cwd: &Path,
) -> Result<PathBuf> {
    if let Some(value) = env_value("PSYCHEVO_HOME", env_map) {
        resolve_explicit_path(Path::new(&value), env_map, cwd)
    } else {
        resolve_explicit_path(Path::new("~/.psychevo"), env_map, cwd)
    }
}

pub(crate) fn resolve_cli_path(path: &Path) -> Result<PathBuf> {
    let env_map = inherited_env();
    resolve_explicit_path(path, &env_map, &env::current_dir()?)
}

pub(crate) fn resolve_explicit_path(
    path: &Path,
    env_map: &BTreeMap<String, String>,
    cwd: &Path,
) -> Result<PathBuf> {
    let expanded = expand_tilde(path, env_map)?;
    if expanded.is_absolute() {
        Ok(expanded)
    } else {
        Ok(cwd.join(expanded))
    }
}

pub(crate) fn expand_tilde(path: &Path, env_map: &BTreeMap<String, String>) -> Result<PathBuf> {
    let raw = path.to_string_lossy();
    if raw == "~" {
        return home_path(env_map);
    }
    if let Some(rest) = raw.strip_prefix("~/") {
        return Ok(home_path(env_map)?.join(rest));
    }
    Ok(path.to_path_buf())
}

pub(crate) fn home_path(env_map: &BTreeMap<String, String>) -> Result<PathBuf> {
    env_value("HOME", env_map)
        .map(PathBuf::from)
        .ok_or_else(|| anyhow::anyhow!("HOME is required to expand ~"))
}

pub(crate) fn env_value(name: &str, env_map: &BTreeMap<String, String>) -> Option<String> {
    env_map
        .get(name)
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

pub(crate) fn inherited_env() -> BTreeMap<String, String> {
    env::vars().collect()
}

pub(crate) fn validate_store_namespace(path: &Path) -> Result<PathBuf> {
    if path.as_os_str().is_empty() {
        bail!("output_root must not be empty");
    }
    if path.is_absolute() {
        bail!("output_root must be relative to the peval root");
    }
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Normal(part) => out.push(part),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                bail!("output_root must not escape the peval root")
            }
        }
    }
    if out.as_os_str().is_empty() {
        bail!("output_root must name a store namespace");
    }
    Ok(out)
}

pub(crate) fn resolve_relative(base: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        base.join(path)
    }
}

pub(crate) fn resolve_command_part(part: &str, task_dir: &Path) -> OsString {
    let candidate = task_dir.join(part);
    if candidate.exists() {
        return absolute_path(&candidate).into_os_string();
    }
    if looks_like_relative_path(part) {
        absolute_path(&resolve_relative(task_dir, Path::new(part))).into_os_string()
    } else {
        OsString::from(part)
    }
}

pub(crate) fn absolute_path(path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(path)
    }
}
