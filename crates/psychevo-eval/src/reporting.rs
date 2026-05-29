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
        if !current.pop() {
            break;
        }
    }
    bail!("could not find eval config TOML from {}", start.display())
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

pub(crate) fn status_class_for_run(status: RunStatus) -> &'static str {
    match status {
        RunStatus::Passed => "present",
        RunStatus::Failed => "failed",
    }
}

pub(crate) fn report_css() -> &'static str {
    r#"<style>
:root{color-scheme:light;--ink:#171717;--muted:#667085;--paper:#f6f7f8;--panel:#ffffff;--line:#d9dee5;--accent:#0f766e;--ok:#166534;--bad:#b42318;--warn:#a15c07}
*{box-sizing:border-box}body{margin:0;background:var(--paper);color:var(--ink);font:14px/1.5 ui-sans-serif,system-ui,-apple-system,BlinkMacSystemFont,"Segoe UI",sans-serif;-webkit-font-smoothing:antialiased}button,input{font:inherit}a{color:var(--accent);text-decoration:none}a:hover{text-decoration:underline}.page{max-width:1240px;margin:0 auto;padding:30px 20px 56px}.mast{display:flex;align-items:flex-end;justify-content:space-between;gap:24px;padding:22px 0 18px;border-bottom:2px solid var(--ink)}.eyebrow{margin:0 0 8px;color:var(--accent);font-weight:800;text-transform:uppercase;letter-spacing:.08em}.mast h1{margin:0;font-family:Georgia,"Times New Roman",serif;font-size:clamp(32px,5vw,64px);line-height:.98;letter-spacing:0}.subline{margin:12px 0 0;color:var(--muted);max-width:860px}.verdict{min-width:150px;padding:14px 16px;border-radius:8px;background:var(--ink);color:var(--panel);text-align:center;font-family:Georgia,"Times New Roman",serif;font-size:20px}.metrics{display:grid;grid-template-columns:repeat(4,minmax(0,1fr));gap:14px;margin:18px 0}.metrics div{background:var(--panel);border-radius:8px;padding:16px 18px;box-shadow:0 1px 4px rgba(16,24,40,.12)}.metrics span{display:block;color:var(--muted)}.metrics strong{display:block;margin-top:8px;font:700 28px/1 Georgia,"Times New Roman",serif;font-variant-numeric:tabular-nums}.toolbar{display:flex;gap:10px;align-items:center;margin:22px 0;flex-wrap:wrap}.toolbar button,.case-toggle,.dataset-filter{min-height:40px;border:0;border-radius:8px;background:var(--ink);color:var(--panel);padding:9px 13px;cursor:pointer;transition:transform .14s ease,opacity .14s ease}.toolbar button:active,.case-toggle:active,.dataset-filter:active{transform:scale(.96)}.toolbar input{min-height:40px;min-width:240px;border:0;border-radius:8px;background:var(--panel);padding:9px 12px;box-shadow:inset 0 0 0 1px var(--line)}.ledger{margin-top:22px;background:var(--panel);border:1px solid var(--line);border-radius:8px;padding:16px;box-shadow:0 1px 4px rgba(16,24,40,.08);overflow:auto}.ledger h2{font-family:Georgia,"Times New Roman",serif;margin:0 0 12px;font-size:24px;letter-spacing:0}.ledger h3{margin:18px 0 10px;font-size:16px}table{width:100%;border-collapse:collapse;min-width:760px}th,td{padding:11px 12px;border-bottom:1px solid var(--line);text-align:left;vertical-align:top}th{color:var(--muted);font-weight:800}td.num{text-align:right;font-variant-numeric:tabular-nums}pre{max-width:520px;max-height:160px;margin:0;overflow:auto;white-space:pre-wrap;word-break:break-word;font:12px/1.45 ui-monospace,SFMono-Regular,Menlo,Consolas,monospace;color:#273142}.muted{color:var(--muted);font-size:12px}.stamp{display:inline-flex;align-items:center;min-height:26px;border-radius:6px;padding:3px 8px;font-weight:800}.stamp.present{background:rgba(22,101,52,.12);color:var(--ok)}.stamp.missing,.stamp.failed{background:rgba(180,35,24,.12);color:var(--bad)}.artifact-img{display:block;max-width:220px;max-height:160px;border-radius:6px;box-shadow:0 1px 4px rgba(16,24,40,.16)}.trajectory-card{margin-top:16px;padding-top:14px;border-top:1px solid var(--line)}.trajectory-card h3{font-family:ui-monospace,SFMono-Regular,Menlo,Consolas,monospace}.trajectory-graph{width:100%;height:auto;min-width:760px;background:#fbfcfd;border:1px solid var(--line);border-radius:8px}.trajectory-graph circle{fill:#fff;stroke:var(--accent);stroke-width:2}.trajectory-graph .user circle{stroke:#52525b}.trajectory-graph text{font:11px ui-monospace,SFMono-Regular,Menlo,Consolas,monospace;fill:#273142}.step{border-bottom:1px solid var(--line);padding:9px 0}.step summary{cursor:pointer;display:flex;justify-content:space-between;gap:16px}.bars{position:relative;height:8px;margin:9px 0;background:#eef2f6;border-radius:999px;overflow:hidden}.bars i,.bars b{position:absolute;top:0;bottom:0;left:0;display:block}.bars i{background:#99d6ce}.bars b{background:rgba(15,118,110,.28)}.links{white-space:nowrap}.links a{margin-right:10px}.case-note{margin-top:8px;color:var(--muted);max-width:420px}.pick{white-space:nowrap;color:var(--muted)}@media(max-width:760px){.mast{display:block}.verdict{margin-top:18px}.metrics{grid-template-columns:repeat(2,minmax(0,1fr))}.page{padding:18px 12px 36px}.toolbar input{width:100%;min-width:0}}
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
