use std::sync::Mutex;

use super::*;

#[derive(Default)]
struct FakeLauncher {
    requests: Mutex<Vec<WorkspaceExternalLaunchRequest>>,
    error: Option<String>,
}

impl WorkspaceExternalLauncher for FakeLauncher {
    fn launch(
        &self,
        request: WorkspaceExternalLaunchRequest,
    ) -> BoxFuture<'_, psychevo_runtime::Result<()>> {
        self.requests
            .lock()
            .expect("fake launcher requests")
            .push(request);
        let result = self
            .error
            .as_ref()
            .map(|message| Err(Error::Message(message.clone())))
            .unwrap_or(Ok(()));
        Box::pin(async move { result })
    }
}

fn test_scope(root: &Path) -> ResolvedScope {
    ResolvedScope {
        cwd: root.to_path_buf(),
        source: super::super::cwd_source(root),
    }
}

#[test]
fn classification_preserves_category_precedence_and_text_like_overlap() {
    let temp = tempfile::tempdir().expect("tempdir");
    for (name, content, category, text_like) in [
        (
            "index.HTML",
            b"<main>Hello</main>".as_slice(),
            wire::WorkspaceExternalFileCategory::Webpage,
            true,
        ),
        (
            "icon.svg",
            b"<svg></svg>".as_slice(),
            wire::WorkspaceExternalFileCategory::Image,
            true,
        ),
        (
            "table.csv",
            b"a,b\n1,2\n".as_slice(),
            wire::WorkspaceExternalFileCategory::Office,
            true,
        ),
        (
            "photo.png",
            b"\x89PNG\r\n".as_slice(),
            wire::WorkspaceExternalFileCategory::Image,
            false,
        ),
    ] {
        let path = temp.path().join(name);
        std::fs::write(&path, content).expect("fixture");
        assert_eq!(
            classify_external_file(&path).expect("classification"),
            ExternalFileClassification {
                category,
                text_like,
            }
        );
    }
}

#[test]
fn classification_probes_utf8_utf16_and_unknown_binary_files() {
    let temp = tempfile::tempdir().expect("tempdir");
    let utf8 = temp.path().join("notes");
    let utf16 = temp.path().join("localized.data");
    let legacy = temp.path().join("legacy.logdata");
    let binary = temp.path().join("opaque.data");
    std::fs::write(&utf8, "extensionless text\n").expect("utf8");
    std::fs::write(&utf16, [0xff, 0xfe, b'h', 0, b'i', 0]).expect("utf16");
    std::fs::write(&legacy, [0xc4, 0xe3, 0xba, 0xc3, b'\n']).expect("legacy text");
    std::fs::write(&binary, [0, 159, 146, 150]).expect("binary");

    for path in [&utf8, &utf16, &legacy] {
        assert_eq!(
            classify_external_file(path).expect("text classification"),
            ExternalFileClassification {
                category: wire::WorkspaceExternalFileCategory::Text,
                text_like: true,
            }
        );
    }
    assert_eq!(
        classify_external_file(&binary).expect("binary classification"),
        ExternalFileClassification {
            category: wire::WorkspaceExternalFileCategory::Other,
            text_like: false,
        }
    );
}

#[test]
fn unreadable_unknown_content_probe_falls_back_to_other() {
    let classification = classify_external_file_with_probe(Path::new("opaque.unknown"), |_| {
        Err(Error::Message("probe read failed".to_string()))
    });

    assert_eq!(
        classification,
        ExternalFileClassification {
            category: wire::WorkspaceExternalFileCategory::Other,
            text_like: false,
        }
    );
}

#[tokio::test]
async fn fake_launcher_receives_canonical_workspace_and_file_without_real_processes() {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path().join("workspace");
    std::fs::create_dir_all(root.join("src")).expect("workspace");
    std::fs::write(root.join("src/lib.rs"), "fn main() {}\n").expect("file");
    let fake = Arc::new(FakeLauncher::default());
    let state = WorkspaceExternalState::for_test(
        wire::WorkspaceExternalHostPlatform::Linux,
        Some(PathBuf::from("/opt/code/bin/code")),
        fake.clone(),
    );
    let scope = test_scope(&root.canonicalize().expect("canonical workspace"));

    let actions =
        workspace_file_external_actions_value(&state, &scope, "src/./lib.rs").expect("actions");
    assert_eq!(actions["preferredAction"], "vscode");
    assert_eq!(
        actions["availableActions"],
        serde_json::json!(["vscode", "systemDefault", "reveal"])
    );

    let opened = workspace_file_open_external_value(
        &state,
        &scope,
        wire::WorkspaceFileOpenExternalParams {
            scope: scope.to_wire_scope(),
            path: "src/lib.rs".to_string(),
            action: wire::WorkspaceExternalFileAction::Vscode,
        },
    )
    .await
    .expect("open");
    assert_eq!(opened["path"], "src/lib.rs");
    assert_eq!(
        *fake.requests.lock().expect("requests"),
        vec![WorkspaceExternalLaunchRequest {
            action: wire::WorkspaceExternalFileAction::Vscode,
            workspace_root: scope.cwd.clone(),
            file: scope.cwd.join("src/lib.rs"),
            vscode_launcher: Some(PathBuf::from("/opt/code/bin/code")),
        }]
    );
}

#[tokio::test]
async fn unavailable_vscode_action_is_rejected_before_launcher_invocation() {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path().join("workspace");
    std::fs::create_dir_all(&root).expect("workspace");
    std::fs::write(root.join("README.md"), "hello\n").expect("file");
    let fake = Arc::new(FakeLauncher::default());
    let state = WorkspaceExternalState::for_test(
        wire::WorkspaceExternalHostPlatform::Linux,
        None,
        fake.clone(),
    );
    let scope = test_scope(&root.canonicalize().expect("canonical workspace"));

    let error = workspace_file_open_external_value(
        &state,
        &scope,
        wire::WorkspaceFileOpenExternalParams {
            scope: scope.to_wire_scope(),
            path: "README.md".to_string(),
            action: wire::WorkspaceExternalFileAction::Vscode,
        },
    )
    .await
    .expect_err("VS Code action must be unavailable");
    assert!(error.to_string().contains("not available"));
    assert!(fake.requests.lock().expect("requests").is_empty());
}

#[test]
fn vscode_launch_arguments_open_the_workspace_and_file_without_forcing_window_policy() {
    let root = Path::new("/workspace/project");
    let file = root.join("src/main.rs");

    assert_eq!(
        vscode_launch_args(root, &file),
        [root.as_os_str().to_os_string(), file.into_os_string()]
    );
}

#[test]
fn a_process_still_running_after_the_observation_window_is_accepted() {
    assert!(
        early_process_observation_result(EarlyProcessObservation::Running, "open test file")
            .is_ok()
    );
}

#[cfg(unix)]
#[tokio::test]
async fn an_opener_script_that_exits_nonzero_early_surfaces_a_bounded_error() {
    use std::os::unix::fs::PermissionsExt;

    let temp = tempfile::tempdir().expect("tempdir");
    let opener = temp.path().join("fake-opener");
    std::fs::write(&opener, "#!/bin/sh\nexit 7\n").expect("opener script");
    std::fs::set_permissions(&opener, std::fs::Permissions::from_mode(0o755))
        .expect("executable opener");

    let error = spawn_host_command_with_observation(
        &opener,
        &[],
        temp.path(),
        HostPlatform::Posix,
        &BTreeMap::new(),
        "open test file",
        Duration::from_millis(500),
    )
    .await
    .expect_err("early non-zero exit must fail");

    assert!(error.to_string().contains("opener exited early"));
    assert!(error.to_string().contains('7'));
    assert!(error.to_string().len() < 320);
}

#[tokio::test]
async fn launcher_failures_surface_without_falling_back_to_another_action() {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path().join("workspace");
    std::fs::create_dir_all(&root).expect("workspace");
    std::fs::write(root.join("README.md"), "hello\n").expect("file");
    let fake = Arc::new(FakeLauncher {
        requests: Mutex::default(),
        error: Some("launcher exploded".to_string()),
    });
    let state = WorkspaceExternalState::for_test(
        wire::WorkspaceExternalHostPlatform::Linux,
        Some(PathBuf::from("/opt/code/bin/code")),
        fake.clone(),
    );
    let scope = test_scope(&root.canonicalize().expect("canonical workspace"));

    let error = workspace_file_open_external_value(
        &state,
        &scope,
        wire::WorkspaceFileOpenExternalParams {
            scope: scope.to_wire_scope(),
            path: "README.md".to_string(),
            action: wire::WorkspaceExternalFileAction::Vscode,
        },
    )
    .await
    .expect_err("launcher error must surface");

    assert_eq!(error.to_string(), "launcher exploded");
    assert_eq!(fake.requests.lock().expect("requests").len(), 1);
}

#[test]
fn linux_file_uri_percent_encodes_non_uri_path_bytes() {
    assert_eq!(
        linux_file_uri(Path::new("/tmp/project/a file#1.txt")),
        "file:///tmp/project/a%20file%231.txt"
    );
}

#[cfg(unix)]
#[test]
fn vscode_detection_uses_the_effective_path_without_starting_it() {
    use std::os::unix::fs::PermissionsExt;

    let temp = tempfile::tempdir().expect("tempdir");
    let bin = temp.path().join("bin");
    std::fs::create_dir_all(&bin).expect("bin");
    let code = bin.join("code");
    std::fs::write(&code, "#!/bin/sh\nexit 0\n").expect("code fixture");
    std::fs::set_permissions(&code, std::fs::Permissions::from_mode(0o755))
        .expect("executable fixture");
    let environment = BTreeMap::from([("PATH".to_string(), bin.to_string_lossy().to_string())]);

    assert_eq!(
        detect_vscode_launcher(
            wire::WorkspaceExternalHostPlatform::Linux,
            &environment,
            temp.path(),
        ),
        Some(code)
    );
}

#[test]
fn windows_well_known_locations_prefer_code_exe_before_command_shims() {
    let environment = BTreeMap::from([(
        "LOCALAPPDATA".to_string(),
        r"C:\Users\Ada\AppData\Local".to_string(),
    )]);
    let candidates =
        vscode_well_known_paths(wire::WorkspaceExternalHostPlatform::Windows, &environment);

    assert!(candidates[0].ends_with("Code.exe"));
    assert!(candidates[1].ends_with(r"bin\code.cmd"));
}

#[test]
fn shell_execute_codes_accept_success_and_reject_documented_failure_range() {
    assert!(shell_execute_code_result(33).is_ok());
    let error = shell_execute_code_result(32).expect_err("code 32 must fail");
    assert!(error.to_string().contains("ShellExecuteW code 32"));
}

#[test]
fn bounded_launch_errors_do_not_split_multibyte_characters() {
    let source = std::io::Error::other("界".repeat(EXTERNAL_ACTION_ERROR_LIMIT));
    let message = bounded_launch_error("open a file", &source).to_string();

    assert!(message.starts_with("failed to open a file: "));
    assert!(message.len() <= "failed to open a file: ".len() + EXTERNAL_ACTION_ERROR_LIMIT);
    assert!(!message.ends_with(char::REPLACEMENT_CHARACTER));
}
