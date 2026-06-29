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
        cwd: &Path,
        lsp: LspConfig,
        lsp_manager: Arc<LspManager>,
        env: BTreeMap<String, String>,
        stream_events: Option<RunStreamSink>,
    ) -> CwdTool {
        CwdTool::with_context(
            cwd.canonicalize().expect("cwd"),
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
        let cwd = temp.path().join("work");
        let home = temp.path().join("home");
        let path_dir = temp.path().join("path");
        fs::create_dir_all(&cwd).expect("cwd");
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
            &cwd,
            LspConfig {
                install_strategy: "auto".to_string(),
                ..Default::default()
            },
            manager,
            env_for(&home, &path_dir),
            Some(stream),
        );
        let file = cwd.join("sample.py");
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
        let cwd = temp.path().join("work");
        let home = temp.path().join("home");
        let path_dir = temp.path().join("path");
        fs::create_dir_all(&cwd).expect("cwd");
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
            &cwd,
            LspConfig {
                install_strategy: "auto".to_string(),
                ..Default::default()
            },
            manager,
            env_for(&home, &path_dir),
            None,
        );
        let target = cwd.join("add.py");
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
        let cwd = temp.path().join("work");
        let home = temp.path().join("home");
        let path_dir = temp.path().join("path");
        fs::create_dir_all(&cwd).expect("cwd");
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
            &cwd,
            LspConfig {
                install_strategy: "manual".to_string(),
                wait_timeout_secs: 1.0,
                ..Default::default()
            },
            manager,
            env_for_with_system_path(&home, &path_dir),
            None,
        );
        let file = cwd.join("sample.py");
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
        let cwd = temp.path().join("work");
        let home = temp.path().join("home");
        let path_dir = temp.path().join("path");
        fs::create_dir_all(&cwd).expect("cwd");
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
            &cwd,
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
        let file = cwd.join("sample.py");
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
