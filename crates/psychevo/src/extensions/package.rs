use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Component, Path, PathBuf};

use futures::StreamExt;
use psychevo_extension_protocol::{ContributionDescriptors, PROTOCOL_VERSION};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::error::{Error, Result};

const EXTENSION_MANIFEST: &str = "psychevo.extension.json";
const CODEX_PLUGIN_MANIFEST: &str = ".codex-plugin/plugin.json";
const CLAUDE_PLUGIN_MANIFEST: &str = ".claude-plugin/plugin.json";
const MAX_DESCRIPTOR_BYTES: usize = 1024 * 1024;
const MAX_ARTIFACT_BYTES: usize = 50 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtensionManifest {
    pub root: PathBuf,
    pub manifest_path: PathBuf,
    pub schema_version: u32,
    pub id: String,
    pub version: String,
    pub display_name: Option<String>,
    pub description: Option<String>,
    pub homepage: Option<String>,
    pub runtime: ExtensionRuntimeSpec,
    pub contributions: ContributionDescriptors,
    pub plugin_manifest: Option<PathBuf>,
    pub unsupported_fields: BTreeSet<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtensionRuntimeSpec {
    pub protocol: String,
    pub executable: PathBuf,
    pub args: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReleaseDescriptor {
    pub schema_version: u32,
    pub id: String,
    pub version: String,
    pub artifacts: BTreeMap<String, ReleaseArtifact>,
}

impl ReleaseDescriptor {
    pub fn artifact_for_target(&self, target: &str) -> Result<&ReleaseArtifact> {
        self.artifacts.get(target).ok_or_else(|| {
            Error::Config(format!(
                "Extension `{}@{}` has no precompiled artifact for target `{target}`",
                self.id, self.version
            ))
        })
    }

    fn validate(&self) -> Result<()> {
        if self.schema_version != 1 {
            return Err(Error::Config(format!(
                "Extension release descriptor uses unsupported schemaVersion {}; expected 1",
                self.schema_version
            )));
        }
        validate_extension_id(&self.id)?;
        if self.version.trim().is_empty() || self.version == "local" {
            return Err(Error::Config(format!(
                "remote Extension `{}` must declare a release version",
                self.id
            )));
        }
        if self.artifacts.is_empty() {
            return Err(Error::Config(format!(
                "Extension `{}@{}` release has no precompiled artifacts",
                self.id, self.version
            )));
        }
        for (target, artifact) in &self.artifacts {
            artifact.validate(target)?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReleaseArtifact {
    pub url: String,
    pub sha256: String,
    pub format: String,
    pub executable: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,
}

impl ReleaseArtifact {
    fn validate(&self, target: &str) -> Result<()> {
        require_https(&self.url, "artifact")?;
        if self.format != "tar.gz" {
            return Err(Error::Config(format!(
                "Extension artifact for `{target}` uses unsupported format `{}`; expected `tar.gz`",
                self.format
            )));
        }
        normalized_sha256(&self.sha256)?;
        if !self.executable.starts_with("./") {
            return Err(Error::Config(format!(
                "Extension artifact executable `{}` must begin with ./",
                self.executable
            )));
        }
        if self
            .size
            .is_some_and(|size| size as usize > MAX_ARTIFACT_BYTES)
        {
            return Err(Error::Config(format!(
                "Extension artifact for `{target}` exceeds {MAX_ARTIFACT_BYTES} byte limit"
            )));
        }
        Ok(())
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawExtensionManifest {
    schema_version: u32,
    id: String,
    version: String,
    #[serde(default)]
    display_name: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    homepage: Option<String>,
    runtime: RawExtensionRuntime,
    #[serde(default)]
    contributions: ContributionDescriptors,
    #[serde(flatten)]
    extra: BTreeMap<String, Value>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawExtensionRuntime {
    protocol: String,
    executable: String,
    #[serde(default)]
    args: Vec<String>,
    #[serde(flatten)]
    extra: BTreeMap<String, Value>,
}

pub fn load_extension_manifest(root: &Path) -> Result<ExtensionManifest> {
    let root = root.to_path_buf();
    let manifest_path = root.join(EXTENSION_MANIFEST);
    let text = fs::read_to_string(&manifest_path).map_err(|err| {
        Error::Config(format!(
            "failed to read Extension manifest {}: {err}",
            manifest_path.display()
        ))
    })?;
    let raw: RawExtensionManifest = serde_json::from_str(&text).map_err(|err| {
        Error::Config(format!(
            "failed to parse Extension manifest {}: {err}",
            manifest_path.display()
        ))
    })?;

    if raw.schema_version != 1 {
        return Err(Error::Config(format!(
            "Extension `{}` uses unsupported schemaVersion {}; expected 1",
            raw.id, raw.schema_version
        )));
    }
    validate_extension_id(&raw.id)?;
    if raw.version.trim().is_empty() {
        return Err(Error::Config(
            "Extension version must not be blank".to_string(),
        ));
    }
    if raw.runtime.protocol != PROTOCOL_VERSION {
        return Err(Error::Config(format!(
            "Extension `{}` uses unsupported protocol `{}`; expected `{PROTOCOL_VERSION}`",
            raw.id, raw.runtime.protocol
        )));
    }
    if !raw.runtime.extra.is_empty() {
        return Err(Error::Config(format!(
            "Extension `{}` runtime contains unsupported fields: {}",
            raw.id,
            raw.runtime
                .extra
                .keys()
                .cloned()
                .collect::<Vec<_>>()
                .join(", ")
        )));
    }
    let executable = resolve_package_relative_path(&root, &raw.runtime.executable)?;
    validate_manifest_commands(&raw.id, &raw.contributions)?;
    validate_manifest_apps(&raw.id, &raw.contributions)?;

    let plugin_manifests = [CODEX_PLUGIN_MANIFEST, CLAUDE_PLUGIN_MANIFEST]
        .into_iter()
        .map(|path| root.join(path))
        .filter(|path| path.is_file())
        .collect::<Vec<_>>();
    if plugin_manifests.len() > 1 {
        return Err(Error::Config(format!(
            "Extension `{}` contains {} recognized Plugin bases; at most one co-root Plugin is allowed",
            raw.id,
            plugin_manifests.len()
        )));
    }

    Ok(ExtensionManifest {
        root,
        manifest_path,
        schema_version: raw.schema_version,
        id: raw.id,
        version: raw.version,
        display_name: non_blank(raw.display_name),
        description: non_blank(raw.description),
        homepage: non_blank(raw.homepage),
        runtime: ExtensionRuntimeSpec {
            protocol: raw.runtime.protocol,
            executable,
            args: raw.runtime.args,
        },
        contributions: raw.contributions,
        plugin_manifest: plugin_manifests.into_iter().next(),
        unsupported_fields: raw.extra.into_keys().collect(),
    })
}

fn validate_extension_id(id: &str) -> Result<()> {
    let valid = !id.is_empty()
        && id.split('.').all(|segment| {
            !segment.is_empty()
                && segment
                    .chars()
                    .next()
                    .is_some_and(|ch| ch.is_ascii_lowercase())
                && segment
                    .chars()
                    .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-')
        });
    if valid {
        Ok(())
    } else {
        Err(Error::Config(format!(
            "Extension id `{id}` must use lowercase dot-separated identifiers"
        )))
    }
}

fn validate_manifest_commands(id: &str, contributions: &ContributionDescriptors) -> Result<()> {
    let mut names = BTreeSet::new();
    for command in &contributions.commands {
        let valid = command
            .name
            .chars()
            .next()
            .is_some_and(|ch| ch.is_ascii_lowercase())
            && command
                .name
                .chars()
                .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-');
        if !valid {
            return Err(Error::Config(format!(
                "Extension `{id}` command `{}` must match [a-z][a-z0-9-]*",
                command.name
            )));
        }
        if !names.insert(command.name.as_str()) {
            return Err(Error::Config(format!(
                "Extension `{id}` declares command `{}` more than once",
                command.name
            )));
        }
    }
    Ok(())
}

fn validate_manifest_apps(id: &str, contributions: &ContributionDescriptors) -> Result<()> {
    let mut ids = BTreeSet::new();
    for app in &contributions.mcp_apps {
        if app.id.trim().is_empty() || !ids.insert(app.id.as_str()) {
            return Err(Error::Config(format!(
                "Extension `{id}` MCP App ids must be non-empty and unique"
            )));
        }
        if app.resource_uri.trim().is_empty() || app.fallback.trim().is_empty() {
            return Err(Error::Config(format!(
                "Extension `{id}` MCP App `{}` requires resourceUri and fallback",
                app.id
            )));
        }
        let resource_domains = app
            .resource_domains
            .iter()
            .map(|domain| https_origin(domain, "MCP App resource domain"))
            .collect::<Result<BTreeSet<_>>>()?;
        for domain in &app.connect_domains {
            https_origin(domain, "MCP App connect domain")?;
        }
        if let Some(resource_url) = app.resource_url.as_deref() {
            let origin = https_url_origin(resource_url, "MCP App resource URL")?;
            if !resource_domains.contains(&origin) {
                return Err(Error::Config(format!(
                    "Extension `{id}` MCP App `{}` resource URL origin `{origin}` is not declared in resourceDomains",
                    app.id
                )));
            }
        }
        if app.allowed_tools.iter().any(|tool| tool.trim().is_empty()) {
            return Err(Error::Config(format!(
                "Extension `{id}` MCP App `{}` contains a blank allowed tool id",
                app.id
            )));
        }
    }
    Ok(())
}

fn https_url_origin(value: &str, label: &str) -> Result<String> {
    let url = reqwest::Url::parse(value)
        .map_err(|_| Error::Config(format!("Extension {label} must be a valid HTTPS URL")))?;
    if url.scheme() != "https"
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err(Error::Config(format!(
            "Extension {label} must use an HTTPS origin without credentials"
        )));
    }
    Ok(url.origin().ascii_serialization())
}

fn https_origin(value: &str, label: &str) -> Result<String> {
    let url = reqwest::Url::parse(value)
        .map_err(|_| Error::Config(format!("Extension {label} must be a valid HTTPS origin")))?;
    let origin = https_url_origin(value, label)?;
    if url.path() != "/" || url.query().is_some() || url.fragment().is_some() {
        return Err(Error::Config(format!(
            "Extension {label} `{value}` must contain only scheme, host, and optional port"
        )));
    }
    Ok(origin)
}

fn resolve_package_relative_path(root: &Path, value: &str) -> Result<PathBuf> {
    if !value.starts_with("./") {
        return Err(Error::Config(format!(
            "Extension executable `{value}` must be an explicit package-relative path beginning with ./"
        )));
    }
    let relative = Path::new(value);
    if relative.is_absolute()
        || relative.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return Err(Error::Config(format!(
            "Extension executable `{value}` must be an explicit package-relative path without .."
        )));
    }
    Ok(root.join(relative.strip_prefix(".").unwrap_or(relative)))
}

fn non_blank(value: Option<String>) -> Option<String> {
    value.and_then(|value| {
        let trimmed = value.trim();
        (!trimmed.is_empty()).then(|| trimmed.to_string())
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExtensionScope {
    Profile,
    Local,
}

impl ExtensionScope {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Profile => "profile",
            Self::Local => "local",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtensionInstallRecord {
    pub id: String,
    pub version: String,
    pub scope: ExtensionScope,
    pub source: String,
    pub source_kind: String,
    pub package_root: PathBuf,
    pub data_root: PathBuf,
    pub fingerprint: String,
    pub trusted_fingerprint: String,
    pub enabled: bool,
    pub manifest_path: PathBuf,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plugin_manifest: Option<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct ExtensionStore {
    profile_home: PathBuf,
    cwd: PathBuf,
}

pub(super) struct ExtensionActivityLock(File);

impl Drop for ExtensionActivityLock {
    fn drop(&mut self) {
        let _ = self.0.unlock();
    }
}

impl ExtensionStore {
    pub fn new(profile_home: impl Into<PathBuf>, cwd: impl Into<PathBuf>) -> Self {
        Self {
            profile_home: profile_home.into(),
            cwd: cwd.into(),
        }
    }

    pub fn cache_root(&self, scope: ExtensionScope) -> PathBuf {
        self.scope_root(scope).join("cache")
    }

    pub fn data_root(&self, scope: ExtensionScope) -> PathBuf {
        self.scope_root(scope).join("data")
    }

    pub fn install_local(
        &self,
        source: &Path,
        scope: ExtensionScope,
    ) -> Result<ExtensionInstallRecord> {
        let package_root = source.canonicalize().map_err(|err| {
            Error::Config(format!(
                "failed to resolve local Extension source {}: {err}",
                source.display()
            ))
        })?;
        let manifest = load_extension_manifest(&package_root)?;
        if manifest.version != "local" {
            return Err(Error::Config(format!(
                "local Extension `{}` must declare version `local`",
                manifest.id
            )));
        }
        validate_materialized_executable(&manifest)?;
        let _activity_lock = self.mutation_lock(&manifest.id, scope)?;

        let cache_root = self.cache_root(scope);
        let data_root = self.data_root(scope).join(&manifest.id);
        let record_root = self.scope_root(scope).join("records");
        let fingerprint = crate::plugins::external_plugin_fingerprint(
            Some(&package_root),
            &manifest.id,
            Some(&manifest.version),
        )?;
        let record = ExtensionInstallRecord {
            id: manifest.id.clone(),
            version: manifest.version.clone(),
            scope,
            source: package_root.display().to_string(),
            source_kind: "local".to_string(),
            package_root,
            data_root: data_root.clone(),
            fingerprint: fingerprint.clone(),
            trusted_fingerprint: fingerprint,
            enabled: true,
            manifest_path: manifest.manifest_path.clone(),
            plugin_manifest: manifest.plugin_manifest.clone(),
        };
        self.validate_enabled_catalog_state(Some((&record, &manifest)), None)?;
        fs::create_dir_all(&cache_root)?;
        fs::create_dir_all(&data_root)?;
        fs::create_dir_all(&record_root)?;
        write_record_atomic(&record_root.join(format!("{}.json", record.id)), &record)?;
        Ok(record)
    }

    pub async fn install_remote(
        &self,
        descriptor_url: &str,
        scope: ExtensionScope,
    ) -> Result<ExtensionInstallRecord> {
        require_https(descriptor_url, "release descriptor")?;
        let client = remote_package_client()?;
        let response = fetch_https(&client, descriptor_url, "release descriptor").await?;
        let descriptor_bytes = download_bounded(response, MAX_DESCRIPTOR_BYTES).await?;
        let descriptor: ReleaseDescriptor = serde_json::from_slice(&descriptor_bytes)
            .map_err(|err| Error::Config(format!("invalid Extension release descriptor: {err}")))?;
        descriptor.validate()?;
        let target = current_target_triple();
        let artifact = descriptor.artifact_for_target(&target)?;
        let response = fetch_https(&client, &artifact.url, "artifact").await?;
        let bytes = download_bounded(response, MAX_ARTIFACT_BYTES).await?;
        self.install_remote_archive(&descriptor, descriptor_url, &target, &bytes, scope)
    }

    pub fn read_record(
        &self,
        id: &str,
        scope: ExtensionScope,
    ) -> Result<Option<ExtensionInstallRecord>> {
        validate_extension_id(id)?;
        let path = self
            .scope_root(scope)
            .join("records")
            .join(format!("{id}.json"));
        match fs::read(&path) {
            Ok(bytes) => serde_json::from_slice(&bytes).map(Some).map_err(|err| {
                Error::Config(format!(
                    "failed to parse Extension record {}: {err}",
                    path.display()
                ))
            }),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(err) => Err(err.into()),
        }
    }

    pub fn records(&self, scope: ExtensionScope) -> Result<Vec<ExtensionInstallRecord>> {
        let root = self.scope_root(scope).join("records");
        if !root.is_dir() {
            return Ok(Vec::new());
        }
        let mut paths = fs::read_dir(root)?
            .collect::<std::io::Result<Vec<_>>>()?
            .into_iter()
            .map(|entry| entry.path())
            .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("json"))
            .collect::<Vec<_>>();
        paths.sort();
        paths
            .into_iter()
            .map(|path| {
                let bytes = fs::read(&path)?;
                serde_json::from_slice(&bytes).map_err(|err| {
                    Error::Config(format!(
                        "failed to parse Extension record {}: {err}",
                        path.display()
                    ))
                })
            })
            .collect()
    }

    pub fn effective_records(&self) -> Result<Vec<ExtensionInstallRecord>> {
        let mut records = BTreeMap::new();
        for record in self.records(ExtensionScope::Profile)? {
            records.insert(record.id.clone(), record);
        }
        for record in self.records(ExtensionScope::Local)? {
            records.insert(record.id.clone(), record);
        }
        Ok(records.into_values().collect())
    }

    pub fn resolve_channel_extension(
        &self,
        channel: &str,
    ) -> Result<(ExtensionInstallRecord, ExtensionManifest)> {
        let mut matches = Vec::new();
        for record in self
            .effective_records()?
            .into_iter()
            .filter(|record| record.enabled)
        {
            let manifest = load_extension_manifest(&record.package_root)?;
            if manifest
                .contributions
                .channels
                .iter()
                .any(|descriptor| descriptor.channel == channel)
            {
                matches.push((record, manifest));
            }
        }
        let (record, manifest) = match matches.len() {
            1 => matches.pop().expect("single Channel Extension"),
            0 => {
                let extension = first_party_channel_extension(channel);
                return Err(Error::Message(format!(
                    "Channel `{channel}` requires Extension `{extension}`; install it with `pevo install {extension}`"
                )));
            }
            _ => {
                return Err(Error::Message(format!(
                    "Channel `{channel}` is declared by multiple enabled Extensions: {}",
                    matches
                        .iter()
                        .map(|(record, _)| record.id.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                )));
            }
        };
        let fingerprint = crate::plugins::external_plugin_fingerprint(
            Some(&record.package_root),
            &record.id,
            Some(&record.version),
        )?;
        if fingerprint != record.fingerprint || fingerprint != record.trusted_fingerprint {
            return Err(Error::Message(format!(
                "Channel Extension `{}` changed after installation; reinstall it before use",
                record.id
            )));
        }
        Ok((record, manifest))
    }

    pub fn remove(
        &self,
        id: &str,
        scope: ExtensionScope,
    ) -> Result<Option<ExtensionInstallRecord>> {
        let Some(record) = self.read_record(id, scope)? else {
            return Ok(None);
        };
        let _activity_lock = self.mutation_lock(id, scope)?;
        if record.source_kind == "https" && !record.package_root.starts_with(self.cache_root(scope))
        {
            return Err(Error::Config(format!(
                "Extension `{id}` cache path is outside the selected store; nothing was removed"
            )));
        }
        self.validate_enabled_catalog_state(None, Some((id, scope)))?;
        let record_path = self
            .scope_root(scope)
            .join("records")
            .join(format!("{id}.json"));
        fs::remove_file(record_path)?;
        if record.source_kind == "https" {
            match fs::remove_dir_all(&record.package_root) {
                Ok(()) => {}
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
                Err(err) => return Err(err.into()),
            }
        }
        Ok(Some(record))
    }

    pub fn set_enabled(
        &self,
        id: &str,
        scope: ExtensionScope,
        enabled: bool,
    ) -> Result<ExtensionInstallRecord> {
        let mut record = self.read_record(id, scope)?.ok_or_else(|| {
            Error::Config(format!(
                "Extension `{id}` is not installed in {} scope",
                scope.as_str()
            ))
        })?;
        let _activity_lock = self.mutation_lock(id, scope)?;
        record.enabled = enabled;
        if enabled {
            let manifest = load_extension_manifest(&record.package_root)?;
            self.validate_enabled_catalog_state(Some((&record, &manifest)), None)?;
        }
        let record_path = self
            .scope_root(scope)
            .join("records")
            .join(format!("{id}.json"));
        write_record_atomic(&record_path, &record)?;
        Ok(record)
    }

    #[cfg(test)]
    pub(crate) fn install_remote_archive_for_test(
        &self,
        descriptor: &ReleaseDescriptor,
        descriptor_url: &str,
        target: &str,
        bytes: &[u8],
        scope: ExtensionScope,
    ) -> Result<ExtensionInstallRecord> {
        descriptor.validate()?;
        self.install_remote_archive(descriptor, descriptor_url, target, bytes, scope)
    }

    fn install_remote_archive(
        &self,
        descriptor: &ReleaseDescriptor,
        descriptor_url: &str,
        target: &str,
        bytes: &[u8],
        scope: ExtensionScope,
    ) -> Result<ExtensionInstallRecord> {
        require_https(descriptor_url, "release descriptor")?;
        let artifact = descriptor.artifact_for_target(target)?;
        if bytes.len() > MAX_ARTIFACT_BYTES {
            return Err(Error::Config(format!(
                "Extension artifact exceeds {MAX_ARTIFACT_BYTES} byte limit"
            )));
        }
        if artifact.size.is_some_and(|size| size != bytes.len() as u64) {
            return Err(Error::Config(format!(
                "Extension artifact size mismatch: expected {}, received {}",
                artifact.size.unwrap_or_default(),
                bytes.len()
            )));
        }
        let expected = normalized_sha256(&artifact.sha256)?;
        let actual = format!("{:x}", Sha256::digest(bytes));
        if actual != expected {
            return Err(Error::Config(format!(
                "Extension artifact SHA-256 mismatch: expected {expected}, received {actual}"
            )));
        }

        let cache_root = self.cache_root(scope);
        let data_root = self.data_root(scope).join(&descriptor.id);
        let record_root = self.scope_root(scope).join("records");
        let _activity_lock = self.mutation_lock(&descriptor.id, scope)?;
        let enabled = self
            .read_record(&descriptor.id, scope)?
            .is_none_or(|record| record.enabled);
        fs::create_dir_all(&cache_root)?;
        let staging = tempfile::Builder::new()
            .prefix(".extension-stage-")
            .tempdir_in(&cache_root)?;
        let archive_path = staging.path().join("artifact.tar.gz");
        let mut archive_file = fs::File::create(&archive_path)?;
        archive_file.write_all(bytes)?;
        archive_file.sync_all()?;
        let package = staging.path().join("package");
        crate::plugins::extract_tar_gz_bounded(&archive_path, &package)?;
        let manifest = load_extension_manifest(&package)?;
        if manifest.id != descriptor.id || manifest.version != descriptor.version {
            return Err(Error::Config(format!(
                "Extension artifact manifest `{}@{}` does not match release `{}@{}`",
                manifest.id, manifest.version, descriptor.id, descriptor.version
            )));
        }
        let expected_executable = resolve_package_relative_path(&package, &artifact.executable)?;
        if manifest.runtime.executable != expected_executable {
            return Err(Error::Config(format!(
                "Extension artifact executable `{}` does not match manifest `{}`",
                artifact.executable,
                manifest.runtime.executable.display()
            )));
        }
        validate_materialized_executable(&manifest)?;
        let fingerprint = crate::plugins::external_plugin_fingerprint(
            Some(&package),
            &manifest.id,
            Some(&manifest.version),
        )?;
        let destination = cache_root.join(&descriptor.id).join(format!(
            "{}-{}",
            safe_segment(&descriptor.version),
            &actual[..12]
        ));
        let staged_record = ExtensionInstallRecord {
            id: descriptor.id.clone(),
            version: descriptor.version.clone(),
            scope,
            source: redact_url(descriptor_url),
            source_kind: "https".to_string(),
            package_root: package.clone(),
            data_root: data_root.clone(),
            fingerprint: fingerprint.clone(),
            trusted_fingerprint: fingerprint.clone(),
            enabled,
            manifest_path: manifest.manifest_path.clone(),
            plugin_manifest: manifest.plugin_manifest.clone(),
        };
        self.validate_enabled_catalog_state(Some((&staged_record, &manifest)), None)?;
        if !destination.exists() {
            if let Some(parent) = destination.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::rename(&package, &destination)?;
        }
        let destination_fingerprint = crate::plugins::external_plugin_fingerprint(
            Some(&destination),
            &manifest.id,
            Some(&manifest.version),
        )?;
        if destination_fingerprint != fingerprint {
            return Err(Error::Config(format!(
                "Extension `{}` cached package fingerprint does not match the verified artifact",
                manifest.id
            )));
        }
        let record = ExtensionInstallRecord {
            id: descriptor.id.clone(),
            version: descriptor.version.clone(),
            scope,
            source: redact_url(descriptor_url),
            source_kind: "https".to_string(),
            package_root: destination.clone(),
            data_root: data_root.clone(),
            fingerprint: destination_fingerprint.clone(),
            trusted_fingerprint: destination_fingerprint,
            enabled,
            manifest_path: destination.join(EXTENSION_MANIFEST),
            plugin_manifest: manifest.plugin_manifest.as_ref().and_then(|path| {
                path.strip_prefix(&package)
                    .ok()
                    .map(|relative| destination.join(relative))
            }),
        };
        fs::create_dir_all(&data_root)?;
        fs::create_dir_all(&record_root)?;
        write_record_atomic(&record_root.join(format!("{}.json", record.id)), &record)?;
        Ok(record)
    }

    fn validate_enabled_catalog_state(
        &self,
        replacement: Option<(&ExtensionInstallRecord, &ExtensionManifest)>,
        removed: Option<(&str, ExtensionScope)>,
    ) -> Result<()> {
        let mut effective = BTreeMap::<String, ExtensionInstallRecord>::new();
        for record in self
            .records(ExtensionScope::Profile)?
            .into_iter()
            .filter(|record| {
                !removed.is_some_and(|(id, scope)| record.id == id && record.scope == scope)
            })
        {
            effective.insert(record.id.clone(), record);
        }
        if let Some((candidate, _)) = replacement
            && candidate.scope == ExtensionScope::Profile
        {
            effective.insert(candidate.id.clone(), candidate.clone());
        }
        for record in self
            .records(ExtensionScope::Local)?
            .into_iter()
            .filter(|record| {
                !removed.is_some_and(|(id, scope)| record.id == id && record.scope == scope)
            })
        {
            effective.insert(record.id.clone(), record);
        }
        if let Some((candidate, _)) = replacement
            && candidate.scope == ExtensionScope::Local
        {
            effective.insert(candidate.id.clone(), candidate.clone());
        }

        let mut manifests = Vec::new();
        for record in effective.into_values().filter(|record| record.enabled) {
            let is_candidate = replacement.is_some_and(|(candidate, _)| {
                record.id == candidate.id && record.scope == candidate.scope
            });
            let fingerprint = crate::plugins::external_plugin_fingerprint(
                Some(&record.package_root),
                &record.id,
                Some(&record.version),
            )?;
            if fingerprint != record.fingerprint || fingerprint != record.trusted_fingerprint {
                if is_candidate {
                    return Err(Error::Config(format!(
                        "Extension `{}` package no longer matches its trusted fingerprint",
                        record.id
                    )));
                }
                continue;
            }
            manifests.push(match replacement {
                Some((_, candidate_manifest)) if is_candidate => candidate_manifest.clone(),
                _ => load_extension_manifest(&record.package_root)?,
            });
        }

        let builtins = crate::command_registry::CLI_COMMANDS
            .iter()
            .flat_map(|command| {
                std::iter::once(command.canonical).chain(command.aliases.iter().copied())
            })
            .chain(
                crate::command_registry::SLASH_COMMANDS
                    .iter()
                    .flat_map(|command| {
                        std::iter::once(command.canonical).chain(command.aliases.iter().copied())
                    })
                    .filter_map(|command| command.strip_prefix('/')),
            )
            .collect::<Vec<_>>();
        ExtensionCommandCatalog::build(&manifests, &builtins)?;
        Ok(())
    }

    fn scope_root(&self, scope: ExtensionScope) -> PathBuf {
        match scope {
            ExtensionScope::Profile => self.profile_home.join("extensions"),
            ExtensionScope::Local => self.cwd.join(".psychevo/extensions"),
        }
    }

    fn mutation_lock(&self, id: &str, scope: ExtensionScope) -> Result<ExtensionActivityLock> {
        let data_root = self.data_root(scope).join(id);
        fs::create_dir_all(&data_root)?;
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(data_root.join(".activity.lock"))?;
        lock.try_lock().map_err(|err| match err {
            std::fs::TryLockError::WouldBlock => Error::Config(format!(
                "Extension `{id}` has an active sidecar lease; close or stop it before changing installation state"
            )),
            std::fs::TryLockError::Error(err) => Error::Io(err),
        })?;
        Ok(ExtensionActivityLock(lock))
    }
}

pub fn first_party_channel_extension(channel: &str) -> &'static str {
    match channel {
        "wechat" => "psychevo.channel.wechat",
        "telegram" => "psychevo.channel.telegram",
        "feishu" | "lark" => "psychevo.channel.feishu-lark",
        _ => "a compatible Channel Extension",
    }
}

pub(super) fn acquire_extension_activity_lock(
    record: &ExtensionInstallRecord,
) -> Result<ExtensionActivityLock> {
    fs::create_dir_all(&record.data_root)?;
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(record.data_root.join(".activity.lock"))?;
    lock.try_lock_shared().map_err(|err| match err {
        std::fs::TryLockError::WouldBlock => Error::Config(format!(
            "Extension `{}` installation state is changing; retry the invocation",
            record.id
        )),
        std::fs::TryLockError::Error(err) => Error::Io(err),
    })?;
    Ok(ExtensionActivityLock(lock))
}

async fn download_bounded(response: reqwest::Response, limit: usize) -> Result<Vec<u8>> {
    if response
        .content_length()
        .is_some_and(|length| length > limit as u64)
    {
        return Err(Error::Config(format!(
            "Extension download exceeds {limit} byte limit"
        )));
    }
    let mut stream = response.bytes_stream();
    let mut bytes = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|_| {
            Error::Config("Extension download failed while reading the response body".to_string())
        })?;
        if bytes.len().saturating_add(chunk.len()) > limit {
            return Err(Error::Config(format!(
                "Extension download exceeds {limit} byte limit"
            )));
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

async fn fetch_https(
    client: &reqwest::Client,
    url: &str,
    label: &str,
) -> Result<reqwest::Response> {
    let response = client.get(url).send().await.map_err(|error| {
        let reason = if error.is_timeout() {
            "timed out"
        } else if error.is_connect() {
            "could not connect"
        } else {
            "request failed"
        };
        Error::Config(format!(
            "Extension {label} download {reason}: {}",
            redact_url(url)
        ))
    })?;
    if response.url().scheme() != "https" {
        return Err(Error::Config(format!(
            "Extension {label} redirected away from HTTPS"
        )));
    }
    if !response.status().is_success() {
        return Err(Error::Config(format!(
            "Extension {label} download returned HTTP {} from {}",
            response.status(),
            redact_url(url)
        )));
    }
    Ok(response)
}

pub(super) fn remote_package_client() -> Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .https_only(true)
        .redirect(reqwest::redirect::Policy::limited(5))
        .build()?)
}

fn require_https(value: &str, label: &str) -> Result<()> {
    let url = reqwest::Url::parse(value)
        .map_err(|_| Error::Config(format!("Extension {label} must be a valid HTTPS URL")))?;
    if url.scheme() != "https" || url.host_str().is_none() {
        return Err(Error::Config(format!("Extension {label} must use HTTPS")));
    }
    Ok(())
}

fn normalized_sha256(value: &str) -> Result<String> {
    let value = value.strip_prefix("sha256:").unwrap_or(value);
    if value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        Ok(value.to_ascii_lowercase())
    } else {
        Err(Error::Config(
            "Extension artifact sha256 must contain 64 hexadecimal characters".to_string(),
        ))
    }
}

fn redact_url(value: &str) -> String {
    let Ok(mut url) = reqwest::Url::parse(value) else {
        return "https://invalid".to_string();
    };
    let _ = url.set_username("");
    let _ = url.set_password(None);
    url.set_query(None);
    url.set_fragment(None);
    url.to_string()
}

fn safe_segment(value: &str) -> String {
    let output = value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '_') {
                ch
            } else {
                '-'
            }
        })
        .collect::<String>();
    if output.is_empty() {
        "release".to_string()
    } else {
        output
    }
}

fn current_target_triple() -> String {
    let arch = std::env::consts::ARCH;
    if cfg!(target_os = "linux") {
        let environment = if cfg!(target_env = "musl") {
            "musl"
        } else {
            "gnu"
        };
        format!("{arch}-unknown-linux-{environment}")
    } else if cfg!(target_os = "macos") {
        format!("{arch}-apple-darwin")
    } else if cfg!(target_os = "windows") {
        let environment = if cfg!(target_env = "gnu") {
            "gnu"
        } else {
            "msvc"
        };
        format!("{arch}-pc-windows-{environment}")
    } else {
        format!("{arch}-unknown-{}", std::env::consts::OS)
    }
}

fn validate_materialized_executable(manifest: &ExtensionManifest) -> Result<()> {
    let executable = manifest.runtime.executable.canonicalize().map_err(|err| {
        Error::Config(format!(
            "Extension `{}` executable {} is unavailable: {err}",
            manifest.id,
            manifest.runtime.executable.display()
        ))
    })?;
    let root = manifest.root.canonicalize()?;
    if !executable.starts_with(&root) || !executable.is_file() {
        return Err(Error::Config(format!(
            "Extension `{}` executable must remain inside its package root",
            manifest.id
        )));
    }
    Ok(())
}

fn write_record_atomic(path: &Path, record: &ExtensionInstallRecord) -> Result<()> {
    let parent = path.parent().ok_or_else(|| {
        Error::Config(format!(
            "Extension record path has no parent: {}",
            path.display()
        ))
    })?;
    fs::create_dir_all(parent)?;
    let mut file = tempfile::NamedTempFile::new_in(parent)?;
    serde_json::to_writer_pretty(file.as_file_mut(), record)?;
    file.as_file_mut().sync_all()?;
    file.persist(path).map_err(|err| Error::Io(err.error))?;
    Ok(())
}

#[derive(Debug, Clone, Default)]
pub struct ExtensionCommandCatalog {
    owners: BTreeMap<String, String>,
}

impl ExtensionCommandCatalog {
    pub fn build(manifests: &[ExtensionManifest], builtins: &[&str]) -> Result<Self> {
        let builtins = builtins.iter().copied().collect::<BTreeSet<_>>();
        let mut owners = BTreeMap::<String, String>::new();
        for manifest in manifests {
            for command in &manifest.contributions.commands {
                if builtins.contains(command.name.as_str()) {
                    return Err(Error::Config(format!(
                        "Extension `{}` command `{}` conflicts with a built-in pevo command",
                        manifest.id, command.name
                    )));
                }
                if let Some(existing) = owners.insert(command.name.clone(), manifest.id.clone()) {
                    return Err(Error::Config(format!(
                        "Extension command `{}` conflicts between `{existing}` and `{}`",
                        command.name, manifest.id
                    )));
                }
            }
        }
        Ok(Self { owners })
    }

    pub fn owner(&self, command: &str) -> Option<&str> {
        self.owners.get(command).map(String::as_str)
    }
}
