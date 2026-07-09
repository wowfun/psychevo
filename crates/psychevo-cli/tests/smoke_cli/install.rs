#[allow(unused_imports)]
pub(crate) use super::*;

pub(crate) fn install_workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("workspace root")
}

pub(crate) fn install_script_path() -> PathBuf {
    install_workspace_root().join("scripts/install.sh")
}

#[cfg(unix)]
fn write_fake_command(bin_dir: &Path, name: &str, body: &str) {
    std::fs::create_dir_all(bin_dir).expect("fake bin");
    let path = bin_dir.join(name);
    std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).expect("fake command");
    {
        use std::os::unix::fs::PermissionsExt;

        let mut permissions = std::fs::metadata(&path)
            .expect("fake command metadata")
            .permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&path, permissions).expect("chmod fake command");
    }
}

#[cfg(unix)]
fn write_fake_pevo(home: &Path) {
    write_fake_command(&home.join(".cargo/bin"), "pevo", "exit 0");
}

#[cfg(unix)]
fn install_preflight_command(bin_dir: &Path, home: &Path) -> Command {
    let mut command = Command::new("/bin/sh");
    command
        .arg(install_script_path())
        .current_dir(install_workspace_root())
        .env_clear()
        .env("HOME", home)
        .env("PATH", bin_dir);
    command
}

#[test]
pub(crate) fn install_rejects_removed_options() {
    for flag in [
        "--repo-url",
        "--ref",
        "--source",
        "--no-web",
        "--no-init",
        "--offline",
        "--web-dist",
        "--dry-run",
    ] {
        let output = Command::new("sh")
            .arg(install_script_path())
            .arg(flag)
            .output()
            .expect("install option");

        assert!(!output.status.success(), "{flag}");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains(&format!("unknown option: {flag}")),
            "{stderr}"
        );
    }
}

#[test]
pub(crate) fn install_requires_checkout_cwd() {
    let temp = tempdir().expect("temp");
    let output = Command::new("sh")
        .arg(install_script_path())
        .current_dir(temp.path())
        .output()
        .expect("install");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Run this script from inside a Psychevo checkout"),
        "{stderr}"
    );
    assert!(
        stderr.contains("git clone https://github.com/wowfun/psychevo.git"),
        "{stderr}"
    );
    assert!(!stderr.contains("checking Cargo"), "{stderr}");
}

#[cfg(unix)]
#[test]
pub(crate) fn install_check_reports_missing_tools_without_mutation() {
    let temp = tempdir().expect("temp");
    let bin = temp.path().join("bin");
    std::fs::create_dir_all(&bin).expect("bin");
    let output = Command::new("/bin/sh")
        .arg(install_script_path())
        .arg("--check")
        .current_dir(install_workspace_root())
        .env_clear()
        .env("HOME", temp.path().join("home"))
        .env("PATH", &bin)
        .output()
        .expect("install check");

    assert!(!output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("pevo install check"), "{stdout}");
    assert!(stdout.contains("cargo: missing"), "{stdout}");
    assert!(stdout.contains("node: missing"), "{stdout}");
    assert!(stdout.contains("pnpm: missing"), "{stdout}");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("CARGO_HTTP_CHECK_REVOKE: (unset)"),
        "{stderr}"
    );
    assert!(
        stderr.contains("CARGO_HTTP_TIMEOUT: 120 (installer default for cargo install)"),
        "{stderr}"
    );
    assert!(
        stderr.contains("CARGO_NET_RETRY: 10 (installer default for cargo install)"),
        "{stderr}"
    );
    assert!(
        stderr.contains("CARGO_HTTP_LOW_SPEED_LIMIT: (unset)"),
        "{stderr}"
    );
    assert!(
        stderr.contains("CARGO_HTTP_MULTIPLEXING: (unset)"),
        "{stderr}"
    );
    assert!(
        !temp.path().join("home/.cargo/bin/pevo").exists(),
        "check mode must not install pevo"
    );
}

#[cfg(unix)]
#[test]
pub(crate) fn install_check_reports_mismatched_pnpm_as_warning() {
    let temp = tempdir().expect("temp");
    let bin = temp.path().join("bin");
    write_fake_command(&bin, "cargo", "printf 'cargo 1.96.0\\n'");
    write_fake_command(&bin, "rustc", "printf 'rustc 1.94.0\\n'");
    write_fake_command(&bin, "cc", "exit 0");
    write_fake_command(&bin, "node", "printf 'v24.0.0\\n'");
    write_fake_command(
        &bin,
        "pnpm",
        "case \"$1\" in\n  --version) printf '1.0.0\\n'; exit 0 ;;\n  config) printf 'https://registry.npmjs.org/\\n'; exit 0 ;;\n  *) exit 0 ;;\nesac",
    );
    let output = Command::new("/bin/sh")
        .arg(install_script_path())
        .arg("--check")
        .current_dir(install_workspace_root())
        .env_clear()
        .env("HOME", temp.path().join("home"))
        .env("PATH", &bin)
        .output()
        .expect("install check");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("pnpm: warn - found 1.0.0, recommended 11.8.0"),
        "{stdout}"
    );
}

#[cfg(unix)]
#[test]
pub(crate) fn install_preflight_reports_missing_native_compiler() {
    let temp = tempdir().expect("temp");
    let bin = temp.path().join("bin");
    write_fake_command(&bin, "cargo", "exit 0");
    write_fake_command(&bin, "rustc", "printf 'rustc 1.94.0\\n'");
    let output = install_preflight_command(&bin, &temp.path().join("home"))
        .output()
        .expect("install preflight");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("native C compiler/linker is required"),
        "{stderr}"
    );
    assert!(
        stderr.contains("cc, gcc, or clang") || stderr.contains("build-essential"),
        "{stderr}"
    );
    assert!(
        stderr.contains("cargo xtask doctor deps check --only install"),
        "{stderr}"
    );
}

#[cfg(unix)]
#[test]
pub(crate) fn install_preflight_reports_missing_node_for_full_install() {
    let temp = tempdir().expect("temp");
    let bin = temp.path().join("bin");
    write_fake_command(&bin, "cargo", "exit 0");
    write_fake_command(&bin, "rustc", "printf 'rustc 1.94.0\\n'");
    write_fake_command(&bin, "cc", "exit 0");
    let output = install_preflight_command(&bin, &temp.path().join("home"))
        .output()
        .expect("install preflight");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Node.js is required to build Workbench assets"),
        "{stderr}"
    );
    assert!(!stderr.contains("--no-web"), "{stderr}");
    assert!(
        stderr.contains("cargo xtask doctor deps check --only install"),
        "{stderr}"
    );
}

#[cfg(unix)]
#[test]
pub(crate) fn install_preflight_reports_missing_pnpm_for_full_install() {
    let temp = tempdir().expect("temp");
    let bin = temp.path().join("bin");
    write_fake_command(&bin, "cargo", "exit 0");
    write_fake_command(&bin, "rustc", "printf 'rustc 1.94.0\\n'");
    write_fake_command(&bin, "cc", "exit 0");
    write_fake_command(&bin, "node", "printf 'v24.0.0\\n'");
    let output = install_preflight_command(&bin, &temp.path().join("home"))
        .output()
        .expect("install preflight");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("pnpm 11.8.0 is required to build Workbench assets"),
        "{stderr}"
    );
    assert!(!stderr.contains("--no-web"), "{stderr}");
    assert!(
        stderr.contains("cargo xtask doctor deps check --only install"),
        "{stderr}"
    );
}

#[cfg(unix)]
#[test]
pub(crate) fn install_preflight_prints_progress_breadcrumbs() {
    let temp = tempdir().expect("temp");
    let bin = temp.path().join("bin");
    write_fake_command(
        &bin,
        "cargo",
        "case \"$1\" in\n  --version) printf 'cargo 1.96.0\\n'; exit 0 ;;\n  install) printf 'fake cargo reached\\n' >&2; exit 42 ;;\n  *) exit 0 ;;\nesac",
    );
    write_fake_command(
        &bin,
        "rustc",
        "case \"$1\" in\n  --version) printf 'rustc 1.94.0\\n'; exit 0 ;;\n  *) exit 0 ;;\nesac",
    );
    write_fake_command(&bin, "cc", "exit 0");
    write_fake_command(&bin, "node", "printf 'v24.0.0\\n'");
    write_fake_command(
        &bin,
        "pnpm",
        "case \"$1\" in\n  --version) printf '11.8.0\\n'; exit 0 ;;\n  config) printf 'https://registry.npmjs.org/\\n'; exit 0 ;;\n  *) exit 0 ;;\nesac",
    );

    let output = install_preflight_command(&bin, &temp.path().join("home"))
        .output()
        .expect("install preflight");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("pevo install: using unix source checkout at"),
        "{stderr}"
    );
    assert!(
        stderr.contains("pevo install: validating source checkout"),
        "{stderr}"
    );
    assert!(stderr.contains("pevo install: checking Cargo"), "{stderr}");
    assert!(
        stderr.contains("pevo install: checking Rust version"),
        "{stderr}"
    );
    assert!(
        stderr.contains("pevo install: checking native build tools"),
        "{stderr}"
    );
    assert!(
        stderr.contains("pevo install: checking Node.js"),
        "{stderr}"
    );
    assert!(stderr.contains("pevo install: checking pnpm"), "{stderr}");
    assert!(stderr.contains("pevo install: installing pevo"), "{stderr}");
    assert!(
        stderr.contains("pevo install: collecting enterprise diagnostics"),
        "{stderr}"
    );
}

#[cfg(unix)]
#[test]
pub(crate) fn install_preflight_warns_for_mismatched_pnpm_and_continues() {
    let temp = tempdir().expect("temp");
    let bin = temp.path().join("bin");
    let home = temp.path().join("home");
    write_fake_pevo(&home);
    write_fake_command(&bin, "cargo", "exit 0");
    write_fake_command(&bin, "rustc", "printf 'rustc 1.94.0\\n'");
    write_fake_command(&bin, "cc", "exit 0");
    write_fake_command(&bin, "node", "printf 'v24.0.0\\n'");
    write_fake_command(
        &bin,
        "pnpm",
        "case \"$1\" in\n  --version) printf '1.0.0\\n'; exit 0 ;;\n  config) printf 'https://registry.npmjs.org/\\n'; exit 0 ;;\n  *) printf 'fake pnpm reached\\n' >&2; exit 42 ;;\nesac",
    );
    let output = install_preflight_command(&bin, &home)
        .output()
        .expect("install preflight");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("warning: pnpm 1.0.0 is installed; pnpm 11.8.0 is recommended"),
        "{stderr}"
    );
    assert!(stderr.contains("fake pnpm reached"), "{stderr}");
    assert!(
        stderr.contains("Enterprise network diagnostics (pnpm install failed)"),
        "{stderr}"
    );
}

#[cfg(unix)]
#[test]
pub(crate) fn install_preflight_bypasses_corepack_project_spec_for_pnpm() {
    let temp = tempdir().expect("temp");
    let bin = temp.path().join("bin");
    let home = temp.path().join("home");
    write_fake_pevo(&home);
    write_fake_command(&bin, "cargo", "exit 0");
    write_fake_command(&bin, "rustc", "printf 'rustc 1.94.0\\n'");
    write_fake_command(&bin, "cc", "exit 0");
    write_fake_command(&bin, "node", "printf 'v24.0.0\\n'");
    write_fake_command(
        &bin,
        "pnpm",
        "if [ \"${COREPACK_ENABLE_PROJECT_SPEC:-}\" != 0 ]; then printf 'corepack attempted project download\\n' >&2; exit 42; fi\nif [ \"${COREPACK_ENABLE_DOWNLOAD_PROMPT:-}\" != 0 ]; then printf 'corepack prompted\\n' >&2; exit 42; fi\ncase \"$1\" in\n  --version) printf '1.0.0\\n'; exit 0 ;;\n  config) printf 'https://registry.npmjs.org/\\n'; exit 0 ;;\n  install) printf 'fake pnpm install reached\\n' >&2; exit 42 ;;\n  *) exit 0 ;;\nesac",
    );
    let output = install_preflight_command(&bin, &home)
        .output()
        .expect("install preflight");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("warning: pnpm 1.0.0 is installed; pnpm 11.8.0 is recommended"),
        "{stderr}"
    );
    assert!(stderr.contains("fake pnpm install reached"), "{stderr}");
    assert!(
        !stderr.contains("corepack attempted project download"),
        "{stderr}"
    );
    assert!(!stderr.contains("corepack prompted"), "{stderr}");
}

#[cfg(unix)]
#[test]
pub(crate) fn install_preflight_rejects_unusable_pnpm_before_cargo_install() {
    let temp = tempdir().expect("temp");
    let bin = temp.path().join("bin");
    write_fake_command(
        &bin,
        "cargo",
        "case \"$1\" in\n  --version) printf 'cargo 1.96.0\\n'; exit 0 ;;\n  install) printf 'fake cargo reached\\n' >&2; exit 42 ;;\n  *) exit 0 ;;\nesac",
    );
    write_fake_command(&bin, "rustc", "printf 'rustc 1.94.0\\n'");
    write_fake_command(&bin, "cc", "exit 0");
    write_fake_command(&bin, "node", "printf 'v24.0.0\\n'");
    write_fake_command(
        &bin,
        "pnpm",
        "case \"$1\" in\n  --version) printf 'corepack certificate failure\\n' >&2; exit 42 ;;\n  config) printf 'https://registry.npmjs.org/\\n'; exit 0 ;;\n  *) exit 42 ;;\nesac",
    );

    let output = install_preflight_command(&bin, &temp.path().join("home"))
        .output()
        .expect("install preflight");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("corepack certificate failure"), "{stderr}");
    assert!(
        stderr.contains("pnpm exists on PATH, but `pnpm --version` failed"),
        "{stderr}"
    );
    assert!(!stderr.contains("--no-web"), "{stderr}");
    assert!(!stderr.contains("fake cargo reached"), "{stderr}");
}

#[cfg(unix)]
#[test]
pub(crate) fn install_check_reports_unusable_pnpm_as_failure() {
    let temp = tempdir().expect("temp");
    let bin = temp.path().join("bin");
    write_fake_command(&bin, "cargo", "printf 'cargo 1.96.0\\n'");
    write_fake_command(&bin, "rustc", "printf 'rustc 1.94.0\\n'");
    write_fake_command(&bin, "cc", "exit 0");
    write_fake_command(&bin, "node", "printf 'v24.0.0\\n'");
    write_fake_command(
        &bin,
        "pnpm",
        "case \"$1\" in\n  --version) printf 'corepack certificate failure\\n' >&2; exit 42 ;;\n  config) printf 'https://registry.npmjs.org/\\n'; exit 0 ;;\n  *) exit 42 ;;\nesac",
    );
    let output = Command::new("/bin/sh")
        .arg(install_script_path())
        .arg("--check")
        .current_dir(install_workspace_root())
        .env_clear()
        .env("HOME", temp.path().join("home"))
        .env("PATH", &bin)
        .output()
        .expect("install check");

    assert!(!output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stdout.contains("pnpm: unusable - pnpm --version failed"),
        "{stdout}"
    );
    assert!(stderr.contains("corepack certificate failure"), "{stderr}");
}

#[cfg(unix)]
#[test]
pub(crate) fn install_windows_preflight_reports_missing_build_tools_before_cargo() {
    let temp = tempdir().expect("temp");
    let bin = temp.path().join("bin");
    write_fake_command(&bin, "uname", "printf 'MINGW64_NT-10.0\\n'");
    write_fake_command(&bin, "cargo", "printf 'fake cargo reached\\n' >&2\nexit 42");
    write_fake_command(&bin, "rustc", "printf 'rustc 1.94.0\\n'");
    let output = install_preflight_command(&bin, &temp.path().join("home"))
        .output()
        .expect("install preflight");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Windows native C/C++ build tools are required"),
        "{stderr}"
    );
    assert!(!stderr.contains("fake cargo reached"), "{stderr}");
}

#[cfg(unix)]
#[test]
pub(crate) fn install_windows_cargo_install_defaults_revocation_check_off() {
    let temp = tempdir().expect("temp");
    let bin = temp.path().join("bin");
    write_fake_command(&bin, "uname", "printf 'MINGW64_NT-10.0\\n'");
    write_fake_command(
        &bin,
        "tee",
        "out=$1\n: > \"$out\"\nwhile IFS= read -r line || [ -n \"$line\" ]; do\n  printf '%s\\n' \"$line\"\n  printf '%s\\n' \"$line\" >> \"$out\"\ndone",
    );
    write_fake_command(
        &bin,
        "cargo",
        "case \"$1\" in\n  --version) printf 'cargo 1.96.0\\n'; exit 0 ;;\n  install) printf 'cargo revoke=%s\\n' \"${CARGO_HTTP_CHECK_REVOKE-unset}\" >&2; [ \"${CARGO_HTTP_CHECK_REVOKE:-}\" = false ] || exit 43; exit 42 ;;\n  *) exit 0 ;;\nesac",
    );
    write_fake_command(&bin, "rustc", "printf 'rustc 1.94.0\\n'");
    write_fake_command(&bin, "cc", "exit 0");
    write_fake_command(&bin, "node", "printf 'v24.0.0\\n'");
    write_fake_command(
        &bin,
        "pnpm",
        "case \"$1\" in\n  --version) printf '11.8.0\\n'; exit 0 ;;\n  config) printf 'https://registry.npmjs.org/\\n'; exit 0 ;;\n  *) exit 0 ;;\nesac",
    );
    let output = install_preflight_command(&bin, &temp.path().join("home"))
        .output()
        .expect("install cargo");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("cargo revoke=false"), "{stderr}");
    assert!(
        stderr.contains("try CARGO_HTTP_MULTIPLEXING=false"),
        "{stderr}"
    );
}

#[cfg(unix)]
#[test]
pub(crate) fn install_windows_cargo_install_preserves_explicit_revocation_setting() {
    let temp = tempdir().expect("temp");
    let bin = temp.path().join("bin");
    write_fake_command(&bin, "uname", "printf 'MINGW64_NT-10.0\\n'");
    write_fake_command(
        &bin,
        "cargo",
        "case \"$1\" in\n  --version) printf 'cargo 1.96.0\\n'; exit 0 ;;\n  install) printf 'cargo revoke=%s\\n' \"${CARGO_HTTP_CHECK_REVOKE-unset}\" >&2; [ \"${CARGO_HTTP_CHECK_REVOKE:-}\" = true ] || exit 43; exit 42 ;;\n  *) exit 0 ;;\nesac",
    );
    write_fake_command(&bin, "rustc", "printf 'rustc 1.94.0\\n'");
    write_fake_command(&bin, "cc", "exit 0");
    write_fake_command(&bin, "node", "printf 'v24.0.0\\n'");
    write_fake_command(
        &bin,
        "pnpm",
        "case \"$1\" in\n  --version) printf '11.8.0\\n'; exit 0 ;;\n  config) printf 'https://registry.npmjs.org/\\n'; exit 0 ;;\n  *) exit 0 ;;\nesac",
    );
    let output = install_preflight_command(&bin, &temp.path().join("home"))
        .env("CARGO_HTTP_CHECK_REVOKE", "true")
        .output()
        .expect("install cargo");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("cargo revoke=true"), "{stderr}");
}

#[cfg(unix)]
#[test]
pub(crate) fn install_windows_locked_pevo_exe_failure_gets_targeted_guidance() {
    let temp = tempdir().expect("temp");
    let bin = temp.path().join("bin");
    write_fake_command(&bin, "uname", "printf 'MINGW64_NT-10.0\\n'");
    write_fake_command(
        &bin,
        "cargo",
        "case \"$1\" in\n  --version) printf 'cargo 1.96.0\\n'; exit 0 ;;\n  install) printf '   Replacing C:\\\\Users\\\\c00845592\\\\.cargo\\\\bin\\\\pevo.exe\\n' >&2; printf 'error: failed to move `C:\\\\Users\\\\c00845592\\\\.cargo\\\\bin\\\\cargo-installU8ZJRb\\\\pevo.exe` to `C:\\\\Users\\\\c00845592\\\\.cargo\\\\bin\\\\pevo.exe`\\n\\nCaused by:\\n  Access is denied. (os error 5)\\n' >&2; exit 101 ;;\n  *) exit 0 ;;\nesac",
    );
    write_fake_command(&bin, "rustc", "printf 'rustc 1.94.0\\n'");
    write_fake_command(&bin, "cc", "exit 0");
    write_fake_command(&bin, "node", "printf 'v24.0.0\\n'");
    write_fake_command(
        &bin,
        "pnpm",
        "case \"$1\" in\n  --version) printf '11.8.0\\n'; exit 0 ;;\n  config) printf 'https://registry.npmjs.org/\\n'; exit 0 ;;\n  *) exit 0 ;;\nesac",
    );

    let output = install_preflight_command(&bin, &temp.path().join("home"))
        .output()
        .expect("install cargo");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr
            .contains("the installed pevo.exe could not be replaced because Windows denied access"),
        "{stderr}"
    );
    assert!(
        stderr.contains("Close running pevo, TUI, Web, Gateway, or serve processes"),
        "{stderr}"
    );
    assert!(
        !stderr.contains("Enterprise network diagnostics (cargo install failed)"),
        "{stderr}"
    );
    assert!(!stderr.contains("native C/C++ build tools"), "{stderr}");
    assert!(
        !stderr.contains("CARGO_HTTP_MULTIPLEXING=false"),
        "{stderr}"
    );
}

#[cfg(unix)]
#[test]
pub(crate) fn install_windows_preflight_stops_existing_managed_gateway() {
    let temp = tempdir().expect("temp");
    let bin = temp.path().join("bin");
    let home = temp.path().join("home");
    write_fake_command(&bin, "uname", "printf 'MINGW64_NT-10.0\\n'");
    write_fake_command(
        &home.join(".cargo/bin"),
        "pevo.exe",
        "printf '%s\\n' \"$*\" >> \"$HOME/gateway-stop.log\"\nexit 0",
    );
    write_fake_command(
        &bin,
        "cargo",
        "case \"$1\" in\n  --version) printf 'cargo 1.96.0\\n'; exit 0 ;;\n  install) printf 'fake cargo failed\\n' >&2; exit 42 ;;\n  *) exit 0 ;;\nesac",
    );
    write_fake_command(&bin, "rustc", "printf 'rustc 1.94.0\\n'");
    write_fake_command(&bin, "cc", "exit 0");
    write_fake_command(&bin, "node", "printf 'v24.0.0\\n'");
    write_fake_command(
        &bin,
        "pnpm",
        "case \"$1\" in\n  --version) printf '11.8.0\\n'; exit 0 ;;\n  config) printf 'https://registry.npmjs.org/\\n'; exit 0 ;;\n  *) exit 0 ;;\nesac",
    );

    let output = install_preflight_command(&bin, &home)
        .output()
        .expect("install cargo");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("pevo install: stopping existing managed Gateway"),
        "{stderr}"
    );
    assert!(
        stderr.contains("Enterprise network diagnostics (cargo install failed)"),
        "{stderr}"
    );
    let stop_log = std::fs::read_to_string(home.join("gateway-stop.log")).expect("stop log");
    assert_eq!(stop_log.trim(), "gateway stop");
}

#[cfg(unix)]
#[test]
pub(crate) fn install_unix_cargo_install_does_not_force_revocation_setting() {
    let temp = tempdir().expect("temp");
    let bin = temp.path().join("bin");
    write_fake_command(
        &bin,
        "cargo",
        "case \"$1\" in\n  --version) printf 'cargo 1.96.0\\n'; exit 0 ;;\n  install) printf 'cargo revoke=%s\\n' \"${CARGO_HTTP_CHECK_REVOKE-unset}\" >&2; [ -z \"${CARGO_HTTP_CHECK_REVOKE+x}\" ] || exit 43; exit 42 ;;\n  *) exit 0 ;;\nesac",
    );
    write_fake_command(&bin, "rustc", "printf 'rustc 1.94.0\\n'");
    write_fake_command(&bin, "cc", "exit 0");
    write_fake_command(&bin, "node", "printf 'v24.0.0\\n'");
    write_fake_command(
        &bin,
        "pnpm",
        "case \"$1\" in\n  --version) printf '11.8.0\\n'; exit 0 ;;\n  config) printf 'https://registry.npmjs.org/\\n'; exit 0 ;;\n  *) exit 0 ;;\nesac",
    );
    let output = install_preflight_command(&bin, &temp.path().join("home"))
        .output()
        .expect("install cargo");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("cargo revoke=unset"), "{stderr}");
}

#[cfg(unix)]
#[test]
pub(crate) fn install_cargo_install_defaults_timeout_and_retry() {
    let temp = tempdir().expect("temp");
    let bin = temp.path().join("bin");
    write_fake_command(
        &bin,
        "cargo",
        "case \"$1\" in\n  --version) printf 'cargo 1.96.0\\n'; exit 0 ;;\n  install) printf 'cargo timeout=%s retry=%s\\n' \"${CARGO_HTTP_TIMEOUT-unset}\" \"${CARGO_NET_RETRY-unset}\" >&2; [ \"${CARGO_HTTP_TIMEOUT:-}\" = 120 ] || exit 43; [ \"${CARGO_NET_RETRY:-}\" = 10 ] || exit 44; exit 42 ;;\n  *) exit 0 ;;\nesac",
    );
    write_fake_command(&bin, "rustc", "printf 'rustc 1.94.0\\n'");
    write_fake_command(&bin, "cc", "exit 0");
    write_fake_command(&bin, "node", "printf 'v24.0.0\\n'");
    write_fake_command(
        &bin,
        "pnpm",
        "case \"$1\" in\n  --version) printf '11.8.0\\n'; exit 0 ;;\n  config) printf 'https://registry.npmjs.org/\\n'; exit 0 ;;\n  *) exit 0 ;;\nesac",
    );
    let output = install_preflight_command(&bin, &temp.path().join("home"))
        .output()
        .expect("install cargo");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("cargo timeout=120 retry=10"), "{stderr}");
}

#[cfg(unix)]
#[test]
pub(crate) fn install_cargo_install_preserves_explicit_timeout_and_retry() {
    let temp = tempdir().expect("temp");
    let bin = temp.path().join("bin");
    write_fake_command(
        &bin,
        "cargo",
        "case \"$1\" in\n  --version) printf 'cargo 1.96.0\\n'; exit 0 ;;\n  install) printf 'cargo timeout=%s retry=%s\\n' \"${CARGO_HTTP_TIMEOUT-unset}\" \"${CARGO_NET_RETRY-unset}\" >&2; [ \"${CARGO_HTTP_TIMEOUT:-}\" = 45 ] || exit 43; [ \"${CARGO_NET_RETRY:-}\" = 2 ] || exit 44; exit 42 ;;\n  *) exit 0 ;;\nesac",
    );
    write_fake_command(&bin, "rustc", "printf 'rustc 1.94.0\\n'");
    write_fake_command(&bin, "cc", "exit 0");
    write_fake_command(&bin, "node", "printf 'v24.0.0\\n'");
    write_fake_command(
        &bin,
        "pnpm",
        "case \"$1\" in\n  --version) printf '11.8.0\\n'; exit 0 ;;\n  config) printf 'https://registry.npmjs.org/\\n'; exit 0 ;;\n  *) exit 0 ;;\nesac",
    );
    let output = install_preflight_command(&bin, &temp.path().join("home"))
        .env("CARGO_HTTP_TIMEOUT", "45")
        .env("CARGO_NET_RETRY", "2")
        .output()
        .expect("install cargo");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("cargo timeout=45 retry=2"), "{stderr}");
}

#[cfg(unix)]
#[test]
pub(crate) fn install_cargo_failure_prints_enterprise_diagnostics() {
    let temp = tempdir().expect("temp");
    let bin = temp.path().join("bin");
    write_fake_command(
        &bin,
        "cargo",
        "case \"$1\" in\n  --version) printf 'cargo 1.96.0\\n'; exit 0 ;;\n  install) printf 'fake cargo failed\\n' >&2; exit 42 ;;\n  *) exit 0 ;;\nesac",
    );
    write_fake_command(&bin, "rustc", "printf 'rustc 1.94.0\\n'");
    write_fake_command(&bin, "cc", "exit 0");
    write_fake_command(&bin, "node", "printf 'v24.0.0\\n'");
    write_fake_command(
        &bin,
        "pnpm",
        "case \"$1\" in\n  --version) printf '11.8.0\\n'; exit 0 ;;\n  config) printf 'https://registry.npmjs.org/\\n'; exit 0 ;;\n  *) exit 0 ;;\nesac",
    );
    let output = install_preflight_command(&bin, &temp.path().join("home"))
        .env("HTTPS_PROXY", "http://user:pass@example.proxy:8080")
        .output()
        .expect("install cargo");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("fake cargo failed"), "{stderr}");
    assert!(
        stderr.contains("Enterprise network diagnostics (cargo install failed)"),
        "{stderr}"
    );
    assert!(
        stderr.contains("HTTPS_PROXY: http://***@example.proxy:8080"),
        "{stderr}"
    );
    assert!(
        stderr.contains("CARGO_HTTP_TIMEOUT: 120 (installer default for cargo install)"),
        "{stderr}"
    );
    assert!(
        stderr.contains("CARGO_NET_RETRY: 10 (installer default for cargo install)"),
        "{stderr}"
    );
    assert!(!stderr.contains("repo_url:"), "{stderr}");
}
