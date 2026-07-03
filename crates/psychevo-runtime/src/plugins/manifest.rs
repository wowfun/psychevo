use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Component, Path, PathBuf};

use serde_json::{Map, Value, json};

use super::types::{
    LoadedPluginManifest, PluginDiagnostic, PluginInterfaceMetadata, PluginManifestKind,
    PluginWorkerSpec,
};
use crate::config::CustomToolsetConfig;
use crate::error::{Error, Result};
use crate::types::{McpServerInput, McpServerPolicy, McpTransportInput};

const NATIVE_MANIFEST: &str = ".psychevo-plugin/plugin.json";
const CODEX_MANIFEST: &str = ".codex-plugin/plugin.json";
const CLAUDE_MANIFEST: &str = ".claude-plugin/plugin.json";
const HERMES_MANIFESTS: [&str; 3] = ["plugin.yaml", "plugin.yml", ".hermes-plugin/plugin.yaml"];

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
    let hermes_manifest_paths = HERMES_MANIFESTS
        .iter()
        .map(|path| root.join(path))
        .filter(|path| path.exists())
        .collect::<Vec<_>>();
    if found.is_empty() {
        if !hermes_manifest_paths.is_empty() {
            return Err(Error::Config(format!(
                "Hermes plugin.yaml is recognized as descriptive metadata only; dynamic register(ctx) plugins are unsupported: {}",
                hermes_manifest_paths[0].display()
            )));
        }
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
    diagnostics.extend(hermes_manifest_paths.into_iter().map(|path| {
        PluginDiagnostic::warning(
            "Hermes plugin.yaml is ignored; dynamic register(ctx) plugins are unsupported",
            Some(path),
        )
    }));

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

    let mut manifest_resources = BTreeSet::new();
    let mut psychevo_extensions = BTreeSet::new();
    let skill_roots = path_list_field(
        object.get("skills"),
        &root,
        "skills",
        &manifest_path,
        &mut diagnostics,
    )?;
    if !skill_roots.is_empty() {
        manifest_resources.insert("skills".to_string());
    }
    if object.contains_key("mcpServers") {
        manifest_resources.insert("mcpServers".to_string());
    }
    if object.contains_key("apps") {
        manifest_resources.insert("apps".to_string());
    }
    if object.contains_key("interface") {
        manifest_resources.insert("interface".to_string());
    }
    let psychevo = object.get("psychevo").and_then(Value::as_object);
    let agent_roots = path_list_field(
        psychevo.and_then(|psychevo| psychevo.get("agents")),
        &root,
        "psychevo.agents",
        &manifest_path,
        &mut diagnostics,
    )?;
    if !agent_roots.is_empty() {
        psychevo_extensions.insert("agents".to_string());
    }
    let hooks = parse_manifest_hooks(object.get("hooks"), &root, &manifest_path, &mut diagnostics)?;
    if hooks.is_some() {
        manifest_resources.insert("hooks".to_string());
    }
    let mcp_servers = parse_manifest_mcp_servers(
        object.get("mcpServers"),
        &root,
        &manifest_path,
        &mut diagnostics,
    )?;
    if !mcp_servers.is_empty()
        || object.contains_key("mcpServers")
        || root.join(".mcp.json").is_file()
    {
        manifest_resources.insert("mcpServers".to_string());
    }
    let toolsets = parse_manifest_toolsets(
        psychevo.and_then(|psychevo| psychevo.get("toolsets")),
        &manifest_path,
        &mut diagnostics,
    );
    for field in ["commands", "providers", "toolsets"] {
        if psychevo.is_some_and(|psychevo| psychevo.contains_key(field)) {
            psychevo_extensions.insert(field.to_string());
        }
    }
    let worker = parse_worker(
        psychevo.and_then(|psychevo| psychevo.get("runtime")),
        &root,
        &manifest_path,
        &mut diagnostics,
    )?;
    if worker.is_some() {
        psychevo_extensions.insert("runtime".to_string());
    }
    let interface = parse_manifest_interface(
        object.get("interface"),
        &root,
        &manifest_path,
        &mut diagnostics,
    );

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
        mcp_servers,
        worker,
        toolsets,
        interface,
        manifest_resources,
        psychevo_extensions,
        supported_fields,
        ignored_fields,
    })
}

fn supported_manifest_field(key: &str) -> bool {
    matches!(
        key,
        "skills" | "mcpServers" | "hooks" | "apps" | "psychevo" | "interface"
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
    if value.is_none() {
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
    }
    if hooks.as_object().is_some_and(|object| !object.is_empty()) {
        Ok(Some(hooks))
    } else {
        Ok(None)
    }
}

fn parse_manifest_interface(
    value: Option<&Value>,
    root: &Path,
    manifest_path: &Path,
    diagnostics: &mut Vec<PluginDiagnostic>,
) -> Option<PluginInterfaceMetadata> {
    let value = value?;
    let Some(object) = value.as_object() else {
        diagnostics.push(PluginDiagnostic::invalid(
            "manifest interface must be an object",
            Some(manifest_path.to_path_buf()),
        ));
        return None;
    };
    Some(PluginInterfaceMetadata {
        display_name: interface_string(
            object,
            &["displayName"],
            "interface.displayName",
            manifest_path,
            diagnostics,
        ),
        short_description: interface_string(
            object,
            &["shortDescription"],
            "interface.shortDescription",
            manifest_path,
            diagnostics,
        ),
        long_description: interface_string(
            object,
            &["longDescription"],
            "interface.longDescription",
            manifest_path,
            diagnostics,
        ),
        developer_name: interface_string(
            object,
            &["developerName"],
            "interface.developerName",
            manifest_path,
            diagnostics,
        ),
        category: interface_string(
            object,
            &["category"],
            "interface.category",
            manifest_path,
            diagnostics,
        ),
        capabilities: interface_string_array(
            object,
            "capabilities",
            "interface.capabilities",
            manifest_path,
            diagnostics,
        ),
        website_url: interface_string(
            object,
            &["websiteUrl", "websiteURL"],
            "interface.websiteUrl",
            manifest_path,
            diagnostics,
        ),
        privacy_policy_url: interface_string(
            object,
            &["privacyPolicyUrl", "privacyPolicyURL"],
            "interface.privacyPolicyUrl",
            manifest_path,
            diagnostics,
        ),
        terms_of_service_url: interface_string(
            object,
            &["termsOfServiceUrl", "termsOfServiceURL"],
            "interface.termsOfServiceUrl",
            manifest_path,
            diagnostics,
        ),
        brand_color: interface_string(
            object,
            &["brandColor"],
            "interface.brandColor",
            manifest_path,
            diagnostics,
        ),
        composer_icon: interface_path(
            object,
            "composerIcon",
            "interface.composerIcon",
            root,
            manifest_path,
            diagnostics,
        ),
        logo: interface_path(
            object,
            "logo",
            "interface.logo",
            root,
            manifest_path,
            diagnostics,
        ),
        logo_dark: interface_path(
            object,
            "logoDark",
            "interface.logoDark",
            root,
            manifest_path,
            diagnostics,
        ),
        screenshots: interface_path_array(
            object,
            "screenshots",
            "interface.screenshots",
            root,
            manifest_path,
            diagnostics,
        ),
    })
}

fn interface_string(
    object: &Map<String, Value>,
    keys: &[&str],
    label: &str,
    manifest_path: &Path,
    diagnostics: &mut Vec<PluginDiagnostic>,
) -> Option<String> {
    let value = first_interface_field(object, keys)?;
    match value {
        Value::String(value) if !value.trim().is_empty() => Some(value.trim().to_string()),
        Value::String(_) | Value::Null => None,
        _ => {
            diagnostics.push(PluginDiagnostic::invalid(
                format!("manifest {label} must be a string"),
                Some(manifest_path.to_path_buf()),
            ));
            None
        }
    }
}

fn interface_string_array(
    object: &Map<String, Value>,
    key: &str,
    label: &str,
    manifest_path: &Path,
    diagnostics: &mut Vec<PluginDiagnostic>,
) -> Vec<String> {
    let Some(value) = object.get(key) else {
        return Vec::new();
    };
    match string_array_value(value, key) {
        Ok(values) => values
            .into_iter()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .collect(),
        Err(_) => {
            diagnostics.push(PluginDiagnostic::invalid(
                format!("manifest {label} must be a string array"),
                Some(manifest_path.to_path_buf()),
            ));
            Vec::new()
        }
    }
}

fn interface_path(
    object: &Map<String, Value>,
    key: &str,
    label: &str,
    root: &Path,
    manifest_path: &Path,
    diagnostics: &mut Vec<PluginDiagnostic>,
) -> Option<PathBuf> {
    let value = object.get(key)?;
    let raw = match value {
        Value::String(raw) if !raw.trim().is_empty() => raw.trim(),
        Value::String(_) | Value::Null => return None,
        _ => {
            diagnostics.push(PluginDiagnostic::invalid(
                format!("manifest {label} must be a string path"),
                Some(manifest_path.to_path_buf()),
            ));
            return None;
        }
    };
    match resolve_manifest_path(root, raw) {
        Ok(path) => Some(path),
        Err(err) => {
            diagnostics.push(PluginDiagnostic::invalid(
                format!("manifest {label} path `{raw}` is invalid: {err}"),
                Some(manifest_path.to_path_buf()),
            ));
            None
        }
    }
}

fn interface_path_array(
    object: &Map<String, Value>,
    key: &str,
    label: &str,
    root: &Path,
    manifest_path: &Path,
    diagnostics: &mut Vec<PluginDiagnostic>,
) -> Vec<PathBuf> {
    let Some(value) = object.get(key) else {
        return Vec::new();
    };
    let Some(items) = value.as_array() else {
        diagnostics.push(PluginDiagnostic::invalid(
            format!("manifest {label} must be a string path array"),
            Some(manifest_path.to_path_buf()),
        ));
        return Vec::new();
    };
    let mut paths = Vec::new();
    for item in items {
        let Some(raw) = item.as_str().map(str::trim).filter(|raw| !raw.is_empty()) else {
            diagnostics.push(PluginDiagnostic::invalid(
                format!("manifest {label} must contain string paths"),
                Some(manifest_path.to_path_buf()),
            ));
            continue;
        };
        match resolve_manifest_path(root, raw) {
            Ok(path) => paths.push(path),
            Err(err) => diagnostics.push(PluginDiagnostic::invalid(
                format!("manifest {label} path `{raw}` is invalid: {err}"),
                Some(manifest_path.to_path_buf()),
            )),
        }
    }
    paths
}

fn first_interface_field<'a>(object: &'a Map<String, Value>, keys: &[&str]) -> Option<&'a Value> {
    keys.iter().find_map(|key| object.get(*key))
}

fn parse_manifest_mcp_servers(
    value: Option<&Value>,
    root: &Path,
    manifest_path: &Path,
    diagnostics: &mut Vec<PluginDiagnostic>,
) -> Result<Vec<McpServerInput>> {
    match value {
        Some(Value::Object(object)) => Ok(parse_mcp_server_document(
            Value::Object(object.clone()),
            root,
            manifest_path,
            diagnostics,
        )),
        Some(Value::String(path)) => {
            let path = match resolve_manifest_path(root, path) {
                Ok(path) => path,
                Err(err) => {
                    diagnostics.push(PluginDiagnostic::invalid(
                        format!("manifest mcpServers path is invalid: {err}"),
                        Some(manifest_path.to_path_buf()),
                    ));
                    return Ok(Vec::new());
                }
            };
            parse_mcp_servers_file(&path, root, manifest_path, diagnostics)
        }
        Some(_) => {
            diagnostics.push(PluginDiagnostic::invalid(
                "manifest mcpServers must be an object or string path",
                Some(manifest_path.to_path_buf()),
            ));
            Ok(Vec::new())
        }
        None => {
            let default_mcp = root.join(".mcp.json");
            if default_mcp.is_file() {
                parse_mcp_servers_file(&default_mcp, root, manifest_path, diagnostics)
            } else {
                Ok(Vec::new())
            }
        }
    }
}

fn parse_mcp_servers_file(
    path: &Path,
    root: &Path,
    manifest_path: &Path,
    diagnostics: &mut Vec<PluginDiagnostic>,
) -> Result<Vec<McpServerInput>> {
    let raw = match fs::read_to_string(path) {
        Ok(raw) => raw,
        Err(err) => {
            diagnostics.push(PluginDiagnostic::invalid(
                format!(
                    "manifest mcpServers file `{}` is invalid: {err}",
                    path.display()
                ),
                Some(manifest_path.to_path_buf()),
            ));
            return Ok(Vec::new());
        }
    };
    let value = match serde_json::from_str::<Value>(&raw) {
        Ok(value) => value,
        Err(err) => {
            diagnostics.push(PluginDiagnostic::invalid(
                format!(
                    "manifest mcpServers file `{}` is invalid: {err}",
                    path.display()
                ),
                Some(manifest_path.to_path_buf()),
            ));
            return Ok(Vec::new());
        }
    };
    Ok(parse_mcp_server_document(
        value,
        root,
        manifest_path,
        diagnostics,
    ))
}

fn parse_mcp_server_document(
    value: Value,
    root: &Path,
    manifest_path: &Path,
    diagnostics: &mut Vec<PluginDiagnostic>,
) -> Vec<McpServerInput> {
    let Some(mut object) = value.as_object().cloned() else {
        diagnostics.push(PluginDiagnostic::invalid(
            "manifest mcpServers document must be an object",
            Some(manifest_path.to_path_buf()),
        ));
        return Vec::new();
    };
    if let Some(Value::Object(servers)) = object.remove("mcpServers") {
        object = servers;
    }
    let mut out = Vec::new();
    for (name, descriptor) in object {
        if name.starts_with('$') {
            continue;
        }
        if let Some(input) =
            parse_mcp_server_descriptor(&name, &descriptor, root, manifest_path, diagnostics)
        {
            out.push(input);
        }
    }
    out
}

fn parse_mcp_server_descriptor(
    name: &str,
    value: &Value,
    root: &Path,
    manifest_path: &Path,
    diagnostics: &mut Vec<PluginDiagnostic>,
) -> Option<McpServerInput> {
    let Some(object) = value.as_object() else {
        diagnostics.push(PluginDiagnostic::invalid(
            format!("mcpServers.{name} must be an object"),
            Some(manifest_path.to_path_buf()),
        ));
        return None;
    };
    let transport_type = object.get("type").and_then(Value::as_str);
    if let Some(kind) = transport_type
        && !matches!(
            kind,
            "stdio" | "http" | "streamable_http" | "streamable-http"
        )
    {
        let policy = parse_manifest_mcp_policy(name, object, manifest_path, diagnostics)?;
        diagnostics.push(PluginDiagnostic::warning(
            format!("mcpServers.{name} uses unsupported transport `{kind}`"),
            Some(manifest_path.to_path_buf()),
        ));
        return Some(
            McpServerInput::new(
                name,
                McpTransportInput::Unsupported {
                    kind: kind.to_string(),
                },
            )
            .with_policy(policy),
        );
    }
    let inferred_http = object.get("url").is_some()
        || matches!(
            transport_type,
            Some("http" | "streamable_http" | "streamable-http")
        );
    if inferred_http {
        return parse_http_mcp_server(name, object, manifest_path, diagnostics);
    }
    if object.get("command").is_some() || matches!(transport_type, Some("stdio")) {
        return parse_stdio_mcp_server(name, object, root, manifest_path, diagnostics);
    }
    diagnostics.push(PluginDiagnostic::invalid(
        format!("mcpServers.{name} must declare command or url"),
        Some(manifest_path.to_path_buf()),
    ));
    None
}

fn parse_stdio_mcp_server(
    name: &str,
    object: &Map<String, Value>,
    root: &Path,
    manifest_path: &Path,
    diagnostics: &mut Vec<PluginDiagnostic>,
) -> Option<McpServerInput> {
    let command_raw = match object.get("command").and_then(Value::as_str) {
        Some(command) if !command.trim().is_empty() => command.trim(),
        _ => {
            diagnostics.push(PluginDiagnostic::invalid(
                format!("mcpServers.{name}.command is required"),
                Some(manifest_path.to_path_buf()),
            ));
            return None;
        }
    };
    let command = if command_raw.starts_with("./") {
        match resolve_manifest_path(root, command_raw) {
            Ok(path) => path,
            Err(err) => {
                diagnostics.push(PluginDiagnostic::invalid(
                    format!("mcpServers.{name}.command `{command_raw}` is invalid: {err}"),
                    Some(manifest_path.to_path_buf()),
                ));
                return None;
            }
        }
    } else {
        PathBuf::from(command_raw)
    };
    let args = match optional_string_array(object, "args") {
        Ok(args) => args,
        Err(err) => {
            diagnostics.push(PluginDiagnostic::invalid(
                format!("mcpServers.{name}.{err}"),
                Some(manifest_path.to_path_buf()),
            ));
            return None;
        }
    };
    let env = match optional_string_map(object, "env") {
        Ok(env) => env,
        Err(err) => {
            diagnostics.push(PluginDiagnostic::invalid(
                format!("mcpServers.{name}.{err}"),
                Some(manifest_path.to_path_buf()),
            ));
            return None;
        }
    };
    let cwd = match object.get("cwd") {
        Some(Value::String(cwd)) => match resolve_manifest_cwd(root, cwd) {
            Ok(cwd) => Some(cwd),
            Err(err) => {
                diagnostics.push(PluginDiagnostic::invalid(
                    format!("mcpServers.{name}.cwd `{cwd}` is invalid: {err}"),
                    Some(manifest_path.to_path_buf()),
                ));
                return None;
            }
        },
        Some(Value::Null) | None => None,
        Some(_) => {
            diagnostics.push(PluginDiagnostic::invalid(
                format!("mcpServers.{name}.cwd must be a string"),
                Some(manifest_path.to_path_buf()),
            ));
            return None;
        }
    };
    let policy = parse_manifest_mcp_policy(name, object, manifest_path, diagnostics)?;
    Some(
        McpServerInput::new(
            name,
            McpTransportInput::Stdio {
                command,
                args,
                env,
                cwd,
            },
        )
        .with_policy(policy),
    )
}

fn parse_http_mcp_server(
    name: &str,
    object: &Map<String, Value>,
    manifest_path: &Path,
    diagnostics: &mut Vec<PluginDiagnostic>,
) -> Option<McpServerInput> {
    let url = match object.get("url").and_then(Value::as_str) {
        Some(url) if !url.trim().is_empty() => url.trim().to_string(),
        _ => {
            diagnostics.push(PluginDiagnostic::invalid(
                format!("mcpServers.{name}.url is required"),
                Some(manifest_path.to_path_buf()),
            ));
            return None;
        }
    };
    let headers = match object
        .get("headers")
        .or_else(|| object.get("httpHeaders"))
        .map(|value| string_map_value(value, "headers"))
        .transpose()
    {
        Ok(headers) => headers.unwrap_or_default(),
        Err(err) => {
            diagnostics.push(PluginDiagnostic::invalid(
                format!("mcpServers.{name}.{err}"),
                Some(manifest_path.to_path_buf()),
            ));
            return None;
        }
    };
    let policy = parse_manifest_mcp_policy(name, object, manifest_path, diagnostics)?;
    Some(
        McpServerInput::new(name, McpTransportInput::StreamableHttp { url, headers })
            .with_policy(policy),
    )
}

fn parse_manifest_mcp_policy(
    name: &str,
    object: &Map<String, Value>,
    manifest_path: &Path,
    diagnostics: &mut Vec<PluginDiagnostic>,
) -> Option<McpServerPolicy> {
    match parse_manifest_mcp_policy_inner(object) {
        Ok(policy) => Some(policy),
        Err(err) => {
            diagnostics.push(PluginDiagnostic::invalid(
                format!("mcpServers.{name}.{err}"),
                Some(manifest_path.to_path_buf()),
            ));
            None
        }
    }
}

fn parse_manifest_mcp_policy_inner(
    object: &Map<String, Value>,
) -> std::result::Result<McpServerPolicy, String> {
    let mut policy = McpServerPolicy::default();
    if let Some(enabled) = optional_bool_value(object, "enabled")? {
        policy.enabled = enabled;
    }
    if let Some(required) = optional_bool_value(object, "required")? {
        policy.required = required;
    }
    policy.enabled_tools = first_string_array_value(object, &["enabled_tools", "enabledTools"])?;
    policy.disabled_tools =
        first_string_array_value(object, &["disabled_tools", "disabledTools"])?.unwrap_or_default();
    if let Some(supports_parallel_tool_calls) = first_bool_value(
        object,
        &["supports_parallel_tool_calls", "supportsParallelToolCalls"],
    )? {
        policy.supports_parallel_tool_calls = supports_parallel_tool_calls;
    }
    policy.startup_timeout_secs =
        first_u64_value(object, &["startup_timeout_secs", "startupTimeoutSecs"])?;
    policy.tool_timeout_secs = first_u64_value(object, &["tool_timeout_secs", "toolTimeoutSecs"])?;
    Ok(policy)
}

fn parse_manifest_toolsets(
    value: Option<&Value>,
    manifest_path: &Path,
    diagnostics: &mut Vec<PluginDiagnostic>,
) -> BTreeMap<String, CustomToolsetConfig> {
    let Some(value) = value else {
        return BTreeMap::new();
    };
    let Some(object) = value.as_object() else {
        diagnostics.push(PluginDiagnostic::invalid(
            "psychevo.toolsets must be an object",
            Some(manifest_path.to_path_buf()),
        ));
        return BTreeMap::new();
    };
    let mut out = BTreeMap::new();
    for (name, value) in object {
        if !valid_toolset_name(name) {
            diagnostics.push(PluginDiagnostic::invalid(
                format!("psychevo.toolsets.{name} has an invalid toolset name"),
                Some(manifest_path.to_path_buf()),
            ));
            continue;
        }
        let Some(toolset) = value.as_object() else {
            diagnostics.push(PluginDiagnostic::invalid(
                format!("psychevo.toolsets.{name} must be an object"),
                Some(manifest_path.to_path_buf()),
            ));
            continue;
        };
        let description = match toolset.get("description") {
            Some(Value::String(description)) => Some(description.clone()),
            Some(Value::Null) | None => None,
            Some(_) => {
                diagnostics.push(PluginDiagnostic::invalid(
                    format!("psychevo.toolsets.{name}.description must be a string"),
                    Some(manifest_path.to_path_buf()),
                ));
                continue;
            }
        };
        let tools = match optional_string_array(toolset, "tools") {
            Ok(tools) => tools,
            Err(err) => {
                diagnostics.push(PluginDiagnostic::invalid(
                    format!("psychevo.toolsets.{name}.{err}"),
                    Some(manifest_path.to_path_buf()),
                ));
                continue;
            }
        };
        let includes = match optional_string_array(toolset, "includes") {
            Ok(includes) => includes,
            Err(err) => {
                diagnostics.push(PluginDiagnostic::invalid(
                    format!("psychevo.toolsets.{name}.{err}"),
                    Some(manifest_path.to_path_buf()),
                ));
                continue;
            }
        };
        out.insert(
            name.clone(),
            CustomToolsetConfig {
                description,
                tools,
                includes,
            },
        );
    }
    out
}

fn optional_string_array(
    object: &Map<String, Value>,
    key: &str,
) -> std::result::Result<Vec<String>, String> {
    match object.get(key) {
        Some(value) => string_array_value(value, key),
        None => Ok(Vec::new()),
    }
}

fn string_array_value(value: &Value, key: &str) -> std::result::Result<Vec<String>, String> {
    let Some(values) = value.as_array() else {
        return Err(format!("{key} must be an array"));
    };
    values
        .iter()
        .map(|value| {
            value
                .as_str()
                .map(str::to_string)
                .ok_or_else(|| format!("{key} must contain strings"))
        })
        .collect()
}

fn first_string_array_value(
    object: &Map<String, Value>,
    keys: &[&str],
) -> std::result::Result<Option<Vec<String>>, String> {
    for key in keys {
        if let Some(value) = object.get(*key) {
            return string_array_value(value, key).map(Some);
        }
    }
    Ok(None)
}

fn optional_string_map(
    object: &Map<String, Value>,
    key: &str,
) -> std::result::Result<BTreeMap<String, String>, String> {
    match object.get(key) {
        Some(value) => string_map_value(value, key),
        None => Ok(BTreeMap::new()),
    }
}

fn string_map_value(
    value: &Value,
    key: &str,
) -> std::result::Result<BTreeMap<String, String>, String> {
    let Some(object) = value.as_object() else {
        return Err(format!("{key} must be an object"));
    };
    object
        .iter()
        .map(|(name, value)| {
            value
                .as_str()
                .map(|value| (name.clone(), value.to_string()))
                .ok_or_else(|| format!("{key}.{name} must be a string"))
        })
        .collect()
}

fn optional_bool_value(
    object: &Map<String, Value>,
    key: &str,
) -> std::result::Result<Option<bool>, String> {
    object
        .get(key)
        .map(|value| {
            value
                .as_bool()
                .ok_or_else(|| format!("{key} must be a boolean"))
        })
        .transpose()
}

fn first_bool_value(
    object: &Map<String, Value>,
    keys: &[&str],
) -> std::result::Result<Option<bool>, String> {
    for key in keys {
        if object.contains_key(*key) {
            return optional_bool_value(object, key);
        }
    }
    Ok(None)
}

fn first_u64_value(
    object: &Map<String, Value>,
    keys: &[&str],
) -> std::result::Result<Option<u64>, String> {
    for key in keys {
        if let Some(value) = object.get(*key) {
            return value
                .as_u64()
                .filter(|value| *value > 0)
                .ok_or_else(|| format!("{key} must be a positive integer"))
                .map(Some);
        }
    }
    Ok(None)
}

fn valid_toolset_name(name: &str) -> bool {
    !name.trim().is_empty()
        && name
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-'))
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
            "psychevo.runtime.worker.command is required",
            Some(manifest_path.to_path_buf()),
        ));
        return Ok(None);
    };
    let command = match resolve_manifest_path(root, command_raw) {
        Ok(path) => path,
        Err(err) => {
            diagnostics.push(PluginDiagnostic::invalid(
                format!("psychevo.runtime.worker.command `{command_raw}` is invalid: {err}"),
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
                .ok_or_else(|| {
                    Error::Config("psychevo.runtime.worker.args must be an array".to_string())
                })?
                .iter()
                .map(|value| {
                    value.as_str().map(str::to_string).ok_or_else(|| {
                        Error::Config(
                            "psychevo.runtime.worker.args must contain strings".to_string(),
                        )
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

fn resolve_manifest_cwd(root: &Path, raw: &str) -> std::result::Result<PathBuf, String> {
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
            .map_err(|err| format!("cwd cannot be canonicalized: {err}"))?;
        if !canonical.starts_with(&root_canonical) {
            return Err("resolved cwd escapes plugin root".to_string());
        }
        Ok(canonical)
    } else {
        let parent = candidate
            .parent()
            .ok_or_else(|| "cwd has no parent".to_string())?;
        if parent.exists() {
            let parent = parent
                .canonicalize()
                .map_err(|err| format!("cwd parent cannot be canonicalized: {err}"))?;
            if !parent.starts_with(&root_canonical) {
                return Err("resolved cwd escapes plugin root".to_string());
            }
        }
        Ok(candidate)
    }
}
