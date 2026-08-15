use std::path::{Path, PathBuf};
use std::process::Command;
use tempfile::tempdir;

pub(crate) fn install_workspace_root() -> PathBuf {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("workspace root");
    psychevo::host_paths::normalized_native_path(&root)
}

pub(crate) fn install_script_path() -> PathBuf {
    install_workspace_root().join("scripts/install.sh")
}

fn write_fake_command(bin_dir: &Path, name: &str, body: &str) {
    std::fs::create_dir_all(bin_dir).expect("fake bin");
    let path = bin_dir.join(name);
    std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).expect("fake command");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        let mut permissions = std::fs::metadata(&path)
            .expect("fake command metadata")
            .permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&path, permissions).expect("chmod fake command");
    }
}

fn pevo_executable_name() -> &'static str {
    if cfg!(windows) { "pevo.exe" } else { "pevo" }
}

fn write_fake_pevo(home: &Path) {
    write_fake_command(&home.join(".cargo/bin"), pevo_executable_name(), "exit 0");
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn write_recording_fake_pevo(home: &Path, log: &Path) {
    let log = shell_quote(&shell_path(log));
    write_fake_command(
        &home.join(".cargo/bin"),
        pevo_executable_name(),
        &format!(
            "printf '%s\\n' \"$*\" >> {log}\nif [ \"${{1:-}}\" = install ] && [ \"${{2:-}}\" = --managed-local ]; then\n  [ -f \"$3/psychevo.extension.json\" ] || exit 41\n  /usr/bin/grep -q '^  \"version\": \"local\"' \"$3/psychevo.extension.json\" || exit 42\n  /usr/bin/grep -q '^      \"version\": \"nested\"' \"$3/psychevo.extension.json\" || exit 44\n  /usr/bin/grep -q '^      \"executable\": \"metadata-only\"' \"$3/psychevo.extension.json\" || exit 45\n  [ \"$(/usr/bin/find \"$3\" -type f | /usr/bin/wc -l)\" -ge 2 ] || exit 43\nfi"
        ),
    );
}

fn write_fake_web_install_prerequisites(bin_dir: &Path, home: &Path, pnpm_body: &str) {
    write_fake_pevo(home);
    write_fake_command(bin_dir, "cargo", "exit 0");
    write_fake_command(bin_dir, "rustc", "printf 'rustc 1.97.0\\n'");
    write_fake_command(bin_dir, "cc", "exit 0");
    write_fake_command(bin_dir, "node", "printf 'v24.0.0\\n'");
    write_fake_command(bin_dir, "pnpm", pnpm_body);
}

#[cfg(unix)]
fn install_shell() -> PathBuf {
    PathBuf::from("/bin/sh")
}

#[cfg(windows)]
fn install_shell() -> PathBuf {
    let runtime = psychevo::host_paths::GitBashRuntime::discover(
        &std::env::vars().collect::<std::collections::BTreeMap<_, _>>(),
    )
    .unwrap_or_else(|error| panic!("native Windows install tests require Git Bash: {error}"));
    let shell = runtime.cygpath.with_file_name("sh.exe");
    assert!(
        shell.is_file(),
        "native Windows install tests require Git Bash sh.exe at {}",
        shell.display()
    );
    shell
}

#[cfg(unix)]
fn shell_path(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

#[cfg(windows)]
fn shell_path(path: &Path) -> String {
    psychevo::host_paths::display_path_for_native_path(path)
}

#[cfg(unix)]
fn write_native_platform_commands(_bin_dir: &Path) {}

#[cfg(windows)]
fn write_native_platform_commands(bin_dir: &Path) {
    write_fake_command(bin_dir, "uname", "exec /usr/bin/uname \"$@\"");
}

fn install_command() -> Command {
    let mut command = Command::new(install_shell());
    command.arg(shell_path(&install_script_path()));
    command
}

fn install_preflight_command(bin_dir: &Path, home: &Path) -> Command {
    write_native_platform_commands(bin_dir);
    let runtime_tmp = home.join("tmp");
    std::fs::create_dir_all(&runtime_tmp).expect("runtime temp");
    let mut command = install_command();
    command
        .current_dir(install_workspace_root())
        .env_clear()
        .env("HOME", shell_path(home))
        .env("TEMP", shell_path(&runtime_tmp))
        .env("TMP", shell_path(&runtime_tmp))
        .env("TMPDIR", shell_path(&runtime_tmp))
        .env("PATH", shell_path(bin_dir));
    command
}

fn successful_install_command(checkout: &Path, bin_dir: &Path, home: &Path, log: &Path) -> Command {
    std::fs::create_dir_all(checkout.join("crates/psychevo-cli")).expect("cli crate");
    std::fs::write(
        checkout.join("Cargo.toml"),
        "[workspace.package]\nrust-version = \"1.97.0\"\n",
    )
    .expect("workspace manifest");
    std::fs::write(
        checkout.join("crates/psychevo-cli/Cargo.toml"),
        "[package]\nname = \"psychevo-cli\"\nversion = \"0.1.0\"\n",
    )
    .expect("cli manifest");
    std::fs::write(
        checkout.join("package.json"),
        "{\n  \"packageManager\": \"pnpm@11.8.0\"\n}\n",
    )
    .expect("package manifest");
    for (id, package, binary, channel) in [
        (
            "psychevo.channel.wechat",
            "psychevo-extension-channel-wechat",
            "psychevo-channel-wechat",
            "wechat",
        ),
        (
            "psychevo.channel.telegram",
            "psychevo-extension-channel-telegram",
            "psychevo-channel-telegram",
            "telegram",
        ),
        (
            "psychevo.channel.feishu-lark",
            "psychevo-extension-channel-feishu-lark",
            "psychevo-channel-feishu-lark",
            "feishu",
        ),
    ] {
        let crate_root = checkout.join("crates").join(package);
        std::fs::create_dir_all(&crate_root).expect("Channel Extension crate");
        std::fs::write(
            crate_root.join("psychevo.extension.json"),
            format!(
                "{{\n  \"schemaVersion\": 1,\n  \"id\": \"{id}\",\n  \"version\": \"0.1.0\",\n  \"runtime\": {{\n    \"protocol\": \"psychevo-extension/1\",\n    \"executable\": \"./{binary}\"\n  }},\n  \"contributions\": {{\n    \"channels\": [{{\n      \"channel\": \"{channel}\",\n      \"version\": \"nested\",\n      \"executable\": \"metadata-only\"\n    }}]\n  }}\n}}\n"
            ),
        )
        .expect("Channel Extension manifest");
    }

    write_native_platform_commands(bin_dir);
    write_recording_fake_pevo(home, log);
    let executable_suffix = if cfg!(windows) { ".exe" } else { "" };
    write_fake_command(
        bin_dir,
        "cargo",
        &format!(
            "case \"$1\" in\n  --version) printf 'cargo 1.97.0\\n' ;;\n  build) /usr/bin/mkdir -p target/release; for binary in psychevo-channel-wechat psychevo-channel-telegram psychevo-channel-feishu-lark; do printf 'fake Channel Extension\\n' > \"target/release/$binary{executable_suffix}\"; /usr/bin/chmod +x \"target/release/$binary{executable_suffix}\"; done ;;\n  *) exit 0 ;;\nesac"
        ),
    );
    write_fake_command(bin_dir, "rustc", "printf 'rustc 1.97.0\\n'");
    write_fake_command(bin_dir, "cc", "exit 0");
    write_fake_command(bin_dir, "node", "printf 'v24.0.0\\n'");
    write_fake_command(
        bin_dir,
        "pnpm",
        "case \"$1\" in\n  --version) printf '11.8.0\\n' ;;\n  --filter) /usr/bin/mkdir -p apps/workbench/dist; printf '<html></html>\\n' > apps/workbench/dist/index.html ;;\n  *) exit 0 ;;\nesac",
    );

    let runtime_tmp = home.join("tmp");
    std::fs::create_dir_all(&runtime_tmp).expect("runtime temp");
    let mut command = install_command();
    command
        .current_dir(checkout)
        .env_clear()
        .env("HOME", shell_path(home))
        .env("TEMP", shell_path(&runtime_tmp))
        .env("TMP", shell_path(&runtime_tmp))
        .env("TMPDIR", shell_path(&runtime_tmp))
        .env("PATH", format!("{}:/usr/bin:/bin", shell_path(bin_dir)));
    command
}

fn recorded_pevo_commands(log: &Path) -> Vec<String> {
    std::fs::read_to_string(log)
        .unwrap_or_default()
        .lines()
        .map(ToOwned::to_owned)
        .collect()
}

fn expected_pevo_commands(commands: &[&str]) -> Vec<String> {
    let mut expected = Vec::new();
    if cfg!(windows) {
        expected.push("gateway stop".to_owned());
    }
    expected.extend(commands.iter().map(|command| (*command).to_owned()));
    expected
}

fn assert_managed_channel_install_command(command: &str, id: &str) {
    assert!(
        command.starts_with("install --managed-local "),
        "unexpected install command: {command}"
    );
    assert!(
        command.contains("/share/psychevo/.extension-install-stage-"),
        "install must use private staging: {command}"
    );
    assert!(
        command.ends_with(&format!("/{id}")),
        "install must target {id}: {command}"
    );
}

fn assert_channel_staging_removed(home: &Path) {
    let share = home.join(".cargo/share/psychevo");
    let remaining = std::fs::read_dir(share)
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter(|name| name.starts_with(".extension-install-stage-"))
        .collect::<Vec<_>>();
    assert!(remaining.is_empty(), "stale staging roots: {remaining:?}");
}

#[tokio::test]
pub(crate) async fn install_rejects_removed_options() {
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
        let output = install_command()
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

#[tokio::test]
pub(crate) async fn install_non_interactive_skips_optional_channel_extensions() {
    let temp = tempdir().expect("temp");
    let checkout = temp.path().join("checkout");
    let bin = temp.path().join("bin");
    let home = temp.path().join("home");
    let log = temp.path().join("pevo.log");
    let output = successful_install_command(&checkout, &bin, &home, &log)
        .output()
        .expect("install");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("non-interactive input; skipping optional Channel integrations"),
        "{stderr}"
    );
    assert_eq!(
        recorded_pevo_commands(&log),
        expected_pevo_commands(&["--help", "init"])
    );
}

#[tokio::test]
pub(crate) async fn install_channels_flag_selects_all_subset_or_none() {
    let cases = [
        (
            "all",
            vec![
                ("psychevo.channel.wechat", "psychevo-channel-wechat"),
                ("psychevo.channel.telegram", "psychevo-channel-telegram"),
                (
                    "psychevo.channel.feishu-lark",
                    "psychevo-channel-feishu-lark",
                ),
            ],
        ),
        (
            "wechat,feishu-lark",
            vec![
                ("psychevo.channel.wechat", "psychevo-channel-wechat"),
                (
                    "psychevo.channel.feishu-lark",
                    "psychevo-channel-feishu-lark",
                ),
            ],
        ),
        ("none", vec![]),
        ("-1", vec![]),
    ];

    for (selection, selected_channels) in cases {
        let temp = tempdir().expect("temp");
        let checkout = temp.path().join("checkout");
        let bin = temp.path().join("bin");
        let home = temp.path().join("home");
        let log = temp.path().join("pevo.log");
        let output = successful_install_command(&checkout, &bin, &home, &log)
            .args(["--channels", selection])
            .output()
            .expect("install");

        assert!(
            output.status.success(),
            "selection {selection}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let expected = expected_pevo_commands(&["--help", "init"]);
        let commands = recorded_pevo_commands(&log);
        assert_eq!(&commands[..expected.len()], expected, "{selection}");
        let installs = &commands[expected.len()..];
        assert_eq!(installs.len(), selected_channels.len(), "{selection}");
        for (command, (id, _binary)) in installs.iter().zip(selected_channels) {
            assert_managed_channel_install_command(command, id);
        }
        assert!(
            commands
                .iter()
                .all(|command| !command.starts_with("install psychevo.channel.")),
            "source install must not resolve remote Extension ids: {commands:?}"
        );
        assert_channel_staging_removed(&home);
    }
}

#[tokio::test]
pub(crate) async fn install_channels_flag_rejects_invalid_or_check_combinations() {
    for args in [
        vec!["--channels", "discord"],
        vec!["--channels", "wechat*"],
        vec!["--channels=all", "--check"],
        vec!["--channels"],
    ] {
        let output = install_command()
            .current_dir(install_workspace_root())
            .args(&args)
            .output()
            .expect("install options");

        assert!(!output.status.success(), "{args:?}");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains("invalid --channels value")
                || stderr.contains("--channels cannot be combined with --check")
                || stderr.contains("--channels requires a value"),
            "{stderr}"
        );
        assert!(!stderr.contains("checking Cargo"), "{stderr}");
    }
}

#[tokio::test]
pub(crate) async fn install_rejects_invalid_channels_before_checkout_discovery() {
    let temp = tempdir().expect("temp");
    let output = install_command()
        .current_dir(temp.path())
        .args(["--channels", "discord"])
        .output()
        .expect("install options");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("invalid --channels value"), "{stderr}");
    assert!(!stderr.contains("Run this script from inside"), "{stderr}");
}

#[cfg(target_os = "linux")]
#[test]
fn install_interactive_channel_menu_selects_all_subset_or_none() {
    use std::io::Write as _;
    use std::process::Stdio;

    let cases = [
        (
            "\n",
            vec![
                "psychevo.channel.wechat",
                "psychevo.channel.telegram",
                "psychevo.channel.feishu-lark",
            ],
            false,
        ),
        (
            "1 3\n",
            vec!["psychevo.channel.wechat", "psychevo.channel.feishu-lark"],
            false,
        ),
        ("invalid\n2\n", vec!["psychevo.channel.telegram"], true),
        ("-1\n", vec![], false),
    ];

    for (input, selected_channels, expects_retry) in cases {
        let temp = tempdir().expect("temp");
        let checkout = temp.path().join("checkout");
        let bin = temp.path().join("bin");
        let home = temp.path().join("home");
        let log = temp.path().join("pevo.log");
        let install = successful_install_command(&checkout, &bin, &home, &log);
        let program = install.get_program().to_string_lossy();
        let arguments = install
            .get_args()
            .map(|argument| shell_quote(&argument.to_string_lossy()))
            .collect::<Vec<_>>()
            .join(" ");
        let child_command = format!("{} {arguments}", shell_quote(&program));

        let mut command = Command::new("script");
        command
            .args([
                "--quiet",
                "--return",
                "--command",
                &child_command,
                "/dev/null",
            ])
            .current_dir(install.get_current_dir().expect("checkout"))
            .env_clear()
            .envs(
                install.get_envs().filter_map(|(key, value)| {
                    value.map(|value| (key.to_owned(), value.to_owned()))
                }),
            )
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = command.spawn().expect("interactive install");
        child
            .stdin
            .take()
            .expect("interactive stdin")
            .write_all(input.as_bytes())
            .expect("selection input");
        let output = child.wait_with_output().expect("interactive output");

        assert!(
            output.status.success(),
            "stdout: {}\nstderr: {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        let expected = expected_pevo_commands(&["--help", "init"]);
        let commands = recorded_pevo_commands(&log);
        assert_eq!(&commands[..expected.len()], expected, "input {input:?}");
        let installs = &commands[expected.len()..];
        assert_eq!(installs.len(), selected_channels.len(), "input {input:?}");
        for (command, id) in installs.iter().zip(selected_channels) {
            assert_managed_channel_install_command(command, id);
        }
        assert_channel_staging_removed(&home);
        let terminal_output = String::from_utf8_lossy(&output.stdout);
        assert!(
            terminal_output.contains(
                "Selection (Enter installs all; use numbers like 1 3; -1 installs none):"
            ),
            "{terminal_output}"
        );
        assert!(
            terminal_output.contains("-1) Do not install Channel integrations"),
            "{terminal_output}"
        );
        assert!(
            !terminal_output.contains("Select integrations [all]"),
            "{terminal_output}"
        );
        assert_eq!(
            terminal_output.contains("Invalid selection."),
            expects_retry,
            "{terminal_output}"
        );
    }
}

#[tokio::test]
pub(crate) async fn install_requires_checkout_cwd() {
    let temp = tempdir().expect("temp");
    let output = install_command()
        .current_dir(psychevo::host_paths::normalized_native_path(temp.path()))
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

#[tokio::test]
pub(crate) async fn install_check_reports_missing_tools_without_mutation() {
    let temp = tempdir().expect("temp");
    let bin = temp.path().join("bin");
    std::fs::create_dir_all(&bin).expect("bin");
    let output = install_preflight_command(&bin, &temp.path().join("home"))
        .arg("--check")
        .output()
        .expect("install check");

    assert!(!output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("pevo install check"), "{stdout}");
    assert!(stdout.contains("cargo: missing"), "{stdout}");
    assert!(stdout.contains("node: missing"), "{stdout}");
    assert!(stdout.contains("pnpm: missing"), "{stdout}");
    let stderr = String::from_utf8_lossy(&output.stderr);
    let expected_revoke = if cfg!(windows) {
        "CARGO_HTTP_CHECK_REVOKE: false (installer default for cargo install)"
    } else {
        "CARGO_HTTP_CHECK_REVOKE: (unset)"
    };
    assert!(stderr.contains(expected_revoke), "{stderr}");
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
        !temp
            .path()
            .join("home/.cargo/bin")
            .join(pevo_executable_name())
            .exists(),
        "check mode must not install pevo"
    );
}

#[tokio::test]
pub(crate) async fn install_check_reports_mismatched_pnpm_as_warning() {
    let temp = tempdir().expect("temp");
    let bin = temp.path().join("bin");
    write_fake_command(&bin, "cargo", "printf 'cargo 1.97.0\\n'");
    write_fake_command(&bin, "rustc", "printf 'rustc 1.97.0\\n'");
    write_fake_command(&bin, "cc", "exit 0");
    write_fake_command(&bin, "node", "printf 'v24.0.0\\n'");
    write_fake_command(
        &bin,
        "pnpm",
        "if [ \"${pnpm_config_pm_on_fail:-}\" != warn ]; then printf '[ERROR] This project is configured to use 11.8.0 of pnpm. Your current pnpm is v11.10.0\\nCorepack invoked pnpm with this version, and pnpm does not switch versions when running under corepack.\\n' >&2; exit 42; fi\ncase \"$1\" in\n  --version) printf '11.10.0\\n'; exit 0 ;;\n  config) printf 'https://registry.npmjs.org/\\n'; exit 0 ;;\n  *) exit 0 ;;\nesac",
    );
    let output = install_preflight_command(&bin, &temp.path().join("home"))
        .arg("--check")
        .output()
        .expect("install check");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("pnpm: warn - found 11.10.0, recommended 11.8.0"),
        "{stdout}"
    );
}

#[tokio::test]
pub(crate) async fn install_check_enforces_rust_1_97_0_boundary() {
    for (version, accepted) in [
        ("1.96.0", false),
        ("1.96.1", false),
        ("1.97.0", true),
        ("1.98.0", true),
    ] {
        let temp = tempdir().expect("temp");
        let bin = temp.path().join("bin");
        write_fake_command(&bin, "cargo", "printf 'cargo 1.97.0\n'");
        write_fake_command(&bin, "rustc", &format!("printf 'rustc {version}\\n'"));
        write_fake_command(&bin, "cc", "exit 0");
        write_fake_command(&bin, "node", "printf 'v24.0.0\n'");
        write_fake_command(
            &bin,
            "pnpm",
            "case \"$1\" in\n  --version) printf '11.8.0\\n'; exit 0 ;;\n  config) printf 'https://registry.npmjs.org/\\n'; exit 0 ;;\n  *) exit 0 ;;\nesac",
        );

        let output = install_preflight_command(&bin, &temp.path().join("home"))
            .arg("--check")
            .output()
            .expect("install check");
        let stdout = String::from_utf8_lossy(&output.stdout);

        assert_eq!(output.status.success(), accepted, "{version}: {stdout}");
        if accepted {
            assert!(
                stdout.contains(&format!("rustc: ok - {version}, requires 1.97.0")),
                "{stdout}"
            );
        } else {
            assert!(
                stdout.contains(&format!(
                    "rustc: outdated - found {version}, requires 1.97.0"
                )),
                "{stdout}"
            );
        }
    }
}

#[cfg(unix)]
#[tokio::test]
pub(crate) async fn install_unix_preflight_reports_missing_native_compiler() {
    let temp = tempdir().expect("temp");
    let bin = temp.path().join("bin");
    write_fake_command(&bin, "cargo", "exit 0");
    write_fake_command(&bin, "rustc", "printf 'rustc 1.97.0\\n'");
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

#[tokio::test]
pub(crate) async fn install_preflight_reports_missing_node_for_full_install() {
    let temp = tempdir().expect("temp");
    let bin = temp.path().join("bin");
    write_fake_command(&bin, "cargo", "exit 0");
    write_fake_command(&bin, "rustc", "printf 'rustc 1.97.0\\n'");
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

#[tokio::test]
pub(crate) async fn install_preflight_reports_missing_pnpm_for_full_install() {
    let temp = tempdir().expect("temp");
    let bin = temp.path().join("bin");
    write_fake_command(&bin, "cargo", "exit 0");
    write_fake_command(&bin, "rustc", "printf 'rustc 1.97.0\\n'");
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

#[tokio::test]
pub(crate) async fn install_preflight_prints_progress_breadcrumbs() {
    let temp = tempdir().expect("temp");
    let bin = temp.path().join("bin");
    write_fake_command(
        &bin,
        "cargo",
        "case \"$1\" in\n  --version) printf 'cargo 1.97.0\\n'; exit 0 ;;\n  install) printf 'fake cargo reached\\n' >&2; exit 42 ;;\n  *) exit 0 ;;\nesac",
    );
    write_fake_command(
        &bin,
        "rustc",
        "case \"$1\" in\n  --version) printf 'rustc 1.97.0\\n'; exit 0 ;;\n  *) exit 0 ;;\nesac",
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
    let platform_breadcrumbs: &[&str] = if cfg!(windows) {
        &["pevo install: using windows-git-bash source checkout at"]
    } else {
        &[
            "pevo install: using unix source checkout at",
            "pevo install: using wsl source checkout at",
        ]
    };
    assert!(
        platform_breadcrumbs
            .iter()
            .any(|breadcrumb| stderr.contains(breadcrumb)),
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
        stderr.contains("pevo install: collecting network diagnostics"),
        "{stderr}"
    );
}

#[tokio::test]
pub(crate) async fn install_preflight_warns_for_mismatched_pnpm_and_continues() {
    let temp = tempdir().expect("temp");
    let bin = temp.path().join("bin");
    let home = temp.path().join("home");
    write_fake_pevo(&home);
    write_fake_command(&bin, "cargo", "exit 0");
    write_fake_command(&bin, "rustc", "printf 'rustc 1.97.0\\n'");
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
        stderr.contains("Network diagnostics (pnpm install failed)"),
        "{stderr}"
    );
}

#[tokio::test]
pub(crate) async fn install_preflight_bypasses_corepack_project_spec_for_pnpm() {
    let temp = tempdir().expect("temp");
    let bin = temp.path().join("bin");
    let home = temp.path().join("home");
    write_fake_pevo(&home);
    write_fake_command(&bin, "cargo", "exit 0");
    write_fake_command(&bin, "rustc", "printf 'rustc 1.97.0\\n'");
    write_fake_command(&bin, "cc", "exit 0");
    write_fake_command(&bin, "node", "printf 'v24.0.0\\n'");
    write_fake_command(
        &bin,
        "pnpm",
        "if [ \"${COREPACK_ENABLE_PROJECT_SPEC:-}\" != 0 ]; then printf 'corepack attempted project download\\n' >&2; exit 42; fi\nif [ \"${COREPACK_ENABLE_DOWNLOAD_PROMPT:-}\" != 0 ]; then printf 'corepack prompted\\n' >&2; exit 42; fi\nif [ \"${COREPACK_ENABLE_STRICT:-}\" != 0 ]; then printf 'corepack strict check remained enabled\\n' >&2; exit 42; fi\nif [ \"${pnpm_config_pm_on_fail:-}\" != warn ]; then printf '[ERROR] This project is configured to use 11.8.0 of pnpm. Your current pnpm is v11.10.0\\nCorepack invoked pnpm with this version, and pnpm does not switch versions when running under corepack.\\n' >&2; exit 42; fi\ncase \"$1\" in\n  --version) printf '11.10.0\\n'; exit 0 ;;\n  config) printf 'https://registry.npmjs.org/\\n'; exit 0 ;;\n  install) printf 'fake pnpm install reached\\n' >&2; exit 42 ;;\n  *) exit 0 ;;\nesac",
    );
    let output = install_preflight_command(&bin, &home)
        .output()
        .expect("install preflight");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("warning: pnpm 11.10.0 is installed; pnpm 11.8.0 is recommended"),
        "{stderr}"
    );
    assert!(stderr.contains("fake pnpm install reached"), "{stderr}");
    assert!(
        !stderr.contains("corepack attempted project download"),
        "{stderr}"
    );
    assert!(!stderr.contains("corepack prompted"), "{stderr}");
    assert!(
        !stderr.contains("corepack strict check remained enabled"),
        "{stderr}"
    );
    assert!(
        !stderr.contains("This project is configured to use 11.8.0 of pnpm"),
        "{stderr}"
    );
}

#[tokio::test]
pub(crate) async fn install_pnpm_defaults_fetch_timeout_without_changing_retry_policy() {
    let temp = tempdir().expect("temp");
    let bin = temp.path().join("bin");
    let home = temp.path().join("home");
    write_fake_web_install_prerequisites(
        &bin,
        &home,
        "case \"$1\" in\n  --version) printf '11.8.0\\n'; exit 0 ;;\n  config) printf 'https://registry.npmjs.org/\\n'; exit 0 ;;\n  install) printf 'pnpm timeout=%s retries=%s retry-max=%s concurrency=%s\\n' \"${pnpm_config_fetch_timeout-unset}\" \"${pnpm_config_fetch_retries-unset}\" \"${pnpm_config_fetch_retry_maxtimeout-unset}\" \"${pnpm_config_network_concurrency-unset}\" >&2; exit 42 ;;\n  *) exit 0 ;;\nesac",
    );

    let output = install_preflight_command(&bin, &home)
        .output()
        .expect("install pnpm");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("pnpm timeout=300000 retries=unset retry-max=unset concurrency=unset"),
        "{stderr}"
    );
    assert!(
        stderr.contains(
            "pnpm_config_fetch_timeout: 300000 (installer default for pnpm subprocesses)"
        ),
        "{stderr}"
    );
    assert!(
        stderr.contains("pnpm_config_fetch_retries: (unset)"),
        "{stderr}"
    );
    assert!(
        stderr.contains("pnpm_config_fetch_retry_maxtimeout: (unset)"),
        "{stderr}"
    );
    assert!(
        stderr.contains("pnpm_config_network_concurrency: (unset)"),
        "{stderr}"
    );
}

#[tokio::test]
pub(crate) async fn install_pnpm_preserves_explicit_fetch_timeout() {
    let temp = tempdir().expect("temp");
    let bin = temp.path().join("bin");
    let home = temp.path().join("home");
    write_fake_web_install_prerequisites(
        &bin,
        &home,
        "case \"$1\" in\n  --version) printf '11.8.0\\n'; exit 0 ;;\n  config) printf 'https://registry.npmjs.org/\\n'; exit 0 ;;\n  install) printf 'pnpm timeout=%s\\n' \"${pnpm_config_fetch_timeout-unset}\" >&2; exit 42 ;;\n  *) exit 0 ;;\nesac",
    );

    let output = install_preflight_command(&bin, &home)
        .env("pnpm_config_fetch_timeout", "90000")
        .output()
        .expect("install pnpm");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("pnpm timeout=90000"), "{stderr}");
    assert!(
        stderr.contains("pnpm_config_fetch_timeout: 90000"),
        "{stderr}"
    );
    assert!(
        !stderr.contains("pnpm_config_fetch_timeout: 300000"),
        "{stderr}"
    );
}

#[tokio::test]
pub(crate) async fn install_distinguishes_dependency_install_and_asset_build_steps() {
    let temp = tempdir().expect("temp");
    let bin = temp.path().join("bin");
    let home = temp.path().join("home");
    write_fake_web_install_prerequisites(
        &bin,
        &home,
        "case \"$1\" in\n  --version) printf '11.8.0\\n'; exit 0 ;;\n  config) printf 'https://registry.npmjs.org/\\n'; exit 0 ;;\n  install) printf 'fake pnpm install reached\\n' >&2; exit 0 ;;\n  --filter) printf 'fake pnpm build reached\\n' >&2; exit 42 ;;\n  *) exit 0 ;;\nesac",
    );

    let output = install_preflight_command(&bin, &home)
        .output()
        .expect("install pnpm");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    let dependency_step = stderr
        .find("pevo install: installing Workbench dependencies")
        .expect("dependency install breadcrumb");
    let dependency_command = stderr
        .find("fake pnpm install reached")
        .expect("pnpm install invocation");
    let build_step = stderr
        .find("pevo install: building Workbench assets")
        .expect("asset build breadcrumb");
    let build_command = stderr
        .find("fake pnpm build reached")
        .expect("pnpm build invocation");
    assert!(dependency_step < dependency_command, "{stderr}");
    assert!(dependency_command < build_step, "{stderr}");
    assert!(build_step < build_command, "{stderr}");
}

#[tokio::test]
pub(crate) async fn install_preflight_rejects_unusable_pnpm_before_cargo_install() {
    let temp = tempdir().expect("temp");
    let bin = temp.path().join("bin");
    write_fake_command(
        &bin,
        "cargo",
        "case \"$1\" in\n  --version) printf 'cargo 1.97.0\\n'; exit 0 ;;\n  install) printf 'fake cargo reached\\n' >&2; exit 42 ;;\n  *) exit 0 ;;\nesac",
    );
    write_fake_command(&bin, "rustc", "printf 'rustc 1.97.0\\n'");
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

#[tokio::test]
pub(crate) async fn install_check_reports_unusable_pnpm_as_failure() {
    let temp = tempdir().expect("temp");
    let bin = temp.path().join("bin");
    write_fake_command(&bin, "cargo", "printf 'cargo 1.97.0\\n'");
    write_fake_command(&bin, "rustc", "printf 'rustc 1.97.0\\n'");
    write_fake_command(&bin, "cc", "exit 0");
    write_fake_command(&bin, "node", "printf 'v24.0.0\\n'");
    write_fake_command(
        &bin,
        "pnpm",
        "case \"$1\" in\n  --version) printf 'corepack certificate failure\\n' >&2; exit 42 ;;\n  config) printf 'https://registry.npmjs.org/\\n'; exit 0 ;;\n  *) exit 42 ;;\nesac",
    );
    let output = install_preflight_command(&bin, &temp.path().join("home"))
        .arg("--check")
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

#[cfg(windows)]
#[tokio::test]
pub(crate) async fn install_windows_preflight_reports_missing_build_tools_before_cargo() {
    let temp = tempdir().expect("temp");
    let bin = temp.path().join("bin");
    write_fake_command(&bin, "cargo", "printf 'fake cargo reached\\n' >&2\nexit 42");
    write_fake_command(&bin, "rustc", "printf 'rustc 1.97.0\\n'");
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

#[cfg(windows)]
#[tokio::test]
pub(crate) async fn install_windows_cargo_install_defaults_revocation_check_off() {
    let temp = tempdir().expect("temp");
    let bin = temp.path().join("bin");
    write_fake_command(
        &bin,
        "tee",
        "out=$1\n: > \"$out\"\nwhile IFS= read -r line || [ -n \"$line\" ]; do\n  printf '%s\\n' \"$line\"\n  printf '%s\\n' \"$line\" >> \"$out\"\ndone",
    );
    write_fake_command(
        &bin,
        "cargo",
        "case \"$1\" in\n  --version) printf 'cargo 1.97.0\\n'; exit 0 ;;\n  install) printf 'cargo revoke=%s\\n' \"${CARGO_HTTP_CHECK_REVOKE-unset}\" >&2; [ \"${CARGO_HTTP_CHECK_REVOKE:-}\" = false ] || exit 43; exit 42 ;;\n  *) exit 0 ;;\nesac",
    );
    write_fake_command(&bin, "rustc", "printf 'rustc 1.97.0\\n'");
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

#[cfg(windows)]
#[tokio::test]
pub(crate) async fn install_windows_cargo_install_preserves_explicit_revocation_setting() {
    let temp = tempdir().expect("temp");
    let bin = temp.path().join("bin");
    write_fake_command(
        &bin,
        "cargo",
        "case \"$1\" in\n  --version) printf 'cargo 1.97.0\\n'; exit 0 ;;\n  install) printf 'cargo revoke=%s\\n' \"${CARGO_HTTP_CHECK_REVOKE-unset}\" >&2; [ \"${CARGO_HTTP_CHECK_REVOKE:-}\" = true ] || exit 43; exit 42 ;;\n  *) exit 0 ;;\nesac",
    );
    write_fake_command(&bin, "rustc", "printf 'rustc 1.97.0\\n'");
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

#[cfg(windows)]
#[tokio::test]
pub(crate) async fn install_windows_locked_pevo_exe_failure_gets_targeted_guidance() {
    let temp = tempdir().expect("temp");
    let bin = temp.path().join("bin");
    write_fake_command(
        &bin,
        "cargo",
        "case \"$1\" in\n  --version) printf 'cargo 1.97.0\\n'; exit 0 ;;\n  install) printf '   Replacing C:\\\\Users\\\\c00845592\\\\.cargo\\\\bin\\\\pevo.exe\\n' >&2; printf 'error: failed to move `C:\\\\Users\\\\c00845592\\\\.cargo\\\\bin\\\\cargo-installU8ZJRb\\\\pevo.exe` to `C:\\\\Users\\\\c00845592\\\\.cargo\\\\bin\\\\pevo.exe`\\n\\nCaused by:\\n  Access is denied. (os error 5)\\n' >&2; exit 101 ;;\n  *) exit 0 ;;\nesac",
    );
    write_fake_command(&bin, "rustc", "printf 'rustc 1.97.0\\n'");
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
        !stderr.contains("Network diagnostics (cargo install failed)"),
        "{stderr}"
    );
    assert!(!stderr.contains("native C/C++ build tools"), "{stderr}");
    assert!(
        !stderr.contains("CARGO_HTTP_MULTIPLEXING=false"),
        "{stderr}"
    );
}

#[cfg(windows)]
#[tokio::test]
pub(crate) async fn install_windows_preflight_stops_existing_managed_gateway() {
    let temp = tempdir().expect("temp");
    let bin = temp.path().join("bin");
    let home = temp.path().join("home");
    write_fake_command(
        &home.join(".cargo/bin"),
        "pevo.exe",
        "printf '%s\\n' \"$*\" >> \"$HOME/gateway-stop.log\"\nexit 0",
    );
    write_fake_command(
        &bin,
        "cargo",
        "case \"$1\" in\n  --version) printf 'cargo 1.97.0\\n'; exit 0 ;;\n  install) printf 'fake cargo failed\\n' >&2; exit 42 ;;\n  *) exit 0 ;;\nesac",
    );
    write_fake_command(&bin, "rustc", "printf 'rustc 1.97.0\\n'");
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
        stderr.contains("Network diagnostics (cargo install failed)"),
        "{stderr}"
    );
    let stop_log = std::fs::read_to_string(home.join("gateway-stop.log")).expect("stop log");
    assert_eq!(stop_log.trim(), "gateway stop");
}

#[cfg(unix)]
#[tokio::test]
pub(crate) async fn install_unix_cargo_install_does_not_force_revocation_setting() {
    let temp = tempdir().expect("temp");
    let bin = temp.path().join("bin");
    write_fake_command(
        &bin,
        "cargo",
        "case \"$1\" in\n  --version) printf 'cargo 1.97.0\\n'; exit 0 ;;\n  install) printf 'cargo revoke=%s\\n' \"${CARGO_HTTP_CHECK_REVOKE-unset}\" >&2; [ -z \"${CARGO_HTTP_CHECK_REVOKE+x}\" ] || exit 43; exit 42 ;;\n  *) exit 0 ;;\nesac",
    );
    write_fake_command(&bin, "rustc", "printf 'rustc 1.97.0\\n'");
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

#[tokio::test]
pub(crate) async fn install_cargo_install_defaults_timeout_and_retry() {
    let temp = tempdir().expect("temp");
    let bin = temp.path().join("bin");
    write_fake_command(
        &bin,
        "cargo",
        "case \"$1\" in\n  --version) printf 'cargo 1.97.0\\n'; exit 0 ;;\n  install) printf 'cargo timeout=%s retry=%s\\n' \"${CARGO_HTTP_TIMEOUT-unset}\" \"${CARGO_NET_RETRY-unset}\" >&2; [ \"${CARGO_HTTP_TIMEOUT:-}\" = 120 ] || exit 43; [ \"${CARGO_NET_RETRY:-}\" = 10 ] || exit 44; exit 42 ;;\n  *) exit 0 ;;\nesac",
    );
    write_fake_command(&bin, "rustc", "printf 'rustc 1.97.0\\n'");
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

#[tokio::test]
pub(crate) async fn install_cargo_install_preserves_explicit_timeout_and_retry() {
    let temp = tempdir().expect("temp");
    let bin = temp.path().join("bin");
    write_fake_command(
        &bin,
        "cargo",
        "case \"$1\" in\n  --version) printf 'cargo 1.97.0\\n'; exit 0 ;;\n  install) printf 'cargo timeout=%s retry=%s\\n' \"${CARGO_HTTP_TIMEOUT-unset}\" \"${CARGO_NET_RETRY-unset}\" >&2; [ \"${CARGO_HTTP_TIMEOUT:-}\" = 45 ] || exit 43; [ \"${CARGO_NET_RETRY:-}\" = 2 ] || exit 44; exit 42 ;;\n  *) exit 0 ;;\nesac",
    );
    write_fake_command(&bin, "rustc", "printf 'rustc 1.97.0\\n'");
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

#[tokio::test]
pub(crate) async fn install_cargo_failure_prints_network_diagnostics() {
    let temp = tempdir().expect("temp");
    let bin = temp.path().join("bin");
    write_fake_command(
        &bin,
        "cargo",
        "case \"$1\" in\n  --version) printf 'cargo 1.97.0\\n'; exit 0 ;;\n  install) printf 'fake cargo failed\\n' >&2; exit 42 ;;\n  *) exit 0 ;;\nesac",
    );
    write_fake_command(&bin, "rustc", "printf 'rustc 1.97.0\\n'");
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
        stderr.contains("Network diagnostics (cargo install failed)"),
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
