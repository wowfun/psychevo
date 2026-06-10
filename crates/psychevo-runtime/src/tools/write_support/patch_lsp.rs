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
                new_path: None,
                hunks: Vec::new(),
            });
        } else if let Some(path) = marker_value(line, "*** Add File:") {
            push_v4a_current(&mut operations, &mut current, &mut current_hunk);
            current = Some(V4aOperation {
                kind: V4aOperationKind::Add,
                file_path: path,
                new_path: None,
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
                new_path: None,
                hunks: Vec::new(),
            });
        } else if let Some(rest) = marker_value(line, "*** Move File:") {
            push_v4a_current(&mut operations, &mut current, &mut current_hunk);
            let Some((src, dst)) = rest.split_once("->") else {
                return Err(Error::Message(format!(
                    "invalid move marker: expected '*** Move File: old -> new', got {line}"
                )));
            };
            operations.push(V4aOperation {
                kind: V4aOperationKind::Move,
                file_path: src.trim().to_string(),
                new_path: Some(dst.trim().to_string()),
                hunks: Vec::new(),
            });
        } else if let Some(path) = marker_value(line, "*** Move to:") {
            let Some(op) = current.as_mut() else {
                return Err(Error::Message(
                    "*** Move to without current file".to_string(),
                ));
            };
            op.new_path = Some(path);
            op.kind = V4aOperationKind::Move;
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
        if op.kind == V4aOperationKind::Move
            && op.new_path.as_deref().unwrap_or_default().trim().is_empty()
        {
            return Err(Error::Message(format!(
                "move operation missing destination: {}",
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
    tool: &WorkdirTool,
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
    tool: &WorkdirTool,
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
    tool: &WorkdirTool,
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
        tool: &WorkdirTool,
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
            workspace_root: tool.workdir().to_path_buf(),
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
        tool: &WorkdirTool,
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
            tool.workdir().to_path_buf(),
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
        tool: &WorkdirTool,
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
                    Ok(()) => stream(RunStreamEvent::Event(json!({
                        "type": "lsp_status",
                        "status": "install_finished",
                        "server_id": server_id,
                    }))),
                    Err(err) => stream(RunStreamEvent::Event(json!({
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
        if let Some(path) = combined_path(&request.env, &request.path_prefixes) {
            command.env("PATH", path);
        }
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
    workdir: PathBuf,
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

impl LspClient {
    pub(crate) fn start(
        command: LspServerCommand,
        workdir: PathBuf,
        timeout: Duration,
    ) -> Result<Self> {
        let mut process = std::process::Command::new(&command.program);
        process
            .args(&command.args)
            .current_dir(&workdir)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        if let Some(path) = &command.env_path {
            process.env("PATH", path);
        }
        let mut child = process
            .spawn()
            .map_err(|err| Error::Message(format!("LSP spawn failed: {err}")))?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| Error::Message("LSP stdin unavailable".to_string()))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| Error::Message("LSP stdout unavailable".to_string()))?;
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let mut reader = std::io::BufReader::new(stdout);
            while let Ok(Some(message)) = read_lsp_message(&mut reader) {
                if tx.send(message).is_err() {
                    break;
                }
            }
        });
        let client = Self {
            command,
            workdir,
            child: Mutex::new(Some(child)),
            stdin: Mutex::new(Some(stdin)),
            rx: Mutex::new(rx),
            io_lock: Mutex::new(()),
            next_id: std::sync::atomic::AtomicI64::new(1),
            versions: Mutex::new(HashMap::new()),
            last: Mutex::new(HashMap::new()),
        };
        client.initialize(timeout)?;
        Ok(client)
    }

    pub(crate) fn initialize(&self, timeout: Duration) -> Result<()> {
        let _io = self
            .io_lock
            .lock()
            .map_err(|_| Error::Message("LSP I/O lock poisoned".to_string()))?;
        let id = self.next_request_id();
        {
            let mut stdin = self
                .stdin
                .lock()
                .map_err(|_| Error::Message("LSP stdin lock poisoned".to_string()))?;
            let stdin = stdin
                .as_mut()
                .ok_or_else(|| Error::Message("LSP stdin unavailable".to_string()))?;
            send_lsp(
                stdin,
                json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "method": "initialize",
                    "params": {
                        "processId": null,
                        "rootUri": file_uri(&self.workdir),
                        "capabilities": {
                            "textDocument": {
                                "publishDiagnostics": { "relatedInformation": false },
                                "diagnostic": { "dynamicRegistration": true }
                            }
                        },
                        "workspaceFolders": [{ "uri": file_uri(&self.workdir), "name": "workspace" }],
                        "clientInfo": { "name": "psychevo", "version": "0" }
                    }
                }),
            )?;
        }
        let rx = self
            .rx
            .lock()
            .map_err(|_| Error::Message("LSP receiver lock poisoned".to_string()))?;
        wait_for_lsp_response(&rx, id, timeout)?;
        drop(rx);
        let mut stdin = self
            .stdin
            .lock()
            .map_err(|_| Error::Message("LSP stdin lock poisoned".to_string()))?;
        let stdin = stdin
            .as_mut()
            .ok_or_else(|| Error::Message("LSP stdin unavailable".to_string()))?;
        send_lsp(
            stdin,
            json!({ "jsonrpc": "2.0", "method": "initialized", "params": {} }),
        )?;
        Ok(())
    }

    pub(crate) fn diagnostics(
        &self,
        path: &Path,
        content: &str,
        timeout: Duration,
    ) -> Result<LspDiagnosticRun> {
        let content_hash = hash_content(content);
        let previous = self
            .last
            .lock()
            .map_err(|_| Error::Message("LSP state lock poisoned".to_string()))?
            .get(path)
            .cloned();
        if previous
            .as_ref()
            .is_some_and(|state| state.content_hash == content_hash)
        {
            return Ok(LspDiagnosticRun {
                diagnostics: previous
                    .as_ref()
                    .map(|state| state.diagnostics.clone())
                    .unwrap_or_default(),
                previous: previous.map(|state| state.diagnostics),
            });
        }
        let _io = self
            .io_lock
            .lock()
            .map_err(|_| Error::Message("LSP I/O lock poisoned".to_string()))?;
        let uri = file_uri(path);
        let version = {
            let mut versions = self
                .versions
                .lock()
                .map_err(|_| Error::Message("LSP version lock poisoned".to_string()))?;
            let version = versions.entry(path.to_path_buf()).or_insert(0);
            *version += 1;
            *version
        };
        {
            let mut stdin = self
                .stdin
                .lock()
                .map_err(|_| Error::Message("LSP stdin lock poisoned".to_string()))?;
            let stdin = stdin
                .as_mut()
                .ok_or_else(|| Error::Message("LSP stdin unavailable".to_string()))?;
            if version == 1 {
                send_lsp(
                    stdin,
                    json!({
                        "jsonrpc": "2.0",
                        "method": "textDocument/didOpen",
                        "params": {
                            "textDocument": {
                                "uri": uri,
                                "languageId": self.command.language_id,
                                "version": version,
                                "text": content
                            }
                        }
                    }),
                )?;
            } else {
                send_lsp(
                    stdin,
                    json!({
                        "jsonrpc": "2.0",
                        "method": "textDocument/didChange",
                        "params": {
                            "textDocument": { "uri": uri, "version": version },
                            "contentChanges": [{ "text": content }]
                        }
                    }),
                )?;
            }
        }
        let diagnostics = {
            let rx = self
                .rx
                .lock()
                .map_err(|_| Error::Message("LSP receiver lock poisoned".to_string()))?;
            wait_for_lsp_diagnostics(&rx, &uri, timeout)?
        };
        self.last
            .lock()
            .map_err(|_| Error::Message("LSP state lock poisoned".to_string()))?
            .insert(
                path.to_path_buf(),
                LspFileState {
                    content_hash,
                    diagnostics: diagnostics.clone(),
                },
            );
        Ok(LspDiagnosticRun {
            diagnostics,
            previous: previous.map(|state| state.diagnostics),
        })
    }

    pub(crate) fn next_request_id(&self) -> i64 {
        self.next_id
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst)
    }

    pub(crate) fn shutdown(&self) {
        if let Ok(mut stdin) = self.stdin.lock()
            && let Some(stdin) = stdin.as_mut()
        {
            let id = self.next_request_id();
            let _ = send_lsp(
                stdin,
                json!({ "jsonrpc": "2.0", "id": id, "method": "shutdown" }),
            );
            let _ = send_lsp(stdin, json!({ "jsonrpc": "2.0", "method": "exit" }));
        }
        if let Ok(mut child) = self.child.lock()
            && let Some(mut child) = child.take()
        {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

impl Drop for LspClient {
    fn drop(&mut self) {
        self.shutdown();
    }
}

pub(crate) fn wait_for_lsp_response(
    rx: &Receiver<Value>,
    id: i64,
    timeout: Duration,
) -> Result<Value> {
    let start = std::time::Instant::now();
    while start.elapsed() < timeout {
        let remaining = timeout.saturating_sub(start.elapsed());
        let message = match rx.recv_timeout(remaining.min(Duration::from_millis(200))) {
            Ok(message) => message,
            Err(RecvTimeoutError::Timeout) => continue,
            Err(RecvTimeoutError::Disconnected) => {
                return Err(Error::Message("LSP connection closed".to_string()));
            }
        };
        if message.get("id").and_then(Value::as_i64) == Some(id) {
            return Ok(message);
        }
    }
    Err(Error::Message("LSP initialize timed out".to_string()))
}

pub(crate) fn wait_for_lsp_diagnostics(
    rx: &Receiver<Value>,
    uri: &str,
    timeout: Duration,
) -> Result<Vec<Value>> {
    let start = std::time::Instant::now();
    while start.elapsed() < timeout {
        let remaining = timeout.saturating_sub(start.elapsed());
        let message = match rx.recv_timeout(remaining.min(Duration::from_millis(200))) {
            Ok(message) => message,
            Err(RecvTimeoutError::Timeout) => continue,
            Err(RecvTimeoutError::Disconnected) => {
                return Err(Error::Message("LSP connection closed".to_string()));
            }
        };
        if message.get("method").and_then(Value::as_str) == Some("textDocument/publishDiagnostics")
            && message
                .pointer("/params/uri")
                .and_then(Value::as_str)
                .is_some_and(|value| value == uri)
        {
            return Ok(message
                .pointer("/params/diagnostics")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default());
        }
    }
    Err(Error::Message("LSP diagnostics timed out".to_string()))
}

pub(crate) fn resolve_lsp_server_with_env(
    path: &Path,
    config: &LspConfig,
    env_map: &BTreeMap<String, String>,
    path_prefixes: &[PathBuf],
) -> LspServerResolution {
    if !config.enabled || config.install_strategy == "off" {
        return LspServerResolution::Skipped;
    }
    let Some(server_match) = lsp_server_match(path) else {
        return LspServerResolution::Skipped;
    };
    if let Some(program) = managed_lsp_binary(&server_match.definition, env_map)
        .filter(|path| is_executable_file(path))
        .or_else(|| find_executable_path(server_match.definition.binary, env_map, path_prefixes))
    {
        return LspServerResolution::Ready(LspServerCommand {
            id: server_match.definition.id.to_string(),
            program: program.display().to_string(),
            args: server_match
                .definition
                .args
                .iter()
                .map(|arg| arg.to_string())
                .collect(),
            language_id: server_match.language_id.to_string(),
            env_path: combined_path(env_map, path_prefixes),
        });
    }
    if config.install_strategy == "auto" && server_match.definition.npm_package.is_some() {
        return LspServerResolution::MissingInstallable(server_match);
    }
    LspServerResolution::Missing
}

pub(crate) fn lsp_server_match(path: &Path) -> Option<LspServerMatch> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    match ext.as_str() {
        "rs" => Some(LspServerMatch {
            definition: LspServerDefinition {
                id: "rust-analyzer",
                binary: "rust-analyzer",
                args: &[],
                npm_package: None,
            },
            language_id: "rust",
        }),
        "py" => Some(LspServerMatch {
            definition: LspServerDefinition {
                id: "pyright",
                binary: "pyright-langserver",
                args: &["--stdio"],
                npm_package: Some("pyright"),
            },
            language_id: "python",
        }),
        "js" | "mjs" | "cjs" => Some(LspServerMatch {
            definition: LspServerDefinition {
                id: "typescript-language-server",
                binary: "typescript-language-server",
                args: &["--stdio"],
                npm_package: Some("typescript-language-server"),
            },
            language_id: "javascript",
        }),
        "jsx" => Some(LspServerMatch {
            definition: LspServerDefinition {
                id: "typescript-language-server",
                binary: "typescript-language-server",
                args: &["--stdio"],
                npm_package: Some("typescript-language-server"),
            },
            language_id: "javascriptreact",
        }),
        "ts" => Some(LspServerMatch {
            definition: LspServerDefinition {
                id: "typescript-language-server",
                binary: "typescript-language-server",
                args: &["--stdio"],
                npm_package: Some("typescript-language-server"),
            },
            language_id: "typescript",
        }),
        "tsx" => Some(LspServerMatch {
            definition: LspServerDefinition {
                id: "typescript-language-server",
                binary: "typescript-language-server",
                args: &["--stdio"],
                npm_package: Some("typescript-language-server"),
            },
            language_id: "typescriptreact",
        }),
        "go" => Some(LspServerMatch {
            definition: LspServerDefinition {
                id: "gopls",
                binary: "gopls",
                args: &[],
                npm_package: None,
            },
            language_id: "go",
        }),
        "yaml" | "yml" => Some(LspServerMatch {
            definition: LspServerDefinition {
                id: "yaml-language-server",
                binary: "yaml-language-server",
                args: &["--stdio"],
                npm_package: Some("yaml-language-server"),
            },
            language_id: "yaml",
        }),
        _ => None,
    }
}

pub(crate) fn managed_lsp_binary(
    definition: &LspServerDefinition,
    env_map: &BTreeMap<String, String>,
) -> Option<PathBuf> {
    let _ = definition.npm_package?;
    let home = resolve_psychevo_home(env_map).ok()?;
    Some(npm_bin_dir(&home.join("lsp").join("node")).join(npm_bin_name(definition.binary)))
}

pub(crate) fn npm_bin_dir(install_dir: &Path) -> PathBuf {
    install_dir.join("node_modules").join(".bin")
}

pub(crate) fn npm_bin_name(name: &str) -> String {
    if cfg!(windows) {
        format!("{name}.cmd")
    } else {
        name.to_string()
    }
}

pub(crate) fn find_executable_path(
    name: &str,
    env_map: &BTreeMap<String, String>,
    path_prefixes: &[PathBuf],
) -> Option<PathBuf> {
    find_on_path(name, combined_path(env_map, path_prefixes))
}

pub(crate) fn combined_path(
    env_map: &BTreeMap<String, String>,
    path_prefixes: &[PathBuf],
) -> Option<OsString> {
    let mut paths = path_prefixes.to_vec();
    if let Some(path) = env_map.get("PATH") {
        paths.extend(std::env::split_paths(path));
    } else if let Some(path) = std::env::var_os("PATH") {
        paths.extend(std::env::split_paths(&path));
    }
    if paths.is_empty() {
        return None;
    }
    std::env::join_paths(paths).ok()
}

pub(crate) fn hash_content(content: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    content.hash(&mut hasher);
    hasher.finish()
}

pub(crate) fn emit_lsp_status(
    tool: &WorkdirTool,
    status: &str,
    server_id: Option<&str>,
    path: Option<&Path>,
    message: Option<String>,
) {
    let Some(stream) = &tool.context.stream_events else {
        return;
    };
    let mut event = serde_json::Map::new();
    event.insert("type".to_string(), json!("lsp_status"));
    event.insert("status".to_string(), json!(status));
    if let Some(server_id) = server_id {
        event.insert("server_id".to_string(), json!(server_id));
    }
    event.insert(
        "workspace".to_string(),
        json!(tool.workdir().display().to_string()),
    );
    if let Some(path) = path {
        event.insert("path".to_string(), json!(tool.relative(path)));
    }
    if let Some(message) = message {
        event.insert("message".to_string(), json!(message));
    }
    stream(RunStreamEvent::Event(Value::Object(event)));
}

#[cfg(test)]
pub(crate) fn command_available(program: &str) -> bool {
    std::env::var_os("PATH")
        .and_then(|paths| {
            std::env::split_paths(&paths)
                .map(|path| path.join(program))
                .find(|path| path.exists())
        })
        .is_some()
}

#[cfg(test)]
pub(crate) fn lsp_diagnostics_with_command(
    server: &LspServerCommand,
    workdir: &Path,
    path: &Path,
    content: &str,
    timeout: Duration,
) -> Result<Vec<Value>> {
    let uri = file_uri(path);
    let mut child = std::process::Command::new(&server.program)
        .args(&server.args)
        .current_dir(workdir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|err| Error::Message(format!("LSP spawn failed: {err}")))?;
    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| Error::Message("LSP stdin unavailable".to_string()))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| Error::Message("LSP stdout unavailable".to_string()))?;
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut reader = std::io::BufReader::new(stdout);
        while let Ok(Some(message)) = read_lsp_message(&mut reader) {
            if tx.send(message).is_err() {
                break;
            }
        }
    });
    send_lsp(
        &mut stdin,
        json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "processId": null,
                "rootUri": file_uri(workdir),
                "capabilities": {
                    "textDocument": {
                        "publishDiagnostics": { "relatedInformation": false },
                        "diagnostic": { "dynamicRegistration": true }
                    }
                },
                "workspaceFolders": [{ "uri": file_uri(workdir), "name": "workspace" }],
                "clientInfo": { "name": "psychevo", "version": "0" }
            }
        }),
    )?;
    let start = std::time::Instant::now();
    while start.elapsed() < timeout {
        let remaining = timeout.saturating_sub(start.elapsed());
        let Ok(message) = rx.recv_timeout(remaining.min(Duration::from_millis(200))) else {
            continue;
        };
        if message.get("id").and_then(Value::as_i64) == Some(1) {
            break;
        }
    }
    send_lsp(
        &mut stdin,
        json!({ "jsonrpc": "2.0", "method": "initialized", "params": {} }),
    )?;
    send_lsp(
        &mut stdin,
        json!({
            "jsonrpc": "2.0",
            "method": "textDocument/didOpen",
            "params": {
                "textDocument": {
                    "uri": uri,
                    "languageId": lsp_language_id(path),
                    "version": 1,
                    "text": content
                }
            }
        }),
    )?;
    let mut diagnostics = Vec::new();
    let start = std::time::Instant::now();
    while start.elapsed() < timeout {
        let remaining = timeout.saturating_sub(start.elapsed());
        let Ok(message) = rx.recv_timeout(remaining.min(Duration::from_millis(200))) else {
            continue;
        };
        if message.get("method").and_then(Value::as_str) == Some("textDocument/publishDiagnostics")
            && message
                .pointer("/params/uri")
                .and_then(Value::as_str)
                .is_some_and(|value| value == uri)
        {
            diagnostics = message
                .pointer("/params/diagnostics")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            break;
        }
    }
    let _ = send_lsp(
        &mut stdin,
        json!({ "jsonrpc": "2.0", "id": 2, "method": "shutdown" }),
    );
    let _ = send_lsp(&mut stdin, json!({ "jsonrpc": "2.0", "method": "exit" }));
    let _ = child.kill();
    let _ = child.wait();
    Ok(diagnostics)
}

pub(crate) fn send_lsp(stdin: &mut std::process::ChildStdin, message: Value) -> Result<()> {
    let body = serde_json::to_string(&message)?;
    std::io::Write::write_all(
        stdin,
        format!("Content-Length: {}\r\n\r\n{}", body.len(), body).as_bytes(),
    )?;
    std::io::Write::flush(stdin)?;
    Ok(())
}

pub(crate) fn read_lsp_message(
    reader: &mut dyn std::io::BufRead,
) -> std::io::Result<Option<Value>> {
    let mut content_len = None;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line)? == 0 {
            return Ok(None);
        }
        let trimmed = line.trim_end();
        if trimmed.is_empty() {
            break;
        }
        if let Some(value) = trimmed.strip_prefix("Content-Length:") {
            content_len = value.trim().parse::<usize>().ok();
        }
    }
    let Some(len) = content_len else {
        return Ok(None);
    };
    let mut body = vec![0u8; len];
    std::io::Read::read_exact(reader, &mut body)?;
    Ok(serde_json::from_slice(&body).ok())
}

pub(crate) fn file_uri(path: &Path) -> String {
    let raw = path
        .canonicalize()
        .unwrap_or_else(|_| path.to_path_buf())
        .to_string_lossy()
        .replace('\\', "/");
    format!("file://{}", percent_encode_path(&raw))
}

pub(crate) fn percent_encode_path(path: &str) -> String {
    let mut out = String::new();
    for byte in path.as_bytes() {
        let ch = *byte as char;
        if ch.is_ascii_alphanumeric() || matches!(ch, '/' | '-' | '_' | '.' | '~' | ':') {
            out.push(ch);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

#[cfg(test)]
pub(crate) fn lsp_language_id(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or_default()
    {
        "rs" => "rust",
        "py" => "python",
        "js" | "mjs" | "cjs" => "javascript",
        "jsx" => "javascriptreact",
        "ts" => "typescript",
        "tsx" => "typescriptreact",
        "go" => "go",
        "yaml" | "yml" => "yaml",
        _ => "plaintext",
    }
}

pub(crate) fn lsp_diag_key(diag: &Value) -> String {
    format!(
        "{}|{}|{}|{}",
        diag.get("severity").and_then(Value::as_i64).unwrap_or(1),
        diag.get("code").map(Value::to_string).unwrap_or_default(),
        diag.get("source")
            .and_then(Value::as_str)
            .unwrap_or_default(),
        diag.get("message")
            .and_then(Value::as_str)
            .unwrap_or_default()
    )
}

pub(crate) fn format_lsp_diagnostics(path: &Path, diagnostics: &[Value]) -> Option<String> {
    if diagnostics.is_empty() {
        return None;
    }
    let mut lines = Vec::new();
    for diag in diagnostics.iter().take(20) {
        let start = diag.pointer("/range/start").unwrap_or(&Value::Null);
        let line = start.get("line").and_then(Value::as_u64).unwrap_or(0) + 1;
        let col = start.get("character").and_then(Value::as_u64).unwrap_or(0) + 1;
        let severity = match diag.get("severity").and_then(Value::as_u64).unwrap_or(1) {
            1 => "error",
            2 => "warning",
            3 => "info",
            4 => "hint",
            _ => "diagnostic",
        };
        let message = diag
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("")
            .replace('\n', " ");
        let source = diag.get("source").and_then(Value::as_str).unwrap_or("");
        let code = diag
            .get("code")
            .map(Value::to_string)
            .unwrap_or_default()
            .trim_matches('"')
            .to_string();
        let label = [source, &code]
            .into_iter()
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>()
            .join(" ");
        lines.push(if label.is_empty() {
            format!("{line}:{col} {severity}: {message}")
        } else {
            format!("{line}:{col} {severity} [{label}]: {message}")
        });
    }
    if diagnostics.len() > 20 {
        lines.push(format!("... {} more diagnostics", diagnostics.len() - 20));
    }
    let body = lines.join("\n");
    let block = format!(
        "<diagnostics file=\"{}\">\n{}\n</diagnostics>",
        path.display(),
        body
    );
    Some(truncate_lint_output(&block))
}

#[cfg(test)]
pub(crate) mod lsp_tests {
    pub(crate) use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn env_for(home: &Path, path: &Path) -> BTreeMap<String, String> {
        BTreeMap::from([
            ("HOME".to_string(), home.display().to_string()),
            ("PSYCHEVO_HOME".to_string(), home.display().to_string()),
            ("PATH".to_string(), path.display().to_string()),
        ])
    }

    fn env_for_with_system_path(home: &Path, path: &Path) -> BTreeMap<String, String> {
        let mut paths = vec![path.to_path_buf()];
        if let Some(current) = std::env::var_os("PATH") {
            paths.extend(std::env::split_paths(&current));
        }
        let path_value = std::env::join_paths(paths).expect("joined PATH");
        BTreeMap::from([
            ("HOME".to_string(), home.display().to_string()),
            ("PSYCHEVO_HOME".to_string(), home.display().to_string()),
            ("PATH".to_string(), path_value.to_string_lossy().to_string()),
        ])
    }

    fn test_tool(
        workdir: &Path,
        lsp: LspConfig,
        lsp_manager: Arc<LspManager>,
        env: BTreeMap<String, String>,
        stream_events: Option<RunStreamSink>,
    ) -> WorkdirTool {
        WorkdirTool::with_context(
            workdir.canonicalize().expect("workdir"),
            ToolRuntimeContext {
                task_id: uuid::Uuid::now_v7().to_string(),
                lsp,
                lsp_manager,
                allow_login_shell: false,
                stream_events,
                env,
                path_prefixes: Vec::new(),
                sandbox_policy: SandboxPolicy::disabled(),
                sandbox_grants: crate::sandbox::SandboxWriteGrants::default(),
            },
        )
    }

    #[cfg(unix)]
    fn write_executable(path: &Path, content: &str) {
        use std::os::unix::fs::PermissionsExt;

        fs::write(path, content).expect("script");
        let mut permissions = fs::metadata(path).expect("metadata").permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(path, permissions).expect("chmod");
    }

    #[test]
    fn lsp_auto_resolution_schedules_install_without_npx() {
        let temp = tempfile::tempdir().expect("temp");
        let home = temp.path().join("home");
        let path_dir = temp.path().join("path");
        fs::create_dir_all(&home).expect("home");
        fs::create_dir_all(&path_dir).expect("path");
        fs::write(path_dir.join("npx"), "not executable").expect("fake npx");
        let config = LspConfig {
            install_strategy: "auto".to_string(),
            ..Default::default()
        };
        let resolution = resolve_lsp_server_with_env(
            Path::new("sample.py"),
            &config,
            &env_for(&home, &path_dir),
            &[],
        );
        match resolution {
            LspServerResolution::MissingInstallable(server_match) => {
                assert_eq!(server_match.definition.id, "pyright");
                assert_eq!(server_match.definition.npm_package, Some("pyright"));
            }
            LspServerResolution::Ready(server) => {
                panic!("expected install scheduling, got {}", server.program)
            }
            LspServerResolution::Missing | LspServerResolution::Skipped => {
                panic!("expected installable pyright")
            }
        }
    }

    #[test]
    fn lsp_manual_and_off_do_not_auto_install_missing_server() {
        let temp = tempfile::tempdir().expect("temp");
        let home = temp.path().join("home");
        let path_dir = temp.path().join("path");
        fs::create_dir_all(&home).expect("home");
        fs::create_dir_all(&path_dir).expect("path");
        let manual = LspConfig {
            install_strategy: "manual".to_string(),
            ..Default::default()
        };
        assert!(matches!(
            resolve_lsp_server_with_env(
                Path::new("sample.py"),
                &manual,
                &env_for(&home, &path_dir),
                &[],
            ),
            LspServerResolution::Missing
        ));
        let off = LspConfig {
            install_strategy: "off".to_string(),
            ..Default::default()
        };
        assert!(matches!(
            resolve_lsp_server_with_env(
                Path::new("sample.py"),
                &off,
                &env_for(&home, &path_dir),
                &[],
            ),
            LspServerResolution::Skipped
        ));
    }

    #[test]
    fn lsp_auto_install_is_background_and_deduplicated() {
        let temp = tempfile::tempdir().expect("temp");
        let workdir = temp.path().join("work");
        let home = temp.path().join("home");
        let path_dir = temp.path().join("path");
        fs::create_dir_all(&workdir).expect("workdir");
        fs::create_dir_all(&home).expect("home");
        fs::create_dir_all(&path_dir).expect("path");
        let calls = Arc::new(AtomicUsize::new(0));
        let calls_for_installer = Arc::clone(&calls);
        let manager = Arc::new(LspManager::new(Arc::new(move |_request| {
            calls_for_installer.fetch_add(1, Ordering::SeqCst);
            std::thread::sleep(Duration::from_millis(200));
            Ok(())
        })));
        let events = Arc::new(Mutex::new(Vec::<Value>::new()));
        let sink_events = Arc::clone(&events);
        let stream: RunStreamSink = Arc::new(move |event| {
            if let RunStreamEvent::Event(value) = event {
                sink_events.lock().expect("events").push(value);
            }
        });
        let tool = test_tool(
            &workdir,
            LspConfig {
                install_strategy: "auto".to_string(),
                ..Default::default()
            },
            manager,
            env_for(&home, &path_dir),
            Some(stream),
        );
        let file = workdir.join("sample.py");
        let first = Instant::now();
        let run = tool
            .context
            .lsp_manager
            .diagnostics(&tool, &file, "print('one')\n")
            .expect("diagnostics");
        assert!(run.diagnostics.is_empty());
        assert!(first.elapsed() < Duration::from_millis(100));
        let _ = tool
            .context
            .lsp_manager
            .diagnostics(&tool, &file, "print('two')\n")
            .expect("diagnostics");
        let deadline = Instant::now() + Duration::from_secs(1);
        while calls.load(Ordering::SeqCst) == 0 && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        let statuses = events
            .lock()
            .expect("events")
            .iter()
            .filter_map(|event| event.get("status").and_then(Value::as_str))
            .map(str::to_string)
            .collect::<Vec<_>>();
        assert!(
            statuses.contains(&"install_started".to_string()),
            "{statuses:?}"
        );
        assert!(statuses.contains(&"installing".to_string()), "{statuses:?}");
    }

    #[cfg(unix)]
    #[test]
    fn python_write_does_not_call_npx_when_lsp_auto_is_missing() {
        let temp = tempfile::tempdir().expect("temp");
        let workdir = temp.path().join("work");
        let home = temp.path().join("home");
        let path_dir = temp.path().join("path");
        fs::create_dir_all(&workdir).expect("workdir");
        fs::create_dir_all(&home).expect("home");
        fs::create_dir_all(&path_dir).expect("path");
        let marker = temp.path().join("npx-called");
        write_executable(
            &path_dir.join("npx"),
            &format!("#!/bin/sh\nprintf called > {}\nsleep 1\n", marker.display()),
        );
        let calls = Arc::new(AtomicUsize::new(0));
        let calls_for_installer = Arc::clone(&calls);
        let manager = Arc::new(LspManager::new(Arc::new(move |_request| {
            calls_for_installer.fetch_add(1, Ordering::SeqCst);
            Ok(())
        })));
        let tool = test_tool(
            &workdir,
            LspConfig {
                install_strategy: "auto".to_string(),
                ..Default::default()
            },
            manager,
            env_for(&home, &path_dir),
            None,
        );
        let target = workdir.join("add.py");
        let value = write_text_to_target(&tool, &target, "print('ok')\n", false, None, None)
            .expect("write");
        assert_eq!(value["error"], Value::Null);
        assert!(
            !marker.exists(),
            "npx should not be invoked from LSP hot path"
        );
        let deadline = Instant::now() + Duration::from_secs(1);
        while calls.load(Ordering::SeqCst) == 0 && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn lsp_fake_server_returns_diagnostics() {
        if !command_available("python3") {
            return;
        }
        let temp = tempfile::tempdir().expect("temp");
        let script = temp.path().join("fake_lsp.py");
        fs::write(
            &script,
            r#"#!/usr/bin/env python3
import json
import sys

def read_msg():
    headers = {}
    while True:
        line = sys.stdin.buffer.readline()
        if not line:
            return None
        line = line.decode("ascii").strip()
        if not line:
            break
        key, value = line.split(":", 1)
        headers[key.lower()] = value.strip()
    length = int(headers.get("content-length", "0"))
    return json.loads(sys.stdin.buffer.read(length).decode("utf-8"))

def send(msg):
    body = json.dumps(msg).encode("utf-8")
    sys.stdout.buffer.write(b"Content-Length: " + str(len(body)).encode("ascii") + b"\r\n\r\n" + body)
    sys.stdout.buffer.flush()

while True:
    msg = read_msg()
    if msg is None:
        break
    method = msg.get("method")
    if method == "initialize":
        send({"jsonrpc":"2.0","id":msg["id"],"result":{"capabilities":{"textDocumentSync":1}}})
    elif method == "textDocument/didOpen":
        doc = msg["params"]["textDocument"]
        diagnostics = []
        if "bad" in doc.get("text", ""):
            diagnostics.append({
                "range": {"start": {"line": 0, "character": 1}, "end": {"line": 0, "character": 4}},
                "severity": 1,
                "source": "fake",
                "code": "E001",
                "message": "bad token"
            })
        send({"jsonrpc":"2.0","method":"textDocument/publishDiagnostics","params":{"uri":doc["uri"],"diagnostics":diagnostics}})
    elif method == "shutdown":
        send({"jsonrpc":"2.0","id":msg["id"],"result":None})
    elif method == "exit":
        break
"#,
        )
        .expect("script");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).expect("chmod");
        }
        let file = temp.path().join("sample.fake");
        fs::write(&file, "bad\n").expect("file");
        let diagnostics = lsp_diagnostics_with_command(
            &LspServerCommand {
                id: "fake".to_string(),
                program: "python3".to_string(),
                args: vec![script.to_string_lossy().to_string()],
                language_id: "plaintext".to_string(),
                env_path: None,
            },
            temp.path(),
            &file,
            "bad\n",
            Duration::from_secs(2),
        )
        .expect("diagnostics");
        assert_eq!(diagnostics.len(), 1);
        let formatted = format_lsp_diagnostics(&file, &diagnostics).expect("formatted");
        assert!(formatted.contains("bad token"));
        assert!(formatted.contains("<diagnostics"));
    }

    #[cfg(unix)]
    #[test]
    fn lsp_manager_reuses_server_and_filters_baseline() {
        if !command_available("python3") {
            return;
        }
        let temp = tempfile::tempdir().expect("temp");
        let workdir = temp.path().join("work");
        let home = temp.path().join("home");
        let path_dir = temp.path().join("path");
        fs::create_dir_all(&workdir).expect("workdir");
        fs::create_dir_all(&home).expect("home");
        fs::create_dir_all(&path_dir).expect("path");
        let count_path = temp.path().join("starts.txt");
        let count_repr = format!("{:?}", count_path.to_string_lossy().to_string());
        let fake_server = r#"#!/usr/bin/env python3
import json
import sys

COUNT = __COUNT__
with open(COUNT, "a", encoding="utf-8") as f:
    f.write("start\n")

def read_msg():
    headers = {}
    while True:
        line = sys.stdin.buffer.readline()
        if not line:
            return None
        line = line.decode("ascii").strip()
        if not line:
            break
        key, value = line.split(":", 1)
        headers[key.lower()] = value.strip()
    length = int(headers.get("content-length", "0"))
    return json.loads(sys.stdin.buffer.read(length).decode("utf-8"))

def send(msg):
    body = json.dumps(msg).encode("utf-8")
    sys.stdout.buffer.write(b"Content-Length: " + str(len(body)).encode("ascii") + b"\r\n\r\n" + body)
    sys.stdout.buffer.flush()

while True:
    msg = read_msg()
    if msg is None:
        break
    method = msg.get("method")
    if method == "initialize":
        send({"jsonrpc":"2.0","id":msg["id"],"result":{"capabilities":{"textDocumentSync":2}}})
    elif method in ("textDocument/didOpen", "textDocument/didChange"):
        if method == "textDocument/didOpen":
            uri = msg["params"]["textDocument"]["uri"]
            text = msg["params"]["textDocument"].get("text", "")
        else:
            uri = msg["params"]["textDocument"]["uri"]
            text = msg["params"]["contentChanges"][0].get("text", "")
        diagnostics = []
        if "bad" in text:
            diagnostics.append({
                "range": {"start": {"line": 0, "character": 0}, "end": {"line": 0, "character": 3}},
                "severity": 1,
                "source": "fake",
                "code": "E001",
                "message": "bad token"
            })
        if "worse" in text:
            diagnostics.append({
                "range": {"start": {"line": 1, "character": 0}, "end": {"line": 1, "character": 5}},
                "severity": 1,
                "source": "fake",
                "code": "E002",
                "message": "worse token"
            })
        send({"jsonrpc":"2.0","method":"textDocument/publishDiagnostics","params":{"uri":uri,"diagnostics":diagnostics}})
    elif method == "shutdown":
        send({"jsonrpc":"2.0","id":msg["id"],"result":None})
    elif method == "exit":
        break
"#
        .replace("__COUNT__", &count_repr);
        write_executable(&path_dir.join("pyright-langserver"), &fake_server);
        let manager = Arc::new(LspManager::new(Arc::new(|_request| {
            Err(Error::Message("unexpected install".to_string()))
        })));
        let tool = test_tool(
            &workdir,
            LspConfig {
                install_strategy: "manual".to_string(),
                wait_timeout_secs: 1.0,
                ..Default::default()
            },
            manager,
            env_for_with_system_path(&home, &path_dir),
            None,
        );
        let file = workdir.join("sample.py");
        fs::write(&file, "bad\n").expect("file");
        let baseline_run = tool
            .context
            .lsp_manager
            .diagnostics(&tool, &file, "bad\n")
            .expect("baseline diagnostics");
        let baseline = LspBaseline {
            diagnostics: baseline_run.diagnostics,
        };
        let block =
            lsp_diagnostics_after(&tool, &file, Some("bad\n"), "bad\nworse\n", Some(baseline))
                .expect("diagnostics block");
        assert!(block.contains("worse token"), "{block}");
        assert!(!block.contains("bad token"), "{block}");
        let starts = fs::read_to_string(count_path).expect("count");
        assert_eq!(starts.lines().count(), 1);
    }

    #[cfg(unix)]
    #[test]
    fn lsp_manager_marks_failed_server_broken() {
        let temp = tempfile::tempdir().expect("temp");
        let workdir = temp.path().join("work");
        let home = temp.path().join("home");
        let path_dir = temp.path().join("path");
        fs::create_dir_all(&workdir).expect("workdir");
        fs::create_dir_all(&home).expect("home");
        fs::create_dir_all(&path_dir).expect("path");
        write_executable(&path_dir.join("pyright-langserver"), "#!/bin/sh\nexit 1\n");
        let events = Arc::new(Mutex::new(Vec::<Value>::new()));
        let sink_events = Arc::clone(&events);
        let stream: RunStreamSink = Arc::new(move |event| {
            if let RunStreamEvent::Event(value) = event {
                sink_events.lock().expect("events").push(value);
            }
        });
        let tool = test_tool(
            &workdir,
            LspConfig {
                install_strategy: "manual".to_string(),
                wait_timeout_secs: 0.1,
                ..Default::default()
            },
            Arc::new(LspManager::new(Arc::new(|_request| {
                Err(Error::Message("unexpected install".to_string()))
            }))),
            env_for(&home, &path_dir),
            Some(stream),
        );
        let file = workdir.join("sample.py");
        let first = tool
            .context
            .lsp_manager
            .diagnostics(&tool, &file, "print('x')\n");
        assert!(first.is_err());
        let second = tool
            .context
            .lsp_manager
            .diagnostics(&tool, &file, "print('x')\n")
            .expect("broken skip");
        assert!(second.diagnostics.is_empty());
        let statuses = events
            .lock()
            .expect("events")
            .iter()
            .filter_map(|event| event.get("status").and_then(Value::as_str))
            .map(str::to_string)
            .collect::<Vec<_>>();
        assert!(statuses.contains(&"failed".to_string()), "{statuses:?}");
        assert!(statuses.contains(&"skipped".to_string()), "{statuses:?}");
    }
}
