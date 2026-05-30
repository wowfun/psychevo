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

pub(crate) fn read_toml<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<T> {
    let raw =
        fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))?;
    toml::from_str(&raw).with_context(|| format!("failed to parse {}", path.display()))
}

pub(crate) fn discover_manifest(start: &Path) -> Result<PathBuf> {
    let mut current = if start.is_file() {
        return Ok(start.to_path_buf());
    } else {
        start.to_path_buf()
    };
    loop {
        let candidate = current.join("eval.toml");
        if candidate.is_file() {
            return Ok(candidate);
        }
        let candidates = discover_eval_template_candidates(&current)?;
        match candidates.as_slice() {
            [candidate] => return Ok(candidate.clone()),
            [] => {}
            _ => {
                let names = candidates
                    .iter()
                    .map(|path| path.display().to_string())
                    .collect::<Vec<_>>()
                    .join(", ");
                bail!(
                    "multiple eval config TOML files found in {}; pass --config <path> explicitly: {}",
                    current.display(),
                    names
                );
            }
        }
        if !current.pop() {
            break;
        }
    }
    bail!("could not find eval config TOML from {}", start.display())
}

pub(crate) fn discover_eval_template_candidates(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut candidates = Vec::new();
    if !dir.is_dir() {
        return Ok(candidates);
    }
    for entry in fs::read_dir(dir).with_context(|| format!("failed to read {}", dir.display()))? {
        let entry = entry?;
        if !entry.file_type()?.is_file() {
            continue;
        }
        let path = entry.path();
        if path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.ends_with(".eval.toml"))
        {
            candidates.push(path);
        }
    }
    candidates.sort();
    Ok(candidates)
}

pub(crate) fn discover_benchmark_manifest(start: &Path) -> Result<PathBuf> {
    let mut current = if start.is_file() {
        if start
            .file_name()
            .is_some_and(|name| name == "benchmark.toml")
        {
            return Ok(start.to_path_buf());
        }
        start.parent().unwrap_or(start).to_path_buf()
    } else {
        start.to_path_buf()
    };
    loop {
        let candidate = current.join("benchmark.toml");
        if candidate.is_file() {
            return Ok(candidate);
        }
        if !current.pop() {
            break;
        }
    }
    bail!("could not find benchmark.toml from {}", start.display())
}

pub(crate) fn load_project_from_selection(
    config: Option<&Path>,
    benchmark: Option<&str>,
    store_root: Option<PathBuf>,
) -> Result<EvalProject> {
    if let Some(path) = config {
        return load_eval_config(&resolve_cli_path(path)?, store_root);
    }
    if let Some(benchmark) = benchmark {
        return load_one_off_benchmark(benchmark, store_root);
    }
    EvalProject::load(env::current_dir()?)
}

pub(crate) fn task_prompt(task: &TaskManifest) -> Result<String> {
    Ok(task.problem_statement.clone())
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
        schema_version: ARTIFACT_SCHEMA_VERSION,
        sequence: events.len() as u64,
        case_id: case_id.to_string(),
        kind: kind.to_string(),
        message: message.to_string(),
        timestamp_ms: Some(now_ms()),
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

pub(crate) fn read_cell_run(root: &Path) -> Result<CellRun> {
    let path = root.join("run.json");
    let raw = fs::read_to_string(&path)
        .with_context(|| format!("failed to read cell run {}", path.display()))?;
    let schema = read_json_schema_version(&raw, &path)?;
    reject_unsupported_artifact(schema, &path)?;
    let mut cell: CellRun = serde_json::from_str(&raw)
        .with_context(|| format!("failed to parse cell run {}", path.display()))?;
    cell.cell_root = root.to_path_buf();
    cell.case.artifacts.result = PathBuf::from("run.json");
    Ok(cell)
}

#[derive(Debug, Deserialize)]
pub(crate) struct JsonSchemaVersion {
    pub schema_version: u32,
}

pub(crate) fn read_json_schema_version(raw: &str, path: &Path) -> Result<u32> {
    let schema: JsonSchemaVersion = serde_json::from_str(raw)
        .with_context(|| format!("failed to parse schema_version in {}", path.display()))?;
    Ok(schema.schema_version)
}

pub(crate) fn read_dataset_entry(path: &Path) -> Result<DatasetEntry> {
    let manifest: DatasetManifest = read_toml(path)?;
    reject_unsupported_index(manifest.schema_version, path)?;
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

pub(crate) fn report_css() -> &'static str {
    r#"<style>
:root{color-scheme:light;--canvas:oklch(96.4% .008 105);--rail:oklch(91.2% .014 105);--surface:oklch(99% .003 105);--surface-2:oklch(94.3% .012 105);--ink:oklch(19% .018 88);--muted:oklch(48% .018 88);--rule:oklch(80% .014 88);--focus:oklch(37% .052 219);--pass-1:oklch(78% .050 151);--pass-2:oklch(69% .066 151);--pass-3:oklch(60% .086 151);--pass-4:oklch(51% .104 151);--pass-5:oklch(42% .116 151);--fail-1:oklch(79% .070 31);--fail-2:oklch(69% .100 31);--fail-3:oklch(59% .132 31);--fail-4:oklch(50% .160 31);--fail-5:oklch(41% .176 31);--timeout-1:oklch(84% .064 76);--timeout-2:oklch(76% .086 76);--timeout-3:oklch(68% .106 76);--timeout-4:oklch(60% .124 76);--timeout-5:oklch(52% .130 76);--mono:ui-monospace,SFMono-Regular,Menlo,Consolas,"Liberation Mono",monospace;--serif:Georgia,"Times New Roman",serif;--sans:system-ui,-apple-system,BlinkMacSystemFont,"Segoe UI",sans-serif;--radius:8px}
*{box-sizing:border-box}
[hidden]{display:none!important}
html{scroll-behavior:smooth}
body{margin:0;background:var(--canvas);color:var(--ink);font:15px/1.55 var(--sans);-webkit-font-smoothing:antialiased}
button,input,select{font:inherit}button{min-height:38px;cursor:pointer;touch-action:manipulation;transition:transform 140ms cubic-bezier(.16,1,.3,1),box-shadow 140ms}button:active{transform:scale(.97)}
button:focus-visible,input:focus-visible,select:focus-visible,summary:focus-visible{outline:2px solid var(--focus);outline-offset:2px}
a{color:var(--focus);text-decoration:none;border-bottom:1px solid color-mix(in oklch,var(--focus),transparent 62%)}
.workspace{min-height:100vh}.main{width:min(1480px,100%);min-width:0;margin:0 auto;padding:28px}
.topline{display:grid;grid-template-columns:minmax(0,1fr) auto;gap:22px;align-items:start;margin-bottom:22px}.topline>*{min-width:0}.top-actions{display:grid;gap:10px;justify-items:end}.eyebrow{margin:0 0 7px;color:var(--muted);font:700 12px/1 var(--mono);letter-spacing:.08em;text-transform:uppercase;overflow-wrap:anywhere}.report-notes{margin-top:12px}.report-note-list{display:grid;gap:8px;max-width:860px}.report-note{border-left:4px solid var(--focus);border-radius:var(--radius);background:color-mix(in oklch,var(--surface),var(--surface-2) 32%);padding:10px 12px;box-shadow:0 1px 3px rgba(49,42,25,.08)}.report-note strong{font:700 12px/1 var(--mono);color:var(--muted);text-transform:uppercase;letter-spacing:.04em}
h1,h2,h3{margin:0;font-family:var(--serif);letter-spacing:0;text-wrap:balance;overflow-wrap:anywhere}h1{font-size:40px;line-height:1}h2{font-size:25px}h3{font-size:20px}.copy{margin:10px 0 0;color:var(--muted);font-size:14px;line-height:1.6;max-width:760px;overflow-wrap:anywhere}
.score-strip{display:grid;grid-template-columns:repeat(3,104px);gap:8px}.score-box,.panel,.trace-panel{background:var(--surface);border-radius:var(--radius);box-shadow:0 18px 42px rgba(49,42,25,.10),0 2px 6px rgba(49,42,25,.08)}.score-box{padding:12px}.score-box strong{display:block;font:700 24px/1 var(--mono);font-variant-numeric:tabular-nums}.score-box span{display:block;margin-top:7px;color:var(--muted);font:700 11px/1 var(--mono);text-transform:uppercase;letter-spacing:.05em}
.serve-actions{display:flex;gap:8px}.serve-actions button{border:0;border-radius:var(--radius);background:var(--ink);color:var(--surface);padding:0 12px}.serve-actions button:disabled{opacity:.45;cursor:not-allowed}
.input,.select{min-height:42px;width:100%;border:1px solid var(--rule);border-radius:var(--radius);background:var(--surface);color:var(--ink);padding:0 12px;box-shadow:0 1px 2px rgba(49,42,25,.06)}
.panel,.trace-panel{padding:18px}.trace-panel{margin-top:18px}.panel-head,.trace-head{display:grid;grid-template-columns:minmax(0,1fr) auto;gap:18px;align-items:start;margin-bottom:14px}.trace-title{display:flex;align-items:baseline;gap:10px;flex-wrap:wrap}.trace-title code{font:700 22px/1 var(--mono);color:var(--ink);overflow-wrap:anywhere}.metric-controls{display:flex;align-items:center;gap:8px}.segmented{display:inline-flex;background:var(--surface-2);border-radius:999px;padding:4px;box-shadow:inset 0 0 0 1px rgba(51,44,27,.08)}.metric-button{border:0;min-height:32px;border-radius:999px;background:transparent;color:var(--muted);padding:0 11px;font:700 12px/1 var(--mono);white-space:nowrap}.metric-button.active{background:var(--surface);color:var(--ink);box-shadow:0 1px 3px rgba(49,42,25,.12)}
.matrix-scroll{overflow-x:auto;padding-bottom:4px}.matrix{display:grid;gap:8px;align-items:stretch;min-width:680px}.axis-head,.task-axis{min-height:46px;display:flex;align-items:center;color:var(--muted);font:12px/1.3 var(--mono)}.agent-head{min-height:46px;border-bottom:1px solid var(--rule);display:flex;align-items:end;padding:0 4px 10px;color:var(--muted);font:12px/1.3 var(--mono)}.task-axis{border-right:1px solid var(--rule);padding-right:12px}
.cell{min-height:106px;width:100%;border:0;border-radius:var(--radius);padding:12px;text-align:left;color:#fff;box-shadow:inset 0 0 0 1px rgba(255,255,255,.22),0 1px 2px rgba(49,42,25,.12)}.cell strong{display:block;font:700 30px/1 var(--mono);font-variant-numeric:tabular-nums}.cell span{display:block;margin-top:8px;color:rgba(255,255,255,.88);font:12px/1.35 var(--mono)}.cell.selected{box-shadow:inset 0 0 0 3px #fff,0 0 0 2px var(--focus),0 8px 18px rgba(49,42,25,.18)}.cell.empty{color:var(--muted);background:repeating-linear-gradient(135deg,var(--surface-2),var(--surface-2) 8px,var(--surface) 8px,var(--surface) 16px);box-shadow:inset 0 0 0 1px var(--rule)}.cell.empty span{color:var(--muted)}.cell.missing-metric strong{opacity:.92}
.passed.shade-1{background:var(--pass-1)}.passed.shade-2{background:var(--pass-2)}.passed.shade-3{background:var(--pass-3)}.passed.shade-4{background:var(--pass-4)}.passed.shade-5{background:var(--pass-5)}.failed.shade-1{background:var(--fail-1)}.failed.shade-2{background:var(--fail-2)}.failed.shade-3{background:var(--fail-3)}.failed.shade-4{background:var(--fail-4)}.failed.shade-5{background:var(--fail-5)}.timeout.shade-1{background:var(--timeout-1);color:var(--ink)}.timeout.shade-2{background:var(--timeout-2);color:var(--ink)}.timeout.shade-3{background:var(--timeout-3);color:var(--ink)}.timeout.shade-4{background:var(--timeout-4);color:var(--ink)}.timeout.shade-5{background:var(--timeout-5);color:#fff}
.trace-meta,.refs{display:flex;flex-wrap:wrap;gap:8px;margin-top:10px}.refs{margin:12px 0 18px}.stamp,.chip,.ref-token{display:inline-flex;align-items:center;min-height:24px;border-radius:999px;padding:0 9px;background:var(--surface-2);color:var(--muted);font:700 11px/1 var(--mono);white-space:nowrap}.tool-name-chip{gap:6px;background:oklch(91% .034 205);color:oklch(31% .070 220);box-shadow:0 4px 10px rgba(36,88,125,.14)}.tool-exec-inline{color:oklch(24% .032 82);font-variant-numeric:tabular-nums}.stamp.passed{color:oklch(30% .092 151);background:oklch(91% .035 151)}.stamp.failed{color:oklch(33% .126 31);background:oklch(92% .039 31)}.stamp.timeout{color:oklch(34% .080 76);background:oklch(92% .043 76)}
.info-grid{display:grid;grid-template-columns:repeat(4,minmax(0,1fr));gap:8px;margin:12px 0 18px}.info-grid div{min-width:0;border-top:1px solid var(--rule);padding-top:9px}.info-grid span{display:block;color:var(--muted);font:700 11px/1 var(--mono);letter-spacing:.05em;text-transform:uppercase}.info-grid strong{display:block;margin-top:6px;font:700 14px/1.3 var(--mono);overflow-wrap:anywhere;font-variant-numeric:tabular-nums}
.steps-head{display:flex;align-items:center;justify-content:space-between;gap:12px;margin-top:18px;flex-wrap:wrap}.steps-head h3{margin:0}.step-actions{display:flex;align-items:center;gap:8px;flex-wrap:wrap;justify-content:flex-end}.step-toggle-button{min-height:30px;border:1px solid color-mix(in oklch,var(--rule),transparent 12%);border-radius:999px;background:var(--surface);color:var(--ink);padding:0 11px;font:700 11px/1 var(--mono);text-transform:uppercase;box-shadow:0 4px 10px rgba(49,42,25,.10)}.step-toggle-button:hover:not(:disabled){background:var(--surface-2);border-color:color-mix(in oklch,var(--focus),transparent 58%)}.step-toggle-button:active:not(:disabled){transform:scale(.97)}.step-toggle-button:disabled{opacity:.45;cursor:not-allowed}.step-list{display:grid;gap:8px;margin-top:12px}.step{border:1px solid var(--rule);border-radius:var(--radius);background:color-mix(in oklch,var(--surface),var(--surface-2) 26%);overflow:hidden}.step[open],.step.selected-step{background:var(--surface);box-shadow:0 8px 20px rgba(49,42,25,.08)}.step>summary{display:grid;grid-template-columns:minmax(0,1fr) minmax(260px,auto);gap:14px;align-items:center;min-height:64px;padding:12px;cursor:pointer;list-style:none}.step>summary::-webkit-details-marker{display:none}.step-row{display:grid;grid-template-columns:48px 74px minmax(0,1fr);gap:10px;align-items:center;min-width:0}.step-id,.role{font:700 11px/1 var(--mono);white-space:nowrap}.step-id{color:var(--muted)}.role{border-radius:999px;min-height:24px;display:inline-flex;align-items:center;justify-content:center;background:var(--surface-2);color:var(--muted);text-transform:uppercase}.role.system{color:oklch(34% .065 285);background:oklch(91% .030 285)}.role.user{color:oklch(32% .063 219);background:oklch(91% .030 219)}.role.agent{color:oklch(29% .084 151);background:oklch(91% .035 151)}.preview{min-width:0;font-size:13px;line-height:1.35;white-space:nowrap;overflow:hidden;text-overflow:ellipsis}.rail{min-width:0;display:flex;align-items:center;justify-content:space-between;gap:8px;color:var(--muted);font:11px/1 var(--mono);white-space:nowrap}.rail:empty{display:none}.rail-tools,.rail-time{display:flex;align-items:center;gap:6px;min-width:0}.rail-tools{justify-content:flex-start;overflow:hidden}.rail-time{justify-content:flex-end;margin-left:auto;flex:0 0 auto}.rail-chip{display:inline-flex;align-items:center;gap:6px;min-height:24px;border:1px solid color-mix(in oklch,var(--rule),transparent 32%);border-radius:999px;padding:0 8px;background:var(--surface);font:700 11px/1 var(--mono);white-space:nowrap}.rail-chip-tools{color:oklch(28% .072 157);background:oklch(93% .038 157);box-shadow:0 5px 12px rgba(24,98,69,.18)}.rail-chip-tool-list{min-width:0;max-width:260px;overflow:hidden;text-overflow:ellipsis;color:oklch(31% .070 220);background:oklch(91% .034 205);box-shadow:0 5px 12px rgba(36,88,125,.16)}.rail-chip-tokens{color:var(--muted);background:color-mix(in oklch,var(--surface),var(--surface-2) 42%);box-shadow:0 4px 10px rgba(49,42,25,.10)}.rail-chip-step-time{color:oklch(30% .052 62);background:oklch(94% .036 75);box-shadow:0 5px 12px rgba(120,82,26,.18)}.rail-chip-elapsed-time{color:oklch(27% .034 245);background:oklch(94% .025 245);box-shadow:0 5px 12px rgba(58,70,118,.16)}.step-body{border-top:1px solid var(--rule);padding:12px;display:grid;gap:10px}
.block{border-radius:var(--radius);background:var(--surface-2);padding:12px;border-left:4px solid var(--rule)}.block summary{cursor:pointer;color:var(--ink);font-weight:650;list-style:none}.block summary::-webkit-details-marker{display:none}.block h4{margin:0 0 8px;font:700 13px/1 var(--mono);letter-spacing:.05em;text-transform:uppercase}pre{margin:8px 0 0;max-height:260px;overflow:auto;white-space:pre-wrap;overflow-wrap:anywhere;color:var(--ink);font:13px/1.5 var(--mono)}.message-block{border-left-color:oklch(55% .055 219)}.reasoning-block{border-left-color:oklch(48% .070 285)}.tool-block{border-left-color:oklch(49% .105 235);background:oklch(94% .026 235)}.observation-block{border-left-color:oklch(51% .095 151);background:oklch(94% .030 151)}.danger{color:oklch(42% .160 31)}.muted{color:var(--muted)}
.path-block{border:1px solid var(--rule);border-radius:var(--radius);background:color-mix(in oklch,var(--surface),var(--surface-2) 22%);padding:12px;margin:4px 0 18px}.path-block h3{margin:0 0 8px}.path-block code{display:block;margin-top:6px;white-space:pre-wrap;overflow-wrap:anywhere;color:var(--muted);font:11px/1.45 var(--mono)}
.leaderboard{margin-top:18px}.leader-list{display:grid;gap:10px}.leader-entry,.trial-details{border:1px solid var(--rule);border-radius:var(--radius);background:color-mix(in oklch,var(--surface),var(--surface-2) 18%);overflow:hidden}.leader-entry>summary,.trial-details>summary{cursor:pointer;list-style:none;display:grid;grid-template-columns:auto minmax(0,1fr) repeat(3,auto);gap:12px;align-items:center;padding:12px;font-family:var(--mono)}.leader-entry>summary::-webkit-details-marker,.trial-details>summary::-webkit-details-marker{display:none}.rank{color:var(--muted)}.table-shell{border:1px solid var(--rule);border-radius:var(--radius);background:color-mix(in oklch,var(--surface),var(--surface-2) 10%);overflow:hidden}.table-wrap{overflow-x:auto}.data-table,table{width:100%;border-collapse:collapse;min-width:760px}.data-table{table-layout:fixed;min-width:1180px}th,td{padding:10px 12px;border-top:1px solid var(--rule);text-align:left;vertical-align:top;overflow-wrap:anywhere}th{color:var(--muted);font:700 12px/1.2 var(--mono);text-transform:uppercase;letter-spacing:.04em}.data-table thead th{padding:7px 8px;background:color-mix(in oklch,var(--surface),var(--surface-2) 38%)}.data-table tbody tr.clickable-row{cursor:pointer}.data-table tbody tr.clickable-row:hover{background:color-mix(in oklch,var(--surface),var(--surface-2) 34%)}.data-table tbody tr.selected-row{background:color-mix(in oklch,var(--focus),var(--surface) 90%);box-shadow:inset 3px 0 0 var(--focus)}td.num,th.num{text-align:right;font-variant-numeric:tabular-nums}.sort-button{width:100%;min-height:34px;border:1px solid transparent;border-radius:6px;background:transparent;color:var(--muted);display:flex;align-items:center;justify-content:space-between;gap:8px;padding:0 5px;font:700 12px/1.2 var(--mono);text-transform:uppercase;letter-spacing:.04em;white-space:nowrap}.sort-label{min-width:0;overflow:hidden;text-overflow:ellipsis;white-space:nowrap}.sort-button:hover{background:color-mix(in oklch,var(--surface),var(--surface-2) 44%)}.sort-button.active{color:var(--ink);background:var(--surface);border-color:color-mix(in oklch,var(--focus),transparent 58%);box-shadow:0 1px 3px rgba(49,42,25,.12)}.static-head{min-height:34px;display:flex;align-items:center;padding:0 4px;color:var(--muted);font:700 12px/1.2 var(--mono);text-transform:uppercase;letter-spacing:.04em;white-space:nowrap;overflow:hidden;text-overflow:ellipsis}th.num .sort-button{text-align:right}.sort-mark{display:inline-flex;align-items:center;justify-content:center;width:22px;height:22px;flex:0 0 22px;border-radius:999px;background:color-mix(in oklch,var(--muted),transparent 86%);color:var(--muted);font-size:14px;line-height:1}.sort-button.active .sort-mark{background:var(--focus);color:#fff;font-size:13px}.table-filters th{border-top:0;padding:0 8px 8px}.multi-filter{position:relative;min-width:0}.multi-filter summary{min-height:34px;border:1px solid color-mix(in oklch,var(--rule),transparent 12%);border-radius:6px;background:var(--surface);color:var(--ink);display:flex;align-items:center;justify-content:space-between;gap:8px;padding:0 8px;cursor:pointer;list-style:none;font:12px/1 var(--mono);text-transform:none;letter-spacing:0}.multi-filter summary::-webkit-details-marker{display:none}.multi-caret{color:var(--muted);font-size:12px}.multi-filter[open] summary{border-color:color-mix(in oklch,var(--focus),transparent 58%);box-shadow:0 0 0 2px color-mix(in oklch,var(--focus),transparent 88%)}.multi-menu{position:absolute;z-index:20;top:38px;left:0;min-width:190px;max-width:320px;max-height:260px;overflow:auto;border:1px solid var(--rule);border-radius:6px;background:var(--surface);box-shadow:0 12px 28px rgba(49,42,25,.18);padding:6px}.multi-option{display:flex;align-items:center;gap:7px;min-height:30px;padding:4px 5px;border-radius:5px;color:var(--ink);font:12px/1.25 var(--mono);text-transform:none;letter-spacing:0}.multi-option:hover{background:var(--surface-2)}.multi-option input{width:13px;height:13px;flex:0 0 auto;accent-color:var(--focus)}.multi-option span{min-width:0;overflow:hidden;text-overflow:ellipsis;white-space:nowrap}.filter-clear{width:100%;min-height:30px;margin-top:5px;border:1px solid var(--rule);border-radius:5px;background:var(--surface-2);color:var(--muted);font:700 11px/1 var(--mono);text-transform:uppercase}.filter-clear:disabled{opacity:.45;cursor:not-allowed}.multi-empty{margin:6px;color:var(--muted);font:12px/1.3 var(--mono)}.filter-slot{display:block;min-height:34px}.table-empty{color:var(--muted);text-align:center;font-family:var(--mono)}.trial-details{margin:10px 12px 12px}.trial-details.flat-trials{margin:14px 0 0}.trial-details>summary{display:block}
.selected-extra{margin:14px 0}.note-list,.selected-evidence-list{display:grid;gap:10px}.manual-note,.analysis-card,.selected-evidence-card{border:1px solid var(--rule);border-radius:var(--radius);background:var(--surface);padding:12px}.selected-evidence-card>summary{cursor:pointer;font:700 13px/1.2 var(--mono);text-transform:uppercase;letter-spacing:.04em;list-style:none}.selected-evidence-card>summary::-webkit-details-marker{display:none}.selected-evidence-card h4{margin:0 0 8px;font:700 13px/1.2 var(--mono);text-transform:uppercase;letter-spacing:.04em;color:var(--muted)}.selected-evidence-card code{font:13px/1.45 var(--mono);overflow-wrap:anywhere}.selected-evidence-card .info-grid{margin:8px 0 0}.evidence-list,.artifact-list{display:grid;gap:7px;margin:8px 0 0;padding-left:18px}.artifact-list li{min-width:0}.artifact-list code{display:block;margin-top:5px;color:var(--ink)}.note-meta{display:flex;align-items:center;gap:8px;flex-wrap:wrap;margin-bottom:8px}.note-meta strong{font:700 12px/1 var(--mono);color:var(--muted);text-transform:uppercase;letter-spacing:.04em}.note-body{font-size:14px;line-height:1.6}.note-body>*:first-child{margin-top:0}.note-body>*:last-child{margin-bottom:0}.note-body h4{margin:12px 0 6px;font:700 15px/1.2 var(--serif)}.note-body p{margin:8px 0}.note-body ul{margin:8px 0;padding-left:20px}.note-body code{font:13px/1.4 var(--mono);background:var(--surface-2);border-radius:4px;padding:1px 4px}.note-code{max-height:none;background:var(--surface-2);border-radius:var(--radius);padding:10px}.note-snippet{display:block;max-width:100%;white-space:nowrap;overflow:hidden;text-overflow:ellipsis}
@media(max-width:980px){.main{padding:18px}.topline{grid-template-columns:1fr}.top-actions{justify-items:stretch}.score-strip{grid-template-columns:repeat(3,minmax(0,1fr))}.serve-actions{display:grid;grid-template-columns:repeat(3,minmax(0,1fr))}.info-grid{grid-template-columns:repeat(2,minmax(0,1fr))}.rail{flex-wrap:wrap}.rail-tools{flex:1 1 220px}.rail-time{margin-left:auto}}
@media(max-width:620px){h1{font-size:30px}.segmented{overflow-x:auto;max-width:100%}.panel-head,.trace-head,.step>summary{grid-template-columns:1fr}.step-row{grid-template-columns:44px 68px minmax(0,1fr)}.preview{white-space:normal}.rail{width:100%;justify-content:space-between}.rail-time{margin-left:auto}.info-grid{grid-template-columns:1fr}.leader-entry>summary{grid-template-columns:1fr}.serve-actions{grid-template-columns:1fr}}
</style>"#
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
        let root = resolve_explicit_path(&path, &env_map, &cwd)?;
        ensure_initialized_workspace(&root)?;
        return Ok(absolute_path(&root));
    }
    if let Some(value) = env_value("PEVAL_ROOT", &env_map) {
        let root = resolve_explicit_path(Path::new(&value), &env_map, &cwd)?;
        ensure_initialized_workspace(&root)?;
        return Ok(absolute_path(&root));
    }

    if let Some(root) = discover_workspace_root(&cwd)? {
        return Ok(root);
    }

    let home = resolve_psychevo_home(&env_map, &cwd)?;
    let config_path = global_peval_config_path(&home);
    if config_path.is_file() {
        let config = read_global_peval_config(&home)?;
        if let Some(default_workspace) = config.default_workspace {
            let root = resolve_config_path(&default_workspace, &home, &env_map)?;
            ensure_initialized_workspace(&root)?;
            return Ok(absolute_path(&root));
        }
    }

    bail!(
        "peval workspace is not initialized; run `peval init`, pass --root/-r, set PEVAL_ROOT, or configure {} with `peval init --default`",
        config_path.display()
    )
}

pub(crate) fn ensure_initialized_workspace(root: &Path) -> Result<()> {
    if !workspace_config_path(root).is_file() {
        bail!(
            "{} is not an initialized peval workspace; run `peval init --root {}`",
            root.display(),
            root.display()
        );
    }
    read_workspace_config(root)?;
    Ok(())
}

pub(crate) fn discover_workspace_root(start: &Path) -> Result<Option<PathBuf>> {
    let mut current = if start.is_file() {
        start.parent().unwrap_or(start).to_path_buf()
    } else {
        start.to_path_buf()
    };
    loop {
        if workspace_config_path(&current).is_file() {
            read_workspace_config(&current)?;
            return Ok(Some(absolute_path(&current)));
        }
        if !current.pop() {
            break;
        }
    }
    Ok(None)
}

pub(crate) fn resolve_config_path(
    path: &Path,
    base: &Path,
    env_map: &BTreeMap<String, String>,
) -> Result<PathBuf> {
    let expanded = expand_tilde(path, env_map)?;
    if expanded.is_absolute() {
        Ok(expanded)
    } else {
        Ok(base.join(expanded))
    }
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
