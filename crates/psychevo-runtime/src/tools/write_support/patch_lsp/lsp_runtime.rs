impl LspClient {
    pub(crate) fn start(
        command: LspServerCommand,
        cwd: PathBuf,
        timeout: Duration,
    ) -> Result<Self> {
        let mut process = std::process::Command::new(&command.program);
        process
            .args(&command.args)
            .current_dir(&cwd)
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
            cwd,
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
                        "rootUri": file_uri(&self.cwd),
                        "capabilities": {
                            "textDocument": {
                                "publishDiagnostics": { "relatedInformation": false },
                                "diagnostic": { "dynamicRegistration": true }
                            }
                        },
                        "workspaceFolders": [{ "uri": file_uri(&self.cwd), "name": "workspace" }],
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
    tool: &CwdTool,
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
        json!(tool.cwd().display().to_string()),
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
    cwd: &Path,
    path: &Path,
    content: &str,
    timeout: Duration,
) -> Result<Vec<Value>> {
    let uri = file_uri(path);
    let mut child = std::process::Command::new(&server.program)
        .args(&server.args)
        .current_dir(cwd)
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
                "rootUri": file_uri(cwd),
                "capabilities": {
                    "textDocument": {
                        "publishDiagnostics": { "relatedInformation": false },
                        "diagnostic": { "dynamicRegistration": true }
                    }
                },
                "workspaceFolders": [{ "uri": file_uri(cwd), "name": "workspace" }],
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
