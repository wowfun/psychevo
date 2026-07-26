use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde_json::{Value, json};
use tempfile::TempDir;

use super::manifest::load_plugin_manifest;
use super::types::{
    LoadedPluginManifest, PluginDiagnostic, PluginInspectOptions, PluginInspection,
    PluginInstallOptions, PluginManifestKind, PluginSourceKind, PluginStageDiagnostic,
};
use super::util::{directory_size, source_slug};
use crate::error::{Error, Result};

const MAX_NPM_ARCHIVE_BYTES: u64 = 50 * 1024 * 1024;
const MAX_NPM_EXTRACTED_BYTES: u64 = 200 * 1024 * 1024;

pub(crate) struct PluginMaterializedSource {
    pub(crate) root: PathBuf,
    pub(crate) source_id: String,
    pub(crate) source_kind: PluginSourceKind,
    pub(crate) npm_registry: Option<String>,
    #[allow(dead_code)]
    pub(crate) temp_dir: Option<TempDir>,
}

pub fn plugin_import_inspect_value(
    home: &Path,
    cwd: &Path,
    options: PluginInspectOptions,
) -> Result<Value> {
    let temp = TempDir::new()?;
    let staging_root = temp.path().to_path_buf();
    let materialized = materialize_source_in_dir(
        home,
        cwd,
        &staging_root,
        &SourceRequest {
            source: options.source,
            source_kind: options.source_kind,
            git_ref: options.git_ref,
            npm_version: options.npm_version,
            npm_registry: options.npm_registry,
        },
        Some(temp),
    )?;
    let inspection = inspect_materialized_source(&materialized)?;
    Ok(json!({
        "success": !inspection.invalid,
        "inspection": inspection,
    }))
}

pub(crate) struct SourceRequest {
    pub(crate) source: String,
    pub(crate) source_kind: Option<PluginSourceKind>,
    pub(crate) git_ref: Option<String>,
    pub(crate) npm_version: Option<String>,
    pub(crate) npm_registry: Option<String>,
}

impl SourceRequest {
    pub(crate) fn from_install(options: &PluginInstallOptions) -> Self {
        Self {
            source: options.source.clone(),
            source_kind: options.source_kind,
            git_ref: options.git_ref.clone(),
            npm_version: options.npm_version.clone(),
            npm_registry: options.npm_registry.clone(),
        }
    }
}

pub(crate) fn materialize_source_in_dir(
    _home: &Path,
    cwd: &Path,
    staging_root: &Path,
    request: &SourceRequest,
    temp_dir: Option<TempDir>,
) -> Result<PluginMaterializedSource> {
    let source_kind = match request.source_kind {
        Some(kind) => kind,
        None => infer_source_kind(&request.source, cwd),
    };
    match source_kind {
        PluginSourceKind::Local => materialize_local(cwd, request, temp_dir),
        PluginSourceKind::Git => materialize_git(staging_root, request, temp_dir),
        PluginSourceKind::Npm => materialize_npm(staging_root, cwd, request, temp_dir),
    }
}

pub(crate) fn inspect_materialized_source(
    materialized: &PluginMaterializedSource,
) -> Result<PluginInspection> {
    let mut stages = vec![PluginStageDiagnostic::new(
        "resolve/fetch",
        "ok",
        format!("resolved {} source", materialized.source_kind.as_str()),
        Some(materialized.root.clone()),
    )];
    match load_plugin_manifest(&materialized.root, false) {
        Ok(manifest) => Ok(inspection_from_manifest(materialized, manifest, stages)),
        Err(manifest_err) => {
            if let Some(inspection) = inspect_hermes(materialized, &mut stages)? {
                return Ok(inspection);
            }
            if let Some(inspection) = inspect_opencode(materialized, &mut stages)? {
                return Ok(inspection);
            }
            stages.push(PluginStageDiagnostic::new(
                "inspect manifest",
                "failed",
                manifest_err.to_string(),
                Some(materialized.root.clone()),
            ));
            Ok(PluginInspection {
                source_kind: materialized.source_kind,
                source_id: materialized.source_id.clone(),
                framework: PluginManifestKind::Unknown,
                canonical_id: source_slug(&materialized.source_id),
                compatibility_profile: None,
                name: "unknown-plugin".to_string(),
                version: None,
                description: None,
                manifest_path: materialized.root.clone(),
                package_root: materialized.root.clone(),
                support: "unsupported".to_string(),
                declared_lanes: Vec::new(),
                component_statuses: Vec::new(),
                unsupported_lanes: Vec::new(),
                diagnostics: vec![PluginDiagnostic::invalid(
                    manifest_err.to_string(),
                    Some(materialized.root.clone()),
                )],
                stages,
                interface: None,
                invalid: true,
            })
        }
    }
}

pub(crate) fn inspection_value(inspection: &PluginInspection) -> Value {
    serde_json::to_value(inspection).unwrap_or_else(|_| json!({}))
}

fn infer_source_kind(source: &str, cwd: &Path) -> PluginSourceKind {
    if resolve_local_source(cwd, source).is_some_and(|path| path.exists()) {
        PluginSourceKind::Local
    } else if looks_like_git_source(source) {
        PluginSourceKind::Git
    } else {
        PluginSourceKind::Local
    }
}

fn materialize_local(
    cwd: &Path,
    request: &SourceRequest,
    temp_dir: Option<TempDir>,
) -> Result<PluginMaterializedSource> {
    let source_path = resolve_local_source(cwd, &request.source)
        .ok_or_else(|| Error::Config(format!("plugin source not found: {}", request.source)))?;
    if !source_path.exists() {
        return Err(Error::Config(format!(
            "plugin source not found: {}",
            request.source
        )));
    }
    let root = source_path.canonicalize()?;
    Ok(PluginMaterializedSource {
        root,
        source_id: format!("local:{}", source_path.display()),
        source_kind: PluginSourceKind::Local,
        npm_registry: None,
        temp_dir,
    })
}

fn materialize_git(
    staging_root: &Path,
    request: &SourceRequest,
    temp_dir: Option<TempDir>,
) -> Result<PluginMaterializedSource> {
    let incoming = staging_root.join("git");
    if incoming.exists() {
        fs::remove_dir_all(&incoming)?;
    }
    let status = Command::new("git")
        .arg("clone")
        .arg(&request.source)
        .arg(&incoming)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()?;
    if !status.success() {
        return Err(Error::Config(format!(
            "git clone failed for {}",
            request.source
        )));
    }
    if let Some(git_ref) = &request.git_ref {
        let status = Command::new("git")
            .arg("-C")
            .arg(&incoming)
            .arg("checkout")
            .arg(git_ref)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()?;
        if !status.success() {
            return Err(Error::Config(format!(
                "git checkout `{git_ref}` failed for {}",
                request.source
            )));
        }
    }
    Ok(PluginMaterializedSource {
        root: incoming,
        source_id: format!(
            "git:{}{}",
            request.source,
            request
                .git_ref
                .as_ref()
                .map(|git_ref| format!("#{git_ref}"))
                .unwrap_or_default()
        ),
        source_kind: PluginSourceKind::Git,
        npm_registry: None,
        temp_dir,
    })
}

fn materialize_npm(
    staging_root: &Path,
    cwd: &Path,
    request: &SourceRequest,
    temp_dir: Option<TempDir>,
) -> Result<PluginMaterializedSource> {
    fs::create_dir_all(staging_root)?;
    let package_spec = npm_package_spec(cwd, request);
    let mut command = Command::new("npm");
    command
        .arg("pack")
        .arg(&package_spec)
        .arg("--ignore-scripts")
        .arg("--json")
        .arg("--pack-destination")
        .arg(staging_root);
    if let Some(registry) = request
        .npm_registry
        .as_deref()
        .filter(|value| !value.is_empty())
    {
        command.arg("--registry").arg(registry);
    }
    let output = command.output()?;
    if !output.status.success() {
        return Err(Error::Config(format!(
            "npm pack failed for {}",
            request.source
        )));
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let pack = serde_json::from_str::<Value>(&stdout)
        .map_err(|err| Error::Config(format!("npm pack returned invalid JSON: {err}")))?;
    let first = pack
        .as_array()
        .and_then(|items| items.first())
        .ok_or_else(|| Error::Config("npm pack returned no package".to_string()))?;
    let filename = first
        .get("filename")
        .and_then(Value::as_str)
        .ok_or_else(|| Error::Config("npm pack result missing filename".to_string()))?;
    let tarball = staging_root.join(filename);
    let archive_size = fs::metadata(&tarball)?.len();
    if archive_size > MAX_NPM_ARCHIVE_BYTES {
        return Err(Error::Config(format!(
            "npm package archive exceeds {} bytes",
            MAX_NPM_ARCHIVE_BYTES
        )));
    }
    let extract_root = staging_root.join("extract");
    fs::create_dir_all(&extract_root)?;
    let status = Command::new("tar")
        .arg("-xzf")
        .arg(&tarball)
        .arg("-C")
        .arg(&extract_root)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()?;
    if !status.success() {
        return Err(Error::Config(format!(
            "failed to extract npm package {}",
            tarball.display()
        )));
    }
    let root = if extract_root.join("package").is_dir() {
        extract_root.join("package")
    } else {
        single_child_dir(&extract_root)?
    };
    let package_json = read_json_file(&root.join("package.json"))?;
    let name = package_json
        .get("name")
        .and_then(Value::as_str)
        .ok_or_else(|| Error::Config("npm package.json missing name".to_string()))?;
    let version = package_json
        .get("version")
        .and_then(Value::as_str)
        .ok_or_else(|| Error::Config("npm package.json missing version".to_string()))?;
    if let Some(expected_name) = npm_expected_package_name(cwd, &request.source)
        && expected_name != name
    {
        return Err(Error::Config(format!(
            "npm package name mismatch: expected `{expected_name}`, got `{name}`"
        )));
    }
    if let Some(expected_version) = request.npm_version.as_deref()
        && expected_version != version
    {
        return Err(Error::Config(format!(
            "npm package version mismatch: expected `{expected_version}`, got `{version}`"
        )));
    }
    let extracted_size = directory_size(&root)?;
    if extracted_size > MAX_NPM_EXTRACTED_BYTES {
        return Err(Error::Config(format!(
            "npm package contents exceed {} bytes",
            MAX_NPM_EXTRACTED_BYTES
        )));
    }
    let registry_suffix = request
        .npm_registry
        .as_ref()
        .filter(|registry| !registry.is_empty())
        .map(|registry| format!("?registry={registry}"))
        .unwrap_or_default();
    Ok(PluginMaterializedSource {
        root,
        source_id: format!("npm:{name}@{version}{registry_suffix}"),
        source_kind: PluginSourceKind::Npm,
        npm_registry: request.npm_registry.clone(),
        temp_dir,
    })
}

fn inspection_from_manifest(
    materialized: &PluginMaterializedSource,
    manifest: LoadedPluginManifest,
    mut stages: Vec<PluginStageDiagnostic>,
) -> PluginInspection {
    let invalid = manifest
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.kind == "invalid");
    stages.push(PluginStageDiagnostic::new(
        "inspect manifest",
        if invalid { "failed" } else { "ok" },
        format!("loaded {} manifest", manifest.kind.as_str()),
        Some(manifest.manifest_path.clone()),
    ));
    stages.push(PluginStageDiagnostic::new(
        "compatibility",
        if manifest.ignored_fields.is_empty() {
            "ok"
        } else {
            "newer_contract"
        },
        format!(
            "evaluated component behavior against {}",
            manifest.compatibility_profile
        ),
        Some(manifest.manifest_path.clone()),
    ));

    let mut declared_lanes = BTreeSet::new();
    for resource in &manifest.manifest_resources {
        match resource.as_str() {
            "skills" => {
                declared_lanes.insert("skills".to_string());
            }
            "mcpServers" => {
                declared_lanes.insert("mcp".to_string());
            }
            "hooks" => {
                declared_lanes.insert("hooks".to_string());
            }
            "interface" => {
                declared_lanes.insert("interface".to_string());
            }
            "apps" => {
                declared_lanes.insert("apps".to_string());
            }
            other => {
                declared_lanes.insert(other.to_string());
            }
        }
    }
    for extension in &manifest.psychevo_extensions {
        match extension.as_str() {
            "runtime" => {
                declared_lanes.insert("tools".to_string());
            }
            "agents" => {
                declared_lanes.insert("agents".to_string());
            }
            "toolsets" => {
                declared_lanes.insert("toolsets".to_string());
            }
            other => {
                declared_lanes.insert(other.to_string());
            }
        }
    }
    let unsupported = Vec::new();
    stages.push(PluginStageDiagnostic::new(
        "target lanes",
        "ok",
        if declared_lanes.is_empty() {
            "no runtime lanes declared".to_string()
        } else {
            format!(
                "declared lanes: {}",
                declared_lanes
                    .iter()
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        },
        Some(manifest.manifest_path.clone()),
    ));
    PluginInspection {
        source_kind: materialized.source_kind,
        source_id: materialized.source_id.clone(),
        framework: manifest.kind,
        canonical_id: manifest.name.clone(),
        compatibility_profile: Some(manifest.compatibility_profile),
        name: manifest.name,
        version: manifest.version,
        description: manifest.description,
        manifest_path: manifest.manifest_path,
        package_root: materialized.root.clone(),
        support: "installable".to_string(),
        declared_lanes: declared_lanes.into_iter().collect(),
        component_statuses: manifest.component_statuses,
        unsupported_lanes: unsupported,
        diagnostics: manifest.diagnostics,
        stages,
        interface: manifest.interface,
        invalid,
    }
}

fn inspect_hermes(
    materialized: &PluginMaterializedSource,
    stages: &mut Vec<PluginStageDiagnostic>,
) -> Result<Option<PluginInspection>> {
    let manifest_path = ["plugin.yaml", "plugin.yml", ".hermes-plugin/plugin.yaml"]
        .into_iter()
        .map(|path| materialized.root.join(path))
        .find(|path| path.is_file());
    let Some(manifest_path) = manifest_path else {
        return Ok(None);
    };
    let raw = fs::read_to_string(&manifest_path)?;
    let yaml: serde_yaml::Value = serde_yaml::from_str(&raw)
        .map_err(|err| Error::Config(format!("{}: {err}", manifest_path.display())))?;
    let name = yaml_string(&yaml, "name").unwrap_or_else(|| "hermes-plugin".to_string());
    let version = yaml_string(&yaml, "version");
    let description = yaml_string(&yaml, "description");
    let mut declared_lanes = Vec::new();
    let mut unsupported = Vec::new();
    if yaml_sequence_non_empty(&yaml, "provides_tools") {
        declared_lanes.push("tools".to_string());
    }
    if yaml_sequence_non_empty(&yaml, "provides_hooks") {
        declared_lanes.push("hooks".to_string());
    }
    if yaml.get("provides_skills").is_some() {
        declared_lanes.push("skills".to_string());
    }
    if yaml.get("requires_env").is_some() {
        unsupported.push("provider_credentials".to_string());
    }
    if materialized.root.join("dashboard").is_dir() {
        unsupported.push("dashboard".to_string());
    }
    unsupported.push("provider_execution".to_string());
    stages.push(PluginStageDiagnostic::new(
        "inspect manifest",
        "ok",
        "loaded Hermes plugin.yaml as inspection metadata",
        Some(manifest_path.clone()),
    ));
    foreign_inspection_stages(stages, &manifest_path, !declared_lanes.is_empty());
    Ok(Some(PluginInspection {
        source_kind: materialized.source_kind,
        source_id: materialized.source_id.clone(),
        framework: PluginManifestKind::Hermes,
        canonical_id: name.clone(),
        compatibility_profile: None,
        name,
        version,
        description,
        manifest_path,
        package_root: materialized.root.clone(),
        support: "inspection_only".to_string(),
        declared_lanes,
        component_statuses: Vec::new(),
        unsupported_lanes: unsupported,
        diagnostics: vec![PluginDiagnostic::warning(
            "Hermes register(ctx) is not imported or executed; declared lanes are descriptive metadata only",
            None,
        )],
        stages: stages.clone(),
        interface: None,
        invalid: false,
    }))
}

fn inspect_opencode(
    materialized: &PluginMaterializedSource,
    stages: &mut Vec<PluginStageDiagnostic>,
) -> Result<Option<PluginInspection>> {
    let package_path = materialized.root.join("package.json");
    if !package_path.is_file() {
        return Ok(None);
    }
    let package = read_json_file(&package_path)?;
    let exports = package.get("exports");
    let has_server = export_target(exports, "./server").is_some() || package.get("main").is_some();
    let has_tui = export_target(exports, "./tui").is_some();
    let has_theme = package.get("oc-themes").is_some();
    if !has_server && !has_tui && !has_theme {
        return Ok(None);
    }
    let name = package
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or("opencode-plugin")
        .to_string();
    let version = package
        .get("version")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned);
    let description = package
        .get("description")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned);
    let mut diagnostics = Vec::new();
    for (label, target) in [
        ("exports./server", export_target(exports, "./server")),
        ("exports./tui", export_target(exports, "./tui")),
        (
            "main",
            package
                .get("main")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned),
        ),
    ] {
        if let Some(target) = target
            && invalid_package_entrypoint(&target)
        {
            diagnostics.push(PluginDiagnostic::invalid(
                format!("OpenCode {label} entrypoint `{target}` escapes package root"),
                Some(package_path.clone()),
            ));
        }
    }
    let failed = diagnostics
        .iter()
        .any(|diagnostic| diagnostic.kind == "invalid");
    let mut declared_lanes = Vec::new();
    let mut unsupported = Vec::new();
    if has_server {
        declared_lanes.push("tools".to_string());
        declared_lanes.push("hooks".to_string());
    }
    if has_tui {
        declared_lanes.push("tui".to_string());
        unsupported.push("tui".to_string());
    }
    if has_theme {
        declared_lanes.push("theme".to_string());
        unsupported.push("theme".to_string());
    }
    stages.push(PluginStageDiagnostic::new(
        "inspect manifest",
        if failed { "failed" } else { "ok" },
        "loaded OpenCode package descriptor",
        Some(package_path.clone()),
    ));
    foreign_inspection_stages(stages, &package_path, !declared_lanes.is_empty());
    Ok(Some(PluginInspection {
        source_kind: materialized.source_kind,
        source_id: materialized.source_id.clone(),
        framework: PluginManifestKind::OpenCode,
        canonical_id: name.clone(),
        compatibility_profile: None,
        name,
        version,
        description,
        manifest_path: package_path,
        package_root: materialized.root.clone(),
        support: "inspection_only".to_string(),
        declared_lanes,
        component_statuses: Vec::new(),
        unsupported_lanes: unsupported,
        diagnostics,
        stages: stages.clone(),
        interface: None,
        invalid: failed,
    }))
}

fn foreign_inspection_stages(
    stages: &mut Vec<PluginStageDiagnostic>,
    manifest_path: &Path,
    has_targets: bool,
) {
    stages.push(PluginStageDiagnostic::new(
        "compatibility",
        "ok",
        "foreign package support is inspection-only",
        Some(manifest_path.to_path_buf()),
    ));
    stages.push(PluginStageDiagnostic::new(
        "target lanes",
        if has_targets { "ok" } else { "warning" },
        if has_targets {
            "foreign descriptor declares target lanes"
        } else {
            "foreign descriptor declares no target lanes"
        },
        Some(manifest_path.to_path_buf()),
    ));
}

fn resolve_local_source(cwd: &Path, source: &str) -> Option<PathBuf> {
    let raw = PathBuf::from(source);
    if raw.is_absolute() {
        Some(raw)
    } else {
        Some(cwd.join(raw))
    }
}

fn looks_like_git_source(source: &str) -> bool {
    source.starts_with("file://")
        || source.contains("://")
        || source.ends_with(".git")
        || source.starts_with("git@")
}

fn npm_package_spec(cwd: &Path, request: &SourceRequest) -> String {
    if let Some(path) = resolve_local_source(cwd, &request.source)
        && path.exists()
    {
        return path.display().to_string();
    }
    if npm_locator_version(&request.source).is_some() {
        request.source.clone()
    } else if let Some(version) = &request.npm_version {
        format!("{}@{}", request.source, version)
    } else {
        request.source.clone()
    }
}

fn npm_expected_package_name(cwd: &Path, source: &str) -> Option<String> {
    if resolve_local_source(cwd, source).is_some_and(|path| path.exists()) {
        None
    } else {
        Some(npm_locator_name(source).to_string())
    }
}

fn npm_locator_name(source: &str) -> &str {
    let Some(index) = npm_locator_version_index(source) else {
        return source;
    };
    &source[..index]
}

fn npm_locator_version(source: &str) -> Option<&str> {
    let index = npm_locator_version_index(source)?;
    source.get(index + 1..)
}

fn npm_locator_version_index(source: &str) -> Option<usize> {
    let slash = source.rfind('/').unwrap_or(0);
    let index = source.rfind('@')?;
    (index > slash).then_some(index)
}

fn single_child_dir(root: &Path) -> Result<PathBuf> {
    let dirs = fs::read_dir(root)?
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .collect::<Vec<_>>();
    match dirs.as_slice() {
        [dir] => Ok(dir.clone()),
        _ => Err(Error::Config(format!(
            "npm package extraction did not produce one package root under {}",
            root.display()
        ))),
    }
}

fn read_json_file(path: &Path) -> Result<Value> {
    let text = fs::read_to_string(path)?;
    serde_json::from_str(&text).map_err(|err| Error::Config(format!("{}: {err}", path.display())))
}

fn yaml_string(value: &serde_yaml::Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(|value| match value {
            serde_yaml::Value::String(value) => Some(value.trim().to_string()),
            serde_yaml::Value::Number(value) => Some(value.to_string()),
            _ => None,
        })
        .filter(|value| !value.is_empty())
}

fn yaml_sequence_non_empty(value: &serde_yaml::Value, key: &str) -> bool {
    value.get(key).is_some_and(|value| match value {
        serde_yaml::Value::Sequence(items) => !items.is_empty(),
        serde_yaml::Value::String(value) => !value.trim().is_empty(),
        serde_yaml::Value::Bool(value) => *value,
        _ => false,
    })
}

fn export_target(exports: Option<&Value>, key: &str) -> Option<String> {
    let exports = exports?;
    match exports {
        Value::Object(object) => object.get(key).and_then(export_value_target),
        _ => None,
    }
}

fn export_value_target(value: &Value) -> Option<String> {
    match value {
        Value::String(value) => Some(value.clone()),
        Value::Object(object) => object
            .get("default")
            .or_else(|| object.get("import"))
            .or_else(|| object.get("require"))
            .and_then(Value::as_str)
            .map(ToOwned::to_owned),
        _ => None,
    }
}

fn invalid_package_entrypoint(target: &str) -> bool {
    let path = Path::new(target);
    path.is_absolute() || target.split('/').any(|part| part == "..")
}
