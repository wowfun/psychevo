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
        .arg("--source")
        .arg(install_workspace_root())
        .arg("--no-init")
        .env_clear()
        .env("HOME", home)
        .env("PATH", bin_dir)
        .env("PEVO_INSTALL_UNAME", "Linux");
    command
}

#[test]
pub(crate) fn install_dry_run_uses_explicit_source_and_default_init() {
    let temp = tempdir().expect("temp");
    let home = temp.path().join("home");
    let root = install_workspace_root();
    let output = Command::new("sh")
        .arg(install_script_path())
        .arg("--dry-run")
        .arg("--source")
        .arg(&root)
        .env("HOME", &home)
        .env_remove("CARGO_HOME")
        .env_remove("CARGO_INSTALL_ROOT")
        .output()
        .expect("install dry run");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stderr).is_empty(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("stdout");
    let source = root.display().to_string();
    assert!(stdout.contains("pevo install dry run"), "{stdout}");
    assert!(stdout.contains("mode: local"), "{stdout}");
    assert!(stdout.contains(&format!("source: {source}")), "{stdout}");
    assert!(
        stdout.contains(&format!(
            "install_command: cargo install --locked --path '{source}/crates/psychevo-cli' --force"
        )),
        "{stdout}"
    );
    assert!(stdout.contains("with_peval: 0"), "{stdout}");
    assert!(stdout.contains("with_web: 1"), "{stdout}");
    assert!(
        stdout.contains(&format!(
            "web_install_command: cd '{source}' && pnpm install --frozen-lockfile"
        )),
        "{stdout}"
    );
    assert!(
        stdout.contains(&format!(
            "web_build_command: cd '{source}' && pnpm --filter @psychevo/workbench build"
        )),
        "{stdout}"
    );
    assert!(
        stdout.contains(&format!("web_asset_source: {source}/apps/workbench/dist")),
        "{stdout}"
    );
    assert!(
        stdout.contains(&format!(
            "init_command: '{}/.cargo/bin/pevo' init",
            home.display()
        )),
        "{stdout}"
    );
}

#[test]
pub(crate) fn install_dry_run_can_skip_web_assets() {
    let temp = tempdir().expect("temp");
    let home = temp.path().join("home");
    let root = install_workspace_root();
    let output = Command::new("sh")
        .arg(install_script_path())
        .arg("--dry-run")
        .arg("--no-web")
        .arg("--source")
        .arg(&root)
        .env("HOME", &home)
        .env_remove("CARGO_HOME")
        .env_remove("CARGO_INSTALL_ROOT")
        .output()
        .expect("install dry run");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("stdout");
    assert!(stdout.contains("with_web: 0"), "{stdout}");
    assert!(stdout.contains("web_asset_install: (skipped)"), "{stdout}");
    assert!(!stdout.contains("web_build_command:"), "{stdout}");
}

#[test]
pub(crate) fn install_dry_run_offline_plans_offline_commands() {
    let temp = tempdir().expect("temp");
    let home = temp.path().join("home");
    let root = install_workspace_root();
    let output = Command::new("sh")
        .arg(install_script_path())
        .arg("--dry-run")
        .arg("--offline")
        .arg("--source")
        .arg(&root)
        .env("HOME", &home)
        .env_remove("CARGO_HOME")
        .env_remove("CARGO_INSTALL_ROOT")
        .output()
        .expect("install dry run");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("stdout");
    let source = root.display().to_string();
    assert!(stdout.contains("offline: 1"), "{stdout}");
    assert!(
        stdout.contains(&format!(
            "install_command: cargo install --locked --offline --path '{source}/crates/psychevo-cli' --force"
        )),
        "{stdout}"
    );
    assert!(
        stdout.contains(&format!(
            "web_install_command: cd '{source}' && pnpm install --frozen-lockfile --offline"
        )),
        "{stdout}"
    );
    assert!(
        stdout.contains(&format!(
            "web_build_command: cd '{source}' && pnpm --offline --filter @psychevo/workbench build"
        )),
        "{stdout}"
    );
}

#[test]
pub(crate) fn install_offline_rejects_clone_mode() {
    let temp = tempdir().expect("temp");
    let output = Command::new("sh")
        .arg(install_script_path())
        .arg("--dry-run")
        .arg("--offline")
        .current_dir(temp.path())
        .output()
        .expect("install dry run");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("--offline requires an existing Psychevo source checkout"),
        "{stderr}"
    );
}

#[test]
pub(crate) fn install_dry_run_web_dist_skips_pnpm_commands() {
    let temp = tempdir().expect("temp");
    let home = temp.path().join("home");
    let dist = temp.path().join("dist");
    std::fs::create_dir_all(&dist).expect("dist");
    std::fs::write(dist.join("index.html"), "<!doctype html>").expect("index");
    let root = install_workspace_root();
    let output = Command::new("sh")
        .arg(install_script_path())
        .arg("--dry-run")
        .arg("--source")
        .arg(&root)
        .arg("--web-dist")
        .arg(&dist)
        .env("HOME", &home)
        .env_remove("CARGO_HOME")
        .env_remove("CARGO_INSTALL_ROOT")
        .output()
        .expect("install dry run");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("stdout");
    assert!(
        stdout.contains("web_install_command: (skipped; --web-dist supplied)"),
        "{stdout}"
    );
    assert!(
        stdout.contains("web_build_command: (skipped; --web-dist supplied)"),
        "{stdout}"
    );
    assert!(
        stdout.contains(&format!("web_asset_source: {}", dist.display())),
        "{stdout}"
    );
    assert!(!stdout.contains("pnpm install"), "{stdout}");
}

#[test]
pub(crate) fn install_dry_run_can_include_peval() {
    let temp = tempdir().expect("temp");
    let home = temp.path().join("home");
    let root = install_workspace_root();
    let output = Command::new("sh")
        .arg(install_script_path())
        .arg("--dry-run")
        .arg("--with-peval")
        .arg("--source")
        .arg(&root)
        .env("HOME", &home)
        .env_remove("CARGO_HOME")
        .env_remove("CARGO_INSTALL_ROOT")
        .output()
        .expect("install dry run");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("stdout");
    let source = root.display().to_string();
    assert!(stdout.contains("with_peval: 1"), "{stdout}");
    assert!(
        stdout.contains(&format!(
            "peval_install_command: cargo install --locked --path '{source}/crates/psychevo-eval' --force"
        )),
        "{stdout}"
    );
    assert!(
        stdout.contains(&format!(
            "peval_binary: {}/.cargo/bin/peval",
            home.display()
        )),
        "{stdout}"
    );
    assert!(
        stdout.contains(&format!(
            "init_command: '{}/.cargo/bin/pevo' init",
            home.display()
        )),
        "{stdout}"
    );
}

#[test]
pub(crate) fn install_dry_run_plans_clone_mode_and_no_init() {
    let temp = tempdir().expect("temp");
    let home = temp.path().join("home");
    let output = Command::new("sh")
        .arg(install_script_path())
        .arg("--dry-run")
        .arg("--repo-url")
        .arg("https://example.invalid/psychevo.git")
        .arg("--ref")
        .arg("test-ref")
        .arg("--no-init")
        .current_dir(temp.path())
        .env("HOME", &home)
        .env_remove("CARGO_HOME")
        .env_remove("CARGO_INSTALL_ROOT")
        .output()
        .expect("install dry run");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("stdout");
    assert!(stdout.contains("mode: clone"), "{stdout}");
    assert!(
        stdout.contains("repo_url: https://example.invalid/psychevo.git"),
        "{stdout}"
    );
    assert!(stdout.contains("repo_ref: test-ref"), "{stdout}");
    assert!(stdout.contains("source: <temporary>/psychevo"), "{stdout}");
    assert!(
        stdout.contains(
            "clone_command: git clone --depth 1 --branch 'test-ref' 'https://example.invalid/psychevo.git' '<temporary>/psychevo'"
        ),
        "{stdout}"
    );
    assert!(stdout.contains("init_command: (skipped)"), "{stdout}");
}

#[cfg(unix)]
#[test]
pub(crate) fn install_preflight_reports_missing_native_compiler() {
    let temp = tempdir().expect("temp");
    let bin = temp.path().join("bin");
    write_fake_command(&bin, "cargo", "exit 0");
    write_fake_command(&bin, "rustc", "printf 'rustc 1.94.0\\n'");
    let output = install_preflight_command(&bin, &temp.path().join("home"))
        .arg("--no-web")
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
pub(crate) fn install_preflight_reports_missing_node_for_default_web_install() {
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
        stderr.contains("Node.js is required to build Web UI assets"),
        "{stderr}"
    );
    assert!(stderr.contains("--no-web"), "{stderr}");
    assert!(
        stderr.contains("cargo xtask doctor deps check --only install"),
        "{stderr}"
    );
}

#[cfg(unix)]
#[test]
pub(crate) fn install_preflight_reports_missing_pnpm_for_default_web_install() {
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
        stderr.contains("pnpm 11.8.0 is required to build Web UI assets"),
        "{stderr}"
    );
    assert!(stderr.contains("--no-web"), "{stderr}");
    assert!(
        stderr.contains("cargo xtask doctor deps check --only install"),
        "{stderr}"
    );
}

#[cfg(unix)]
#[test]
pub(crate) fn install_no_web_bypasses_node_and_pnpm_preflight() {
    let temp = tempdir().expect("temp");
    let bin = temp.path().join("bin");
    write_fake_command(&bin, "cargo", "printf 'fake cargo reached\\n' >&2\nexit 42");
    write_fake_command(&bin, "rustc", "printf 'rustc 1.94.0\\n'");
    write_fake_command(&bin, "cc", "exit 0");
    let output = install_preflight_command(&bin, &temp.path().join("home"))
        .arg("--no-web")
        .output()
        .expect("install preflight");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("fake cargo reached"), "{stderr}");
    assert!(
        stderr.contains("Enterprise network diagnostics (cargo install failed)"),
        "{stderr}"
    );
    assert!(
        !stderr.contains("Node.js is required") && !stderr.contains("pnpm is required"),
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
    write_fake_command(&bin, "corepack", "exit 0");
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
    assert!(
        !stderr.contains("corepack enable && corepack prepare pnpm@11.8.0 --activate"),
        "{stderr}"
    );
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
        .arg("--source")
        .arg(install_workspace_root())
        .env_clear()
        .env("HOME", temp.path().join("home"))
        .env("PATH", &bin)
        .env("PEVO_INSTALL_UNAME", "Linux")
        .output()
        .expect("install check");

    assert!(!output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("pevo install check"), "{stdout}");
    assert!(stdout.contains("cargo: missing"), "{stdout}");
    assert!(stdout.contains("node: missing"), "{stdout}");
    assert!(stdout.contains("pnpm: missing"), "{stdout}");
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
        .arg("--source")
        .arg(install_workspace_root())
        .env_clear()
        .env("HOME", temp.path().join("home"))
        .env("PATH", &bin)
        .env("PEVO_INSTALL_UNAME", "Linux")
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
pub(crate) fn install_windows_preflight_reports_missing_build_tools_before_cargo() {
    let temp = tempdir().expect("temp");
    let bin = temp.path().join("bin");
    write_fake_command(&bin, "cargo", "printf 'fake cargo reached\\n' >&2\nexit 42");
    write_fake_command(&bin, "rustc", "printf 'rustc 1.94.0\\n'");
    let output = Command::new("/bin/sh")
        .arg(install_script_path())
        .arg("--source")
        .arg(install_workspace_root())
        .arg("--no-web")
        .arg("--no-init")
        .env_clear()
        .env("HOME", temp.path().join("home"))
        .env("PATH", &bin)
        .env("PEVO_INSTALL_UNAME", "MINGW64_NT-10.0")
        .output()
        .expect("install preflight");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Windows native build tools were not detected"),
        "{stderr}"
    );
    assert!(!stderr.contains("fake cargo reached"), "{stderr}");
}

#[cfg(unix)]
#[test]
pub(crate) fn install_clone_failure_prints_enterprise_diagnostics() {
    let temp = tempdir().expect("temp");
    let bin = temp.path().join("bin");
    write_fake_command(
        &bin,
        "mktemp",
        "template=$2\nprintf '%s\\n' \"${template%/*}/pevo-install.fake\"",
    );
    write_fake_command(&bin, "git", "printf 'fake clone failed\\n' >&2\nexit 42");
    let output = Command::new("/bin/sh")
        .arg(install_script_path())
        .arg("--repo-url")
        .arg("https://example.invalid/psychevo.git")
        .arg("--ref")
        .arg("test-ref")
        .current_dir(temp.path())
        .env_clear()
        .env("HOME", temp.path().join("home"))
        .env("PATH", &bin)
        .env("HTTPS_PROXY", "http://user:pass@example.proxy:8080")
        .output()
        .expect("install clone");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Enterprise network diagnostics (git clone failed)"),
        "{stderr}"
    );
    assert!(
        stderr.contains("repo_url: https://example.invalid/psychevo.git"),
        "{stderr}"
    );
    assert!(
        stderr.contains("HTTPS_PROXY: http://***@example.proxy:8080"),
        "{stderr}"
    );
}

#[cfg(unix)]
#[test]
pub(crate) fn install_pnpm_failure_prints_enterprise_diagnostics() {
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
        "case \"$1\" in\n  --version) printf '11.8.0\\n'; exit 0 ;;\n  config) printf 'https://registry.npmjs.org/\\n'; exit 0 ;;\n  *) printf 'fake pnpm failed\\n' >&2; exit 42 ;;\nesac",
    );
    let output = install_preflight_command(&bin, &home)
        .env("HTTPS_PROXY", "http://proxy.example:8080")
        .output()
        .expect("install pnpm");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("fake pnpm failed"), "{stderr}");
    assert!(
        stderr.contains("Enterprise network diagnostics (pnpm install failed)"),
        "{stderr}"
    );
    assert!(
        stderr.contains("pnpm registry: https://registry.npmjs.org/"),
        "{stderr}"
    );
}

#[test]
pub(crate) fn install_dry_run_uses_windows_binary_name_for_git_bash() {
    let temp = tempdir().expect("temp");
    let home = temp.path().join("home");
    let root = install_workspace_root();
    let output = Command::new("sh")
        .arg(install_script_path())
        .arg("--dry-run")
        .arg("--source")
        .arg(&root)
        .current_dir(temp.path())
        .env("HOME", &home)
        .env("PEVO_INSTALL_UNAME", "MINGW64_NT-10.0")
        .env_remove("CARGO_HOME")
        .env_remove("CARGO_INSTALL_ROOT")
        .output()
        .expect("install dry run");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("stdout");
    assert!(stdout.contains("platform: windows-git-bash"), "{stdout}");
    assert!(
        stdout.contains(&format!(
            "pevo_binary: {}/.cargo/bin/pevo.exe",
            home.display()
        )),
        "{stdout}"
    );
    assert!(
        stdout.contains(&format!(
            "init_command: '{}/.cargo/bin/pevo.exe' init",
            home.display()
        )),
        "{stdout}"
    );
}
