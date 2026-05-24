#[allow(unused_imports)]
pub(crate) use super::*;

#[allow(unused_imports)]
use serde_json::json;

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
    let fresh = run_lsp_diagnostics(tool, path, post_content).ok()?;
    let baseline_keys = baseline
        .map(|baseline| {
            baseline
                .diagnostics
                .iter()
                .map(lsp_diag_key)
                .collect::<HashSet<_>>()
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
    let Some(server) = resolve_lsp_server(path, tool.lsp_config()) else {
        return Ok(Vec::new());
    };
    let timeout = Duration::from_secs_f64(tool.lsp_config().wait_timeout_secs.max(0.1))
        + Duration::from_secs(2);
    lsp_diagnostics_with_command(&server, tool.workdir(), path, content, timeout)
}

#[derive(Clone)]
pub(crate) struct LspServerCommand {
    pub(crate) program: String,
    pub(crate) args: Vec<String>,
}

pub(crate) fn resolve_lsp_server(path: &Path, config: &LspConfig) -> Option<LspServerCommand> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    let (bin, args, auto): (&str, &[&str], LspAutoCommand<'_>) = match ext.as_str() {
        "rs" => ("rust-analyzer", &[], None),
        "py" => (
            "pyright-langserver",
            &["--stdio"],
            Some(("npx", &["-y", "pyright-langserver", "--stdio"])),
        ),
        "js" | "jsx" | "ts" | "tsx" => (
            "typescript-language-server",
            &["--stdio"],
            Some(("npx", &["-y", "typescript-language-server", "--stdio"])),
        ),
        "go" => ("gopls", &[], None),
        "yaml" | "yml" => (
            "yaml-language-server",
            &["--stdio"],
            Some(("npx", &["-y", "yaml-language-server", "--stdio"])),
        ),
        _ => return None,
    };
    if command_available(bin) {
        return Some(LspServerCommand {
            program: bin.to_string(),
            args: args.iter().map(|arg| arg.to_string()).collect(),
        });
    }
    if config.install_strategy == "auto"
        && let Some((program, args)) = auto
    {
        return Some(LspServerCommand {
            program: program.to_string(),
            args: args.iter().map(|arg| arg.to_string()).collect(),
        });
    }
    None
}

pub(crate) fn command_available(program: &str) -> bool {
    std::env::var_os("PATH")
        .and_then(|paths| {
            std::env::split_paths(&paths)
                .map(|path| path.join(program))
                .find(|path| path.exists())
        })
        .is_some()
}

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
                program: "python3".to_string(),
                args: vec![script.to_string_lossy().to_string()],
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
}
