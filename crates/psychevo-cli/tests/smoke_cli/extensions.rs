use std::fs;
#[cfg(unix)]
use std::io::Write;
#[cfg(unix)]
use std::process::Stdio;

use serde_json::Value;
use tempfile::tempdir;

use super::{init_tui_home, pevo_cmd};

#[cfg(unix)]
#[test]
fn extension_lifecycle_dispatch_and_policy_are_process_complete() {
    use std::os::unix::fs::PermissionsExt;

    let temp = tempdir().expect("temp");
    let home = init_tui_home(temp.path());
    let workspace = temp.path().join("workspace");
    let extension = temp.path().join("echo-extension");
    fs::create_dir_all(&workspace).expect("workspace");
    fs::create_dir_all(&extension).expect("extension");
    fs::copy(
        concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../psychevo/tests/fixtures/extension_echo_sidecar.py"
        ),
        extension.join("sidecar"),
    )
    .expect("copy sidecar");
    let mut permissions = fs::metadata(extension.join("sidecar"))
        .expect("sidecar metadata")
        .permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(extension.join("sidecar"), permissions).expect("sidecar executable");
    fs::write(
        extension.join("psychevo.extension.json"),
        r#"{
          "schemaVersion": 1,
          "id": "example.echo",
          "version": "local",
          "runtime": {
            "protocol": "psychevo-extension/1",
            "executable": "./sidecar"
          },
          "contributions": {
            "commands": [{
              "name": "echo",
              "usage": "pevo echo [ARGS]...",
              "summary": "Echo literal arguments"
            }]
          }
        }"#,
    )
    .expect("manifest");

    let install = command(&home, &workspace)
        .args(["install", extension.to_str().expect("path"), "--json"])
        .output()
        .expect("install Extension");
    assert_success(&install);
    let install_json: Value = serde_json::from_slice(&install.stdout).expect("install json");
    assert_eq!(install_json["extension"]["id"], "example.echo");
    assert_eq!(install_json["extension"]["enabled"], true);

    let list = command(&home, &workspace)
        .args(["list", "--json"])
        .output()
        .expect("list Extensions");
    assert_success(&list);
    let list_json: Value = serde_json::from_slice(&list.stdout).expect("list json");
    assert_eq!(list_json["extensions"][0]["id"], "example.echo");

    let help = command(&home, &workspace)
        .arg("--help")
        .output()
        .expect("root help");
    assert_success(&help);
    let help = String::from_utf8(help.stdout).expect("help UTF-8");
    assert!(help.contains("Extensions:"));
    assert!(help.contains("echo"));
    assert!(help.contains("Echo literal arguments"));
    assert!(
        !home
            .join("extensions/data/example.echo/lifecycle.log")
            .exists()
    );

    let invoke = command(&home, &workspace)
        .args(["echo", "a b", "literal"])
        .output()
        .expect("direct Extension command");
    assert_success(&invoke);
    assert_eq!(
        String::from_utf8_lossy(&invoke.stdout).trim(),
        r#"["a b", "literal"]"#
    );
    assert_eq!(
        fs::read_to_string(home.join("extensions/data/example.echo/lifecycle.log"))
            .expect("lifecycle log"),
        "initialize\nshutdown\n"
    );

    let mut tui = command(&home, &workspace)
        .arg("tui")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn scripted TUI");
    tui.stdin
        .take()
        .expect("TUI stdin")
        .write_all(b"/echo \"tui value\"\n/quit\n")
        .expect("write TUI commands");
    let tui = tui.wait_with_output().expect("wait for scripted TUI");
    assert_success(&tui);
    assert!(
        String::from_utf8_lossy(&tui.stdout).contains(r#"["tui value"]"#),
        "stdout: {}",
        String::from_utf8_lossy(&tui.stdout)
    );
    assert_eq!(
        fs::read_to_string(home.join("extensions/data/example.echo/lifecycle.log"))
            .expect("TUI lifecycle log"),
        "initialize\nshutdown\ninitialize\nshutdown\n"
    );

    let disable = command(&home, &workspace)
        .args(["config", "extension", "example.echo", "--disable", "--json"])
        .output()
        .expect("disable Extension");
    assert_success(&disable);
    let disabled = command(&home, &workspace)
        .args(["echo"])
        .output()
        .expect("disabled dispatch");
    assert!(!disabled.status.success());
    assert!(String::from_utf8_lossy(&disabled.stderr).contains("unrecognized pevo command `echo`"));

    let enable = command(&home, &workspace)
        .args(["config", "extension", "example.echo", "--enable"])
        .output()
        .expect("enable Extension");
    assert_success(&enable);

    let remove = command(&home, &workspace)
        .args(["remove", "example.echo", "--json"])
        .output()
        .expect("remove Extension");
    assert_success(&remove);
    assert!(home.join("extensions/data/example.echo").is_dir());
    assert!(!home.join("extensions/records/example.echo.json").exists());
}

#[cfg(unix)]
#[test]
fn temporary_extension_dispatch_writes_no_install_record() {
    use std::os::unix::fs::PermissionsExt;

    let temp = tempdir().expect("temp");
    let home = init_tui_home(temp.path());
    let workspace = temp.path().join("workspace");
    let extension = temp.path().join("temporary-extension");
    fs::create_dir_all(&workspace).expect("workspace");
    fs::create_dir_all(&extension).expect("extension");
    fs::copy(
        concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../psychevo/tests/fixtures/extension_echo_sidecar.py"
        ),
        extension.join("sidecar"),
    )
    .expect("copy sidecar");
    let mut permissions = fs::metadata(extension.join("sidecar"))
        .expect("sidecar metadata")
        .permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(extension.join("sidecar"), permissions).expect("sidecar executable");
    fs::write(
        extension.join("psychevo.extension.json"),
        r#"{
          "schemaVersion": 1,
          "id": "example.temporary",
          "version": "local",
          "runtime": {"protocol": "psychevo-extension/1", "executable": "./sidecar"},
          "contributions": {"commands": [{"name": "temporary", "usage": "pevo temporary [ARGS]...", "summary": "fixture"}]}
        }"#,
    )
    .expect("manifest");

    let invoke = command(&home, &workspace)
        .args(["-e", extension.to_str().expect("path"), "temporary", "one"])
        .output()
        .expect("temporary Extension command");
    assert_success(&invoke);
    assert_eq!(String::from_utf8_lossy(&invoke.stdout).trim(), r#"["one"]"#);
    assert!(!home.join("extensions/records").exists());
}

#[cfg(unix)]
#[test]
fn bare_temporary_extension_loads_lazy_tui_command_without_installing() {
    use std::os::unix::fs::PermissionsExt;

    let temp = tempdir().expect("temp");
    let home = init_tui_home(temp.path());
    let workspace = temp.path().join("workspace");
    let extension = temp.path().join("temporary-tui-extension");
    fs::create_dir_all(&workspace).expect("workspace");
    fs::create_dir_all(&extension).expect("extension");
    fs::copy(
        concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../psychevo/tests/fixtures/extension_echo_sidecar.py"
        ),
        extension.join("sidecar"),
    )
    .expect("copy sidecar");
    let mut permissions = fs::metadata(extension.join("sidecar"))
        .expect("sidecar metadata")
        .permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(extension.join("sidecar"), permissions).expect("sidecar executable");
    fs::write(
        extension.join("psychevo.extension.json"),
        r#"{
          "schemaVersion": 1,
          "id": "example.temporary-tui",
          "version": "local",
          "runtime": {"protocol": "psychevo-extension/1", "executable": "./sidecar"},
          "contributions": {"commands": [{
            "name": "temporary-tui",
            "usage": "/temporary-tui [ARGS]...",
            "summary": "Temporary TUI fixture",
            "surfaces": ["tui"]
          }]}
        }"#,
    )
    .expect("manifest");

    let mut child = command(&home, &workspace)
        .args(["-e", extension.to_str().expect("path")])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn temporary Extension TUI");
    child
        .stdin
        .take()
        .expect("TUI stdin")
        .write_all(b"/help\n/temporary-tui one 'two words'\n/quit\n")
        .expect("write TUI commands");
    let output = child.wait_with_output().expect("wait for TUI");

    assert_success(&output);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Extensions"), "stdout: {stdout}");
    assert!(stdout.contains("/temporary-tui"), "stdout: {stdout}");
    assert!(
        stdout.contains(r#"["one", "two words"]"#),
        "stdout: {stdout}"
    );
    assert!(!home.join("extensions/records").exists());
}

fn command(home: &std::path::Path, workspace: &std::path::Path) -> std::process::Command {
    let mut command = pevo_cmd(home);
    command.env("PSYCHEVO_HOME", home).current_dir(workspace);
    command
}

fn assert_success(output: &std::process::Output) {
    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
