#[allow(unused_imports)]
pub(crate) use super::*;

#[allow(unused_imports)]
use serde_json::json;

use std::collections::hash_map::DefaultHasher;
use std::ffi::OsString;
use std::hash::{Hash, Hasher};
use std::sync::mpsc::{Receiver, RecvTimeoutError};

use crate::managed_tools::{find_on_path, is_executable_file, resolve_psychevo_home};

pub(crate) fn parse_v4a_patch(patch: &str) -> Result<Vec<V4aOperation>> {
    let lines = patch.lines().collect::<Vec<_>>();
    let start = lines
        .iter()
        .position(|line| line.trim() == "*** Begin Patch")
        .map(|idx| idx + 1)
        .unwrap_or(0);
    let end = lines
        .iter()
        .position(|line| line.trim() == "*** End Patch")
        .unwrap_or(lines.len());
    let mut operations = Vec::new();
    let mut current: Option<V4aOperation> = None;
    let mut current_hunk: Option<V4aHunk> = None;
    for line in &lines[start..end] {
        if let Some(path) = marker_value(line, "*** Update File:") {
            push_v4a_current(&mut operations, &mut current, &mut current_hunk);
            current = Some(V4aOperation {
                kind: V4aOperationKind::Update,
                file_path: path,
                hunks: Vec::new(),
            });
        } else if let Some(path) = marker_value(line, "*** Add File:") {
            push_v4a_current(&mut operations, &mut current, &mut current_hunk);
            current = Some(V4aOperation {
                kind: V4aOperationKind::Add,
                file_path: path,
                hunks: Vec::new(),
            });
            current_hunk = Some(V4aHunk {
                context_hint: None,
                lines: Vec::new(),
            });
        } else if let Some(path) = marker_value(line, "*** Delete File:") {
            push_v4a_current(&mut operations, &mut current, &mut current_hunk);
            operations.push(V4aOperation {
                kind: V4aOperationKind::Delete,
                file_path: path,
                hunks: Vec::new(),
            });
        } else if marker_value(line, "*** Move File:").is_some()
            || marker_value(line, "*** Move to:").is_some()
        {
            return Err(Error::Message(
                "patch moves are not supported; use Add/Delete or an explicit shell operation"
                    .to_string(),
            ));
        } else if line.starts_with("@@") {
            if let Some(op) = current.as_mut()
                && let Some(hunk) = current_hunk.take()
                && !hunk.lines.is_empty()
            {
                op.hunks.push(hunk);
            }
            current_hunk = Some(V4aHunk {
                context_hint: parse_context_hint(line),
                lines: Vec::new(),
            });
        } else if let Some(op) = current.as_mut() {
            let hunk = current_hunk.get_or_insert_with(|| V4aHunk {
                context_hint: None,
                lines: Vec::new(),
            });
            if let Some(content) = line.strip_prefix('+') {
                hunk.lines.push(V4aLine {
                    prefix: '+',
                    content: content.to_string(),
                });
            } else if let Some(content) = line.strip_prefix('-') {
                hunk.lines.push(V4aLine {
                    prefix: '-',
                    content: content.to_string(),
                });
            } else if let Some(content) = line.strip_prefix(' ') {
                hunk.lines.push(V4aLine {
                    prefix: ' ',
                    content: content.to_string(),
                });
            } else if line.starts_with('\\') {
                continue;
            } else if !line.is_empty() || op.kind == V4aOperationKind::Add {
                hunk.lines.push(V4aLine {
                    prefix: ' ',
                    content: (*line).to_string(),
                });
            }
        }
    }
    push_v4a_current(&mut operations, &mut current, &mut current_hunk);
    if operations.is_empty() {
        return Err(Error::Message("patch contains no operations".to_string()));
    }
    for op in &operations {
        if op.file_path.trim().is_empty() {
            return Err(Error::Message("patch operation has empty path".to_string()));
        }
        if op.kind == V4aOperationKind::Update && op.hunks.is_empty() {
            return Err(Error::Message(format!(
                "update operation has no hunks: {}",
                op.file_path
            )));
        }
    }
    Ok(operations)
}

pub(crate) fn marker_value(line: &str, marker: &str) -> Option<String> {
    line.trim()
        .strip_prefix(marker)
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

pub(crate) fn push_v4a_current(
    operations: &mut Vec<V4aOperation>,
    current: &mut Option<V4aOperation>,
    current_hunk: &mut Option<V4aHunk>,
) {
    if let Some(mut op) = current.take() {
        if let Some(hunk) = current_hunk.take()
            && !hunk.lines.is_empty()
        {
            op.hunks.push(hunk);
        }
        operations.push(op);
    }
}

pub(crate) fn parse_context_hint(line: &str) -> Option<String> {
    let trimmed = line.trim();
    let inner = trimmed.strip_prefix("@@")?.strip_suffix("@@")?.trim();
    (!inner.is_empty()).then(|| inner.to_string())
}

pub(crate) fn apply_v4a_update_hunks(
    content: &str,
    hunks: &[V4aHunk],
) -> std::result::Result<String, String> {
    let mut updated = content.to_string();
    for hunk in hunks {
        let search_lines = hunk
            .lines
            .iter()
            .filter(|line| line.prefix == ' ' || line.prefix == '-')
            .map(|line| line.content.as_str())
            .collect::<Vec<_>>();
        let replacement_lines = hunk
            .lines
            .iter()
            .filter(|line| line.prefix == ' ' || line.prefix == '+')
            .map(|line| line.content.as_str())
            .collect::<Vec<_>>();
        if search_lines.is_empty() {
            let insert_text = replacement_lines.join("\n");
            updated =
                apply_addition_only_hunk(&updated, hunk.context_hint.as_deref(), &insert_text)?;
            continue;
        }
        let search = search_lines.join("\n");
        let replacement = replacement_lines.join("\n");
        match fuzzy_find_and_replace(&updated, &search, &replacement, false) {
            Ok(outcome) => updated = outcome.content,
            Err(err) => {
                return Err(format!(
                    "hunk {} not found: {err}",
                    hunk.context_hint
                        .as_ref()
                        .map(|hint| format!("{hint:?}"))
                        .unwrap_or_else(|| "(no hint)".to_string())
                ));
            }
        }
    }
    Ok(updated)
}

pub(crate) fn apply_addition_only_hunk(
    content: &str,
    context_hint: Option<&str>,
    insert_text: &str,
) -> std::result::Result<String, String> {
    if insert_text.is_empty() {
        return Ok(content.to_string());
    }
    let Some(hint) = context_hint else {
        return Ok(format!(
            "{}\n{}\n",
            content.trim_end_matches('\n'),
            insert_text
        ));
    };
    let matches = strategy_exact(content, hint);
    if matches.is_empty() {
        return Err(format!(
            "addition-only hunk context hint {hint:?} not found"
        ));
    }
    if matches.len() > 1 {
        return Err(format!(
            "addition-only hunk context hint {hint:?} is ambiguous ({} occurrences)",
            matches.len()
        ));
    }
    let insert_at = content[matches[0].end..]
        .find('\n')
        .map(|idx| matches[0].end + idx + 1)
        .unwrap_or(content.len());
    let mut out = String::new();
    out.push_str(&content[..insert_at]);
    out.push_str(insert_text);
    out.push('\n');
    out.push_str(&content[insert_at..]);
    Ok(out)
}

pub(crate) fn v4a_add_content(op: &V4aOperation) -> String {
    op.hunks
        .iter()
        .flat_map(|hunk| &hunk.lines)
        .filter(|line| line.prefix == '+')
        .map(|line| line.content.as_str())
        .collect::<Vec<_>>()
        .join("\n")
}

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

pub(crate) fn run_lsp_diagnostics(
    tool: &CwdTool,
    path: &Path,
    content: &str,
) -> Result<Vec<Value>> {
    tool.context
        .lsp_manager
        .diagnostics(tool, path, content)
        .map(|run| run.diagnostics)
}

#[derive(Clone)]
pub(crate) struct LspServerCommand {
    pub(crate) id: String,
    pub(crate) program: String,
    pub(crate) args: Vec<String>,
    pub(crate) language_id: String,
    pub(crate) env: BTreeMap<String, String>,
    pub(crate) env_path: Option<OsString>,
}

#[derive(Clone)]
pub(crate) struct LspDiagnosticRun {
    pub(crate) diagnostics: Vec<Value>,
    pub(crate) previous: Option<Vec<Value>>,
}

#[derive(Clone)]
pub(crate) struct LspInstallRequest {
    pub(crate) server_id: String,
    pub(crate) package: String,
    pub(crate) install_dir: PathBuf,
    pub(crate) bin_path: PathBuf,
    pub(crate) env: BTreeMap<String, String>,
    pub(crate) path_prefixes: Vec<PathBuf>,
}

type LspInstaller = Arc<dyn Fn(LspInstallRequest) -> Result<()> + Send + Sync>;

pub(crate) struct LspManager {
    state: Mutex<LspManagerState>,
    installer: LspInstaller,
}

#[derive(Default)]
pub(crate) struct LspManagerState {
    clients: HashMap<LspClientKey, Arc<LspClient>>,
    broken: HashSet<LspClientKey>,
    installing: HashSet<String>,
    failed_installs: HashSet<String>,
}

#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub(crate) struct LspClientKey {
    server_id: String,
    workspace_root: PathBuf,
}

#[derive(Clone, Copy)]
pub(crate) struct LspServerDefinition {
    id: &'static str,
    binary: &'static str,
    args: &'static [&'static str],
    npm_package: Option<&'static str>,
}

#[derive(Clone)]
pub(crate) struct LspServerMatch {
    definition: LspServerDefinition,
    language_id: &'static str,
}

pub(crate) enum LspServerResolution {
    Ready(LspServerCommand),
    MissingInstallable(LspServerMatch),
    Missing,
    Skipped,
}

pub(crate) static DEFAULT_LSP_MANAGER: LazyLock<Arc<LspManager>> =
    LazyLock::new(|| Arc::new(LspManager::new(default_lsp_installer())));

pub(crate) fn default_lsp_manager() -> Arc<LspManager> {
    Arc::clone(&DEFAULT_LSP_MANAGER)
}

impl LspManager {
    pub(crate) fn new(installer: LspInstaller) -> Self {
        Self {
            state: Mutex::new(LspManagerState::default()),
            installer,
        }
    }

    pub(crate) fn diagnostics(
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

    pub(crate) fn client_for(
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
        let client = Arc::new(LspClient::start(
            server,
            tool.cwd().to_path_buf(),
            timeout,
        )?);
        self.state
            .lock()
            .map_err(|_| Error::Message("LSP state lock poisoned".to_string()))?
            .clients
            .insert(key.clone(), Arc::clone(&client));
        emit_lsp_status(tool, "started", Some(&key.server_id), None, None);
        Ok(client)
    }

    pub(crate) fn mark_broken(&self, key: &LspClientKey) {
        if let Ok(mut state) = self.state.lock() {
            state.broken.insert(key.clone());
        }
    }

    pub(crate) fn remove_client(&self, key: &LspClientKey) {
        if let Ok(mut state) = self.state.lock()
            && let Some(client) = state.clients.remove(key)
        {
            client.shutdown();
        }
    }

    pub(crate) fn schedule_install(
        self: &Arc<Self>,
        tool: &CwdTool,
        server_match: &LspServerMatch,
    ) {
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

pub(crate) fn default_lsp_installer() -> LspInstaller {
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

pub(crate) struct LspClient {
    command: LspServerCommand,
    cwd: PathBuf,
    child: Mutex<Option<std::process::Child>>,
    stdin: Mutex<Option<std::process::ChildStdin>>,
    rx: Mutex<Receiver<Value>>,
    io_lock: Mutex<()>,
    next_id: std::sync::atomic::AtomicI64,
    versions: Mutex<HashMap<PathBuf, i64>>,
    last: Mutex<HashMap<PathBuf, LspFileState>>,
}

#[derive(Clone)]
pub(crate) struct LspFileState {
    content_hash: u64,
    diagnostics: Vec<Value>,
}
