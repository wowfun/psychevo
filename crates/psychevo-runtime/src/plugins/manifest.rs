use std::collections::BTreeSet;
use std::fs;
use std::path::{Component, Path, PathBuf};

use serde_json::{Map, Value, json};

use super::types::{LoadedPluginManifest, PluginDiagnostic, PluginManifestKind, PluginWorkerSpec};
use crate::error::{Error, Result};

const NATIVE_MANIFEST: &str = ".psychevo-plugin/plugin.json";
const CODEX_MANIFEST: &str = ".codex-plugin/plugin.json";
const CLAUDE_MANIFEST: &str = ".claude-plugin/plugin.json";

pub fn load_plugin_manifest(root: &Path, allow_compat_dev: bool) -> Result<LoadedPluginManifest> {
    let root = root.to_path_buf();
    let candidates = [
        (NATIVE_MANIFEST, PluginManifestKind::Psychevo),
        (CODEX_MANIFEST, PluginManifestKind::Codex),
        (CLAUDE_MANIFEST, PluginManifestKind::Claude),
    ];
    let mut found = candidates
        .iter()
        .filter_map(|(path, kind)| {
            let full = root.join(path);
            full.exists().then_some((full, *kind))
        })
        .collect::<Vec<_>>();
    if found.is_empty() {
        return Err(Error::Config(format!(
            "plugin manifest not found under {}",
            root.display()
        )));
    }
    let (manifest_path, kind) = found.remove(0);
    let ignored_manifest_paths = found.into_iter().map(|(path, _)| path).collect::<Vec<_>>();
    let text = fs::read_to_string(&manifest_path)?;
    let raw: Value = serde_json::from_str(&text)
        .map_err(|err| Error::Config(format!("{}: {err}", manifest_path.display())))?;
    let object = raw.as_object().ok_or_else(|| {
        Error::Config(format!(
            "{} must contain a JSON object",
            manifest_path.display()
        ))
    })?;
    let mut diagnostics = ignored_manifest_paths
        .iter()
        .map(|path| {
            PluginDiagnostic::warning(
                format!(
                    "recognized manifest ignored because {} was selected",
                    manifest_path.display()
                ),
                Some(path.clone()),
            )
        })
        .collect::<Vec<_>>();

    let name = string_field(object, "name")
        .or_else(|| string_field(object, "id"))
        .unwrap_or_else(|| "dev-plugin".to_string());
    let version = string_field(object, "version");
    let description = string_field(object, "description");
    if kind == PluginManifestKind::Psychevo {
        if string_field(object, "name").is_none() {
            diagnostics.push(PluginDiagnostic::invalid(
                "native plugin manifest requires name",
                Some(manifest_path.clone()),
            ));
        }
        if version.is_none() {
            diagnostics.push(PluginDiagnostic::invalid(
                "native plugin manifest requires version",
                Some(manifest_path.clone()),
            ));
        }
        if description.is_none() {
            diagnostics.push(PluginDiagnostic::invalid(
                "native plugin manifest requires description",
                Some(manifest_path.clone()),
            ));
        }
    } else if !allow_compat_dev && (version.is_none() || name.trim().is_empty()) {
        diagnostics.push(PluginDiagnostic::invalid(
            "compatibility manifest install requires resolvable name and version",
            Some(manifest_path.clone()),
        ));
    } else {
        diagnostics.push(PluginDiagnostic::warning(
            format!(
                "{} compatibility manifest loaded with Psychevo field-subset semantics",
                kind.as_str()
            ),
            Some(manifest_path.clone()),
        ));
    }

    let mut supported_fields = BTreeSet::new();
    let mut ignored_fields = BTreeSet::new();
    for key in object.keys() {
        if supported_manifest_field(key) {
            supported_fields.insert(key.clone());
        } else if !matches!(
            key.as_str(),
            "name" | "id" | "version" | "description" | "keywords" | "author"
        ) {
            ignored_fields.insert(key.clone());
            diagnostics.push(PluginDiagnostic::warning(
                format!("manifest field `{key}` is ignored"),
                Some(manifest_path.clone()),
            ));
        }
    }

    let mut capability_families = BTreeSet::new();
    let skill_roots = path_list_field(
        object.get("skills"),
        &root,
        "skills",
        &manifest_path,
        &mut diagnostics,
    )?;
    if !skill_roots.is_empty() {
        capability_families.insert("skills".to_string());
    }
    let agent_roots = path_list_field(
        object.get("agents"),
        &root,
        "agents",
        &manifest_path,
        &mut diagnostics,
    )?;
    if !agent_roots.is_empty() {
        capability_families.insert("agents".to_string());
    }
    let hooks = parse_manifest_hooks(object.get("hooks"), &root, &manifest_path, &mut diagnostics)?;
    if hooks.is_some() {
        capability_families.insert("hooks".to_string());
    }
    for (field, family) in [
        ("mcpServers", "mcp"),
        ("tools", "tools"),
        ("commands", "commands"),
        ("toolsets", "tools"),
        ("providers", "providers"),
        ("agentBackends", "agents"),
    ] {
        if object.contains_key(field) {
            capability_families.insert(family.to_string());
        }
    }
    let worker = parse_worker(
        object.get("runtime"),
        &root,
        &manifest_path,
        &mut diagnostics,
    )?;
    if worker.is_some() {
        capability_families.insert("runtime".to_string());
        capability_families.insert("tools".to_string());
    }
    let interface = object.get("interface").cloned();

    Ok(LoadedPluginManifest {
        root,
        manifest_path,
        kind,
        name,
        version,
        description,
        diagnostics,
        ignored_manifest_paths,
        skill_roots,
        agent_roots,
        hooks,
        worker,
        interface,
        capability_families,
        supported_fields,
        ignored_fields,
    })
}

fn supported_manifest_field(key: &str) -> bool {
    matches!(
        key,
        "skills"
            | "mcpServers"
            | "tools"
            | "hooks"
            | "agents"
            | "agentBackends"
            | "commands"
            | "toolsets"
            | "providers"
            | "runtime"
            | "interface"
    )
}

fn string_field(object: &Map<String, Value>, key: &str) -> Option<String> {
    object
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn path_list_field(
    value: Option<&Value>,
    root: &Path,
    field: &str,
    manifest_path: &Path,
    diagnostics: &mut Vec<PluginDiagnostic>,
) -> Result<Vec<PathBuf>> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    let raw_paths = match value {
        Value::String(path) => vec![path.clone()],
        Value::Array(paths) => paths
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_string)
            .collect(),
        _ => {
            diagnostics.push(PluginDiagnostic::warning(
                format!("manifest {field} must be a string or string array"),
                Some(manifest_path.to_path_buf()),
            ));
            Vec::new()
        }
    };
    let mut out = Vec::new();
    for raw in raw_paths {
        match resolve_manifest_path(root, &raw) {
            Ok(path) => out.push(path),
            Err(err) => diagnostics.push(PluginDiagnostic::invalid(
                format!("manifest {field} path `{raw}` is invalid: {err}"),
                Some(manifest_path.to_path_buf()),
            )),
        }
    }
    Ok(out)
}

fn parse_manifest_hooks(
    value: Option<&Value>,
    root: &Path,
    manifest_path: &Path,
    diagnostics: &mut Vec<PluginDiagnostic>,
) -> Result<Option<Value>> {
    let mut hooks = json!({});
    if let Some(value) = value {
        match value {
            Value::Object(_) => merge_hook_declarations(&mut hooks, value.clone()),
            Value::String(_) | Value::Array(_) => {
                for path in path_list_field(Some(value), root, "hooks", manifest_path, diagnostics)?
                {
                    match load_hook_file(&path) {
                        Ok(value) => merge_hook_declarations(&mut hooks, value),
                        Err(err) => diagnostics.push(PluginDiagnostic::invalid(
                            format!("manifest hooks file `{}` is invalid: {err}", path.display()),
                            Some(manifest_path.to_path_buf()),
                        )),
                    }
                }
            }
            _ => diagnostics.push(PluginDiagnostic::warning(
                "manifest hooks must be an object, string path, or string path array",
                Some(manifest_path.to_path_buf()),
            )),
        }
    }
    let default_hooks = root.join("hooks/hooks.json");
    if default_hooks.exists() {
        match load_hook_file(&default_hooks) {
            Ok(value) => merge_hook_declarations(&mut hooks, value),
            Err(err) => diagnostics.push(PluginDiagnostic::invalid(
                format!(
                    "default hooks file `{}` is invalid: {err}",
                    default_hooks.display()
                ),
                Some(manifest_path.to_path_buf()),
            )),
        }
    }
    if hooks.as_object().is_some_and(|object| !object.is_empty()) {
        Ok(Some(hooks))
    } else {
        Ok(None)
    }
}

fn load_hook_file(path: &Path) -> Result<Value> {
    let raw = fs::read_to_string(path)?;
    let value: Value = serde_json::from_str(&raw)?;
    Ok(value.get("hooks").cloned().unwrap_or(value))
}

fn merge_hook_declarations(base: &mut Value, overlay: Value) {
    let Some(base_object) = base.as_object_mut() else {
        return;
    };
    let overlay = overlay.get("hooks").cloned().unwrap_or(overlay);
    let Some(overlay_object) = overlay.as_object() else {
        return;
    };
    for (event, value) in overlay_object {
        if event == "state" {
            continue;
        }
        match (base_object.get_mut(event), value) {
            (Some(Value::Array(existing)), Value::Array(additional)) => {
                existing.extend(additional.iter().cloned());
            }
            (Some(existing), Value::Array(additional)) => {
                let previous = existing.clone();
                *existing = Value::Array(
                    std::iter::once(previous)
                        .chain(additional.iter().cloned())
                        .collect(),
                );
            }
            (Some(Value::Array(existing)), other) => existing.push(other.clone()),
            (Some(existing), other) => {
                let previous = existing.clone();
                *existing = Value::Array(vec![previous, other.clone()]);
            }
            (None, other) => {
                base_object.insert(event.clone(), other.clone());
            }
        }
    }
}

fn parse_worker(
    value: Option<&Value>,
    root: &Path,
    manifest_path: &Path,
    diagnostics: &mut Vec<PluginDiagnostic>,
) -> Result<Option<PluginWorkerSpec>> {
    let Some(runtime) = value.and_then(Value::as_object) else {
        return Ok(None);
    };
    let Some(worker) = runtime.get("worker").and_then(Value::as_object) else {
        return Ok(None);
    };
    let Some(command_raw) = worker.get("command").and_then(Value::as_str) else {
        diagnostics.push(PluginDiagnostic::invalid(
            "runtime.worker.command is required",
            Some(manifest_path.to_path_buf()),
        ));
        return Ok(None);
    };
    let command = match resolve_manifest_path(root, command_raw) {
        Ok(path) => path,
        Err(err) => {
            diagnostics.push(PluginDiagnostic::invalid(
                format!("runtime.worker.command `{command_raw}` is invalid: {err}"),
                Some(manifest_path.to_path_buf()),
            ));
            return Ok(None);
        }
    };
    let args = worker
        .get("args")
        .map(|value| {
            value
                .as_array()
                .ok_or_else(|| Error::Config("runtime.worker.args must be an array".to_string()))?
                .iter()
                .map(|value| {
                    value.as_str().map(str::to_string).ok_or_else(|| {
                        Error::Config("runtime.worker.args must contain strings".to_string())
                    })
                })
                .collect::<Result<Vec<_>>>()
        })
        .transpose()?
        .unwrap_or_default();
    Ok(Some(PluginWorkerSpec { command, args }))
}

fn resolve_manifest_path(root: &Path, raw: &str) -> std::result::Result<PathBuf, String> {
    if !raw.starts_with("./") {
        return Err("must start with ./".to_string());
    }
    let path = Path::new(raw);
    if path.is_absolute() {
        return Err("must not be absolute".to_string());
    }
    for component in path.components() {
        if matches!(component, Component::ParentDir) {
            return Err("must not contain ..".to_string());
        }
    }
    let candidate = root.join(path);
    let root_canonical = root
        .canonicalize()
        .map_err(|err| format!("plugin root cannot be canonicalized: {err}"))?;
    if candidate.exists() {
        let canonical = candidate
            .canonicalize()
            .map_err(|err| format!("path cannot be canonicalized: {err}"))?;
        if !canonical.starts_with(&root_canonical) {
            return Err("resolved path escapes plugin root".to_string());
        }
        Ok(canonical)
    } else {
        let parent = candidate
            .parent()
            .ok_or_else(|| "path has no parent".to_string())?;
        if parent.exists() {
            let parent = parent
                .canonicalize()
                .map_err(|err| format!("path parent cannot be canonicalized: {err}"))?;
            if !parent.starts_with(&root_canonical) {
                return Err("resolved path escapes plugin root".to_string());
            }
        }
        Ok(candidate)
    }
}
