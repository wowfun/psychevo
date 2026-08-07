use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use psychevo::extensions::protocol::HostCapabilities;
use psychevo::extensions::{
    ExtensionHostMode, ExtensionInstallRecord, ExtensionLease, ExtensionRuntime, ExtensionScope,
    ExtensionStore, load_extension_manifest,
};
use psychevo::{Error, Result};
use psychevo_gateway_protocol::capability_results::{
    ExtensionAppCloseResult, ExtensionAppOpenResult, ExtensionListResult, ExtensionMutationResult,
    ExtensionReadResult, ExtensionView,
};
use serde_json::to_value;
use uuid::Uuid;

struct AppLeaseEntry {
    extension_id: String,
    owner: String,
    lease: Option<ExtensionLease>,
}

#[derive(Default)]
struct ExtensionLeaseRegistryInner {
    apps: BTreeMap<String, AppLeaseEntry>,
    activities: BTreeMap<String, ExtensionActivityEntry>,
    runtimes: BTreeMap<String, Arc<ExtensionRuntime>>,
}

struct ExtensionActivityEntry {
    extension_id: String,
    reason: String,
}

#[derive(Clone, Default)]
pub(super) struct ExtensionAppLeaseStore {
    inner: Arc<Mutex<ExtensionLeaseRegistryInner>>,
}

pub(super) struct ExtensionActivityRegistration<'a> {
    inner: Arc<Mutex<ExtensionLeaseRegistryInner>>,
    guard: MutexGuard<'a, ExtensionLeaseRegistryInner>,
}

pub(super) struct ExtensionActivityGuard {
    id: String,
    inner: Arc<Mutex<ExtensionLeaseRegistryInner>>,
}

struct AppLeaseReservation {
    lease_id: String,
    owner: String,
    inner: Arc<Mutex<ExtensionLeaseRegistryInner>>,
    committed: bool,
}

struct ExtensionMutationGuard<'a> {
    _guard: MutexGuard<'a, ExtensionLeaseRegistryInner>,
}

pub(super) struct ExtensionAppOpenRequest<'a> {
    pub(super) owner: &'a str,
    pub(super) home: &'a std::path::Path,
    pub(super) cwd: &'a std::path::Path,
    pub(super) inherited_env: &'a BTreeMap<String, String>,
    pub(super) selector: &'a str,
    pub(super) scope_name: Option<&'a str>,
    pub(super) app_id: &'a str,
}

impl Drop for ExtensionActivityGuard {
    fn drop(&mut self) {
        self.inner
            .lock()
            .expect("Extension lease registry poisoned")
            .activities
            .remove(&self.id);
    }
}

impl Drop for AppLeaseReservation {
    fn drop(&mut self) {
        if !self.committed {
            self.inner
                .lock()
                .expect("Extension lease registry poisoned")
                .apps
                .remove(&self.lease_id);
        }
    }
}

impl AppLeaseReservation {
    fn lease_id(&self) -> &str {
        &self.lease_id
    }

    fn commit(mut self, lease: ExtensionLease) -> Result<()> {
        let mut inner = self
            .inner
            .lock()
            .expect("Extension lease registry poisoned");
        let entry = inner.apps.get_mut(&self.lease_id).ok_or_else(|| {
            Error::Config("Extension App lease was cancelled while opening".to_string())
        })?;
        if entry.owner != self.owner {
            return Err(Error::Config(
                "Extension App lease belongs to another connection".to_string(),
            ));
        }
        entry.lease = Some(lease);
        self.committed = true;
        Ok(())
    }
}

impl ExtensionActivityRegistration<'_> {
    pub(super) fn register(
        mut self,
        extension_id: String,
        reason: impl Into<String>,
    ) -> ExtensionActivityGuard {
        let id = Uuid::now_v7().to_string();
        self.guard.activities.insert(
            id.clone(),
            ExtensionActivityEntry {
                extension_id,
                reason: reason.into(),
            },
        );
        ExtensionActivityGuard {
            id,
            inner: Arc::clone(&self.inner),
        }
    }
}

impl ExtensionAppLeaseStore {
    pub(super) fn runtime_for(
        &self,
        record: ExtensionInstallRecord,
        manifest: psychevo::extensions::ExtensionManifest,
        inherited_env: &BTreeMap<String, String>,
    ) -> Result<Arc<ExtensionRuntime>> {
        let key = format!(
            "{}@{}:{}",
            record.id,
            record.scope.as_str(),
            record.fingerprint
        );
        let mut inner = self
            .inner
            .lock()
            .expect("Extension lease registry poisoned");
        if let Some(runtime) = inner.runtimes.get(&key) {
            return Ok(Arc::clone(runtime));
        }
        let runtime = ExtensionRuntime::with_capabilities(
            record,
            manifest,
            inherited_env.clone(),
            ExtensionHostMode::Leased {
                idle_timeout: Duration::from_secs(5 * 60),
            },
            HostCapabilities {
                structured_displays: true,
                mcp_apps: true,
                channels: true,
            },
        )?;
        inner.runtimes.insert(key, Arc::clone(&runtime));
        Ok(runtime)
    }

    pub(super) fn begin_activity(&self) -> ExtensionActivityRegistration<'_> {
        ExtensionActivityRegistration {
            inner: Arc::clone(&self.inner),
            guard: self
                .inner
                .lock()
                .expect("Extension lease registry poisoned"),
        }
    }

    fn reserve_app(&self, owner: &str, extension_id: String) -> AppLeaseReservation {
        let lease_id = Uuid::now_v7().to_string();
        self.inner
            .lock()
            .expect("Extension lease registry poisoned")
            .apps
            .insert(
                lease_id.clone(),
                AppLeaseEntry {
                    extension_id,
                    owner: owner.to_string(),
                    lease: None,
                },
            );
        AppLeaseReservation {
            lease_id,
            owner: owner.to_string(),
            inner: Arc::clone(&self.inner),
            committed: false,
        }
    }

    fn mutation_guard(
        &self,
        extension_key: &str,
        extension_id: &str,
    ) -> Result<ExtensionMutationGuard<'_>> {
        let mut guard = self
            .inner
            .lock()
            .expect("Extension lease registry poisoned");
        let active_reason = guard
            .apps
            .values()
            .find(|entry| entry.extension_id == extension_key)
            .map(|_| "MCP App")
            .or_else(|| {
                guard
                    .activities
                    .values()
                    .find(|entry| entry.extension_id == extension_key)
                    .map(|entry| entry.reason.as_str())
            });
        if let Some(reason) = active_reason {
            return Err(Error::Config(format!(
                "Extension `{extension_id}` has an active {reason} lease; close or stop it before changing installation state"
            )));
        }
        let runtime_prefix = format!("{extension_key}:");
        guard
            .runtimes
            .retain(|key, _| !key.starts_with(&runtime_prefix));
        Ok(ExtensionMutationGuard { _guard: guard })
    }

    pub(super) async fn release(&self, owner: &str, lease_id: &str) -> Result<bool> {
        let entry = {
            let mut inner = self
                .inner
                .lock()
                .expect("Extension lease registry poisoned");
            let entries = &mut inner.apps;
            if entries
                .get(lease_id)
                .is_some_and(|entry| entry.owner != owner)
            {
                return Err(Error::Config(
                    "Extension App lease belongs to another connection".to_string(),
                ));
            }
            entries.remove(lease_id)
        };
        if let Some(entry) = entry {
            if let Some(lease) = entry.lease {
                lease.release().await?;
            }
            Ok(true)
        } else {
            Ok(false)
        }
    }

    pub(super) async fn release_owner(&self, owner: &str) {
        let leases = {
            let mut inner = self
                .inner
                .lock()
                .expect("Extension lease registry poisoned");
            let entries = &mut inner.apps;
            let ids = entries
                .iter()
                .filter(|(_, entry)| entry.owner == owner)
                .map(|(id, _)| id.clone())
                .collect::<Vec<_>>();
            ids.into_iter()
                .filter_map(|id| entries.remove(&id).and_then(|entry| entry.lease))
                .collect::<Vec<_>>()
        };
        for lease in leases {
            let _ = lease.release().await;
        }
    }

    pub(super) fn reason_for(&self, extension_key: &str) -> Option<String> {
        let inner = self
            .inner
            .lock()
            .expect("Extension lease registry poisoned");
        inner
            .apps
            .values()
            .any(|entry| entry.extension_id == extension_key)
            .then(|| "mcp_app".to_string())
            .or_else(|| {
                inner
                    .activities
                    .values()
                    .find(|entry| entry.extension_id == extension_key)
                    .map(|entry| entry.reason.clone())
            })
    }
}

pub(super) fn extension_list_result(
    home: &std::path::Path,
    cwd: &std::path::Path,
) -> Result<ExtensionListResult> {
    let store = ExtensionStore::new(home, cwd);
    let mut records = store.records(ExtensionScope::Profile)?;
    records.extend(store.records(ExtensionScope::Local)?);
    records.sort_by(|left, right| {
        left.id
            .cmp(&right.id)
            .then_with(|| left.scope.as_str().cmp(right.scope.as_str()))
    });
    let extensions = records.iter().map(extension_view).collect();
    Ok(ExtensionListResult {
        count: records.len(),
        extensions,
    })
}

pub(super) fn extension_read_result(
    home: &std::path::Path,
    cwd: &std::path::Path,
    selector: &str,
    scope_name: Option<&str>,
) -> Result<ExtensionReadResult> {
    let store = ExtensionStore::new(home, cwd);
    let (id, scope) = select_record(&store, selector, scope_name)?;
    let record = store.read_record(&id, scope)?.ok_or_else(|| {
        Error::Config(format!(
            "Extension `{id}` is not installed in {} scope",
            scope.as_str()
        ))
    })?;
    let manifest = load_extension_manifest(&record.package_root)?;
    Ok(ExtensionReadResult {
        extension: extension_view(&record),
        manifest: to_value(manifest)?,
    })
}

pub(super) fn extension_remove_result(
    leases: &ExtensionAppLeaseStore,
    home: &std::path::Path,
    cwd: &std::path::Path,
    selector: &str,
    scope_name: Option<&str>,
) -> Result<ExtensionMutationResult> {
    let store = ExtensionStore::new(home, cwd);
    let (id, scope) = select_record(&store, selector, scope_name)?;
    let _mutation = leases.mutation_guard(&extension_key(&id, scope), &id)?;
    let record = store.remove(&id, scope)?.ok_or_else(|| {
        Error::Config(format!(
            "Extension `{id}` is not installed in {} scope",
            scope.as_str()
        ))
    })?;
    Ok(ExtensionMutationResult {
        success: true,
        id: record.id,
        scope: record.scope.as_str().to_string(),
        enabled: None,
    })
}

pub(super) fn extension_set_enabled_result(
    leases: &ExtensionAppLeaseStore,
    home: &std::path::Path,
    cwd: &std::path::Path,
    selector: &str,
    scope_name: Option<&str>,
    enabled: bool,
) -> Result<ExtensionMutationResult> {
    let store = ExtensionStore::new(home, cwd);
    let (id, scope) = select_record(&store, selector, scope_name)?;
    let _mutation = leases.mutation_guard(&extension_key(&id, scope), &id)?;
    let record = store.set_enabled(&id, scope, enabled)?;
    Ok(ExtensionMutationResult {
        success: true,
        id: record.id,
        scope: record.scope.as_str().to_string(),
        enabled: Some(record.enabled),
    })
}

pub(super) async fn extension_app_open_result(
    leases: &ExtensionAppLeaseStore,
    request: ExtensionAppOpenRequest<'_>,
) -> Result<ExtensionAppOpenResult> {
    let ExtensionAppOpenRequest {
        owner,
        home,
        cwd,
        inherited_env,
        selector,
        scope_name,
        app_id,
    } = request;
    let store = ExtensionStore::new(home, cwd);
    let (id, scope) = select_record(&store, selector, scope_name)?;
    let reservation = leases.reserve_app(owner, extension_key(&id, scope));
    let record = store.read_record(&id, scope)?.ok_or_else(|| {
        Error::Config(format!(
            "Extension `{id}` is not installed in {} scope",
            scope.as_str()
        ))
    })?;
    if !record.enabled {
        return Err(Error::Config(format!("Extension `{id}` is disabled")));
    }
    let actual = psychevo::plugins::external_plugin_fingerprint(
        Some(&record.package_root),
        &record.id,
        Some(&record.version),
    )?;
    if actual != record.fingerprint || actual != record.trusted_fingerprint {
        return Err(Error::Config(format!(
            "Extension `{id}` changed after installation; reinstall it before opening an App"
        )));
    }
    let manifest = load_extension_manifest(&record.package_root)?;
    let app = manifest
        .contributions
        .mcp_apps
        .iter()
        .find(|app| app.id == app_id)
        .cloned()
        .ok_or_else(|| {
            Error::Config(format!(
                "Extension `{id}` does not declare MCP App `{app_id}`"
            ))
        })?;
    if !app.allowed_tools.is_empty() {
        return Err(Error::Config(format!(
            "MCP App `{app_id}` requires tool routing through an active Thread surface; use its fallback until that surface owns approval"
        )));
    }
    let resource_url = app.resource_url.clone().ok_or_else(|| {
        Error::Config(format!(
            "MCP App `{app_id}` has no verified Web/Desktop resource URL"
        ))
    })?;
    let runtime = leases.runtime_for(record, manifest, inherited_env)?;
    let lease = runtime.acquire().await?;
    let contributions = lease.contributions().await;
    let contributions = match contributions {
        Ok(contributions) => contributions,
        Err(err) => {
            let _ = lease.release().await;
            return Err(err);
        }
    };
    let runtime_app = contributions
        .mcp_apps
        .iter()
        .find(|runtime_app| {
            runtime_app.id == app.id && runtime_app.resource_uri == app.resource_uri
        })
        .ok_or_else(|| {
            Error::Config(format!(
                "Extension `{id}` did not confirm MCP App `{app_id}` through contributions/list"
            ))
        });
    if let Err(err) = runtime_app {
        let _ = lease.release().await;
        return Err(err);
    }
    let result = ExtensionAppOpenResult {
        lease_id: reservation.lease_id().to_string(),
        extension_id: id,
        app_id: app.id,
        resource_uri: app.resource_uri,
        resource_url,
        resource_domains: app.resource_domains,
        connect_domains: app.connect_domains,
        allowed_tools: app.allowed_tools,
        fallback: app.fallback,
    };
    reservation.commit(lease)?;
    Ok(result)
}

pub(super) async fn extension_app_close_result(
    leases: &ExtensionAppLeaseStore,
    owner: &str,
    lease_id: &str,
) -> Result<ExtensionAppCloseResult> {
    Ok(ExtensionAppCloseResult {
        released: leases.release(owner, lease_id).await?,
    })
}

fn select_record(
    store: &ExtensionStore,
    selector: &str,
    scope_name: Option<&str>,
) -> Result<(String, ExtensionScope)> {
    let (id, inline_scope) = selector
        .rsplit_once('@')
        .and_then(|(id, scope)| parse_scope(scope).ok().map(|scope| (id, scope)))
        .map_or((selector, None), |(id, scope)| (id, Some(scope)));
    let explicit_scope = match scope_name {
        Some(value) => Some(parse_scope(value)?),
        None => None,
    };
    if inline_scope.is_some() && explicit_scope.is_some() && inline_scope != explicit_scope {
        return Err(Error::Config(
            "Extension selector scope conflicts with scopeName".to_string(),
        ));
    }
    if let Some(scope) = inline_scope.or(explicit_scope) {
        return Ok((id.to_string(), scope));
    }
    let mut matches = Vec::new();
    for scope in [ExtensionScope::Local, ExtensionScope::Profile] {
        if store.read_record(id, scope)?.is_some() {
            matches.push(scope);
        }
    }
    match matches.as_slice() {
        [] => Err(Error::Config(format!("Extension `{id}` is not installed"))),
        [scope] => Ok((id.to_string(), *scope)),
        _ => Err(Error::Config(format!(
            "Extension `{id}` exists in profile and local scopes; select `{id}@profile` or `{id}@local`"
        ))),
    }
}

fn parse_scope(value: &str) -> Result<ExtensionScope> {
    match value.trim() {
        "profile" => Ok(ExtensionScope::Profile),
        "local" => Ok(ExtensionScope::Local),
        other => Err(Error::Config(format!(
            "invalid Extension scope `{other}`; expected `profile` or `local`"
        ))),
    }
}

fn extension_key(id: &str, scope: ExtensionScope) -> String {
    format!("{id}@{}", scope.as_str())
}

fn extension_view(record: &ExtensionInstallRecord) -> ExtensionView {
    let mut diagnostics = Vec::new();
    let manifest = match load_extension_manifest(&record.package_root) {
        Ok(manifest) => Some(manifest),
        Err(err) => {
            diagnostics.push(err.to_string());
            None
        }
    };
    let actual_fingerprint = psychevo::plugins::external_plugin_fingerprint(
        Some(&record.package_root),
        &record.id,
        Some(&record.version),
    )
    .map_err(|err| diagnostics.push(err.to_string()))
    .ok();
    let trusted = actual_fingerprint.as_deref() == Some(record.fingerprint.as_str())
        && record.fingerprint == record.trusted_fingerprint;
    if actual_fingerprint.is_some() && !trusted {
        diagnostics
            .push("installed package fingerprint no longer matches trust record".to_string());
    }
    if let Some(manifest) = &manifest
        && (manifest.id != record.id || manifest.version != record.version)
    {
        diagnostics.push("manifest identity no longer matches install record".to_string());
    }
    let mut permissions = BTreeSet::new();
    if let Some(manifest) = &manifest {
        for command in &manifest.contributions.commands {
            permissions.extend(command.required_capabilities.iter().cloned());
        }
        for channel in &manifest.contributions.channels {
            permissions.insert(format!("channel:{}", channel.channel));
        }
        for tool in &manifest.contributions.tools {
            permissions.insert(format!("tool:{}", tool.name));
        }
        for hook in &manifest.contributions.hooks {
            permissions.insert(format!("hook:{}", hook.name));
        }
    }
    let app_readiness = match &manifest {
        None => "unavailable",
        Some(manifest) if manifest.contributions.mcp_apps.is_empty() => "not_applicable",
        Some(manifest)
            if manifest.contributions.mcp_apps.iter().all(|app| {
                app.resource_url.is_some()
                    && !app.resource_domains.is_empty()
                    && !app.fallback.trim().is_empty()
                    && app.allowed_tools.is_empty()
            }) =>
        {
            "ready"
        }
        Some(_) => "fallback_only",
    };
    ExtensionView {
        id: record.id.clone(),
        selector: format!("{}@{}", record.id, record.scope.as_str()),
        version: record.version.clone(),
        display_name: manifest
            .as_ref()
            .and_then(|manifest| manifest.display_name.clone()),
        description: manifest
            .as_ref()
            .and_then(|manifest| manifest.description.clone()),
        source: record.source.clone(),
        source_kind: record.source_kind.clone(),
        scope: record.scope.as_str().to_string(),
        enabled: record.enabled,
        trusted,
        fingerprint: actual_fingerprint.unwrap_or_else(|| record.fingerprint.clone()),
        trusted_fingerprint: record.trusted_fingerprint.clone(),
        protocol: manifest
            .as_ref()
            .map(|manifest| manifest.runtime.protocol.clone())
            .unwrap_or_else(|| "unknown".to_string()),
        protocol_compatible: manifest.is_some(),
        permissions: permissions.into_iter().collect(),
        sidecar_state: "not_started".to_string(),
        lease_reason: None,
        csp_app_readiness: app_readiness.to_string(),
        co_root_plugin: record
            .plugin_manifest
            .as_ref()
            .map(|path| path.display().to_string()),
        diagnostics,
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    #[cfg(unix)]
    fn install_fixture(
        home: &std::path::Path,
        cwd: &std::path::Path,
        id: &str,
        scope: ExtensionScope,
    ) {
        use std::os::unix::fs::PermissionsExt;

        let root = cwd.join(format!("fixture-{id}"));
        fs::create_dir_all(&root).expect("fixture root");
        fs::write(root.join("sidecar"), "#!/bin/sh\nexit 0\n").expect("sidecar");
        let mut permissions = fs::metadata(root.join("sidecar"))
            .expect("metadata")
            .permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(root.join("sidecar"), permissions).expect("permissions");
        fs::write(
            root.join("psychevo.extension.json"),
            format!(r#"{{"schemaVersion":1,"id":"{id}","version":"local","runtime":{{"protocol":"psychevo-extension/1","executable":"./sidecar"}},"contributions":{{"commands":[{{"name":"hello","usage":"hello","summary":"Hello","requiredCapabilities":["workspace.read"]}}]}}}}"#),
        )
        .expect("manifest");
        ExtensionStore::new(home, cwd)
            .install_local(&root, scope)
            .expect("install fixture");
    }

    #[cfg(unix)]
    fn install_app_fixture(
        home: &std::path::Path,
        cwd: &std::path::Path,
    ) -> ExtensionInstallRecord {
        use std::os::unix::fs::PermissionsExt;

        let root = cwd.join("fixture-example-app");
        fs::create_dir_all(&root).expect("fixture root");
        let source = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../psychevo/tests/fixtures/extension_echo_sidecar.py");
        fs::copy(source, root.join("sidecar.py")).expect("copy sidecar");
        let mut permissions = fs::metadata(root.join("sidecar.py"))
            .expect("metadata")
            .permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(root.join("sidecar.py"), permissions).expect("permissions");
        fs::write(
            root.join("psychevo.extension.json"),
            r#"{
              "schemaVersion": 1,
              "id": "example.app",
              "version": "local",
              "runtime": {
                "protocol": "psychevo-extension/1",
                "executable": "./sidecar.py"
              },
              "contributions": {
                "mcpApps": [{
                  "id": "dashboard",
                  "resourceUri": "ui://example/dashboard.html",
                  "fallback": "Use the dashboard text fallback.",
                  "resourceUrl": "https://apps.example.test/dashboard.html",
                  "resourceDomains": ["https://apps.example.test"]
                }]
              }
            }"#,
        )
        .expect("manifest");
        ExtensionStore::new(home, cwd)
            .install_local(&root, ExtensionScope::Profile)
            .expect("install App fixture")
    }

    #[test]
    #[cfg(unix)]
    fn static_list_reports_policy_without_starting_sidecar() {
        let temp = tempfile::tempdir().expect("temp");
        let home = temp.path().join("home");
        let cwd = temp.path().join("workspace");
        fs::create_dir_all(&cwd).expect("cwd");
        install_fixture(&home, &cwd, "example.extension", ExtensionScope::Profile);

        let result = extension_list_result(&home, &cwd).expect("list");
        assert_eq!(result.count, 1);
        let extension = &result.extensions[0];
        assert!(extension.enabled);
        assert!(extension.trusted);
        assert_eq!(extension.permissions, ["workspace.read"]);
        assert_eq!(extension.sidecar_state, "not_started");
        assert_eq!(extension.lease_reason, None);
    }

    #[test]
    #[cfg(unix)]
    fn ambiguous_selector_requires_scope_and_mutations_are_explicit() {
        let temp = tempfile::tempdir().expect("temp");
        let home = temp.path().join("home");
        let cwd = temp.path().join("workspace");
        fs::create_dir_all(&cwd).expect("cwd");
        install_fixture(&home, &cwd, "example.extension", ExtensionScope::Profile);
        install_fixture(&home, &cwd, "example.extension", ExtensionScope::Local);

        let error = extension_read_result(&home, &cwd, "example.extension", None)
            .expect_err("ambiguous selector");
        assert!(
            error
                .to_string()
                .contains("exists in profile and local scopes")
        );
        let leases = ExtensionAppLeaseStore::default();
        let mutation = extension_set_enabled_result(
            &leases,
            &home,
            &cwd,
            "example.extension@local",
            None,
            false,
        )
        .expect("disable local");
        assert_eq!(mutation.enabled, Some(false));
        let read = extension_read_result(&home, &cwd, "example.extension@local", None)
            .expect("read local");
        assert!(!read.extension.enabled);
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn app_open_confirms_runtime_descriptor_and_owns_connection_lease() {
        let temp = tempfile::tempdir().expect("temp");
        let home = temp.path().join("home");
        let cwd = temp.path().join("workspace");
        fs::create_dir_all(&cwd).expect("cwd");
        install_app_fixture(&home, &cwd);
        let leases = ExtensionAppLeaseStore::default();

        let opened = extension_app_open_result(
            &leases,
            ExtensionAppOpenRequest {
                owner: "connection-a",
                home: &home,
                cwd: &cwd,
                inherited_env: &BTreeMap::new(),
                selector: "example.app@profile",
                scope_name: None,
                app_id: "dashboard",
            },
        )
        .await
        .expect("open App");
        assert_eq!(
            opened.resource_url,
            "https://apps.example.test/dashboard.html"
        );
        assert_eq!(
            leases.reason_for("example.app@profile").as_deref(),
            Some("mcp_app")
        );
        let mutation =
            extension_set_enabled_result(&leases, &home, &cwd, "example.app@profile", None, false)
                .expect_err("active App blocks policy mutation");
        assert!(mutation.to_string().contains("active MCP App lease"));
        let error = extension_app_close_result(&leases, "connection-b", &opened.lease_id)
            .await
            .expect_err("other owner");
        assert!(error.to_string().contains("another connection"));
        let closed = extension_app_close_result(&leases, "connection-a", &opened.lease_id)
            .await
            .expect("close App");
        assert!(closed.released);
        assert_eq!(leases.reason_for("example.app@profile"), None);
        extension_set_enabled_result(&leases, &home, &cwd, "example.app@profile", None, false)
            .expect("closed App permits policy mutation");
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn concurrent_app_leases_reuse_one_fingerprint_runtime() {
        let temp = tempfile::tempdir().expect("temp");
        let home = temp.path().join("home");
        let cwd = temp.path().join("workspace");
        fs::create_dir_all(&cwd).expect("cwd");
        let record = install_app_fixture(&home, &cwd);
        let leases = ExtensionAppLeaseStore::default();

        let first = extension_app_open_result(
            &leases,
            ExtensionAppOpenRequest {
                owner: "connection-a",
                home: &home,
                cwd: &cwd,
                inherited_env: &BTreeMap::new(),
                selector: "example.app@profile",
                scope_name: None,
                app_id: "dashboard",
            },
        )
        .await
        .expect("first App open");
        let second = extension_app_open_result(
            &leases,
            ExtensionAppOpenRequest {
                owner: "connection-b",
                home: &home,
                cwd: &cwd,
                inherited_env: &BTreeMap::new(),
                selector: "example.app@profile",
                scope_name: None,
                app_id: "dashboard",
            },
        )
        .await
        .expect("second App open");

        let lifecycle =
            fs::read_to_string(record.data_root.join("lifecycle.log")).expect("lifecycle log");
        assert_eq!(
            lifecycle
                .lines()
                .filter(|line| *line == "initialize")
                .count(),
            1,
            "one Gateway runtime must own all leases for a fingerprint"
        );

        extension_app_close_result(&leases, "connection-a", &first.lease_id)
            .await
            .expect("close first App");
        extension_app_close_result(&leases, "connection-b", &second.lease_id)
            .await
            .expect("close second App");
    }

    #[test]
    #[cfg(unix)]
    fn channel_activity_blocks_mutation_until_its_guard_drops() {
        let temp = tempfile::tempdir().expect("temp");
        let home = temp.path().join("home");
        let cwd = temp.path().join("workspace");
        fs::create_dir_all(&cwd).expect("cwd");
        install_fixture(&home, &cwd, "example.extension", ExtensionScope::Profile);
        let leases = ExtensionAppLeaseStore::default();
        let activity = leases
            .begin_activity()
            .register("example.extension@profile".to_string(), "Channel");

        let error =
            extension_remove_result(&leases, &home, &cwd, "example.extension@profile", None)
                .expect_err("active Channel blocks removal");
        assert!(error.to_string().contains("active Channel lease"));

        drop(activity);
        extension_remove_result(&leases, &home, &cwd, "example.extension@profile", None)
            .expect("stopped Channel permits removal");
    }
}
