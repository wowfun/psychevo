use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use futures::future::BoxFuture;
use psychevo_agent_core::{ToolBinding, ToolExecutionMode, ToolOutput};
use psychevo_ai::AbortSignal;
use psychevo_runtime::{Error, Result};
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::sync::{Mutex, MutexGuard, Notify, mpsc, oneshot};
use tokio::time::timeout;

const DEFAULT_REQUEST_TIMEOUT: Duration = Duration::from_secs(20);
const ELICITATION_TIMEOUT: Duration = Duration::from_secs(120);
const ELICITATION_REQUEST_TIMEOUT: Duration = Duration::from_secs(125);
const RUNTIME_INVENTORY_RETRY_DELAY: Duration = Duration::from_secs(5);
const REVIEWED_CODEX_VERSION: &str = "0.144.1";
const REQUIRED_CODEX_METHODS: &[&str] = &[
    "marketplace/add",
    "marketplace/remove",
    "marketplace/upgrade",
    "plugin/list",
    "plugin/installed",
    "plugin/read",
    "plugin/install",
    "plugin/uninstall",
    "app/list",
    "hooks/list",
    "mcpServer/oauth/login",
    "mcpServerStatus/list",
    "mcpServer/tool/call",
    "thread/start",
    "thread/archive",
];
const CONNECT_SESSION_TTL: Duration = Duration::from_secs(5 * 60);

fn log_codex_authority_event(event: &str, cwd: &Path, reason: Option<&str>) {
    eprintln!(
        "{}",
        json!({
            "target": "psychevo.codex_plugins",
            "event": event,
            "cwd": cwd,
            "reason": reason,
        })
    );
}

#[derive(Debug, Clone)]
struct BrokerCommand {
    program: PathBuf,
    args: Vec<String>,
}

impl BrokerCommand {
    fn from_profile(
        config: &psychevo_runtime::CodexPluginsConfig,
        env: &BTreeMap<String, String>,
    ) -> Self {
        let program = config
            .binary
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("codex"));
        #[cfg(test)]
        let program = env
            .get("PSYCHEVO_CODEX_BIN")
            .filter(|value| !value.trim().is_empty())
            .map(PathBuf::from)
            .unwrap_or(program);
        #[cfg(not(test))]
        let _ = env;
        Self {
            program,
            args: vec![
                "app-server".to_string(),
                "--strict-config".to_string(),
                "-c".to_string(),
                "cli_auth_credentials_store=\"file\"".to_string(),
                "--listen".to_string(),
                "stdio://".to_string(),
            ],
        }
    }
}

/// Versioned external capability authority for Codex plugins.
///
/// The public surface is deliberately small: authority state, management
/// operations, a non-blocking turn snapshot/lease, and shutdown. Process,
/// compatibility, inventory, trust, and connection details stay behind this
/// boundary.
pub(super) struct CodexPluginAuthority {
    command: std::sync::RwLock<BrokerCommand>,
    env: BTreeMap<String, String>,
    enabled: AtomicBool,
    private_home: PathBuf,
    auth_available: AtomicBool,
    process_ready: AtomicBool,
    enforce_compatibility: bool,
    enforce_policy: bool,
    incompatible: AtomicBool,
    negotiated_version: std::sync::RwLock<Option<String>>,
    compatibility_error: std::sync::RwLock<Option<String>>,
    generation: std::sync::atomic::AtomicU64,
    inventory_ready: AtomicBool,
    connect_sessions: Mutex<BTreeMap<String, CodexConnectSession>>,
    active_leases: Mutex<BTreeMap<String, u64>>,
    lease_notify: Notify,
    draining: AtomicBool,
    destructive_mutation: Mutex<()>,
    request_timeout: Duration,
    runtime_inventory_retry_delay: Duration,
    process: Mutex<Option<Arc<BrokerProcess>>>,
    thread_ids: Mutex<BTreeMap<String, String>>,
    runtime_inventories: Mutex<BTreeMap<PathBuf, Arc<Mutex<CachedCodexRuntimeInventory>>>>,
}

// Keep the server wiring stable while the module name is migrated separately.
pub(super) type CodexCapabilityBroker = CodexPluginAuthority;

pub(super) struct CodexRuntimeContributions {
    pub(super) capability_roots: Vec<psychevo_runtime::SelectedCapabilityRoot>,
    pub(super) runtime_tools: Vec<psychevo_runtime::RuntimeTool>,
    pub(super) lease_id: Option<String>,
}

struct RuntimePluginDetail {
    identity: CodexPluginIdentity,
    plugin_id: String,
    plugin: Value,
    package_root: Option<PathBuf>,
}

#[derive(Clone)]
struct CodexRuntimeProfile {
    capability_roots: Vec<psychevo_runtime::SelectedCapabilityRoot>,
    delegated_tools: Vec<CodexDelegatedToolDescriptor>,
}

#[derive(Clone, PartialEq)]
#[allow(dead_code)]
struct CodexRuntimeInventory {
    capability_roots: Vec<psychevo_runtime::SelectedCapabilityRoot>,
    delegated_servers: BTreeSet<String>,
    delegated_tools: Vec<CodexDelegatedToolDescriptor>,
    warnings: Vec<String>,
}

#[derive(Default)]
struct CachedCodexRuntimeInventory {
    inventory: Option<Arc<CodexRuntimeInventory>>,
    failure: Option<CachedCodexRuntimeInventoryFailure>,
}

struct CachedCodexRuntimeInventoryFailure {
    message: String,
    retry_after: Instant,
}

#[derive(Clone, PartialEq)]
struct CodexDelegatedToolDescriptor {
    name: String,
    server_name: String,
    remote_name: String,
    description: String,
    parameters: Value,
}

#[derive(Debug, Clone)]
struct CodexConnectSession {
    selector: String,
    component_id: String,
    kind: String,
    authorization_url: String,
    expires_at: Instant,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct CodexTrustRecord {
    fingerprint: String,
    codex_version: String,
    trusted_at_ms: i64,
}

impl CodexPluginAuthority {
    pub(super) fn new(env: &BTreeMap<String, String>) -> Self {
        let profile_home = env
            .get("PSYCHEVO_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(".psychevo"));
        let config =
            psychevo_runtime::load_codex_plugins_profile_config(&profile_home).unwrap_or_default();
        let private_home = profile_home.join("codex");
        if config.enabled {
            let _ = ensure_private_home(&private_home);
        }
        let auth_available = config.enabled && prepare_private_auth_link(&private_home, env);
        let mut process_env = env.clone();
        process_env.remove("CODEX_HOME");
        process_env.remove("PSYCHEVO_CODEX_BIN");
        process_env.insert(
            "CODEX_HOME".to_string(),
            private_home.to_string_lossy().to_string(),
        );
        Self {
            command: std::sync::RwLock::new(BrokerCommand::from_profile(&config, env)),
            env: process_env,
            enabled: AtomicBool::new(config.enabled),
            private_home,
            auth_available: AtomicBool::new(auth_available),
            process_ready: AtomicBool::new(false),
            enforce_compatibility: true,
            enforce_policy: true,
            incompatible: AtomicBool::new(false),
            negotiated_version: std::sync::RwLock::new(None),
            compatibility_error: std::sync::RwLock::new(None),
            generation: std::sync::atomic::AtomicU64::new(1),
            inventory_ready: AtomicBool::new(false),
            connect_sessions: Mutex::new(BTreeMap::new()),
            active_leases: Mutex::new(BTreeMap::new()),
            lease_notify: Notify::new(),
            draining: AtomicBool::new(false),
            destructive_mutation: Mutex::new(()),
            request_timeout: DEFAULT_REQUEST_TIMEOUT,
            runtime_inventory_retry_delay: RUNTIME_INVENTORY_RETRY_DELAY,
            process: Mutex::new(None),
            thread_ids: Mutex::new(BTreeMap::new()),
            runtime_inventories: Mutex::new(BTreeMap::new()),
        }
    }

    #[cfg(test)]
    fn with_command(
        command: BrokerCommand,
        env: BTreeMap<String, String>,
        request_timeout: Duration,
    ) -> Self {
        Self::with_command_and_runtime_retry(
            command,
            env,
            request_timeout,
            RUNTIME_INVENTORY_RETRY_DELAY,
        )
    }

    #[cfg(test)]
    fn with_command_and_runtime_retry(
        command: BrokerCommand,
        env: BTreeMap<String, String>,
        request_timeout: Duration,
        runtime_inventory_retry_delay: Duration,
    ) -> Self {
        Self {
            command: std::sync::RwLock::new(command),
            env,
            enabled: AtomicBool::new(true),
            private_home: PathBuf::from("/fake"),
            auth_available: AtomicBool::new(false),
            process_ready: AtomicBool::new(false),
            enforce_compatibility: false,
            enforce_policy: false,
            incompatible: AtomicBool::new(false),
            negotiated_version: std::sync::RwLock::new(None),
            compatibility_error: std::sync::RwLock::new(None),
            generation: std::sync::atomic::AtomicU64::new(1),
            inventory_ready: AtomicBool::new(false),
            connect_sessions: Mutex::new(BTreeMap::new()),
            active_leases: Mutex::new(BTreeMap::new()),
            lease_notify: Notify::new(),
            draining: AtomicBool::new(false),
            destructive_mutation: Mutex::new(()),
            request_timeout,
            runtime_inventory_retry_delay,
            process: Mutex::new(None),
            thread_ids: Mutex::new(BTreeMap::new()),
            runtime_inventories: Mutex::new(BTreeMap::new()),
        }
    }

    pub(super) async fn request(&self, method: &str, params: Value) -> Result<Value> {
        self.request_with_context(method, params, None).await
    }

    async fn request_with_context(
        &self,
        method: &str,
        params: Value,
        context: Option<&CodexElicitationContext>,
    ) -> Result<Value> {
        if !self.enabled.load(Ordering::Acquire) {
            return Err(Error::Message(
                "Codex plugin authority is disabled in the active profile".to_string(),
            ));
        }
        let process = {
            let mut slot = self.process.lock().await;
            if slot.is_none() {
                let expected_home = self
                    .enforce_compatibility
                    .then_some(self.private_home.as_path());
                let command = self
                    .command
                    .read()
                    .expect("Codex command lock poisoned")
                    .clone();
                let spawned =
                    BrokerProcess::spawn(&command, &self.env, self.request_timeout, expected_home)
                        .await;
                let process = match spawned {
                    Ok(process) => process,
                    Err(err) => {
                        let message = err.to_string();
                        self.incompatible.store(
                            message.contains("Codex plugin compatibility profile"),
                            Ordering::Release,
                        );
                        *self
                            .compatibility_error
                            .write()
                            .expect("Codex compatibility error lock poisoned") = Some(message);
                        return Err(err);
                    }
                };
                *self
                    .negotiated_version
                    .write()
                    .expect("Codex version lock poisoned") = process.codex_version.clone();
                *self
                    .compatibility_error
                    .write()
                    .expect("Codex compatibility error lock poisoned") = None;
                self.incompatible.store(false, Ordering::Release);
                *slot = Some(Arc::new(process));
                self.process_ready.store(true, Ordering::Release);
            }
            slot.as_ref().expect("broker process initialized").clone()
        };
        let result = process
            .request_with_context(
                method,
                params,
                if context.is_some() {
                    ELICITATION_REQUEST_TIMEOUT
                } else {
                    self.request_timeout
                },
                context,
            )
            .await;
        let effective_plugins_changed = process.take_effective_plugins_changed();
        let broker_replaced = result.as_ref().is_err_and(|err| {
            let message = err.to_string();
            !message.starts_with("Codex broker request failed")
        });
        if broker_replaced {
            *self
                .compatibility_error
                .write()
                .expect("Codex compatibility error lock poisoned") =
                result.as_ref().err().map(ToString::to_string);
            let removed = {
                let mut slot = self.process.lock().await;
                if slot
                    .as_ref()
                    .is_some_and(|current| Arc::ptr_eq(current, &process))
                {
                    slot.take()
                } else {
                    None
                }
            };
            if let Some(process) = removed {
                process.kill().await;
            }
            self.process_ready.store(false, Ordering::Release);
        }
        if effective_plugins_changed || broker_replaced {
            self.invalidate_runtime_inventories().await;
        }
        result
    }

    pub(super) fn authority_view(&self) -> Value {
        let enabled = self.enabled.load(Ordering::Acquire);
        let incompatible = self.incompatible.load(Ordering::Acquire);
        let version = self
            .negotiated_version
            .read()
            .expect("Codex version lock poisoned")
            .clone();
        let reason = self
            .compatibility_error
            .read()
            .expect("Codex compatibility error lock poisoned")
            .clone();
        json!({
            "kind": "codex",
            "enabled": enabled,
            "runtime": if !enabled {
                "disabled"
            } else if self.draining.load(Ordering::Acquire) {
                "draining"
            } else if incompatible {
                "incompatible"
            } else if reason.is_some() {
                "unavailable"
            } else if self.process_ready.load(Ordering::Acquire) {
                "ready"
            } else {
                "starting"
            },
            "auth": if self.auth_available.load(Ordering::Acquire) {
                "available"
            } else {
                "unavailable"
            },
            "resolvedBinary": self.command.read().expect("Codex command lock poisoned").program,
            "version": version,
            "compatibilityProfile": psychevo_runtime::CODEX_PLUGIN_COMPATIBILITY_PROFILE,
            "privateHome": self.private_home,
            "platform": std::env::consts::OS,
            "generation": self.generation.load(Ordering::Acquire),
            "inventoryReady": self.inventory_ready.load(Ordering::Acquire),
            "reason": reason,
            "securityNotes": [
                "Codex runs with a Psychevo-private CODEX_HOME.",
                "Authentication is linked without reading or copying credential contents.",
                "Only reviewed Codex CLI 0.144.1 is admitted; version drift is blocked before inventory or execution."
            ]
        })
    }

    pub(super) fn is_enabled(&self) -> bool {
        self.enabled.load(Ordering::Acquire)
    }

    async fn begin_draining(&self) -> DrainingMutationGuard<'_> {
        let mutation = self.destructive_mutation.lock().await;
        self.draining.store(true, Ordering::Release);
        loop {
            let notified = self.lease_notify.notified();
            if self.active_leases.lock().await.is_empty() {
                break;
            }
            notified.await;
        }
        DrainingMutationGuard {
            authority: self,
            _mutation: mutation,
        }
    }

    pub(super) async fn release_turn_lease(&self, lease_id: &str) {
        if self.active_leases.lock().await.remove(lease_id).is_some() {
            self.lease_notify.notify_waiters();
        }
    }

    pub(super) async fn write_authority(
        &self,
        enabled: bool,
        binary: Option<&str>,
    ) -> Result<Value> {
        let profile_home = self.private_home.parent().ok_or_else(|| {
            Error::Message("Codex private home has no profile parent".to_string())
        })?;
        let write =
            psychevo_runtime::write_codex_plugins_profile_config(profile_home, enabled, binary)?;
        let _draining = self.begin_draining().await;
        self.enabled.store(false, Ordering::Release);
        self.stop().await;
        let config = psychevo_runtime::CodexPluginsConfig {
            enabled,
            binary: binary
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string),
        };
        *self.command.write().expect("Codex command lock poisoned") =
            BrokerCommand::from_profile(&config, &self.env);
        if enabled {
            ensure_private_home(&self.private_home)?;
        }
        let auth_available = enabled && prepare_private_auth_link(&self.private_home, &self.env);
        self.auth_available.store(auth_available, Ordering::Release);
        self.incompatible.store(false, Ordering::Release);
        *self
            .negotiated_version
            .write()
            .expect("Codex version lock poisoned") = None;
        *self
            .compatibility_error
            .write()
            .expect("Codex compatibility error lock poisoned") = None;
        self.invalidate_runtime_inventories().await;
        self.enabled.store(enabled, Ordering::Release);
        Ok(json!({
            "success": true,
            "write": write,
            "authority": self.authority_view(),
        }))
    }

    pub(super) async fn refresh_authority(&self, cwd: &Path) -> Result<Value> {
        if !self.is_enabled() {
            return Err(Error::Message(
                "Codex plugin authority is disabled in the active profile".to_string(),
            ));
        }
        let _draining = self.begin_draining().await;
        self.stop().await;
        self.invalidate_runtime_inventories().await;
        let refresh = self.prepare_runtime_inventory(cwd).await;
        refresh?;
        Ok(json!({
            "success": true,
            "authority": self.authority_view(),
        }))
    }

    pub(super) async fn catalog_add(
        &self,
        source: &str,
        git_ref: Option<&str>,
        sparse_paths: &[String],
    ) -> Result<Value> {
        let result = self
            .request(
                "marketplace/add",
                json!({
                    "source": source,
                    "refName": git_ref,
                    "sparsePaths": (!sparse_paths.is_empty()).then_some(sparse_paths),
                }),
            )
            .await?;
        self.invalidate_runtime_inventories().await;
        Ok(result)
    }

    pub(super) async fn catalog_remove(&self, marketplace_name: &str) -> Result<Value> {
        let _draining = self.begin_draining().await;
        let result = self
            .request(
                "marketplace/remove",
                json!({"marketplaceName": marketplace_name}),
            )
            .await;
        let result = result?;
        self.invalidate_runtime_inventories().await;
        Ok(result)
    }

    pub(super) async fn catalog_upgrade(
        &self,
        marketplace_name: Option<&str>,
        source: Option<&str>,
        git_ref: Option<&str>,
        sparse_paths: &[String],
    ) -> Result<Value> {
        let _draining = self.begin_draining().await;
        let result = self
            .request(
                "marketplace/upgrade",
                json!({
                    "marketplaceName": marketplace_name,
                    "source": source,
                    "refName": git_ref,
                    "sparsePaths": (!sparse_paths.is_empty()).then_some(sparse_paths),
                }),
            )
            .await;
        let result = result?;
        self.invalidate_runtime_inventories().await;
        Ok(result)
    }

    pub(super) fn trust_value(
        &self,
        identity: &CodexPluginIdentity,
        detail: &Value,
    ) -> Result<Value> {
        let fingerprint = codex_detail_fingerprint(identity, detail)?;
        let records = self.load_trust_records()?;
        let record = records.get(&identity.selector());
        let codex_version = self
            .negotiated_version
            .read()
            .expect("Codex version lock poisoned")
            .clone()
            .unwrap_or_else(|| "unknown".to_string());
        let status = match record {
            Some(record)
                if record.fingerprint == fingerprint && record.codex_version == codex_version =>
            {
                "trusted"
            }
            Some(_) => "modified",
            None => "untrusted",
        };
        Ok(json!({
            "required": true,
            "status": status,
            "fingerprint": fingerprint,
            "trustedFingerprint": record.map(|record| record.fingerprint.clone()),
            "trustedCodexVersion": record.map(|record| record.codex_version.clone()),
            "trustedAtMs": record.map(|record| record.trusted_at_ms),
        }))
    }

    pub(super) fn set_trust(
        &self,
        identity: &CodexPluginIdentity,
        detail: &Value,
        trusted: bool,
    ) -> Result<Value> {
        let fingerprint = codex_detail_fingerprint(identity, detail)?;
        let codex_version = self
            .negotiated_version
            .read()
            .expect("Codex version lock poisoned")
            .clone()
            .ok_or_else(|| Error::Message("Codex version is not negotiated".to_string()))?;
        let mut records = self.load_trust_records()?;
        if trusted {
            records.insert(
                identity.selector(),
                CodexTrustRecord {
                    fingerprint: fingerprint.clone(),
                    codex_version,
                    trusted_at_ms: super::gateway_now_ms(),
                },
            );
        } else {
            records.remove(&identity.selector());
        }
        self.write_trust_records(&records)?;
        Ok(json!({
            "success": true,
            "selector": identity.selector(),
            "trusted": trusted,
            "trust": self.trust_value(identity, detail)?,
        }))
    }

    fn load_trust_records(&self) -> Result<BTreeMap<String, CodexTrustRecord>> {
        let path = self.private_home.join("plugin-trust.json");
        if !path.exists() {
            return Ok(BTreeMap::new());
        }
        let bytes = std::fs::read(&path)?;
        serde_json::from_slice(&bytes).map_err(Into::into)
    }

    fn write_trust_records(&self, records: &BTreeMap<String, CodexTrustRecord>) -> Result<()> {
        std::fs::create_dir_all(&self.private_home)?;
        let path = self.private_home.join("plugin-trust.json");
        let temporary = self.private_home.join("plugin-trust.json.tmp");
        std::fs::write(&temporary, serde_json::to_vec_pretty(records)?)?;
        std::fs::rename(temporary, path)?;
        Ok(())
    }

    pub(super) async fn connect_start(
        &self,
        selector: &str,
        component_id: &str,
        kind: Option<&str>,
    ) -> Result<Value> {
        CodexPluginIdentity::parse_selector(selector)?.ok_or_else(|| {
            Error::Message("Connect requires a Codex authority plugin selector".to_string())
        })?;
        let kind = kind.unwrap_or("app");
        let (authorization_url, status) = if kind == "app" {
            let apps = self
                .request("app/list", json!({"threadId":null,"forceRefetch":true}))
                .await?;
            let app = apps
                .get("data")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .find(|app| app.get("id").and_then(Value::as_str) == Some(component_id))
                .ok_or_else(|| Error::Message(format!("Codex App not found: {component_id}")))?;
            if app.get("isAccessible").and_then(Value::as_bool) == Some(true) {
                (String::new(), "succeeded")
            } else {
                let url = app
                    .get("installUrl")
                    .and_then(Value::as_str)
                    .ok_or_else(|| {
                        Error::Message(format!("Codex App `{component_id}` has no install URL"))
                    })?;
                validate_connect_url(url)?;
                (url.to_string(), "pending")
            }
        } else if kind == "mcp" {
            let result = self
                .request(
                    "mcpServer/oauth/login",
                    json!({
                        "name": component_id,
                        "threadId": null,
                        "scopes": null,
                        "timeoutSecs": 300,
                    }),
                )
                .await?;
            let url = result
                .get("authorizationUrl")
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    Error::Message("Codex MCP OAuth response has no authorization URL".to_string())
                })?;
            validate_connect_url(url)?;
            (url.to_string(), "pending")
        } else {
            return Err(Error::Message(format!(
                "unsupported Codex connection kind `{kind}`"
            )));
        };
        let session_id = format!("codex-connect:{}", uuid::Uuid::now_v7());
        self.connect_sessions.lock().await.insert(
            session_id.clone(),
            CodexConnectSession {
                selector: selector.to_string(),
                component_id: component_id.to_string(),
                kind: kind.to_string(),
                authorization_url: authorization_url.clone(),
                expires_at: Instant::now() + CONNECT_SESSION_TTL,
            },
        );
        Ok(json!({
            "sessionId": session_id,
            "status": status,
            "installUrl": (kind == "app" && !authorization_url.is_empty()).then_some(authorization_url.clone()),
            "authorizationUrl": (kind == "mcp" && !authorization_url.is_empty()).then_some(authorization_url),
            "expiresInSeconds": CONNECT_SESSION_TTL.as_secs(),
        }))
    }

    pub(super) async fn connect_status(&self, session_id: &str) -> Result<Value> {
        let Some(session) = self.connect_sessions.lock().await.get(session_id).cloned() else {
            return Ok(json!({"sessionId":session_id,"status":"expired"}));
        };
        if Instant::now() >= session.expires_at {
            self.connect_sessions.lock().await.remove(session_id);
            return Ok(json!({"sessionId":session_id,"status":"expired"}));
        }
        let status = if session.kind == "app" {
            let apps = self
                .request("app/list", json!({"threadId":null,"forceRefetch":true}))
                .await?;
            if apps
                .get("data")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .any(|app| {
                    app.get("id").and_then(Value::as_str) == Some(&session.component_id)
                        && app.get("isAccessible").and_then(Value::as_bool) == Some(true)
                })
            {
                "succeeded"
            } else {
                "pending"
            }
        } else {
            let servers = self
                .request(
                    "mcpServerStatus/list",
                    json!({"threadId":null,"detail":"toolsAndAuthOnly"}),
                )
                .await?;
            if servers
                .get("data")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .any(|server| {
                    server.get("name").and_then(Value::as_str) == Some(&session.component_id)
                        && matches!(
                            server.get("authStatus").and_then(Value::as_str),
                            Some("oAuth" | "oauth" | "bearerToken")
                        )
                })
            {
                "succeeded"
            } else {
                "pending"
            }
        };
        Ok(json!({
            "sessionId": session_id,
            "selector": session.selector,
            "componentId": session.component_id,
            "kind": session.kind,
            "status": status,
            "authorizationUrl": session.authorization_url,
        }))
    }

    pub(super) async fn plugin_list(&self, cwd: &std::path::Path) -> Result<Value> {
        self.request(
            "plugin/list",
            json!({
                "cwds": [cwd],
                "marketplaceKinds": null,
            }),
        )
        .await
    }

    async fn plugin_installed(&self, cwd: &Path) -> Result<Value> {
        self.request(
            "plugin/installed",
            json!({
                "cwds": [cwd],
                "installSuggestionPluginNames": [],
            }),
        )
        .await
    }

    pub(super) async fn plugin_read(
        &self,
        cwd: &std::path::Path,
        identity: &CodexPluginIdentity,
    ) -> Result<Value> {
        let catalog = self.plugin_list(cwd).await?;
        let target = find_catalog_plugin(&catalog, identity)?;
        self.plugin_read_target(identity, &target).await
    }

    pub(super) async fn plugin_install(
        &self,
        cwd: &std::path::Path,
        identity: &CodexPluginIdentity,
    ) -> Result<Value> {
        let catalog = self.plugin_list(cwd).await?;
        let target = find_catalog_plugin(&catalog, identity)?;
        let result = self
            .request(
                "plugin/install",
                json!({
                    "pluginName": identity.plugin,
                    "marketplacePath": target.marketplace_path,
                    "remoteMarketplaceName": target.remote_marketplace_name,
                }),
            )
            .await?;
        if !self.enforce_policy {
            self.invalidate_runtime_inventories().await;
            let _ = self.prepare_runtime_inventory(cwd).await;
            return Ok(result);
        }
        let mut completed_steps = vec!["materialized"];
        let detail = match self.plugin_read_target(identity, &target).await {
            Ok(detail) => {
                completed_steps.push("detail_reread");
                detail
            }
            Err(err) => {
                self.invalidate_runtime_inventories().await;
                return Ok(json!({
                    "success": false,
                    "partial": true,
                    "completedSteps": completed_steps,
                    "failedStep": "detail_reread",
                    "reason": err.to_string(),
                    "materialization": result,
                    "safeState": "Disabled",
                }));
            }
        };
        let fingerprint = match codex_detail_fingerprint(identity, &detail) {
            Ok(fingerprint) => {
                completed_steps.push("fingerprint");
                fingerprint
            }
            Err(err) => {
                self.invalidate_runtime_inventories().await;
                return Ok(json!({
                    "success": false,
                    "partial": true,
                    "completedSteps": completed_steps,
                    "failedStep": "fingerprint",
                    "reason": err.to_string(),
                    "materialization": result,
                    "safeState": "Needs trust",
                }));
            }
        };
        let profile_home = self.private_home.parent().ok_or_else(|| {
            Error::Message("Codex private home has no profile parent".to_string())
        })?;
        if let Err(err) = psychevo_runtime::codex_plugin_set_enabled_value(
            profile_home,
            cwd,
            psychevo_runtime::PluginScope::Global,
            &identity.selector(),
            Some(true),
        ) {
            self.invalidate_runtime_inventories().await;
            return Ok(json!({
                "success": false,
                "partial": true,
                "completedSteps": completed_steps,
                "failedStep": "profile_allow",
                "reason": err.to_string(),
                "materialization": result,
                "fingerprint": fingerprint,
                "safeState": "Disabled",
            }));
        }
        completed_steps.push("profile_allow");
        if let Err(err) = self.set_trust(identity, &detail, true) {
            self.invalidate_runtime_inventories().await;
            return Ok(json!({
                "success": false,
                "partial": true,
                "completedSteps": completed_steps,
                "failedStep": "trust",
                "reason": err.to_string(),
                "materialization": result,
                "fingerprint": fingerprint,
                "safeState": "Needs trust",
            }));
        }
        completed_steps.push("trust");
        self.invalidate_runtime_inventories().await;
        completed_steps.push("generation_published");
        Ok(json!({
            "success": true,
            "partial": false,
            "completedSteps": completed_steps,
            "materialization": result,
            "detail": detail,
            "fingerprint": fingerprint,
            "policy": psychevo_runtime::codex_plugin_policy_value(
                profile_home,
                cwd,
                &identity.selector(),
            )?,
            "trust": self.trust_value(identity, &detail)?,
            "generation": self.generation.load(Ordering::Acquire),
        }))
    }

    pub(super) async fn plugin_uninstall(
        &self,
        cwd: &std::path::Path,
        identity: &CodexPluginIdentity,
    ) -> Result<Value> {
        let catalog = self.plugin_list(cwd).await?;
        let target = find_catalog_plugin(&catalog, identity)?;
        let _draining = self.begin_draining().await;
        let result = self
            .request("plugin/uninstall", json!({"pluginId": target.plugin_id}))
            .await;
        let result = result?;
        self.invalidate_runtime_inventories().await;
        let _ = self.prepare_runtime_inventory(cwd).await;
        Ok(result)
    }

    pub(super) async fn stop(&self) {
        if let Some(process) = self.process.lock().await.take() {
            process.kill().await;
        }
        self.process_ready.store(false, Ordering::Release);
    }

    async fn ensure_ephemeral_thread(
        &self,
        psychevo_thread_id: &str,
        cwd: &std::path::Path,
    ) -> Result<String> {
        let mut thread_ids = self.thread_ids.lock().await;
        if let Some(thread_id) = thread_ids.get(psychevo_thread_id) {
            return Ok(thread_id.clone());
        }
        let response = self
            .request(
                "thread/start",
                json!({
                    "cwd": cwd,
                    "ephemeral": true,
                }),
            )
            .await?;
        let thread_id = response
            .pointer("/thread/id")
            .or_else(|| response.get("threadId"))
            .and_then(Value::as_str)
            .ok_or_else(|| {
                Error::Message("Codex broker thread/start response has no thread id".to_string())
            })?
            .to_string();
        thread_ids.insert(psychevo_thread_id.to_string(), thread_id.clone());
        Ok(thread_id)
    }

    pub(super) async fn archive_ephemeral_thread(&self, psychevo_thread_id: &str) {
        let thread_id = self.thread_ids.lock().await.remove(psychevo_thread_id);
        if let Some(thread_id) = thread_id {
            let _ = self
                .request("thread/archive", json!({"threadId":thread_id}))
                .await;
        }
    }

    async fn forget_ephemeral_thread(&self, psychevo_thread_id: &str) {
        self.thread_ids.lock().await.remove(psychevo_thread_id);
    }

    pub(super) async fn prepare_runtime_inventory(&self, cwd: &Path) -> Result<()> {
        self.runtime_inventory(cwd).await.map(|_| ())
    }

    /// Revalidates package fingerprints and policy off the turn hot path.
    /// Existing ready inventory stays readable while app-server and filesystem
    /// inspection run; a changed effective inventory is published atomically as
    /// a new generation.
    pub(super) async fn refresh_runtime_inventory(&self, cwd: &Path) -> Result<()> {
        if !self.is_enabled() {
            return Ok(());
        }
        let key = tokio::fs::canonicalize(cwd)
            .await
            .unwrap_or_else(|_| cwd.to_path_buf());
        let existing = self.runtime_inventories.lock().await.get(&key).cloned();
        let Some(existing) = existing else {
            return self.prepare_runtime_inventory(&key).await;
        };
        let fresh = Arc::new(self.load_runtime_inventory(&key).await?);
        let mut cached = existing.lock().await;
        let changed = cached
            .inventory
            .as_deref()
            .is_none_or(|current| current != fresh.as_ref());
        cached.inventory = Some(fresh);
        cached.failure = None;
        self.inventory_ready.store(true, Ordering::Release);
        drop(cached);
        if changed {
            self.generation.fetch_add(1, Ordering::AcqRel);
        }
        Ok(())
    }

    pub(super) async fn invalidate_runtime_inventories(&self) {
        self.runtime_inventories.lock().await.clear();
        self.inventory_ready.store(false, Ordering::Release);
        self.generation.fetch_add(1, Ordering::AcqRel);
    }

    pub(super) async fn runtime_contributions(
        &self,
        state: super::WebState,
        cwd: &std::path::Path,
        psychevo_thread_id: &str,
        turn_id: Option<String>,
        event_sink: Option<super::GatewayEventSink>,
    ) -> Result<CodexRuntimeContributions> {
        let Some(profile) = self.ready_runtime_profile(cwd).await else {
            log_codex_authority_event("inventory_not_ready", cwd, None);
            return Ok(CodexRuntimeContributions {
                capability_roots: Vec::new(),
                runtime_tools: Vec::new(),
                lease_id: None,
            });
        };
        if self.draining.load(Ordering::Acquire) {
            log_codex_authority_event("turn_skipped_while_draining", cwd, None);
            return Ok(CodexRuntimeContributions {
                capability_roots: Vec::new(),
                runtime_tools: Vec::new(),
                lease_id: None,
            });
        }
        let generation = self.generation.load(Ordering::Acquire);
        let lease_id = format!(
            "{}:{}:{}:{}",
            psychevo_thread_id,
            turn_id.as_deref().unwrap_or("unassigned"),
            generation,
            uuid::Uuid::now_v7()
        );
        {
            let mut active_leases = self.active_leases.lock().await;
            // `begin_draining` sets this flag before it waits on the same map.
            // Rechecking while holding the map lock closes the admission/drain race.
            if self.draining.load(Ordering::Acquire) {
                log_codex_authority_event("turn_skipped_while_draining", cwd, None);
                return Ok(CodexRuntimeContributions {
                    capability_roots: Vec::new(),
                    runtime_tools: Vec::new(),
                    lease_id: None,
                });
            }
            active_leases.insert(lease_id.clone(), generation);
        }
        let CodexRuntimeProfile {
            capability_roots,
            delegated_tools,
        } = profile;
        let tools = delegated_tools
            .into_iter()
            .map(|descriptor| {
                let source = format!("codex:mcp:{}", descriptor.server_name);
                psychevo_runtime::RuntimeTool::with_source(
                    Arc::new(CodexMcpTool {
                        state: state.clone(),
                        cwd: cwd.to_path_buf(),
                        psychevo_thread_id: psychevo_thread_id.to_string(),
                        turn_id: turn_id.clone(),
                        event_sink: event_sink.clone(),
                        name: descriptor.name,
                        server_name: descriptor.server_name,
                        remote_name: descriptor.remote_name,
                        description: descriptor.description,
                        parameters: descriptor.parameters,
                    }),
                    source,
                    "codex_capability_broker",
                )
            })
            .collect();
        Ok(CodexRuntimeContributions {
            capability_roots,
            runtime_tools: tools,
            lease_id: Some(lease_id),
        })
    }

    async fn ready_runtime_profile(&self, cwd: &Path) -> Option<CodexRuntimeProfile> {
        let key = std::fs::canonicalize(cwd).unwrap_or_else(|_| cwd.to_path_buf());
        let entry = self.runtime_inventories.lock().await.get(&key).cloned()?;
        let cached = entry.try_lock().ok()?;
        let inventory = cached.inventory.as_ref()?;
        Some(CodexRuntimeProfile {
            capability_roots: inventory.capability_roots.clone(),
            delegated_tools: inventory.delegated_tools.clone(),
        })
    }

    async fn runtime_inventory(&self, cwd: &Path) -> Result<Arc<CodexRuntimeInventory>> {
        let key = tokio::fs::canonicalize(cwd)
            .await
            .unwrap_or_else(|_| cwd.to_path_buf());
        let entry_cell = {
            let mut inventories = self.runtime_inventories.lock().await;
            inventories
                .entry(key.clone())
                .or_insert_with(|| Arc::new(Mutex::new(CachedCodexRuntimeInventory::default())))
                .clone()
        };
        let mut entry = entry_cell.lock().await;
        if let Some(inventory) = entry.inventory.as_ref() {
            return Ok(inventory.clone());
        }
        if let Some(failure) = entry.failure.as_ref() {
            if Instant::now() < failure.retry_after {
                return Err(Error::Message(failure.message.clone()));
            }
            entry.failure = None;
        }

        match self.load_runtime_inventory(&key).await {
            Ok(inventory) => {
                for warning in &inventory.warnings {
                    log_codex_authority_event("inventory_component_degraded", &key, Some(warning));
                }
                let inventory = Arc::new(inventory);
                entry.inventory = Some(inventory.clone());
                self.inventory_ready.store(true, Ordering::Release);
                Ok(inventory)
            }
            Err(err) => {
                let message = err.to_string();
                entry.failure = Some(CachedCodexRuntimeInventoryFailure {
                    message: message.clone(),
                    retry_after: Instant::now() + self.runtime_inventory_retry_delay,
                });
                let mut inventories = self.runtime_inventories.lock().await;
                inventories.entry(key).or_insert(entry_cell.clone());
                Err(Error::Message(message))
            }
        }
    }

    async fn load_runtime_inventory(&self, cwd: &Path) -> Result<CodexRuntimeInventory> {
        let (mut plugins, mut warnings) = self.enabled_installed_plugin_details(cwd).await?;
        self.resolve_hook_package_roots(cwd, &mut plugins, &mut warnings)
            .await;

        let mut capability_roots = Vec::new();
        let mut delegated_servers = BTreeSet::new();
        for plugin in &plugins {
            if let Some(root) = &plugin.package_root {
                capability_roots.push(psychevo_runtime::SelectedCapabilityRoot::codex_local(
                    plugin.identity.selector(),
                    plugin.identity.plugin.clone(),
                    plugin.identity.marketplace.clone(),
                    root,
                ));
            } else {
                delegated_servers.extend(
                    plugin
                        .plugin
                        .get("mcpServers")
                        .and_then(Value::as_array)
                        .into_iter()
                        .flatten()
                        .filter_map(Value::as_str)
                        .map(str::to_string),
                );
                if plugin
                    .plugin
                    .get("skills")
                    .and_then(Value::as_array)
                    .is_some_and(|skills| !skills.is_empty())
                    || plugin
                        .plugin
                        .get("hooks")
                        .and_then(Value::as_array)
                        .is_some_and(|hooks| !hooks.is_empty())
                {
                    warnings.push(format!(
                        "Codex plugin `{}` exposes portable components but no installed package root",
                        plugin.identity.canonical_id()
                    ));
                }
            }
            if plugin
                .plugin
                .get("apps")
                .and_then(Value::as_array)
                .is_some_and(|apps| !apps.is_empty())
            {
                delegated_servers.insert("codex_apps".to_string());
            }
        }
        capability_roots.sort_by(|left, right| left.id.cmp(&right.id));
        capability_roots.dedup_by(|left, right| left.id == right.id);

        let inventory_thread_id = format!("codex-inventory:{}", cwd.display());
        let delegated_tools = if delegated_servers.is_empty() {
            Vec::new()
        } else {
            match self
                .load_delegated_tool_descriptors(cwd, &inventory_thread_id, &delegated_servers)
                .await
            {
                Ok(tools) => tools,
                Err(err) => {
                    log_codex_authority_event(
                        "delegated_tool_inventory_unavailable",
                        cwd,
                        Some(&err.to_string()),
                    );
                    Vec::new()
                }
            }
        };

        Ok(CodexRuntimeInventory {
            capability_roots,
            delegated_servers,
            delegated_tools,
            warnings,
        })
    }

    async fn enabled_installed_plugin_details(
        &self,
        cwd: &Path,
    ) -> Result<(Vec<RuntimePluginDetail>, Vec<String>)> {
        let catalog = self.plugin_installed(cwd).await?;
        let mut details = Vec::new();
        let mut warnings = Vec::new();
        for marketplace in catalog
            .get("marketplaces")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let Some(marketplace_name) = marketplace.get("name").and_then(Value::as_str) else {
                continue;
            };
            for summary in marketplace
                .get("plugins")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                if summary.get("installed").and_then(Value::as_bool) != Some(true)
                    || summary.get("enabled").and_then(Value::as_bool) != Some(true)
                {
                    continue;
                }
                let Some(plugin_name) = summary.get("name").and_then(Value::as_str) else {
                    continue;
                };
                let identity = CodexPluginIdentity {
                    plugin: plugin_name.to_string(),
                    marketplace: marketplace_name.to_string(),
                };
                if self.enforce_policy {
                    let profile_home = self.private_home.parent().unwrap_or_else(|| Path::new("."));
                    let policy = psychevo_runtime::codex_plugin_policy_value(
                        profile_home,
                        cwd,
                        &identity.selector(),
                    )?;
                    if policy.get("effectiveEnabled").and_then(Value::as_bool) != Some(true) {
                        continue;
                    }
                }
                let target = match find_catalog_plugin(&catalog, &identity) {
                    Ok(target) => target,
                    Err(err) => {
                        warnings.push(err.to_string());
                        continue;
                    }
                };
                let plugin_id = target.plugin_id.clone();
                match self.plugin_read_target(&identity, &target).await {
                    Ok(detail) => {
                        if self.enforce_policy {
                            match self.trust_value(&identity, &detail) {
                                Ok(trust)
                                    if trust.get("status").and_then(Value::as_str)
                                        == Some("trusted") => {}
                                Ok(_) => {
                                    warnings.push(format!(
                                        "Codex plugin `{}` is not trusted for its current package fingerprint",
                                        identity.canonical_id()
                                    ));
                                    continue;
                                }
                                Err(err) => {
                                    warnings.push(format!(
                                        "Codex plugin `{}` trust could not be verified: {err}",
                                        identity.canonical_id()
                                    ));
                                    continue;
                                }
                            }
                        }
                        let plugin = detail.get("plugin").cloned().unwrap_or(detail);
                        let package_root = codex_package_root(&plugin);
                        details.push(RuntimePluginDetail {
                            identity,
                            plugin_id,
                            plugin,
                            package_root,
                        });
                    }
                    Err(err) => warnings.push(format!(
                        "Codex plugin `{}` could not be projected: {err}",
                        identity.canonical_id()
                    )),
                }
            }
        }
        Ok((details, warnings))
    }

    async fn load_delegated_tool_descriptors(
        &self,
        cwd: &Path,
        psychevo_thread_id: &str,
        delegated_servers: &BTreeSet<String>,
    ) -> Result<Vec<CodexDelegatedToolDescriptor>> {
        let codex_thread_id = self
            .ensure_ephemeral_thread(psychevo_thread_id, cwd)
            .await?;
        let inventory = self
            .request(
                "mcpServerStatus/list",
                json!({
                    "threadId": codex_thread_id,
                    "detail": "toolsAndAuthOnly",
                }),
            )
            .await?;
        let mut tools = Vec::new();
        for server in inventory
            .get("data")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let Some(server_name) = server.get("name").and_then(Value::as_str) else {
                continue;
            };
            if !delegated_servers.contains(server_name) {
                continue;
            }
            let Some(server_tools) = server.get("tools").and_then(Value::as_object) else {
                continue;
            };
            for (tool_name, descriptor) in server_tools {
                tools.push(CodexDelegatedToolDescriptor {
                    name: format!("mcp__{server_name}__{tool_name}"),
                    server_name: server_name.to_string(),
                    remote_name: tool_name.clone(),
                    description: descriptor
                        .get("description")
                        .and_then(Value::as_str)
                        .unwrap_or("Codex App tool")
                        .to_string(),
                    parameters: descriptor
                        .get("inputSchema")
                        .or_else(|| descriptor.get("input_schema"))
                        .cloned()
                        .unwrap_or_else(|| json!({"type":"object","properties":{}})),
                });
            }
        }
        tools.sort_by(|left, right| left.name.cmp(&right.name));
        Ok(tools)
    }

    async fn plugin_read_target(
        &self,
        identity: &CodexPluginIdentity,
        target: &CatalogPluginTarget,
    ) -> Result<Value> {
        self.request(
            "plugin/read",
            json!({
                "pluginName": identity.plugin,
                "marketplacePath": target.marketplace_path,
                "remoteMarketplaceName": target.remote_marketplace_name,
            }),
        )
        .await
    }

    async fn resolve_hook_package_roots(
        &self,
        cwd: &Path,
        plugins: &mut [RuntimePluginDetail],
        warnings: &mut Vec<String>,
    ) {
        let unresolved_ids = plugins
            .iter()
            .filter(|plugin| {
                plugin.package_root.is_none()
                    && plugin
                        .plugin
                        .get("hooks")
                        .and_then(Value::as_array)
                        .is_some_and(|hooks| !hooks.is_empty())
            })
            .map(|plugin| plugin.plugin_id.clone())
            .collect::<BTreeSet<_>>();
        if unresolved_ids.is_empty() {
            return;
        }
        let hooks = match self.request("hooks/list", json!({"cwds":[cwd]})).await {
            Ok(hooks) => hooks,
            Err(err) => {
                warnings.push(format!("Codex hook roots could not be resolved: {err}"));
                return;
            }
        };
        for hook in hooks
            .get("data")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .flat_map(|entry| {
                entry
                    .get("hooks")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
            })
        {
            let Some(plugin_id) = hook.get("pluginId").and_then(Value::as_str) else {
                continue;
            };
            if !unresolved_ids.contains(plugin_id) {
                continue;
            }
            let Some(source_path) = hook.get("sourcePath").and_then(Value::as_str) else {
                continue;
            };
            if let Some(root) = find_codex_package_root(Path::new(source_path))
                && let Some(plugin) = plugins
                    .iter_mut()
                    .find(|plugin| plugin.plugin_id == plugin_id)
            {
                plugin.package_root = Some(root);
            }
        }
    }
}

fn prepare_private_auth_link(private_home: &Path, env: &BTreeMap<String, String>) -> bool {
    let Some(global_home) = env.get("HOME").map(PathBuf::from) else {
        return false;
    };
    let source = global_home.join(".codex").join("auth.json");
    if !source.is_file() || std::fs::create_dir_all(private_home).is_err() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::{PermissionsExt, symlink};
        let _ = std::fs::set_permissions(private_home, std::fs::Permissions::from_mode(0o700));
        let destination = private_home.join("auth.json");
        if let Ok(metadata) = std::fs::symlink_metadata(&destination) {
            return metadata.file_type().is_symlink()
                && std::fs::read_link(&destination).ok().as_deref() == Some(source.as_path());
        }
        return symlink(&source, destination).is_ok();
    }
    #[cfg(windows)]
    {
        let destination = private_home.join("auth.json");
        if destination.exists() {
            return false;
        }
        return std::fs::hard_link(source, destination).is_ok();
    }
    #[allow(unreachable_code)]
    false
}

fn ensure_private_home(private_home: &Path) -> Result<()> {
    std::fs::create_dir_all(private_home)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(private_home, std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

fn validate_connect_url(url: &str) -> Result<()> {
    let has_explicit_authority = url
        .split_once("://")
        .is_some_and(|(_, authority_and_path)| {
            !authority_and_path.is_empty() && !authority_and_path.starts_with(['/', '?', '#'])
        });
    if !has_explicit_authority {
        return Err(Error::Message(
            "Codex connection URL must use HTTPS or a loopback HTTP origin".to_string(),
        ));
    }
    let parsed = reqwest::Url::parse(url).map_err(|_| {
        Error::Message("Codex connection URL must use HTTPS or a loopback HTTP origin".to_string())
    })?;
    let host = parsed.host_str().ok_or_else(|| {
        Error::Message("Codex connection URL must use HTTPS or a loopback HTTP origin".to_string())
    })?;
    let ip_host = host
        .strip_prefix('[')
        .and_then(|value| value.strip_suffix(']'))
        .unwrap_or(host);
    let loopback_host = host.eq_ignore_ascii_case("localhost")
        || ip_host
            .parse::<std::net::IpAddr>()
            .is_ok_and(|address| address.is_loopback());
    let allowed = parsed.scheme() == "https" || (parsed.scheme() == "http" && loopback_host);
    if allowed {
        Ok(())
    } else {
        Err(Error::Message(
            "Codex connection URL must use HTTPS or a loopback HTTP origin".to_string(),
        ))
    }
}

struct DrainingMutationGuard<'a> {
    authority: &'a CodexPluginAuthority,
    _mutation: MutexGuard<'a, ()>,
}

impl Drop for DrainingMutationGuard<'_> {
    fn drop(&mut self) {
        self.authority.draining.store(false, Ordering::Release);
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct CodexPluginIdentity {
    pub(super) plugin: String,
    pub(super) marketplace: String,
}

impl CodexPluginIdentity {
    pub(super) fn parse_selector(selector: &str) -> Result<Option<Self>> {
        let Some(value) = selector.strip_prefix("codex:") else {
            return Ok(None);
        };
        let Some((plugin, marketplace)) = value.rsplit_once('@') else {
            return Err(Error::Message(
                "Codex plugin selector must be `codex:<plugin>@<marketplace>`".to_string(),
            ));
        };
        if plugin.trim().is_empty() || marketplace.trim().is_empty() {
            return Err(Error::Message(
                "Codex plugin selector must include plugin and marketplace".to_string(),
            ));
        }
        Ok(Some(Self {
            plugin: plugin.to_string(),
            marketplace: marketplace.to_string(),
        }))
    }

    fn canonical_id(&self) -> String {
        format!("{}@{}", self.plugin, self.marketplace)
    }

    pub(super) fn selector(&self) -> String {
        format!("codex:{}", self.canonical_id())
    }
}

struct CatalogPluginTarget {
    marketplace_path: Option<String>,
    remote_marketplace_name: Option<String>,
    plugin_id: String,
}

fn codex_package_root(plugin: &Value) -> Option<PathBuf> {
    let mut candidates = Vec::new();
    if plugin
        .pointer("/summary/source/type")
        .and_then(Value::as_str)
        == Some("local")
        && let Some(path) = plugin
            .pointer("/summary/source/path")
            .and_then(Value::as_str)
    {
        candidates.push(PathBuf::from(path));
    }
    candidates.extend(
        plugin
            .get("skills")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|skill| skill.get("path").and_then(Value::as_str))
            .map(PathBuf::from),
    );
    for key in ["composerIcon", "logo", "logoDark"] {
        if let Some(path) = plugin
            .pointer(&format!("/summary/interface/{key}"))
            .and_then(Value::as_str)
        {
            candidates.push(PathBuf::from(path));
        }
    }
    candidates.extend(
        plugin
            .pointer("/summary/interface/screenshots")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(PathBuf::from),
    );
    candidates
        .into_iter()
        .find_map(|path| find_codex_package_root(&path))
}

fn codex_detail_fingerprint(identity: &CodexPluginIdentity, detail: &Value) -> Result<String> {
    let plugin = detail.get("plugin").unwrap_or(detail);
    let root = codex_package_root(plugin);
    let version = plugin
        .pointer("/summary/localVersion")
        .or_else(|| plugin.pointer("/summary/version"))
        .or_else(|| plugin.get("version"))
        .and_then(Value::as_str);
    psychevo_runtime::external_plugin_fingerprint(root.as_deref(), &identity.selector(), version)
}

fn find_codex_package_root(path: &Path) -> Option<PathBuf> {
    let mut cursor = if path.is_file() {
        path.parent()?.to_path_buf()
    } else {
        path.to_path_buf()
    };
    loop {
        if cursor.join(".codex-plugin").join("plugin.json").is_file() {
            return Some(cursor);
        }
        if !cursor.pop() {
            return None;
        }
    }
}

fn find_catalog_plugin(
    catalog: &Value,
    identity: &CodexPluginIdentity,
) -> Result<CatalogPluginTarget> {
    let marketplaces = catalog
        .get("marketplaces")
        .and_then(Value::as_array)
        .ok_or_else(|| Error::Message("Codex plugin catalog response is malformed".to_string()))?;
    let marketplace = marketplaces
        .iter()
        .find(|marketplace| {
            marketplace.get("name").and_then(Value::as_str) == Some(&identity.marketplace)
        })
        .ok_or_else(|| {
            Error::Message(format!(
                "Codex marketplace not found: {}",
                identity.marketplace
            ))
        })?;
    let plugin = marketplace
        .get("plugins")
        .and_then(Value::as_array)
        .and_then(|plugins| {
            plugins
                .iter()
                .find(|plugin| plugin.get("name").and_then(Value::as_str) == Some(&identity.plugin))
        })
        .ok_or_else(|| {
            Error::Message(format!(
                "Codex plugin not found: {}",
                identity.canonical_id()
            ))
        })?;
    let marketplace_path = marketplace
        .get("path")
        .and_then(Value::as_str)
        .map(str::to_string);
    Ok(CatalogPluginTarget {
        remote_marketplace_name: marketplace_path
            .is_none()
            .then(|| identity.marketplace.clone()),
        marketplace_path,
        plugin_id: plugin
            .get("id")
            .and_then(Value::as_str)
            .ok_or_else(|| Error::Message("Codex plugin catalog item has no id".to_string()))?
            .to_string(),
    })
}

pub(super) fn merge_plugin_list(mut native: Value, codex: Result<Value>) -> Value {
    let Some(native_object) = native.as_object_mut() else {
        return native;
    };
    let plugins = native_object
        .entry("plugins")
        .or_insert_with(|| json!([]))
        .as_array_mut();
    let Some(plugins) = plugins else {
        return native;
    };
    let codex_authority = match codex {
        Ok(catalog) => {
            for marketplace in catalog
                .get("marketplaces")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                let marketplace_name = marketplace
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown");
                for plugin in marketplace
                    .get("plugins")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                {
                    let Some(name) = plugin.get("name").and_then(Value::as_str) else {
                        continue;
                    };
                    let identity = CodexPluginIdentity {
                        plugin: name.to_string(),
                        marketplace: marketplace_name.to_string(),
                    };
                    let installed = plugin
                        .get("installed")
                        .and_then(Value::as_bool)
                        .unwrap_or(false);
                    let enabled = plugin
                        .get("enabled")
                        .and_then(Value::as_bool)
                        .unwrap_or(false);
                    let description = plugin
                        .pointer("/interface/shortDescription")
                        .cloned()
                        .unwrap_or(Value::Null);
                    plugins.push(json!({
                        "name": name,
                        "selector": identity.selector(),
                        "canonical_id": identity.canonical_id(),
                        "authority": {
                            "kind": "codex",
                            "plugin": name,
                            "marketplace": marketplace_name,
                        },
                        "scope_name": "codex_home",
                        "enablement_scope_name": "codex_home",
                        "removable": installed,
                        "package_mutable": true,
                        "enablement_mutable": false,
                        "version": plugin.get("localVersion").or_else(|| plugin.get("version")).cloned(),
                        "description": description,
                        "source_id": format!("codex:{marketplace_name}"),
                        "source": marketplace_name,
                        "source_kind": "codex_marketplace",
                        "scope": "codex_home",
                        "manifest_kind": "codex",
                        "compatibility_profile": psychevo_runtime::CODEX_PLUGIN_COMPATIBILITY_PROFILE,
                        "component_statuses": [],
                        "enabled": enabled,
                        "installed": installed,
                        "readiness": if installed && enabled { "Ready" } else if installed { "Disabled" } else { "Available" },
                        "status": if installed { "Installed" } else { "Available" },
                        "interface": plugin.get("interface").cloned(),
                        "keywords": plugin.get("keywords").cloned().unwrap_or_else(|| json!([])),
                    }));
                }
            }
            json!({"readiness":"ready","owner":"codex"})
        }
        Err(err) => json!({
            "readiness":"unavailable",
            "owner":"codex",
            "reason":err.to_string(),
        }),
    };
    let count = plugins.len();
    let _ = plugins;
    native_object.insert("codex_authority".to_string(), codex_authority);
    native_object.insert("count".to_string(), json!(count));
    native
}

pub(super) fn apply_authority_view(mut value: Value, codex_authority: Value) -> Value {
    let Some(object) = value.as_object_mut() else {
        return value;
    };
    object.insert("codex_authority".to_string(), codex_authority.clone());
    object.insert(
        "authorities".to_string(),
        json!([
            {
                "kind": "psychevo",
                "enabled": true,
                "runtime": "ready",
                "auth": "available"
            },
            codex_authority
        ]),
    );
    value
}

pub(super) fn apply_codex_policy_views(mut value: Value, home: &Path, cwd: &Path) -> Result<Value> {
    for plugin in value
        .get_mut("plugins")
        .and_then(Value::as_array_mut)
        .into_iter()
        .flatten()
    {
        let Some(selector) = plugin.get("selector").and_then(Value::as_str) else {
            continue;
        };
        if !selector.starts_with("codex:") {
            continue;
        }
        let policy = psychevo_runtime::codex_plugin_policy_value(home, cwd, selector)?;
        let enabled = policy
            .get("effectiveEnabled")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let installed = plugin
            .get("installed")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        if let Some(object) = plugin.as_object_mut() {
            object.insert("policy".to_string(), policy);
            object.insert("enabled".to_string(), Value::Bool(enabled));
            object.insert("enablement_mutable".to_string(), Value::Bool(true));
            object.insert(
                "enablement_scope_name".to_string(),
                Value::String("profile".to_string()),
            );
            object.insert(
                "readiness".to_string(),
                Value::String(
                    if installed && enabled {
                        "Needs trust"
                    } else if installed {
                        "Disabled"
                    } else {
                        "Available"
                    }
                    .to_string(),
                ),
            );
        }
    }
    Ok(value)
}

pub(super) fn apply_codex_plugin_runtime_state(
    mut value: Value,
    policy: Value,
    trust: Value,
) -> Value {
    if let Some(plugin) = value.get_mut("plugin").and_then(Value::as_object_mut) {
        let enabled = policy
            .get("effectiveEnabled")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let installed = plugin
            .get("installed")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let trusted = trust.get("status").and_then(Value::as_str) == Some("trusted");
        plugin.insert("policy".to_string(), policy);
        plugin.insert("trust".to_string(), trust);
        plugin.insert("enabled".to_string(), Value::Bool(enabled));
        plugin.insert("enablement_mutable".to_string(), Value::Bool(true));
        plugin.insert(
            "readiness".to_string(),
            Value::String(
                if !installed {
                    "Available"
                } else if !enabled {
                    "Disabled"
                } else if !trusted {
                    "Needs trust"
                } else {
                    "Ready"
                }
                .to_string(),
            ),
        );
    }
    value
}

pub(super) fn codex_plugin_read_value(identity: &CodexPluginIdentity, detail: Value) -> Value {
    let plugin = detail.get("plugin").cloned().unwrap_or(detail);
    let native_package_root = codex_package_root(&plugin).is_some();
    let summary = plugin.get("summary").cloned().unwrap_or_else(|| json!({}));
    let installed = summary
        .get("installed")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let enabled = summary
        .get("enabled")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let ready = installed && enabled;
    let mut statuses = Vec::new();
    let component = |component: &str, level: &str, owner: &str, readiness: &str, reason: &str| {
        json!({
            "component": component,
            "compatibilityProfile": psychevo_runtime::CODEX_PLUGIN_COMPATIBILITY_PROFILE,
            "highestLevel": level,
            "executionOwner": owner,
            "readiness": readiness,
            "reason": reason,
        })
    };
    if plugin
        .get("skills")
        .and_then(Value::as_array)
        .is_some_and(|items| !items.is_empty())
    {
        statuses.push(component(
            "skills",
            if native_package_root {
                "execute"
            } else {
                "inspect"
            },
            if native_package_root {
                "psychevo_native"
            } else {
                "metadata_only"
            },
            if !native_package_root {
                "unavailable"
            } else if ready {
                "ready"
            } else {
                "disabled"
            },
            if native_package_root {
                "portable skill content is projected from the Codex-owned installed package"
            } else {
                "Codex did not expose an installed package root for native skill projection"
            },
        ));
    }
    if plugin
        .get("hooks")
        .and_then(Value::as_array)
        .is_some_and(|items| !items.is_empty())
    {
        statuses.push(component(
            "hooks",
            if native_package_root {
                "execute"
            } else {
                "inspect"
            },
            if native_package_root {
                "psychevo_native"
            } else {
                "metadata_only"
            },
            if native_package_root {
                "needs_trust"
            } else {
                "unavailable"
            },
            if native_package_root {
                "hook declarations require Psychevo normalized-hash trust"
            } else {
                "Codex did not expose an installed package root for native hook projection"
            },
        ));
    }
    if plugin
        .get("mcpServers")
        .and_then(Value::as_array)
        .is_some_and(|items| !items.is_empty())
    {
        let remote = plugin
            .get("mcpServers")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .any(|server| {
                server.get("url").and_then(Value::as_str).is_some()
                    || server.get("appId").and_then(Value::as_str).is_some()
                    || server.get("remote").and_then(Value::as_bool) == Some(true)
            });
        let psychevo_owned = native_package_root && !remote;
        statuses.push(component(
            "mcp_servers",
            if psychevo_owned {
                "execute"
            } else {
                "delegate"
            },
            if psychevo_owned {
                "psychevo_native"
            } else {
                "codex_broker"
            },
            if ready { "ready" } else { "disabled" },
            if psychevo_owned {
                "ordinary MCP declarations use Psychevo MCP policy"
            } else {
                "remote or app-backed MCP configuration and execution remain Codex-owned"
            },
        ));
    }
    if plugin
        .get("apps")
        .and_then(Value::as_array)
        .is_some_and(|items| !items.is_empty())
    {
        statuses.push(component(
            "apps",
            "delegate",
            "codex_broker",
            if ready { "ready" } else { "needs_setup" },
            "Apps inventory, authentication, and tool calls remain Codex-owned",
        ));
    }
    if plugin
        .get("appTemplates")
        .and_then(Value::as_array)
        .is_some_and(|items| !items.is_empty())
    {
        statuses.push(component(
            "app_templates",
            "inspect",
            "metadata_only",
            "metadata_only",
            "templates describe Apps and execute only after a Codex-owned App is materialized",
        ));
    }
    if plugin
        .get("scheduledTasks")
        .and_then(Value::as_array)
        .is_some_and(|items| !items.is_empty())
    {
        statuses.push(component(
            "scheduled_tasks",
            "inspect",
            "metadata_only",
            "unsupported",
            "Codex scheduled tasks are not admitted to the Psychevo scheduler",
        ));
    }
    if summary.get("interface").is_some()
        || plugin.get("defaultPrompt").is_some()
        || plugin.get("prompts").is_some()
    {
        statuses.push(component(
            "interface_prompts",
            "inspect",
            "psychevo_gui",
            "metadata_only",
            "safe interface fields are displayed; prompts are never injected into a turn",
        ));
    }
    if plugin
        .get("browserExtensions")
        .and_then(Value::as_array)
        .is_some_and(|items| !items.is_empty())
    {
        statuses.push(component(
            "browser_extensions",
            "inspect",
            "metadata_only",
            "unsupported",
            "browser extension execution is unsupported and cannot be inferred from inventory",
        ));
    }
    if let Some(fields) = plugin.as_object() {
        let known = BTreeSet::from([
            "summary",
            "description",
            "skills",
            "hooks",
            "mcpServers",
            "apps",
            "appTemplates",
            "scheduledTasks",
            "defaultPrompt",
            "prompts",
            "browserExtensions",
        ]);
        let unknown = fields
            .keys()
            .filter(|key| !known.contains(key.as_str()))
            .cloned()
            .collect::<Vec<_>>();
        if !unknown.is_empty() {
            statuses.push(component(
                "unknown_fields",
                "inspect",
                "metadata_only",
                "diagnostic_only",
                &format!(
                    "unrecognized manifest fields are not executed: {}",
                    unknown.join(", ")
                ),
            ));
        }
    }
    json!({
        "plugin": {
            "name": identity.plugin,
            "selector": identity.selector(),
            "canonical_id": identity.canonical_id(),
            "authority": {
                "kind": "codex",
                "plugin": identity.plugin,
                "marketplace": identity.marketplace,
            },
            "version": summary.get("localVersion").or_else(|| summary.get("version")).cloned(),
            "description": plugin.get("description").cloned(),
            "source_id": format!("codex:{}", identity.marketplace),
            "scope_name": "codex_home",
            "manifest_kind": "codex",
            "compatibility_profile": psychevo_runtime::CODEX_PLUGIN_COMPATIBILITY_PROFILE,
            "component_statuses": statuses,
            "installed": installed,
            "enabled": enabled,
            "readiness": if ready { "Ready" } else if installed { "Disabled" } else { "Available" },
            "status": if installed { "Installed" } else { "Available" },
            "interface": summary.get("interface").cloned(),
        },
        "manifest": plugin,
        "inspection": {
            "authority": "codex",
            "compatibility_profile": psychevo_runtime::CODEX_PLUGIN_COMPATIBILITY_PROFILE,
            "component_statuses": statuses,
        }
    })
}

#[derive(Clone)]
struct CodexMcpTool {
    state: super::WebState,
    cwd: PathBuf,
    psychevo_thread_id: String,
    turn_id: Option<String>,
    event_sink: Option<super::GatewayEventSink>,
    name: String,
    server_name: String,
    remote_name: String,
    description: String,
    parameters: Value,
}

impl ToolBinding for CodexMcpTool {
    fn name(&self) -> &str {
        &self.name
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn parameters(&self) -> Value {
        self.parameters.clone()
    }

    fn execution_mode(&self) -> ToolExecutionMode {
        ToolExecutionMode::Sequential
    }

    fn execute(
        &self,
        _tool_call_id: String,
        args: Value,
        abort: AbortSignal,
    ) -> BoxFuture<'static, ToolOutput> {
        let tool = self.clone();
        Box::pin(async move {
            if abort.aborted() {
                return ToolOutput::error("aborted");
            }
            let context = CodexElicitationContext {
                state: tool.state.clone(),
                psychevo_thread_id: tool.psychevo_thread_id.clone(),
                turn_id: tool.turn_id.clone(),
                event_sink: tool.event_sink.clone(),
            };
            let codex_thread_id = match tool
                .state
                .inner
                .codex_capability_broker
                .ensure_ephemeral_thread(&tool.psychevo_thread_id, &tool.cwd)
                .await
            {
                Ok(thread_id) => thread_id,
                Err(err) => return ToolOutput::error(err.to_string()),
            };
            match tool
                .state
                .inner
                .codex_capability_broker
                .request_with_context(
                    "mcpServer/tool/call",
                    json!({
                        "threadId": codex_thread_id,
                        "server": tool.server_name,
                        "tool": tool.remote_name,
                        "arguments": args,
                    }),
                    Some(&context),
                )
                .await
            {
                Ok(value) if value.get("isError").and_then(Value::as_bool) == Some(true) => {
                    ToolOutput::error(value.to_string())
                }
                Ok(value) => ToolOutput::ok(value),
                Err(err) => {
                    tool.state
                        .inner
                        .codex_capability_broker
                        .forget_ephemeral_thread(&tool.psychevo_thread_id)
                        .await;
                    ToolOutput::error(err.to_string())
                }
            }
        })
    }
}

#[derive(Clone)]
struct CodexElicitationContext {
    state: super::WebState,
    psychevo_thread_id: String,
    turn_id: Option<String>,
    event_sink: Option<super::GatewayEventSink>,
}

impl CodexElicitationContext {
    async fn route(&self, message: &Value) -> Value {
        let Some(event_sink) = self.event_sink.as_ref() else {
            return declined_elicitation();
        };
        let params = message.get("params").cloned().unwrap_or_else(|| json!({}));
        let projection = project_elicitation(&params);
        let action_id = format!("codex-elicitation:{}", uuid::Uuid::now_v7());
        let (responder, receiver) = oneshot::channel();
        self.state
            .inner
            .codex_elicitations
            .lock()
            .expect("Codex elicitation registry poisoned")
            .insert(
                action_id.clone(),
                PendingCodexElicitation {
                    fields: projection.fields,
                    mode: projection.mode,
                    meta: params.get("_meta").cloned(),
                    event_sink: event_sink.clone(),
                    responder,
                },
            );
        event_sink(super::GatewayEvent::ActionRequested {
            action: super::PendingActionView {
                action_id: action_id.clone(),
                kind: super::GatewayActionKind::Clarify,
                title: Some("Codex App request".to_string()),
                summary: params
                    .get("message")
                    .and_then(Value::as_str)
                    .map(str::to_string),
                payload: json!({
                    "owner": "codex_capability_broker",
                    "raw": projection.raw,
                }),
                thread_id: Some(self.psychevo_thread_id.clone()),
                turn_id: self.turn_id.clone(),
                activity_id: None,
                source_key: None,
                owner_id: None,
                lease_expires_at_ms: None,
            },
        });
        let decision = tokio::time::timeout(ELICITATION_TIMEOUT, receiver).await;
        self.state
            .inner
            .codex_elicitations
            .lock()
            .expect("Codex elicitation registry poisoned")
            .remove(&action_id);
        match decision {
            Ok(Ok(result)) => result,
            _ => {
                event_sink(super::GatewayEvent::ActionResolved {
                    action_id,
                    kind: super::GatewayActionKind::Clarify,
                    outcome: super::GatewayActionOutcome::TimedOut,
                    payload: json!({"owner":"codex_capability_broker"}),
                });
                json!({"action":"cancel","content":null,"_meta":null})
            }
        }
    }
}

fn declined_elicitation() -> Value {
    json!({"action":"decline","content":null,"_meta":null})
}

#[derive(Debug, Clone)]
enum ElicitationFieldKind {
    String,
    Boolean,
    Number,
    StringArray,
    UrlAcceptance,
}

#[derive(Debug, Clone)]
struct ElicitationField {
    name: String,
    kind: ElicitationFieldKind,
    required: bool,
}

pub(super) struct PendingCodexElicitation {
    fields: Vec<ElicitationField>,
    mode: String,
    meta: Option<Value>,
    event_sink: super::GatewayEventSink,
    responder: oneshot::Sender<Value>,
}

struct ElicitationProjection {
    fields: Vec<ElicitationField>,
    mode: String,
    raw: Value,
}

fn project_elicitation(params: &Value) -> ElicitationProjection {
    let mode = params
        .get("mode")
        .and_then(Value::as_str)
        .unwrap_or("form")
        .to_string();
    let message = params
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or("The Codex App needs more information.");
    if mode == "url" {
        let url = params
            .get("url")
            .and_then(Value::as_str)
            .unwrap_or_default();
        return ElicitationProjection {
            fields: vec![ElicitationField {
                name: "url".to_string(),
                kind: ElicitationFieldKind::UrlAcceptance,
                required: true,
            }],
            mode,
            raw: json!({
                "url": url,
                "questions": [{
                    "header": "Open link",
                    "question": message,
                    "options": [{"label":"Open","description":url}],
                    "multiple": false,
                    "custom": false,
                    "secret": false,
                }]
            }),
        };
    }
    let schema = params.get("requestedSchema").and_then(Value::as_object);
    let properties = schema
        .and_then(|schema| schema.get("properties"))
        .and_then(Value::as_object);
    let required = schema
        .and_then(|schema| schema.get("required"))
        .and_then(Value::as_array)
        .map(|values| {
            values
                .iter()
                .filter_map(Value::as_str)
                .collect::<std::collections::BTreeSet<_>>()
        })
        .unwrap_or_default();
    let mut fields = Vec::new();
    let mut questions = Vec::new();
    for (name, property) in properties.into_iter().flatten() {
        let property_type = property
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or("string");
        let kind = match property_type {
            "boolean" => ElicitationFieldKind::Boolean,
            "number" | "integer" => ElicitationFieldKind::Number,
            "array" => ElicitationFieldKind::StringArray,
            _ => ElicitationFieldKind::String,
        };
        let options = elicitation_options(property, property_type);
        let has_options = !options.is_empty();
        let question = property
            .get("description")
            .or_else(|| property.get("title"))
            .and_then(Value::as_str)
            .unwrap_or(message);
        questions.push(json!({
            "header": name,
            "question": question,
            "options": options,
            "multiple": property_type == "array",
            "custom": property_type != "boolean" && !has_options,
            "secret": property.get("format").and_then(Value::as_str) == Some("password"),
            "required": required.contains(name.as_str()),
        }));
        fields.push(ElicitationField {
            name: name.clone(),
            kind,
            required: required.contains(name.as_str()),
        });
    }
    ElicitationProjection {
        fields,
        mode,
        raw: json!({"questions":questions}),
    }
}

fn elicitation_options(property: &Value, property_type: &str) -> Vec<Value> {
    if property_type == "openai/imagePicker" {
        return property
            .get("items")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|item| {
                let id = item.get("id")?.as_str()?;
                let title = item.get("title").and_then(Value::as_str).unwrap_or("");
                let image = item.get("image").and_then(Value::as_str);
                Some(json!({"label":id,"description":title,"image":image}))
            })
            .collect();
    }
    let values = property
        .get("enum")
        .and_then(Value::as_array)
        .map(|values| {
            let names = property.get("enumNames").and_then(Value::as_array);
            values
                .iter()
                .enumerate()
                .map(|(index, value)| {
                    let label = value
                        .as_str()
                        .map(str::to_string)
                        .unwrap_or_else(|| value.to_string());
                    let description = names
                        .and_then(|names| names.get(index))
                        .and_then(Value::as_str)
                        .unwrap_or("");
                    json!({"label":label,"description":description})
                })
                .collect::<Vec<_>>()
        })
        .or_else(|| titled_elicitation_options(property.get("oneOf")))
        .or_else(|| {
            let items = property.get("items")?;
            items
                .get("enum")
                .and_then(Value::as_array)
                .map(|values| {
                    values
                        .iter()
                        .map(|value| {
                            let label = value
                                .as_str()
                                .map(str::to_string)
                                .unwrap_or_else(|| value.to_string());
                            json!({"label":label,"description":""})
                        })
                        .collect::<Vec<_>>()
                })
                .or_else(|| {
                    titled_elicitation_options(items.get("anyOf").or_else(|| items.get("oneOf")))
                })
        });
    values.unwrap_or_else(|| {
        if property_type == "boolean" {
            vec![
                json!({"label":"Yes","description":""}),
                json!({"label":"No","description":""}),
            ]
        } else {
            Vec::new()
        }
    })
}

fn titled_elicitation_options(options: Option<&Value>) -> Option<Vec<Value>> {
    options.and_then(Value::as_array).map(|options| {
        options
            .iter()
            .filter_map(|option| {
                let value = option.get("const")?.as_str()?;
                let title = option.get("title").and_then(Value::as_str).unwrap_or("");
                Some(json!({"label":value,"description":title}))
            })
            .collect()
    })
}

pub(super) fn respond_to_elicitation(
    state: &super::WebState,
    interaction_id: &str,
    response: psychevo_gateway_protocol::ThreadInteractionResponse,
) -> Result<Option<psychevo_gateway_protocol::ThreadInteractionRespondResult>> {
    if !matches!(
        response,
        psychevo_gateway_protocol::ThreadInteractionResponse::Clarify { .. }
            | psychevo_gateway_protocol::ThreadInteractionResponse::CancelClarify
    ) {
        return Err(Error::Message(
            "Codex App elicitation requires a clarify response".to_string(),
        ));
    }
    let Some(pending) = state
        .inner
        .codex_elicitations
        .lock()
        .expect("Codex elicitation registry poisoned")
        .remove(interaction_id)
    else {
        return Ok(None);
    };
    let (result, outcome) = match response {
        psychevo_gateway_protocol::ThreadInteractionResponse::Clarify { answers } => {
            let result = accepted_elicitation(&pending, answers);
            (result, super::GatewayActionOutcome::Accepted)
        }
        psychevo_gateway_protocol::ThreadInteractionResponse::CancelClarify => (
            json!({"action":"cancel","content":null,"_meta":pending.meta}),
            super::GatewayActionOutcome::Cancelled,
        ),
        _ => unreachable!("response kind checked before taking the pending elicitation"),
    };
    (pending.event_sink)(super::GatewayEvent::ActionResolved {
        action_id: interaction_id.to_string(),
        kind: super::GatewayActionKind::Clarify,
        outcome,
        payload: json!({"owner":"codex_capability_broker"}),
    });
    let _ = pending.responder.send(result);
    Ok(Some(
        psychevo_gateway_protocol::ThreadInteractionRespondResult {
            accepted: true,
            interaction_id: interaction_id.to_string(),
            outcome,
        },
    ))
}

fn accepted_elicitation(pending: &PendingCodexElicitation, answers: Vec<Vec<String>>) -> Value {
    if pending.mode == "url" {
        let action = if answers
            .first()
            .and_then(|answers| answers.first())
            .is_some_and(|answer| answer == "Open")
        {
            "accept"
        } else {
            "decline"
        };
        return json!({"action":action,"content":null,"_meta":pending.meta});
    }
    let mut content = serde_json::Map::new();
    for (index, field) in pending.fields.iter().enumerate() {
        let values = answers.get(index).cloned().unwrap_or_default();
        if values.is_empty() && !field.required {
            continue;
        }
        let value = match field.kind {
            ElicitationFieldKind::Boolean => values
                .first()
                .map(|value| Value::Bool(value.eq_ignore_ascii_case("yes") || value == "true"))
                .unwrap_or(Value::Null),
            ElicitationFieldKind::Number => values
                .first()
                .and_then(|value| value.parse::<serde_json::Number>().ok())
                .map(Value::Number)
                .unwrap_or(Value::Null),
            ElicitationFieldKind::StringArray => {
                Value::Array(values.into_iter().map(Value::String).collect())
            }
            ElicitationFieldKind::String | ElicitationFieldKind::UrlAcceptance => values
                .into_iter()
                .next()
                .map(Value::String)
                .unwrap_or(Value::Null),
        };
        content.insert(field.name.clone(), value);
    }
    json!({"action":"accept","content":content,"_meta":pending.meta})
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CodexNegotiatedProfile {
    version: String,
}

fn validate_reviewed_profile(
    initialize: &Value,
    expected_home: &Path,
) -> Result<CodexNegotiatedProfile> {
    let user_agent = initialize
        .get("userAgent")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            Error::Message(format!(
                "Codex plugin compatibility profile `{}` requires initialize.userAgent",
                psychevo_runtime::CODEX_PLUGIN_COMPATIBILITY_PROFILE
            ))
        })?;
    let version = extract_semantic_version(user_agent).ok_or_else(|| {
        Error::Message(format!(
            "Codex plugin compatibility profile `{}` could not extract a version from userAgent",
            psychevo_runtime::CODEX_PLUGIN_COMPATIBILITY_PROFILE
        ))
    })?;
    if version != REVIEWED_CODEX_VERSION {
        return Err(Error::Message(format!(
            "Codex plugin compatibility profile `{}` reviewed `{REVIEWED_CODEX_VERSION}` but resolved `{version}`",
            psychevo_runtime::CODEX_PLUGIN_COMPATIBILITY_PROFILE
        )));
    }
    let reported_home = initialize
        .get("codexHome")
        .and_then(Value::as_str)
        .map(PathBuf::from)
        .ok_or_else(|| {
            Error::Message(format!(
                "Codex plugin compatibility profile `{}` requires initialize.codexHome",
                psychevo_runtime::CODEX_PLUGIN_COMPATIBILITY_PROFILE
            ))
        })?;
    let expected = std::fs::canonicalize(expected_home).map_err(|err| {
        Error::Message(format!(
            "Codex plugin compatibility profile `{}` could not canonicalize private home `{}`: {err}",
            psychevo_runtime::CODEX_PLUGIN_COMPATIBILITY_PROFILE,
            expected_home.display()
        ))
    })?;
    let reported = std::fs::canonicalize(&reported_home).map_err(|err| {
        Error::Message(format!(
            "Codex plugin compatibility profile `{}` could not canonicalize reported home `{}`: {err}",
            psychevo_runtime::CODEX_PLUGIN_COMPATIBILITY_PROFILE,
            reported_home.display()
        ))
    })?;
    if reported != expected {
        return Err(Error::Message(format!(
            "Codex plugin compatibility profile `{}` rejected codexHome `{}`; expected Psychevo private home `{}`",
            psychevo_runtime::CODEX_PLUGIN_COMPATIBILITY_PROFILE,
            reported.display(),
            expected.display()
        )));
    }
    Ok(CodexNegotiatedProfile { version })
}

fn extract_semantic_version(user_agent: &str) -> Option<String> {
    user_agent
        .split(|ch: char| !(ch.is_ascii_digit() || ch == '.'))
        .find(|candidate| {
            let mut parts = candidate.split('.');
            let valid = (0..3).all(|_| {
                parts.next().is_some_and(|part| {
                    !part.is_empty() && part.chars().all(|ch| ch.is_ascii_digit())
                })
            });
            valid && parts.next().is_none()
        })
        .map(str::to_string)
}

struct BrokerProcess {
    child: Mutex<Child>,
    writer: mpsc::UnboundedSender<Value>,
    pending: Arc<Mutex<BTreeMap<u64, oneshot::Sender<Value>>>>,
    elicitation_contexts: Arc<Mutex<BTreeMap<String, CodexElicitationContext>>>,
    next_id: std::sync::atomic::AtomicU64,
    effective_plugins_changed: Arc<AtomicBool>,
    codex_version: Option<String>,
}

impl BrokerProcess {
    async fn spawn(
        broker_command: &BrokerCommand,
        env: &BTreeMap<String, String>,
        request_timeout: Duration,
        expected_home: Option<&Path>,
    ) -> Result<Self> {
        let mut command = Command::new(&broker_command.program);
        command
            .args(&broker_command.args)
            .env_clear()
            .envs(env)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        let mut child = command.spawn().map_err(|err| {
            Error::Message(format!(
                "Codex capability broker could not start `{}`: {err}",
                broker_command.program.display()
            ))
        })?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| Error::Message("Codex broker stdin unavailable".to_string()))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| Error::Message("Codex broker stdout unavailable".to_string()))?;
        let stderr = child.stderr.take();
        let (writer, writer_rx) = mpsc::unbounded_channel();
        let pending = Arc::new(Mutex::new(BTreeMap::new()));
        let elicitation_contexts = Arc::new(Mutex::new(BTreeMap::new()));
        let effective_plugins_changed = Arc::new(AtomicBool::new(false));
        spawn_broker_writer(stdin, writer_rx);
        spawn_broker_reader(
            stdout,
            writer.clone(),
            pending.clone(),
            elicitation_contexts.clone(),
            effective_plugins_changed.clone(),
        );
        if let Some(stderr) = stderr {
            spawn_broker_stderr(stderr);
        }
        let mut process = Self {
            child: Mutex::new(child),
            writer,
            pending,
            elicitation_contexts,
            next_id: std::sync::atomic::AtomicU64::new(1),
            effective_plugins_changed,
            codex_version: None,
        };
        let initialize = process
            .request_with_context(
                "initialize",
                json!({
                    "clientInfo": {
                        "name": "psychevo-capability-broker",
                        "title": "Psychevo",
                        "version": env!("CARGO_PKG_VERSION"),
                    },
                    "capabilities": {
                        "experimentalApi": true,
                        "mcpServerOpenaiFormElicitation": true,
                    }
                }),
                request_timeout,
                None,
            )
            .await?;
        if let Some(expected_home) = expected_home {
            let profile = validate_reviewed_profile(&initialize, expected_home)?;
            process.codex_version = Some(profile.version);
        }
        process.write_message(json!({"jsonrpc":"2.0","method":"initialized"}))?;
        if expected_home.is_some() {
            process.probe_required_methods(request_timeout).await?;
        }
        Ok(process)
    }

    async fn probe_required_methods(&self, request_timeout: Duration) -> Result<()> {
        for method in REQUIRED_CODEX_METHODS {
            match self
                .request_with_context(method, Value::Null, request_timeout, None)
                .await
            {
                Ok(_) => {
                    return Err(Error::Message(format!(
                        "Codex plugin compatibility profile `{}` rejected `{method}` because the invalid-parameter probe unexpectedly succeeded",
                        psychevo_runtime::CODEX_PLUGIN_COMPATIBILITY_PROFILE
                    )));
                }
                Err(err) => {
                    let message = err.to_string().to_ascii_lowercase();
                    if message.contains("method not found") || message.contains("-32601") {
                        return Err(Error::Message(format!(
                            "Codex plugin compatibility profile `{}` requires method `{method}`",
                            psychevo_runtime::CODEX_PLUGIN_COMPATIBILITY_PROFILE
                        )));
                    }
                    if !message.contains("-32602") {
                        return Err(Error::Message(format!(
                            "Codex plugin compatibility profile `{}` expected `{method}` to reject the probe during argument parsing: {err}",
                            psychevo_runtime::CODEX_PLUGIN_COMPATIBILITY_PROFILE
                        )));
                    }
                }
            }
        }
        Ok(())
    }

    async fn request_with_context(
        &self,
        method: &str,
        params: Value,
        request_timeout: Duration,
        context: Option<&CodexElicitationContext>,
    ) -> Result<Value> {
        let id = self.next_id.fetch_add(1, Ordering::AcqRel);
        let context_key = context.and_then(|_| elicitation_key(&params));
        if let (Some(key), Some(context)) = (context_key.as_ref(), context) {
            self.elicitation_contexts
                .lock()
                .await
                .insert(key.clone(), context.clone());
        }
        let (sender, receiver) = oneshot::channel();
        self.pending.lock().await.insert(id, sender);
        if let Err(err) = self.write_message(json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        })) {
            self.pending.lock().await.remove(&id);
            if let Some(key) = context_key {
                self.elicitation_contexts.lock().await.remove(&key);
            }
            return Err(err);
        }
        let response = timeout(request_timeout, receiver).await;
        self.pending.lock().await.remove(&id);
        if let Some(key) = context_key {
            self.elicitation_contexts.lock().await.remove(&key);
        }
        let message = response
            .map_err(|_| {
                Error::Message(format!(
                    "Codex capability broker request `{method}` timed out"
                ))
            })?
            .map_err(|_| {
                Error::Message("Codex capability broker exited unexpectedly".to_string())
            })?;
        if let Some(error) = message.get("error") {
            let code = error.get("code").and_then(Value::as_i64);
            return Err(Error::Message(format!(
                "Codex broker request failed{}: {}",
                code.map(|code| format!(" ({code})")).unwrap_or_default(),
                error
                    .get("message")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown JSON-RPC error")
            )));
        }
        Ok(message.get("result").cloned().unwrap_or(Value::Null))
    }

    fn take_effective_plugins_changed(&self) -> bool {
        self.effective_plugins_changed.swap(false, Ordering::AcqRel)
    }

    fn write_message(&self, message: Value) -> Result<()> {
        self.writer.send(message).map_err(|_| {
            Error::Message("Codex capability broker writer is unavailable".to_string())
        })
    }

    async fn kill(&self) {
        let _ = self.child.lock().await.kill().await;
    }
}

fn spawn_broker_writer(mut stdin: ChildStdin, mut receiver: mpsc::UnboundedReceiver<Value>) {
    tokio::spawn(async move {
        while let Some(message) = receiver.recv().await {
            let Ok(mut bytes) = serde_json::to_vec(&message) else {
                continue;
            };
            bytes.push(b'\n');
            if stdin.write_all(&bytes).await.is_err() || stdin.flush().await.is_err() {
                break;
            }
        }
    });
}

fn spawn_broker_reader(
    stdout: ChildStdout,
    writer: mpsc::UnboundedSender<Value>,
    pending: Arc<Mutex<BTreeMap<u64, oneshot::Sender<Value>>>>,
    elicitation_contexts: Arc<Mutex<BTreeMap<String, CodexElicitationContext>>>,
    effective_plugins_changed: Arc<AtomicBool>,
) {
    tokio::spawn(async move {
        let mut lines = BufReader::new(stdout).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            let Ok(message) = serde_json::from_str::<Value>(&line) else {
                continue;
            };
            if message.get("method").is_some() && message.get("id").is_some() {
                let writer = writer.clone();
                let contexts = elicitation_contexts.clone();
                tokio::spawn(async move {
                    respond_to_broker_server_request(message, writer, contexts).await;
                });
                continue;
            }
            if message.get("method").and_then(Value::as_str) == Some("account/updated") {
                effective_plugins_changed.store(true, Ordering::Release);
                continue;
            }
            let Some(id) = message.get("id").and_then(Value::as_u64) else {
                continue;
            };
            if let Some(sender) = pending.lock().await.remove(&id) {
                let _ = sender.send(message);
            }
        }
        pending.lock().await.clear();
    });
}

async fn respond_to_broker_server_request(
    message: Value,
    writer: mpsc::UnboundedSender<Value>,
    contexts: Arc<Mutex<BTreeMap<String, CodexElicitationContext>>>,
) {
    let id = message.get("id").cloned().unwrap_or(Value::Null);
    let method = message
        .get("method")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let response = if method == "mcpServer/elicitation/request" {
        let key = message.get("params").and_then(elicitation_key);
        let context = {
            let contexts = contexts.lock().await;
            key.as_ref()
                .and_then(|key| contexts.get(key))
                .cloned()
                .or_else(|| {
                    (contexts.len() == 1)
                        .then(|| contexts.values().next().cloned())
                        .flatten()
                })
        };
        let result = if let Some(context) = context {
            context.route(&message).await
        } else {
            declined_elicitation()
        };
        json!({"jsonrpc":"2.0","id":id,"result":result})
    } else {
        json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": {"code":-32601,"message":format!("unsupported broker callback: {method}")},
        })
    };
    let _ = writer.send(response);
}

fn elicitation_key(params: &Value) -> Option<String> {
    let thread_id = params.get("threadId").and_then(Value::as_str)?;
    let turn_id = params
        .get("turnId")
        .and_then(Value::as_str)
        .unwrap_or_default();
    Some(format!("{thread_id}\u{0}{turn_id}"))
}

fn spawn_broker_stderr(stderr: tokio::process::ChildStderr) {
    tokio::spawn(async move {
        let mut lines = BufReader::new(stderr).lines();
        let mut emitted = 0usize;
        while let Ok(Some(line)) = lines.next_line().await {
            if emitted >= 256 {
                continue;
            }
            emitted += 1;
            let lower = line.to_ascii_lowercase();
            let classification = if ["token", "secret", "authorization", "credential"]
                .iter()
                .any(|needle| lower.contains(needle))
            {
                "sensitive"
            } else {
                "diagnostic"
            };
            eprintln!(
                "{}",
                json!({
                    "target": "psychevo.codex_plugins",
                    "event": "broker_stderr",
                    "classification": classification,
                    "bytes": line.len().min(2_048),
                    "truncated": line.len() > 2_048,
                    "redacted": true,
                })
            );
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;

    #[cfg(unix)]
    const CODEX_APP_SERVER_FIXTURE: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/codex_app_server.py"
    ));

    #[cfg(unix)]
    fn write_codex_app_server_fixture(script: &Path, scenario: &str, mut config: Value) {
        config
            .as_object_mut()
            .expect("Codex fixture config object")
            .insert("scenario".to_string(), json!(scenario));
        fs::write(script, CODEX_APP_SERVER_FIXTURE).expect("Codex app-server fixture");
        fs::write(
            script.with_extension("json"),
            serde_json::to_vec(&config).expect("Codex fixture config JSON"),
        )
        .expect("Codex fixture config");
        let mut permissions = fs::metadata(script)
            .expect("fixture metadata")
            .permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(script, permissions).expect("fixture permissions");
    }

    #[test]
    fn connect_url_validation_uses_the_parsed_origin() {
        for allowed in [
            "https://apps.example.test/install/review",
            "http://localhost:4711/callback",
            "http://127.0.0.1:4711/callback",
            "http://[::1]:4711/callback",
        ] {
            validate_connect_url(allowed)
                .unwrap_or_else(|error| panic!("expected `{allowed}` to be accepted: {error}"));
        }
        for rejected in [
            "http://apps.example.test/install/review",
            "http://localhost.evil.example/install/review",
            "http://127.0.0.1@evil.example/install/review",
            "https:///missing-authority",
            "javascript:alert(1)",
        ] {
            assert!(
                validate_connect_url(rejected).is_err(),
                "expected `{rejected}` to be rejected"
            );
        }
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn default_off_authority_does_not_spawn_codex() {
        let temp = tempfile::tempdir().expect("temp");
        let profile_home = temp.path().join("psychevo");
        let cwd = temp.path().join("work");
        let script = temp.path().join("fake-codex.py");
        let spawned = temp.path().join("spawned");
        fs::create_dir_all(&profile_home).expect("profile home");
        fs::create_dir_all(&cwd).expect("cwd");
        fs::write(profile_home.join("config.toml"), "# default off\n").expect("config");
        fs::write(
            &script,
            format!(
                "#!/usr/bin/env python3\nfrom pathlib import Path\nPath({spawned}).write_text('spawned')\n",
                spawned = serde_json::to_string(&spawned).expect("spawned json"),
            ),
        )
        .expect("script");
        let mut permissions = fs::metadata(&script).expect("metadata").permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&script, permissions).expect("chmod");
        let env = BTreeMap::from([
            ("HOME".to_string(), temp.path().display().to_string()),
            (
                "PSYCHEVO_HOME".to_string(),
                profile_home.display().to_string(),
            ),
            (
                "PSYCHEVO_CODEX_BIN".to_string(),
                script.display().to_string(),
            ),
        ]);

        let broker = CodexCapabilityBroker::new(&env);
        let error = broker
            .plugin_list(&cwd)
            .await
            .expect_err("disabled authority");

        assert!(error.to_string().contains("disabled"));
        assert!(
            !spawned.exists(),
            "feature-off listing must not spawn Codex"
        );
        assert_eq!(broker.authority_view()["runtime"], "disabled");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn enabled_authority_forces_private_home_and_links_auth_without_copying() {
        let temp = tempfile::tempdir().expect("temp");
        let user_home = temp.path().join("user");
        let profile_home = temp.path().join("psychevo");
        let private_home = profile_home.join("codex");
        let cwd = temp.path().join("work");
        let script = temp.path().join("fake-codex.py");
        let log = temp.path().join("process.json");
        fs::create_dir_all(user_home.join(".codex")).expect("global Codex home");
        fs::create_dir_all(&profile_home).expect("profile home");
        fs::create_dir_all(&cwd).expect("cwd");
        fs::write(user_home.join(".codex/auth.json"), "secret-never-read").expect("auth");
        fs::write(
            profile_home.join("config.toml"),
            "[codex_plugins]\nenabled = true\n",
        )
        .expect("config");
        write_codex_app_server_fixture(&script, "private_home", json!({"log": log}));
        let env = BTreeMap::from([
            ("HOME".to_string(), user_home.display().to_string()),
            (
                "PSYCHEVO_HOME".to_string(),
                profile_home.display().to_string(),
            ),
            (
                "PSYCHEVO_CODEX_BIN".to_string(),
                script.display().to_string(),
            ),
            (
                "CODEX_HOME".to_string(),
                temp.path().join("must-not-inherit").display().to_string(),
            ),
            (
                "PATH".to_string(),
                std::env::var("PATH").unwrap_or_default(),
            ),
        ]);

        let broker = CodexCapabilityBroker::new(&env);
        broker
            .plugin_installed(&cwd)
            .await
            .expect("private broker request");

        let process: Value = serde_json::from_str(&fs::read_to_string(&log).expect("process log"))
            .expect("process json");
        assert_eq!(process["codexHome"], private_home.display().to_string());
        assert_eq!(
            process["argv"],
            json!([
                "app-server",
                "--strict-config",
                "-c",
                "cli_auth_credentials_store=\"file\"",
                "--listen",
                "stdio://"
            ])
        );
        assert_eq!(
            fs::read_link(private_home.join("auth.json")).expect("auth symlink"),
            user_home.join(".codex/auth.json")
        );
        assert_eq!(broker.authority_view()["auth"], "available");
        broker.stop().await;
    }

    #[test]
    fn reviewed_profile_accepts_arbitrary_originator_and_rejects_version_or_home_drift() {
        let temp = tempfile::tempdir().expect("temp");
        let private_home = temp.path().join("codex");
        let other_home = temp.path().join("other");
        fs::create_dir_all(&private_home).expect("private home");
        fs::create_dir_all(&other_home).expect("other home");

        for user_agent in [
            "codex_cli_rs/0.144.1 (Linux 6.8; x86_64)",
            "codex_vscode/0.144.1 (extension; originator=desktop)",
            "third-party-originator 0.144.1",
        ] {
            let profile = validate_reviewed_profile(
                &json!({
                    "userAgent": user_agent,
                    "codexHome": private_home,
                }),
                &private_home,
            )
            .expect("reviewed profile");
            assert_eq!(profile.version, REVIEWED_CODEX_VERSION);
        }

        let version = validate_reviewed_profile(
            &json!({
                "userAgent": "codex_cli_rs/0.145.0",
                "codexHome": private_home,
            }),
            &private_home,
        )
        .expect_err("unknown version");
        assert!(version.to_string().contains("reviewed `0.144.1`"));

        let home = validate_reviewed_profile(
            &json!({
                "userAgent": "codex_cli_rs/0.144.1",
                "codexHome": other_home,
            }),
            &private_home,
        )
        .expect_err("home drift");
        assert!(home.to_string().contains("private home"));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn preflight_rejects_missing_method_without_fetching_catalog() {
        let temp = tempfile::tempdir().expect("temp");
        let profile_home = temp.path().join("psychevo");
        let private_home = profile_home.join("codex");
        let cwd = temp.path().join("work");
        let script = temp.path().join("fake-codex.py");
        let log = temp.path().join("calls.log");
        fs::create_dir_all(&profile_home).expect("profile home");
        fs::create_dir_all(&cwd).expect("cwd");
        fs::write(
            profile_home.join("config.toml"),
            format!(
                "[codex_plugins]\nenabled = true\nbinary = {:?}\n",
                script.display().to_string()
            ),
        )
        .expect("config");
        write_codex_app_server_fixture(&script, "preflight_missing_method", json!({"log": log}));
        let broker = CodexCapabilityBroker::new(&BTreeMap::from([
            (
                "PSYCHEVO_HOME".to_string(),
                profile_home.display().to_string(),
            ),
            (
                "PATH".to_string(),
                std::env::var("PATH").unwrap_or_default(),
            ),
        ]));

        let error = broker.plugin_list(&cwd).await.expect_err("missing method");

        assert!(error.to_string().contains("requires method `app/list`"));
        assert_eq!(broker.authority_view()["runtime"], "incompatible");
        let calls = fs::read_to_string(&log).expect("preflight log");
        assert!(calls.lines().all(|line| line.ends_with(":null")));
        assert!(!calls.contains("plugin/list:normal"));
        assert!(private_home.is_dir());
        broker.stop().await;
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn broker_handshakes_reuses_process_and_declines_unrouted_elicitation() {
        let temp = tempfile::tempdir().expect("temp");
        let script = temp.path().join("fake-codex.py");
        write_codex_app_server_fixture(&script, "handshake", json!({}));
        let broker = CodexCapabilityBroker::with_command(
            BrokerCommand {
                program: script,
                args: Vec::new(),
            },
            BTreeMap::from([(
                "PATH".to_string(),
                std::env::var("PATH").unwrap_or_default(),
            )]),
            Duration::from_secs(3),
        );

        let plugins = broker
            .request("plugin/list", json!({"cwds":[]}))
            .await
            .expect("plugin list");
        let identity = CodexPluginIdentity::parse_selector("codex:review@openai")
            .expect("selector")
            .expect("codex identity");
        let detail = broker
            .plugin_read(temp.path(), &identity)
            .await
            .expect("plugin read");
        let install = broker
            .plugin_install(temp.path(), &identity)
            .await
            .expect("plugin install");
        let uninstall = broker
            .plugin_uninstall(temp.path(), &identity)
            .await
            .expect("plugin uninstall");
        let apps = broker
            .request("app/list", json!({"forceRefetch":false}))
            .await
            .expect("app list");

        assert_eq!(plugins["marketplaces"][0]["name"], "openai");
        assert_eq!(detail["plugin"]["description"], "Review");
        assert_eq!(install["authPolicy"], "ON_USE");
        assert_eq!(uninstall, json!({}));
        assert_eq!(apps["data"], json!([]));
        broker.stop().await;
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn elicitation_wait_does_not_block_catalog_or_another_request() {
        let temp = tempfile::tempdir().expect("temp");
        let cwd = temp.path().join("work");
        let home = temp.path().join("home");
        fs::create_dir_all(&cwd).expect("cwd");
        fs::create_dir_all(&home).expect("home");
        fs::write(home.join("config.toml"), "# config\n").expect("config");
        let script = temp.path().join("fake-codex.py");
        write_codex_app_server_fixture(&script, "elicitation_wait", json!({}));
        let env = BTreeMap::from([(
            "PATH".to_string(),
            std::env::var("PATH").unwrap_or_default(),
        )]);
        let broker = Arc::new(CodexCapabilityBroker::with_command(
            BrokerCommand {
                program: script,
                args: Vec::new(),
            },
            env,
            Duration::from_secs(3),
        ));
        let runtime =
            psychevo_runtime::StateRuntime::open(temp.path().join("state.db")).expect("state");
        let gateway = crate::Gateway::new(runtime);
        let state = super::super::WebState::new(super::super::GatewayWebServerConfig::new(
            gateway,
            home,
            cwd,
            None,
            BTreeMap::new(),
            temp.path().join("static"),
        ));
        let (action_tx, mut action_rx) = tokio::sync::mpsc::unbounded_channel();
        let event_sink: crate::GatewayEventSink = Arc::new(move |event| {
            if let crate::GatewayEvent::ActionRequested { action } = event {
                let _ = action_tx.send(action.action_id);
            }
        });
        let context = CodexElicitationContext {
            state: state.clone(),
            psychevo_thread_id: "psychevo-thread".to_string(),
            turn_id: Some("turn-a".to_string()),
            event_sink: Some(event_sink),
        };
        let tool_broker = broker.clone();
        let tool = tokio::spawn(async move {
            tool_broker
                .request_with_context(
                    "mcpServer/tool/call",
                    json!({
                        "threadId":"codex-thread",
                        "turnId":"turn-a",
                        "serverName":"codex_apps",
                        "toolName":"review",
                        "arguments":{},
                    }),
                    Some(&context),
                )
                .await
        });
        let action_id = tokio::time::timeout(Duration::from_secs(1), action_rx.recv())
            .await
            .expect("elicitation action")
            .expect("action id");

        let catalog =
            tokio::time::timeout(Duration::from_millis(250), broker.plugin_list(temp.path()))
                .await
                .expect("catalog must not wait for elicitation")
                .expect("catalog response");
        assert_eq!(catalog["marketplaces"], json!([]));

        respond_to_elicitation(
            &state,
            &action_id,
            psychevo_gateway_protocol::ThreadInteractionResponse::Clarify {
                answers: vec![vec!["Yes".to_string()]],
            },
        )
        .expect("elicitation response");
        let tool_result = tool.await.expect("tool task").expect("tool response");
        assert_eq!(tool_result["content"][0]["text"], "done");
        broker.stop().await;
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn runtime_inventory_is_single_flight_per_canonical_cwd() {
        let temp = tempfile::tempdir().expect("temp");
        let cwd = temp.path().join("work");
        let other_cwd = temp.path().join("other");
        fs::create_dir_all(&cwd).expect("cwd");
        fs::create_dir_all(&other_cwd).expect("other cwd");
        let script = temp.path().join("fake-codex.py");
        let log = temp.path().join("calls.log");
        write_codex_app_server_fixture(&script, "inventory_single_flight", json!({"log": log}));
        let broker = CodexCapabilityBroker::with_command(
            BrokerCommand {
                program: script,
                args: Vec::new(),
            },
            BTreeMap::from([(
                "PATH".to_string(),
                std::env::var("PATH").unwrap_or_default(),
            )]),
            Duration::from_secs(1),
        );

        let (left, right) = tokio::join!(
            broker.prepare_runtime_inventory(&cwd),
            broker.prepare_runtime_inventory(&cwd)
        );
        left.expect("left inventory");
        right.expect("right inventory");
        assert_eq!(
            fs::read_to_string(&log).expect("single-flight log"),
            "plugin-installed\n"
        );

        broker
            .prepare_runtime_inventory(&other_cwd)
            .await
            .expect("other cwd inventory");
        assert_eq!(
            fs::read_to_string(&log).expect("per-cwd log"),
            "plugin-installed\nplugin-installed\n"
        );
        broker.stop().await;
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn account_update_notification_invalidates_runtime_inventory() {
        let temp = tempfile::tempdir().expect("temp");
        let cwd = temp.path().join("work");
        fs::create_dir_all(&cwd).expect("cwd");
        let script = temp.path().join("fake-codex.py");
        let log = temp.path().join("calls.log");
        write_codex_app_server_fixture(&script, "account_update", json!({"log": log}));
        let broker = CodexCapabilityBroker::with_command(
            BrokerCommand {
                program: script,
                args: Vec::new(),
            },
            BTreeMap::from([(
                "PATH".to_string(),
                std::env::var("PATH").unwrap_or_default(),
            )]),
            Duration::from_secs(1),
        );

        broker
            .prepare_runtime_inventory(&cwd)
            .await
            .expect("inventory before account update");
        broker
            .prepare_runtime_inventory(&cwd)
            .await
            .expect("inventory after account update");
        assert_eq!(
            fs::read_to_string(&log).expect("account update log"),
            "plugin-installed\nplugin-installed\n"
        );
        broker.stop().await;
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn initialize_and_thread_start_prewarms_inventory_without_blocking() {
        let temp = tempfile::tempdir().expect("temp");
        let cwd = temp.path().join("work");
        let home = temp.path().join("home");
        fs::create_dir_all(&cwd).expect("cwd");
        fs::create_dir_all(&home).expect("home");
        let script = temp.path().join("fake-codex.py");
        let log = temp.path().join("calls.log");
        let release = temp.path().join("release");
        write_codex_app_server_fixture(
            &script,
            "prewarm_inventory",
            json!({
                "log": log,
                "release": release,
                "private_home": home.join("codex"),
            }),
        );
        fs::write(
            home.join("config.toml"),
            format!(
                "[codex_plugins]\nenabled = true\nbinary = {:?}\n",
                script.display().to_string()
            ),
        )
        .expect("Codex authority config");
        let env = BTreeMap::from([
            ("HOME".to_string(), temp.path().display().to_string()),
            ("PSYCHEVO_HOME".to_string(), home.display().to_string()),
            (
                "PSYCHEVO_CODEX_BIN".to_string(),
                script.display().to_string(),
            ),
            (
                "PATH".to_string(),
                std::env::var("PATH").unwrap_or_default(),
            ),
        ]);
        let runtime =
            psychevo_runtime::StateRuntime::open(temp.path().join("state.db")).expect("state");
        let gateway = crate::Gateway::new(runtime);
        let config = super::super::GatewayWebServerConfig::new(
            gateway,
            home,
            cwd.clone(),
            None,
            env,
            temp.path().join("static"),
        );
        let state = super::super::WebState::new(config);
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();

        tokio::time::timeout(
            Duration::from_millis(100),
            super::super::handle_rpc(
                state.clone(),
                super::super::AuthContext::Bearer,
                tx.clone(),
                super::super::RpcRequest {
                    jsonrpc: "2.0".to_string(),
                    id: Some(json!(1)),
                    method: "initialize".to_string(),
                    params: None,
                },
            ),
        )
        .await
        .expect("initialize must not wait for runtime inventory")
        .expect("initialize response");
        tokio::time::timeout(Duration::from_secs(1), async {
            while !log.exists() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("inventory prewarm started");

        let degraded = tokio::time::timeout(
            Duration::from_millis(100),
            state.inner.codex_capability_broker.runtime_contributions(
                state.clone(),
                &cwd,
                "pending-inventory-thread",
                Some("pending-inventory-turn".to_string()),
                None,
            ),
        )
        .await
        .expect("provider hot path must not wait for Codex inventory")
        .expect("degraded contributions");
        assert!(degraded.capability_roots.is_empty());
        assert!(degraded.runtime_tools.is_empty());

        let thread_state = state.clone();
        let thread_cwd = cwd.clone();
        let mut draft_open = tokio::spawn(async move {
            super::super::handle_rpc(
                thread_state,
                super::super::AuthContext::Bearer,
                tx,
                super::super::RpcRequest {
                    jsonrpc: "2.0".to_string(),
                    id: Some(json!(2)),
                    method: "thread/draft/open".to_string(),
                    params: Some(json!({
                        "origin": {
                            "cwd": thread_cwd,
                            "source": {"kind":"web","rawId":"prewarm-test"}
                        },
                        "targetIntent": {"kind":"default"}
                    })),
                },
            )
            .await
        });
        let completed_without_inventory =
            tokio::time::timeout(Duration::from_millis(100), &mut draft_open).await;
        let draft_open_was_ready = completed_without_inventory.is_ok();
        fs::write(&release, "ready").expect("release inventory");
        match completed_without_inventory {
            Ok(response) => response
                .expect("thread/draft/open task")
                .expect("thread/draft/open response"),
            Err(_) => draft_open
                .await
                .expect("thread/draft/open task after inventory release")
                .expect("thread/draft/open response after inventory release"),
        };
        assert!(
            draft_open_was_ready,
            "thread/draft/open must not wait for runtime inventory"
        );
        assert_eq!(
            fs::read_to_string(&log).expect("prewarm log"),
            "plugin-installed\n"
        );
        state.inner.codex_capability_broker.stop().await;
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn failed_runtime_inventory_is_negative_cached_until_retry_delay() {
        let temp = tempfile::tempdir().expect("temp");
        let cwd = temp.path().join("work");
        fs::create_dir_all(&cwd).expect("cwd");
        let script = temp.path().join("fake-codex.py");
        let log = temp.path().join("calls.log");
        write_codex_app_server_fixture(&script, "failed_inventory", json!({"log": log}));
        let broker = CodexCapabilityBroker::with_command_and_runtime_retry(
            BrokerCommand {
                program: script,
                args: Vec::new(),
            },
            BTreeMap::from([(
                "PATH".to_string(),
                std::env::var("PATH").unwrap_or_default(),
            )]),
            Duration::from_secs(1),
            Duration::from_millis(10),
        );

        assert!(broker.runtime_inventory(&cwd).await.is_err());
        assert!(broker.runtime_inventory(&cwd).await.is_err());
        assert_eq!(
            fs::read_to_string(&log).expect("negative-cache log"),
            "plugin-installed\n"
        );

        tokio::time::sleep(Duration::from_millis(20)).await;
        assert!(broker.runtime_inventory(&cwd).await.is_err());
        assert_eq!(
            fs::read_to_string(&log).expect("retry log"),
            "plugin-installed\nplugin-installed\n"
        );
        broker.stop().await;
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn catalog_mutation_publishes_a_new_generation_for_next_turns() {
        let temp = tempfile::tempdir().expect("temp");
        let cwd = temp.path().join("work");
        let home = temp.path().join("home");
        let alpha = temp.path().join("alpha");
        let beta = temp.path().join("beta");
        fs::create_dir_all(&cwd).expect("cwd");
        fs::create_dir_all(alpha.join(".codex-plugin")).expect("alpha manifest dir");
        fs::create_dir_all(beta.join(".codex-plugin")).expect("beta manifest dir");
        fs::write(alpha.join(".codex-plugin/plugin.json"), "{}").expect("alpha manifest");
        fs::write(beta.join(".codex-plugin/plugin.json"), "{}").expect("beta manifest");
        let script = temp.path().join("fake-codex.py");
        let log = temp.path().join("calls.log");
        write_codex_app_server_fixture(
            &script,
            "catalog_mutation",
            json!({"alpha": alpha, "beta": beta, "log": log}),
        );
        let env = BTreeMap::from([
            ("HOME".to_string(), temp.path().display().to_string()),
            ("PSYCHEVO_HOME".to_string(), home.display().to_string()),
            (
                "PSYCHEVO_CODEX_BIN".to_string(),
                script.display().to_string(),
            ),
            (
                "PATH".to_string(),
                std::env::var("PATH").unwrap_or_default(),
            ),
        ]);
        let broker = CodexCapabilityBroker::with_command(
            BrokerCommand {
                program: script.clone(),
                args: Vec::new(),
            },
            env.clone(),
            Duration::from_secs(2),
        );
        let runtime =
            psychevo_runtime::StateRuntime::open(temp.path().join("state.db")).expect("state");
        let gateway = crate::Gateway::new(runtime);
        let config = super::super::GatewayWebServerConfig::new(
            gateway,
            home,
            cwd.clone(),
            None,
            env,
            temp.path().join("static"),
        );
        let state = super::super::WebState::new(config);

        broker
            .prepare_runtime_inventory(&cwd)
            .await
            .expect("prewarm alpha inventory");
        let alpha_profile = broker
            .runtime_contributions(state.clone(), &cwd, "thread-alpha", None, None)
            .await
            .expect("alpha profile");
        assert_eq!(alpha_profile.capability_roots[0].id, "codex:alpha@openai");

        let beta_identity = CodexPluginIdentity {
            plugin: "beta".to_string(),
            marketplace: "openai".to_string(),
        };
        broker
            .plugin_install(&cwd, &beta_identity)
            .await
            .expect("install beta");
        assert_eq!(alpha_profile.capability_roots[0].id, "codex:alpha@openai");
        broker
            .release_turn_lease(alpha_profile.lease_id.as_deref().expect("alpha lease"))
            .await;
        let beta_profile = broker
            .runtime_contributions(state.clone(), &cwd, "thread-beta", None, None)
            .await
            .expect("beta profile");
        assert_eq!(beta_profile.capability_roots[0].id, "codex:beta@openai");

        broker
            .release_turn_lease(beta_profile.lease_id.as_deref().expect("beta lease"))
            .await;
        broker
            .plugin_uninstall(&cwd, &beta_identity)
            .await
            .expect("uninstall beta");
        assert_eq!(beta_profile.capability_roots[0].id, "codex:beta@openai");
        let empty_profile = broker
            .runtime_contributions(state.clone(), &cwd, "thread-empty", None, None)
            .await
            .expect("empty profile");
        assert!(empty_profile.capability_roots.is_empty());
        broker
            .release_turn_lease(empty_profile.lease_id.as_deref().expect("empty lease"))
            .await;
        assert_eq!(
            fs::read_to_string(&log).expect("mutation log"),
            "installed:alpha\ninstall\ninstalled:beta\nuninstall\ninstalled:none\n"
        );
        broker.stop().await;
    }

    #[test]
    fn list_merge_preserves_authority_qualified_identity() {
        let native = json!({
            "plugins": [{
                "name":"review",
                "selector":"profile:review@local",
                "authority":{"kind":"psychevo","selector":"profile:review@local"}
            }],
            "count":1
        });
        let codex = json!({
            "marketplaces":[{
                "name":"openai",
                "path":null,
                "plugins":[{
                    "id":"review@openai",
                    "name":"review",
                    "installed":false,
                    "enabled":false,
                    "interface":{"shortDescription":"Review"}
                }]
            }]
        });

        let merged = merge_plugin_list(native, Ok(codex));

        assert_eq!(merged["count"], 2);
        assert_eq!(merged["plugins"][0]["selector"], "profile:review@local");
        assert_eq!(merged["plugins"][1]["selector"], "codex:review@openai");
        assert_eq!(merged["plugins"][1]["canonical_id"], "review@openai");
        assert_eq!(merged["plugins"][1]["authority"]["kind"], "codex");
    }

    #[test]
    fn codex_read_projects_component_owners_without_claiming_native_apps() {
        let temp = tempfile::tempdir().expect("temp");
        let package = temp.path().join("review");
        fs::create_dir_all(package.join(".codex-plugin")).expect("manifest dir");
        fs::create_dir_all(package.join("skills/review")).expect("skill dir");
        fs::write(package.join(".codex-plugin/plugin.json"), "{}").expect("manifest");
        fs::write(package.join("skills/review/SKILL.md"), "# Review").expect("skill");
        let identity = CodexPluginIdentity {
            plugin: "review".to_string(),
            marketplace: "openai".to_string(),
        };
        let value = codex_plugin_read_value(
            &identity,
            json!({"plugin":{
                "summary":{"installed":true,"enabled":true,"source":{"type":"local","path":package}},
                "skills":[{"name":"review","path":package.join("skills/review/SKILL.md")}],
                "hooks":[],
                "mcpServers":[{"name":"local-tools","command":"node"}],
                "apps":[{"id":"review-app"}],
                "appTemplates":[{"id":"review-template"}],
                "scheduledTasks":[{"id":"nightly"}],
                "defaultPrompt":"hidden prompt",
                "browserExtensions":[{"id":"review-browser"}],
                "futureComponent":{"enabled":true}
            }}),
        );

        assert_eq!(
            value["plugin"]["component_statuses"][0]["executionOwner"],
            "psychevo_native"
        );
        assert_eq!(
            value["plugin"]["component_statuses"][2]["highestLevel"],
            "delegate"
        );
        assert_eq!(
            value["plugin"]["component_statuses"][2]["executionOwner"],
            "codex_broker"
        );
        let statuses = value["plugin"]["component_statuses"]
            .as_array()
            .expect("component statuses");
        let status = |component: &str| {
            statuses
                .iter()
                .find(|status| status["component"] == component)
                .unwrap_or_else(|| panic!("missing {component} status"))
        };
        assert_eq!(status("mcp_servers")["executionOwner"], "psychevo_native");
        assert_eq!(status("app_templates")["readiness"], "metadata_only");
        assert_eq!(status("scheduled_tasks")["readiness"], "unsupported");
        assert_eq!(
            status("interface_prompts")["executionOwner"],
            "psychevo_gui"
        );
        assert_eq!(status("browser_extensions")["readiness"], "unsupported");
        assert_eq!(status("unknown_fields")["readiness"], "diagnostic_only");
    }

    #[tokio::test]
    async fn destructive_mutation_drain_waits_for_active_turn_lease() {
        let broker = Arc::new(CodexCapabilityBroker::with_command(
            BrokerCommand {
                program: PathBuf::from("unused-codex"),
                args: Vec::new(),
            },
            BTreeMap::new(),
            Duration::from_millis(50),
        ));
        broker
            .active_leases
            .lock()
            .await
            .insert("turn-lease".to_string(), 7);

        let draining_broker = broker.clone();
        let (release_drain_tx, release_drain_rx) = oneshot::channel();
        let mut drain = tokio::spawn(async move {
            let _draining = draining_broker.begin_draining().await;
            let _ = release_drain_rx.await;
        });
        tokio::time::timeout(Duration::from_secs(1), async {
            while !broker.draining.load(Ordering::Acquire) {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("draining state becomes observable");
        assert!(
            tokio::time::timeout(Duration::from_millis(25), &mut drain)
                .await
                .is_err(),
            "destructive mutation must remain pending while the turn lease is active"
        );

        broker.release_turn_lease("turn-lease").await;
        tokio::time::timeout(Duration::from_millis(25), &mut drain)
            .await
            .expect_err("draining mutation remains active after lease release");
        release_drain_tx.send(()).expect("release drain mutation");
        drain.await.expect("drain task");
        assert!(!broker.draining.load(Ordering::Acquire));
    }

    #[tokio::test]
    async fn destructive_mutations_hold_one_continuous_draining_owner() {
        let broker = Arc::new(CodexCapabilityBroker::with_command(
            BrokerCommand {
                program: PathBuf::from("unused-codex"),
                args: Vec::new(),
            },
            BTreeMap::new(),
            Duration::from_millis(50),
        ));
        let first = broker.begin_draining().await;
        let second_broker = broker.clone();
        let (second_entered_tx, mut second_entered_rx) = oneshot::channel();
        let (second_release_tx, second_release_rx) = oneshot::channel();
        let second = tokio::spawn(async move {
            let _draining = second_broker.begin_draining().await;
            second_entered_tx.send(()).expect("second mutation entered");
            let _ = second_release_rx.await;
        });

        assert!(
            tokio::time::timeout(Duration::from_millis(25), &mut second_entered_rx)
                .await
                .is_err(),
            "the second destructive mutation must wait for the first owner"
        );
        drop(first);
        tokio::time::timeout(Duration::from_secs(1), &mut second_entered_rx)
            .await
            .expect("second mutation enters")
            .expect("second entered signal");
        assert!(broker.draining.load(Ordering::Acquire));
        second_release_tx.send(()).expect("release second mutation");
        second.await.expect("second mutation task");
        assert!(!broker.draining.load(Ordering::Acquire));
    }

    #[test]
    fn trust_is_bound_to_package_fingerprint_and_reviewed_codex_version() {
        let temp = tempfile::tempdir().expect("temp");
        let home = temp.path().join("home");
        let package = temp.path().join("review");
        fs::create_dir_all(package.join(".codex-plugin")).expect("manifest dir");
        fs::write(package.join(".codex-plugin/plugin.json"), "{}").expect("manifest");
        fs::write(package.join("payload.txt"), "v1").expect("payload");
        let broker = CodexCapabilityBroker::new(&BTreeMap::from([
            ("PSYCHEVO_HOME".to_string(), home.display().to_string()),
            (
                "PATH".to_string(),
                std::env::var("PATH").unwrap_or_default(),
            ),
        ]));
        *broker
            .negotiated_version
            .write()
            .expect("Codex version lock") = Some(REVIEWED_CODEX_VERSION.to_string());
        let identity = CodexPluginIdentity {
            plugin: "review".to_string(),
            marketplace: "openai".to_string(),
        };
        let detail = json!({"plugin":{"summary":{
            "installed":true,
            "localVersion":"1.0.0",
            "source":{"type":"local","path":package.clone()}
        }}});

        broker
            .set_trust(&identity, &detail, true)
            .expect("trust current package");
        assert_eq!(
            broker.trust_value(&identity, &detail).expect("trust")["status"],
            "trusted"
        );

        fs::write(package.join("payload.txt"), "v2").expect("mutate payload");
        assert_eq!(
            broker.trust_value(&identity, &detail).expect("drift trust")["status"],
            "modified"
        );
        broker
            .set_trust(&identity, &detail, true)
            .expect("trust new fingerprint");
        *broker
            .negotiated_version
            .write()
            .expect("Codex version lock") = Some("0.144.2".to_string());
        assert_eq!(
            broker
                .trust_value(&identity, &detail)
                .expect("version drift")["status"],
            "modified"
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn install_materializes_then_allows_trusts_and_publishes_generation() {
        let temp = tempfile::tempdir().expect("temp");
        let profile_home = temp.path().join("psychevo");
        let private_home = profile_home.join("codex");
        let cwd = temp.path().join("work");
        let package = temp.path().join("review");
        let script = temp.path().join("fake-codex.py");
        fs::create_dir_all(&profile_home).expect("profile home");
        fs::create_dir_all(&cwd).expect("cwd");
        fs::create_dir_all(package.join(".codex-plugin")).expect("manifest dir");
        fs::write(package.join(".codex-plugin/plugin.json"), "{}").expect("manifest");
        fs::write(
            profile_home.join("config.toml"),
            format!(
                "[codex_plugins]\nenabled = true\nbinary = {:?}\n",
                script.display().to_string()
            ),
        )
        .expect("config");
        write_codex_app_server_fixture(
            &script,
            "install_materializes",
            json!({"package": package}),
        );
        let broker = CodexCapabilityBroker::new(&BTreeMap::from([
            (
                "HOME".to_string(),
                temp.path().join("user").display().to_string(),
            ),
            (
                "PSYCHEVO_HOME".to_string(),
                profile_home.display().to_string(),
            ),
            (
                "PATH".to_string(),
                std::env::var("PATH").unwrap_or_default(),
            ),
        ]));
        let identity = CodexPluginIdentity {
            plugin: "review".to_string(),
            marketplace: "openai".to_string(),
        };

        let result = broker
            .plugin_install(&cwd, &identity)
            .await
            .expect("install journey");

        assert_eq!(broker.authority_view()["auth"], "unavailable");
        assert_eq!(broker.authority_view()["runtime"], "ready");
        assert_eq!(result["success"], true);
        assert_eq!(
            result["completedSteps"],
            json!([
                "materialized",
                "detail_reread",
                "fingerprint",
                "profile_allow",
                "trust",
                "generation_published"
            ])
        );
        assert_eq!(result["policy"]["profileEnabled"], true);
        assert_eq!(result["trust"]["status"], "trusted");
        assert!(result["generation"].as_u64().unwrap_or_default() > 1);
        assert!(private_home.join("plugin-trust.json").is_file());
        let config = fs::read_to_string(profile_home.join("config.toml")).expect("profile policy");
        assert!(config.contains("codex:review@openai"));
        broker.stop().await;
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn app_connect_uses_install_url_and_observes_accessibility() {
        let temp = tempfile::tempdir().expect("temp");
        let script = temp.path().join("fake-codex.py");
        write_codex_app_server_fixture(&script, "app_connect", json!({}));
        let broker = CodexCapabilityBroker::with_command(
            BrokerCommand {
                program: script,
                args: Vec::new(),
            },
            BTreeMap::from([(
                "PATH".to_string(),
                std::env::var("PATH").unwrap_or_default(),
            )]),
            Duration::from_secs(1),
        );

        let started = broker
            .connect_start("codex:review@openai", "review-app", Some("app"))
            .await
            .expect("connect start");
        assert_eq!(started["status"], "pending");
        assert_eq!(
            started["installUrl"],
            "https://apps.example.test/install/review"
        );
        assert!(started["authorizationUrl"].is_null());
        let completed = broker
            .connect_status(started["sessionId"].as_str().expect("session id"))
            .await
            .expect("connect status");
        assert_eq!(completed["status"], "succeeded");
        assert_eq!(
            broker
                .connect_status("lost-after-restart")
                .await
                .expect("expired status")["status"],
            "expired"
        );
        broker.stop().await;
    }

    #[test]
    fn standard_elicitation_projection_preserves_optional_and_titled_choices() {
        let projection = project_elicitation(&json!({
            "mode":"form",
            "message":"Configure",
            "requestedSchema":{
                "type":"object",
                "properties":{
                    "mode":{"type":"string","oneOf":[
                        {"const":"safe","title":"Safe mode"},
                        {"const":"fast","title":"Fast mode"}
                    ]},
                    "tags":{"type":"array","items":{"anyOf":[
                        {"const":"a","title":"Alpha"},
                        {"const":"b","title":"Beta"}
                    ]}}
                },
                "required":["mode"]
            }
        }));

        assert_eq!(
            projection.raw["questions"][0]["options"][0]["label"],
            "safe"
        );
        assert_eq!(
            projection.raw["questions"][0]["options"][0]["description"],
            "Safe mode"
        );
        assert_eq!(projection.raw["questions"][0]["required"], true);
        assert_eq!(projection.raw["questions"][1]["multiple"], true);
        assert_eq!(projection.raw["questions"][1]["required"], false);

        let image_picker = project_elicitation(&json!({
            "mode":"openai/form",
            "message":"Choose a report",
            "requestedSchema":{
                "type":"object",
                "properties":{
                    "template":{
                        "type":"openai/imagePicker",
                        "items":[{
                            "id":"monthly-review",
                            "title":"Monthly review",
                            "image":"data:image/png;base64,AA=="
                        }]
                    }
                },
                "required":["template"]
            }
        }));
        assert_eq!(
            image_picker.raw["questions"][0]["options"][0],
            json!({
                "label":"monthly-review",
                "description":"Monthly review",
                "image":"data:image/png;base64,AA=="
            })
        );
        assert_eq!(image_picker.raw["questions"][0]["custom"], false);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn installed_package_projects_native_root_and_delegated_app_tool_with_elicitation() {
        let temp = tempfile::tempdir().expect("temp");
        let cwd = temp.path().join("work");
        let home = temp.path().join("home");
        let package = temp.path().join("review-plugin");
        fs::create_dir_all(&cwd).expect("cwd");
        fs::create_dir_all(package.join(".codex-plugin")).expect("manifest dir");
        fs::create_dir_all(package.join("skills/review")).expect("skill dir");
        fs::write(
            package.join(".codex-plugin/plugin.json"),
            r#"{"name":"review","skills":"./skills","apps":"./.app.json"}"#,
        )
        .expect("manifest");
        fs::write(package.join("skills/review/SKILL.md"), "# Review").expect("skill");
        fs::write(package.join(".app.json"), "{}").expect("app");
        let log = temp.path().join("broker.log");
        let script = temp.path().join("fake-codex.py");
        write_codex_app_server_fixture(
            &script,
            "installed_package",
            json!({"package": package, "log": log}),
        );
        fs::create_dir_all(&home).expect("profile home");
        fs::write(
            home.join("config.toml"),
            format!(
                "[codex_plugins]\nenabled = true\nbinary = {:?}\n",
                script.display().to_string()
            ),
        )
        .expect("Codex authority config");

        let env = BTreeMap::from([
            ("HOME".to_string(), temp.path().display().to_string()),
            ("PSYCHEVO_HOME".to_string(), home.display().to_string()),
            (
                "PSYCHEVO_CODEX_BIN".to_string(),
                script.display().to_string(),
            ),
            (
                "PATH".to_string(),
                std::env::var("PATH").unwrap_or_default(),
            ),
        ]);
        let runtime =
            psychevo_runtime::StateRuntime::open(temp.path().join("state.db")).expect("state");
        let gateway = crate::Gateway::new(runtime);
        let config = super::super::GatewayWebServerConfig::new(
            gateway,
            home,
            cwd.clone(),
            None,
            env,
            temp.path().join("static"),
        );
        let state = super::super::WebState::new(config);
        let events = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let events_for_sink = events.clone();
        let event_sink: crate::GatewayEventSink = std::sync::Arc::new(move |event| {
            events_for_sink.lock().expect("events").push(event);
        });

        let installed = state
            .inner
            .codex_capability_broker
            .plugin_install(
                &cwd,
                &CodexPluginIdentity {
                    plugin: "review".to_string(),
                    marketplace: "openai".to_string(),
                },
            )
            .await
            .expect("install and prewarm reviewed plugin");
        assert_eq!(installed["success"], true);
        state
            .inner
            .codex_capability_broker
            .prepare_runtime_inventory(&cwd)
            .await
            .expect("prewarm reviewed plugin inventory");
        fs::write(&log, "").expect("clear setup log");

        let contributions = tokio::time::timeout(
            Duration::from_millis(500),
            state.inner.codex_capability_broker.runtime_contributions(
                state.clone(),
                &cwd,
                "psychevo-thread-1",
                Some("turn-1".to_string()),
                Some(event_sink.clone()),
            ),
        )
        .await
        .expect("runtime assembly must not wait for delayed plugin/list")
        .expect("runtime contributions");
        assert_eq!(contributions.capability_roots.len(), 1);
        assert!(matches!(
            contributions.capability_roots[0].authority,
            psychevo_runtime::CapabilityRootAuthority::Codex { .. }
        ));
        assert_eq!(contributions.runtime_tools.len(), 1);
        assert_eq!(
            contributions.runtime_tools[0].name(),
            "mcp__codex_apps__review"
        );
        let second = state
            .inner
            .codex_capability_broker
            .runtime_contributions(
                state.clone(),
                &cwd,
                "psychevo-thread-1",
                Some("turn-2".to_string()),
                None,
            )
            .await
            .expect("frozen runtime contributions");
        assert_eq!(second.capability_roots, contributions.capability_roots);
        assert_eq!(second.runtime_tools.len(), 1);
        assert_eq!(fs::read_to_string(&log).expect("runtime inventory log"), "");

        let other_thread = state
            .inner
            .codex_capability_broker
            .runtime_contributions(
                state.clone(),
                &cwd,
                "psychevo-thread-2",
                Some("turn-3".to_string()),
                None,
            )
            .await
            .expect("shared runtime inventory");
        assert_eq!(
            other_thread.capability_roots,
            contributions.capability_roots
        );
        assert_eq!(other_thread.runtime_tools.len(), 1);
        assert_eq!(fs::read_to_string(&log).expect("shared inventory log"), "");

        let tool = CodexMcpTool {
            state: state.clone(),
            cwd: cwd.clone(),
            psychevo_thread_id: "psychevo-thread-1".to_string(),
            turn_id: Some("turn-1".to_string()),
            event_sink: Some(event_sink),
            name: "mcp__codex_apps__review".to_string(),
            server_name: "codex_apps".to_string(),
            remote_name: "review".to_string(),
            description: "Review app".to_string(),
            parameters: json!({"type":"object"}),
        };
        let (_abort_tx, abort_rx) = tokio::sync::watch::channel(false);
        let run =
            tokio::spawn(tool.execute("call-1".to_string(), json!({}), AbortSignal::new(abort_rx)));
        let action_id = tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                if let Some(action_id) =
                    events
                        .lock()
                        .expect("events")
                        .iter()
                        .find_map(|event| match event {
                            crate::GatewayEvent::ActionRequested { action } => {
                                Some(action.action_id.clone())
                            }
                            _ => None,
                        })
                {
                    break action_id;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("elicitation action");
        let response = respond_to_elicitation(
            &state,
            &action_id,
            psychevo_gateway_protocol::ThreadInteractionResponse::Clarify {
                answers: vec![vec!["Yes".to_string()]],
            },
        )
        .expect("elicitation response")
        .expect("pending elicitation");
        assert!(response.accepted);
        let output = run.await.expect("tool task");
        assert!(!output.is_error);
        assert_eq!(output.json["structuredContent"]["ok"], true);

        state
            .inner
            .codex_capability_broker
            .archive_ephemeral_thread("psychevo-thread-1")
            .await;
        state
            .inner
            .codex_capability_broker
            .archive_ephemeral_thread("psychevo-thread-2")
            .await;
        for lease_id in [
            contributions.lease_id.as_deref(),
            second.lease_id.as_deref(),
            other_thread.lease_id.as_deref(),
        ]
        .into_iter()
        .flatten()
        {
            state
                .inner
                .codex_capability_broker
                .release_turn_lease(lease_id)
                .await;
        }
        assert_eq!(
            fs::read_to_string(&log).expect("archive log"),
            "codex-thread-1\n"
        );
        state.inner.codex_capability_broker.stop().await;
    }

    #[tokio::test]
    #[ignore = "live Codex app-server opt-in"]
    async fn live_codex_plugin_broker_lists_current_catalog() {
        let env = std::env::vars().collect::<BTreeMap<_, _>>();
        let broker = CodexCapabilityBroker::new(&env);
        let cwd = std::env::current_dir().expect("current directory");

        let catalog = broker
            .plugin_list(&cwd)
            .await
            .expect("current Codex plugin catalog");
        assert!(
            catalog
                .get("marketplaces")
                .and_then(Value::as_array)
                .is_some(),
            "plugin/list must return the pinned marketplaces array: {catalog}"
        );
        let merged = merge_plugin_list(json!({"plugins":[],"count":0}), Ok(catalog));
        assert_eq!(merged["codex_authority"]["readiness"], "ready");
        assert!(merged["plugins"].is_array());
        broker.stop().await;
    }
}
