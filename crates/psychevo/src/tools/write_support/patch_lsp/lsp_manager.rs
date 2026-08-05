use std::collections::{BTreeMap, HashMap, HashSet};
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::mpsc::Receiver;
use std::sync::{Arc, LazyLock, Mutex};
use std::thread;
use std::time::Duration;

use serde_json::{Value, json};

use super::super::text_edit::LspBaseline;
use super::lsp_runtime::{
    emit_lsp_status, find_executable_path, format_lsp_diagnostics, lsp_diag_key, lsp_server_match,
    npm_bin_dir, npm_bin_name, resolve_lsp_server_with_env,
};
use crate::error::{Error, Result};
use crate::managed_tools::{is_executable_file, resolve_psychevo_home};
use crate::tools::cwd::CwdTool;
use crate::types::RunStreamEvent;

pub(crate) fn snapshot_lsp_baseline(
    tool: &CwdTool,
    path: &Path,
    pre_content: Option<&str>,
) -> Option<LspBaseline> {
    if !tool.lsp_config().enabled {
        return None;
    }
    let content = pre_content?;
    run_lsp_diagnostics(tool, path, content)
        .map(|diagnostics| LspBaseline { diagnostics })
        .ok()
}

pub(crate) fn lsp_diagnostics_after(
    tool: &CwdTool,
    path: &Path,
    _pre_content: Option<&str>,
    post_content: &str,
    baseline: Option<LspBaseline>,
) -> Option<String> {
    if !tool.lsp_config().enabled {
        return None;
    }
    let fresh_run = tool
        .context
        .lsp_manager
        .diagnostics(tool, path, post_content)
        .ok()?;
    let fresh = fresh_run.diagnostics;
    let baseline_keys = baseline
        .map(|baseline| {
            baseline
                .diagnostics
                .iter()
                .map(lsp_diag_key)
                .collect::<HashSet<_>>()
        })
        .or_else(|| {
            fresh_run
                .previous
                .map(|previous| previous.iter().map(lsp_diag_key).collect::<HashSet<_>>())
        })
        .unwrap_or_default();
    let introduced = fresh
        .into_iter()
        .filter(|diag| !baseline_keys.contains(&lsp_diag_key(diag)))
        .collect::<Vec<_>>();
    format_lsp_diagnostics(path, &introduced)
}

fn run_lsp_diagnostics(tool: &CwdTool, path: &Path, content: &str) -> Result<Vec<Value>> {
    tool.context
        .lsp_manager
        .diagnostics(tool, path, content)
        .map(|run| run.diagnostics)
}

#[derive(Clone)]
pub(super) struct LspServerCommand {
    pub(super) id: String,
    pub(super) program: String,
    pub(super) args: Vec<String>,
    pub(super) language_id: String,
    pub(super) env: BTreeMap<String, String>,
    pub(super) env_path: Option<OsString>,
}

#[derive(Clone)]
pub(super) struct LspDiagnosticRun {
    pub(super) diagnostics: Vec<Value>,
    pub(super) previous: Option<Vec<Value>>,
}

#[derive(Clone)]
pub(super) struct LspInstallRequest {
    pub(super) server_id: String,
    pub(super) package: String,
    pub(super) install_dir: PathBuf,
    pub(super) bin_path: PathBuf,
    pub(super) env: BTreeMap<String, String>,
    pub(super) path_prefixes: Vec<PathBuf>,
}

pub(super) type LspInstaller = Arc<dyn Fn(LspInstallRequest) -> Result<()> + Send + Sync>;

pub(crate) struct LspManager {
    state: Mutex<LspManagerState>,
    installer: LspInstaller,
}

#[derive(Default)]
struct LspManagerState {
    clients: HashMap<LspClientKey, Arc<LspClient>>,
    broken: HashSet<LspClientKey>,
    installing: HashSet<String>,
    failed_installs: HashSet<String>,
}

#[derive(Clone, Debug, Hash, PartialEq, Eq)]
struct LspClientKey {
    server_id: String,
    workspace_root: PathBuf,
}

#[derive(Clone, Copy)]
pub(super) struct LspServerDefinition {
    pub(super) id: &'static str,
    pub(super) binary: &'static str,
    pub(super) args: &'static [&'static str],
    pub(super) npm_package: Option<&'static str>,
}

#[derive(Clone)]
pub(super) struct LspServerMatch {
    pub(super) definition: LspServerDefinition,
    pub(super) language_id: &'static str,
}

pub(super) enum LspServerResolution {
    Ready(LspServerCommand),
    MissingInstallable(LspServerMatch),
    Missing,
    Skipped,
}

static DEFAULT_LSP_MANAGER: LazyLock<Arc<LspManager>> =
    LazyLock::new(|| Arc::new(LspManager::new(default_lsp_installer())));

pub(crate) fn default_lsp_manager() -> Arc<LspManager> {
    Arc::clone(&DEFAULT_LSP_MANAGER)
}

impl LspManager {
    pub(super) fn new(installer: LspInstaller) -> Self {
        Self {
            state: Mutex::new(LspManagerState::default()),
            installer,
        }
    }

    pub(super) fn diagnostics(
        self: &Arc<Self>,
        tool: &CwdTool,
        path: &Path,
        content: &str,
    ) -> Result<LspDiagnosticRun> {
        if !tool.lsp_config().enabled {
            return Ok(LspDiagnosticRun {
                diagnostics: Vec::new(),
                previous: None,
            });
        }
        let timeout = Duration::from_secs_f64(tool.lsp_config().wait_timeout_secs.max(0.1))
            + Duration::from_secs(2);
        let resolution = resolve_lsp_server_with_env(
            path,
            tool.lsp_config(),
            &tool.context.env,
            &tool.context.path_prefixes,
        );
        let server = match resolution {
            LspServerResolution::Ready(server) => server,
            LspServerResolution::MissingInstallable(server_match) => {
                self.schedule_install(tool, &server_match);
                return Ok(LspDiagnosticRun {
                    diagnostics: Vec::new(),
                    previous: None,
                });
            }
            LspServerResolution::Missing => {
                emit_lsp_status(
                    tool,
                    "skipped",
                    lsp_server_match(path).map(|server_match| server_match.definition.id),
                    Some(path),
                    Some("language server unavailable".to_string()),
                );
                return Ok(LspDiagnosticRun {
                    diagnostics: Vec::new(),
                    previous: None,
                });
            }
            LspServerResolution::Skipped => {
                return Ok(LspDiagnosticRun {
                    diagnostics: Vec::new(),
                    previous: None,
                });
            }
        };
        let key = LspClientKey {
            server_id: server.id.clone(),
            workspace_root: tool.cwd().to_path_buf(),
        };
        if self
            .state
            .lock()
            .map_err(|_| Error::Message("LSP state lock poisoned".to_string()))?
            .broken
            .contains(&key)
        {
            emit_lsp_status(
                tool,
                "skipped",
                Some(&server.id),
                Some(path),
                Some("language server marked broken for this workspace".to_string()),
            );
            return Ok(LspDiagnosticRun {
                diagnostics: Vec::new(),
                previous: None,
            });
        }
        let client = match self.client_for(tool, server.clone(), &key, timeout) {
            Ok(client) => client,
            Err(err) => {
                self.mark_broken(&key);
                emit_lsp_status(
                    tool,
                    "failed",
                    Some(&server.id),
                    Some(path),
                    Some(err.to_string()),
                );
                return Err(err);
            }
        };
        match client.diagnostics(path, content, timeout) {
            Ok(run) => Ok(run),
            Err(err) => {
                self.mark_broken(&key);
                self.remove_client(&key);
                emit_lsp_status(
                    tool,
                    "failed",
                    Some(&server.id),
                    Some(path),
                    Some(err.to_string()),
                );
                Err(err)
            }
        }
    }

    fn client_for(
        self: &Arc<Self>,
        tool: &CwdTool,
        server: LspServerCommand,
        key: &LspClientKey,
        timeout: Duration,
    ) -> Result<Arc<LspClient>> {
        if let Some(client) = self
            .state
            .lock()
            .map_err(|_| Error::Message("LSP state lock poisoned".to_string()))?
            .clients
            .get(key)
            .cloned()
        {
            return Ok(client);
        }
        let client = Arc::new(LspClient::start(server, tool.cwd().to_path_buf(), timeout)?);
        self.state
            .lock()
            .map_err(|_| Error::Message("LSP state lock poisoned".to_string()))?
            .clients
            .insert(key.clone(), Arc::clone(&client));
        emit_lsp_status(tool, "started", Some(&key.server_id), None, None);
        Ok(client)
    }

    fn mark_broken(&self, key: &LspClientKey) {
        if let Ok(mut state) = self.state.lock() {
            state.broken.insert(key.clone());
        }
    }

    fn remove_client(&self, key: &LspClientKey) {
        if let Ok(mut state) = self.state.lock()
            && let Some(client) = state.clients.remove(key)
        {
            client.shutdown();
        }
    }

    fn schedule_install(self: &Arc<Self>, tool: &CwdTool, server_match: &LspServerMatch) {
        let Some(package) = server_match.definition.npm_package else {
            return;
        };
        let install_key = package.to_string();
        let Ok(home) = resolve_psychevo_home(&tool.context.env) else {
            emit_lsp_status(
                tool,
                "install_failed",
                Some(server_match.definition.id),
                None,
                Some("could not resolve PSYCHEVO_HOME".to_string()),
            );
            return;
        };
        {
            let mut state = match self.state.lock() {
                Ok(state) => state,
                Err(_) => return,
            };
            if state.failed_installs.contains(&install_key) {
                emit_lsp_status(
                    tool,
                    "install_failed",
                    Some(server_match.definition.id),
                    None,
                    Some("managed install previously failed in this runtime".to_string()),
                );
                return;
            }
            if !state.installing.insert(install_key.clone()) {
                emit_lsp_status(
                    tool,
                    "installing",
                    Some(server_match.definition.id),
                    None,
                    Some("managed install already in progress".to_string()),
                );
                return;
            }
        }
        let install_dir = home.join("lsp").join("node");
        let bin_path = npm_bin_dir(&install_dir).join(npm_bin_name(server_match.definition.binary));
        let request = LspInstallRequest {
            server_id: server_match.definition.id.to_string(),
            package: package.to_string(),
            install_dir,
            bin_path,
            env: tool.context.env.clone(),
            path_prefixes: tool.context.path_prefixes.clone(),
        };
        let installer = Arc::clone(&self.installer);
        let manager = Arc::clone(self);
        let stream = tool.context.stream_events.clone();
        emit_lsp_status(
            tool,
            "install_started",
            Some(server_match.definition.id),
            None,
            Some(format!("installing npm package {package}")),
        );
        thread::spawn(move || {
            let server_id = request.server_id.clone();
            let package = request.package.clone();
            let result = installer(request);
            if let Ok(mut state) = manager.state.lock() {
                state.installing.remove(&package);
                if result.is_err() {
                    state.failed_installs.insert(package.clone());
                }
            }
            if let Some(stream) = stream {
                match result {
                    Ok(()) => stream(RunStreamEvent::value(json!({
                        "type": "lsp_status",
                        "status": "install_finished",
                        "server_id": server_id,
                    }))),
                    Err(err) => stream(RunStreamEvent::value(json!({
                        "type": "lsp_status",
                        "status": "install_failed",
                        "server_id": server_id,
                        "message": err.to_string(),
                    }))),
                }
            }
        });
    }
}

fn default_lsp_installer() -> LspInstaller {
    Arc::new(|request| {
        fs::create_dir_all(&request.install_dir)?;
        let npm = find_executable_path("npm", &request.env, &request.path_prefixes)
            .unwrap_or_else(|| PathBuf::from("npm"));
        let mut command = std::process::Command::new(npm);
        command
            .arg("install")
            .arg("--prefix")
            .arg(&request.install_dir)
            .arg(&request.package)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        crate::process_env::apply_process_env(
            &mut command,
            &request.env,
            crate::process_env::ProcessEnvOptions::new(&request.path_prefixes),
        )?;
        let status = command
            .status()
            .map_err(|err| Error::Message(format!("failed to start npm install: {err}")))?;
        if !status.success() {
            return Err(Error::Message(format!(
                "npm install failed for {} with status {status}",
                request.package
            )));
        }
        if !is_executable_file(&request.bin_path) {
            return Err(Error::Message(format!(
                "managed LSP install did not create {}",
                request.bin_path.display()
            )));
        }
        Ok(())
    })
}

pub(super) struct LspClient {
    pub(super) command: LspServerCommand,
    pub(super) cwd: PathBuf,
    pub(super) child: Mutex<Option<std::process::Child>>,
    pub(super) stdin: Mutex<Option<std::process::ChildStdin>>,
    pub(super) rx: Mutex<Receiver<Value>>,
    pub(super) io_lock: Mutex<()>,
    pub(super) next_id: std::sync::atomic::AtomicI64,
    pub(super) versions: Mutex<HashMap<PathBuf, i64>>,
    pub(super) last: Mutex<HashMap<PathBuf, LspFileState>>,
}

#[derive(Clone)]
pub(super) struct LspFileState {
    pub(super) content_hash: u64,
    pub(super) diagnostics: Vec<Value>,
}
