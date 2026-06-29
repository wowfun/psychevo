use std::fs;
use std::path::Path;
use std::process::{Command as ProcessCommand, Stdio};

use anyhow::{Context, Result, bail};
use clap::{Subcommand, ValueEnum};
use serde::Serialize;

#[derive(Debug, Subcommand)]
pub(crate) enum DepsCommand {
    Check {
        #[arg(long, value_enum, default_value_t = DepsScope::All)]
        only: DepsScope,
        #[arg(long)]
        json: bool,
    },
    Install {
        #[arg(long, value_enum)]
        only: DepsScope,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, ValueEnum)]
#[value(rename_all = "kebab-case")]
pub(crate) enum DepsScope {
    All,
    Core,
    Install,
    Sqlite,
    Vhs,
    Playwright,
}

impl DepsScope {
    fn rows(self) -> &'static [DependencyGroup] {
        match self {
            Self::All => &[
                DependencyGroup::Core,
                DependencyGroup::Sqlite,
                DependencyGroup::Vhs,
                DependencyGroup::Playwright,
            ],
            Self::Core => &[DependencyGroup::Core],
            Self::Install => &[DependencyGroup::Install],
            Self::Sqlite => &[DependencyGroup::Sqlite],
            Self::Vhs => &[DependencyGroup::Vhs],
            Self::Playwright => &[DependencyGroup::Playwright],
        }
    }
}

impl std::fmt::Display for DepsScope {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::All => f.write_str("all"),
            Self::Core => f.write_str("core"),
            Self::Install => f.write_str("install"),
            Self::Sqlite => f.write_str("sqlite"),
            Self::Vhs => f.write_str("vhs"),
            Self::Playwright => f.write_str("playwright"),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DependencyGroup {
    Core,
    Install,
    Sqlite,
    Vhs,
    Playwright,
}

impl DependencyGroup {
    fn id(self) -> &'static str {
        match self {
            Self::Core => "core",
            Self::Install => "install",
            Self::Sqlite => "sqlite",
            Self::Vhs => "vhs",
            Self::Playwright => "playwright",
        }
    }

    fn commands(self) -> &'static [&'static str] {
        match self {
            Self::Core => &["cargo", "node", "pnpm"],
            Self::Install => &[],
            Self::Sqlite => &["sqlite3"],
            Self::Vhs => &["vhs", "ttyd", "ffmpeg", "python3", "git"],
            Self::Playwright => &["pnpm"],
        }
    }

    fn missing_commands(self, probe: &dyn CommandProbe) -> Vec<String> {
        match self {
            Self::Install => {
                let mut missing = Vec::new();
                for command in ["git", "cargo"] {
                    if !probe.has_command(command) {
                        missing.push(command.to_string());
                    }
                }
                if !["cc", "gcc", "clang"]
                    .into_iter()
                    .any(|command| probe.has_command(command))
                {
                    missing.push("cc|gcc|clang".to_string());
                }
                for command in ["node", "pnpm"] {
                    if !probe.has_command(command) {
                        missing.push(command.to_string());
                    }
                }
                missing
            }
            _ => self
                .commands()
                .iter()
                .copied()
                .filter(|command| !probe.has_command(command))
                .map(str::to_string)
                .collect(),
        }
    }

    fn install_hint(self) -> Vec<String> {
        match self {
            Self::Core => vec![
                "xtask does not install core toolchains; use scripts/install.sh or your normal Rust/Node/pnpm setup".to_string(),
            ],
            Self::Install => vec![
                "xtask checks source-install prerequisites but does not install them".to_string(),
                "install git, Rust/Cargo, a native C compiler/linker, Node.js, and pnpm with your platform package manager or upstream installers".to_string(),
            ],
            Self::Sqlite => vec![
                "cargo xtask doctor deps install --only sqlite".to_string(),
                "sudo apt-get update && sudo apt-get install -y sqlite3".to_string(),
            ],
            Self::Vhs => vec![
                "cargo xtask doctor deps install --only vhs".to_string(),
                "sudo apt-get update".to_string(),
                "sudo apt-get install -y ca-certificates curl gpg python3 git".to_string(),
                "curl -fsSL https://repo.charm.sh/apt/gpg.key | sudo gpg --dearmor --yes -o /etc/apt/keyrings/charm.gpg".to_string(),
                "echo 'deb [signed-by=/etc/apt/keyrings/charm.gpg] https://repo.charm.sh/apt/ * *' | sudo tee /etc/apt/sources.list.d/charm.list >/dev/null".to_string(),
                "sudo apt-get update && sudo apt-get install -y vhs ttyd ffmpeg".to_string(),
            ],
            Self::Playwright => vec![
                "cargo xtask doctor deps install --only playwright".to_string(),
                "pnpm exec playwright install --with-deps chromium".to_string(),
            ],
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
struct DepsReport {
    scopes: Vec<DependencyRow>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
struct DependencyRow {
    scope: &'static str,
    status: DependencyStatus,
    missing: Vec<String>,
    install_hint: Vec<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum DependencyStatus {
    Ok,
    Missing,
}

trait CommandProbe {
    fn has_command(&self, command: &str) -> bool;
}

struct SystemProbe;

impl CommandProbe for SystemProbe {
    fn has_command(&self, command: &str) -> bool {
        command_exists(command)
    }
}

pub(crate) fn run(command: DepsCommand, root: &Path) -> Result<()> {
    match command {
        DepsCommand::Check { only, json } => {
            let report = check_deps(only, &SystemProbe);
            if json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                print_report(&report);
            }
            Ok(())
        }
        DepsCommand::Install { only } => install_deps(root, only),
    }
}

fn check_deps(scope: DepsScope, probe: &dyn CommandProbe) -> DepsReport {
    let scopes = scope
        .rows()
        .iter()
        .copied()
        .map(|group| check_group(group, probe))
        .collect();
    DepsReport { scopes }
}

fn check_group(group: DependencyGroup, probe: &dyn CommandProbe) -> DependencyRow {
    let missing = group.missing_commands(probe);
    DependencyRow {
        scope: group.id(),
        status: if missing.is_empty() {
            DependencyStatus::Ok
        } else {
            DependencyStatus::Missing
        },
        install_hint: if missing.is_empty() {
            Vec::new()
        } else {
            group.install_hint()
        },
        missing,
    }
}

fn print_report(report: &DepsReport) {
    for row in &report.scopes {
        match row.status {
            DependencyStatus::Ok => {
                eprintln!("{}: ok", row.scope);
            }
            DependencyStatus::Missing => {
                eprintln!("{}: missing {}", row.scope, row.missing.join(" "));
                eprintln!("install hint:");
                for hint in &row.install_hint {
                    eprintln!("  {hint}");
                }
            }
        }
    }
}

fn install_deps(root: &Path, scope: DepsScope) -> Result<()> {
    if scope == DepsScope::Core {
        bail!(
            "xtask does not install core toolchains; use scripts/install.sh or your normal Rust/Node/pnpm setup"
        );
    }
    if scope == DepsScope::Install {
        bail!(
            "xtask does not install source-install prerequisites; install git, Rust/Cargo, a native C compiler/linker, Node.js, and pnpm with your platform package manager or upstream installers"
        );
    }
    let installer = SystemInstaller::new()?;
    for group in scope.rows().iter().copied() {
        match group {
            DependencyGroup::Core => {
                eprintln!(
                    "core: check only; use scripts/install.sh or your normal Rust/Node/pnpm setup"
                );
            }
            DependencyGroup::Install => {
                eprintln!(
                    "install: check only; install source-install prerequisites with your platform package manager or upstream installers"
                );
            }
            DependencyGroup::Sqlite => installer.install_sqlite()?,
            DependencyGroup::Vhs => installer.install_vhs()?,
            DependencyGroup::Playwright => installer.install_playwright(root)?,
        }
    }
    let report = check_deps(scope, &SystemProbe);
    print_report(&report);
    Ok(())
}

struct SystemInstaller {
    apt_updated: std::cell::Cell<bool>,
}

impl SystemInstaller {
    fn new() -> Result<Self> {
        require_debian_install()?;
        Ok(Self {
            apt_updated: std::cell::Cell::new(false),
        })
    }

    fn install_sqlite(&self) -> Result<()> {
        if command_exists("sqlite3") {
            return Ok(());
        }
        self.apt_install(&["sqlite3"])
    }

    fn install_vhs(&self) -> Result<()> {
        let base_missing: Vec<_> = ["python3", "git"]
            .into_iter()
            .filter(|command| !command_exists(command))
            .collect();
        if !base_missing.is_empty() {
            self.apt_install(&base_missing)?;
        }

        let charm_missing: Vec<_> = ["vhs", "ttyd", "ffmpeg"]
            .into_iter()
            .filter(|command| !command_exists(command))
            .collect();
        if !charm_missing.is_empty() {
            self.install_charm_repo()?;
            self.apt_install(&["vhs", "ttyd", "ffmpeg"])?;
        }
        Ok(())
    }

    fn install_playwright(&self, root: &Path) -> Result<()> {
        if !command_exists("pnpm") {
            bail!(
                "pnpm is required for Playwright browser installation. Install Node.js/pnpm first."
            );
        }
        run_status(
            ProcessCommand::new("pnpm")
                .args(["exec", "playwright", "install", "--with-deps", "chromium"])
                .current_dir(root),
        )
    }

    fn install_charm_repo(&self) -> Result<()> {
        let bootstrap_missing = !command_exists("curl")
            || !command_exists("gpg")
            || !debian_package_installed("ca-certificates");
        if bootstrap_missing {
            self.apt_install(&["ca-certificates", "curl", "gpg"])?;
        }

        self.run_root(ProcessCommand::new("mkdir").args(["-p", "/etc/apt/keyrings"]))?;
        run_shell_root(
            "curl -fsSL https://repo.charm.sh/apt/gpg.key | gpg --dearmor --yes -o /etc/apt/keyrings/charm.gpg",
        )?;
        run_shell_root(
            "printf '%s\n' 'deb [signed-by=/etc/apt/keyrings/charm.gpg] https://repo.charm.sh/apt/ * *' > /etc/apt/sources.list.d/charm.list",
        )?;
        self.apt_updated.set(false);
        self.apt_update_once()
    }

    fn apt_install(&self, packages: &[&str]) -> Result<()> {
        self.apt_update_once()?;
        let mut command = ProcessCommand::new("apt-get");
        command.arg("install").arg("-y").args(packages);
        self.run_root(&mut command)
    }

    fn apt_update_once(&self) -> Result<()> {
        if self.apt_updated.get() {
            return Ok(());
        }
        self.run_root(ProcessCommand::new("apt-get").arg("update"))?;
        self.apt_updated.set(true);
        Ok(())
    }

    fn run_root(&self, command: &mut ProcessCommand) -> Result<()> {
        run_root(command)
    }
}

fn command_exists(command: &str) -> bool {
    let path = Path::new(command);
    if path.components().count() > 1 {
        return path.is_file();
    }
    let Some(paths) = std::env::var_os("PATH") else {
        return false;
    };
    std::env::split_paths(&paths).any(|dir| dir.join(command).is_file())
}

fn debian_package_installed(package: &str) -> bool {
    ProcessCommand::new("dpkg")
        .args(["-s", package])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

fn require_debian_install() -> Result<()> {
    if is_debian_like() {
        Ok(())
    } else {
        bail!(
            "--install currently supports Debian/Ubuntu systems with apt-get only. Install the reported tools with your platform package manager, then rerun check."
        )
    }
}

fn is_debian_like() -> bool {
    if !command_exists("apt-get") {
        return false;
    }
    let Ok(contents) = fs::read_to_string("/etc/os-release") else {
        return false;
    };
    os_release_field(&contents, "ID")
        .map(|id| id == "debian" || id == "ubuntu")
        .unwrap_or(false)
        || os_release_field(&contents, "ID_LIKE")
            .map(|id_like| id_like.split_whitespace().any(|value| value == "debian"))
            .unwrap_or(false)
}

fn os_release_field(contents: &str, key: &str) -> Option<String> {
    contents.lines().find_map(|line| {
        let (field, value) = line.split_once('=')?;
        if field == key {
            Some(value.trim_matches('"').to_string())
        } else {
            None
        }
    })
}

fn run_root(command: &mut ProcessCommand) -> Result<()> {
    if is_root() {
        run_status(command)
    } else {
        command_exists("sudo")
            .then_some(())
            .ok_or_else(|| anyhow::anyhow!("sudo is required for --install on Debian/Ubuntu."))?;
        let mut sudo = ProcessCommand::new("sudo");
        sudo.arg(command.get_program()).args(command.get_args());
        run_status(&mut sudo)
    }
}

fn run_shell_root(script: &str) -> Result<()> {
    if is_root() {
        run_status(ProcessCommand::new("sh").arg("-c").arg(script))
    } else {
        command_exists("sudo")
            .then_some(())
            .ok_or_else(|| anyhow::anyhow!("sudo is required for --install on Debian/Ubuntu."))?;
        run_status(ProcessCommand::new("sudo").args(["sh", "-c", script]))
    }
}

fn run_status(command: &mut ProcessCommand) -> Result<()> {
    let status = command
        .status()
        .with_context(|| format!("spawn command {command:?}"))?;
    if status.success() {
        Ok(())
    } else {
        bail!("command failed with status {status}: {command:?}")
    }
}

fn is_root() -> bool {
    ProcessCommand::new("id")
        .arg("-u")
        .output()
        .ok()
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|value| value.trim() == "0")
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[derive(Default)]
    struct FakeProbe {
        commands: HashSet<&'static str>,
    }

    impl FakeProbe {
        fn with(commands: &[&'static str]) -> Self {
            Self {
                commands: commands.iter().copied().collect(),
            }
        }
    }

    impl CommandProbe for FakeProbe {
        fn has_command(&self, command: &str) -> bool {
            self.commands.contains(command)
        }
    }

    #[test]
    fn check_reports_missing_scope_with_install_hint() {
        let report = check_deps(DepsScope::Vhs, &FakeProbe::with(&["python3", "git"]));
        assert_eq!(report.scopes.len(), 1);
        let row = &report.scopes[0];
        assert_eq!(row.scope, "vhs");
        assert_eq!(row.status, DependencyStatus::Missing);
        assert_eq!(row.missing, vec!["vhs", "ttyd", "ffmpeg"]);
        assert!(
            row.install_hint
                .iter()
                .any(|hint| hint == "cargo xtask doctor deps install --only vhs")
        );
    }

    #[test]
    fn check_reports_ok_when_scope_commands_exist() {
        let report = check_deps(
            DepsScope::Core,
            &FakeProbe::with(&["cargo", "node", "pnpm"]),
        );
        assert_eq!(report.scopes[0].status, DependencyStatus::Ok);
        assert!(report.scopes[0].missing.is_empty());
        assert!(report.scopes[0].install_hint.is_empty());
    }

    #[test]
    fn install_scope_reports_source_install_prerequisites() {
        let report = check_deps(DepsScope::Install, &FakeProbe::with(&["git", "node"]));
        assert_eq!(report.scopes.len(), 1);
        let row = &report.scopes[0];
        assert_eq!(row.scope, "install");
        assert_eq!(row.status, DependencyStatus::Missing);
        assert_eq!(row.missing, vec!["cargo", "cc|gcc|clang", "pnpm"]);
        assert!(
            row.install_hint
                .iter()
                .any(|hint| hint.contains("does not install them"))
        );
    }

    #[test]
    fn install_scope_accepts_any_native_compiler_command() {
        let report = check_deps(
            DepsScope::Install,
            &FakeProbe::with(&["git", "cargo", "gcc", "node", "pnpm"]),
        );
        assert_eq!(report.scopes[0].status, DependencyStatus::Ok);
        assert!(report.scopes[0].missing.is_empty());
    }

    #[test]
    fn all_scope_expands_in_stable_order() {
        let report = check_deps(DepsScope::All, &FakeProbe::default());
        let scopes: Vec<_> = report.scopes.iter().map(|row| row.scope).collect();
        assert_eq!(scopes, vec!["core", "sqlite", "vhs", "playwright"]);
    }

    #[test]
    fn json_output_contains_scope_status_and_install_hint() {
        let report = check_deps(DepsScope::Sqlite, &FakeProbe::default());
        let json = serde_json::to_value(&report).expect("json");
        assert_eq!(json["scopes"][0]["scope"], "sqlite");
        assert_eq!(json["scopes"][0]["status"], "missing");
        assert_eq!(json["scopes"][0]["missing"][0], "sqlite3");
        assert_eq!(
            json["scopes"][0]["install_hint"][0],
            "cargo xtask doctor deps install --only sqlite"
        );
    }

    #[test]
    fn install_scope_json_uses_source_install_scope_name() {
        let report = check_deps(DepsScope::Install, &FakeProbe::default());
        let json = serde_json::to_value(&report).expect("json");
        assert_eq!(json["scopes"][0]["scope"], "install");
        assert_eq!(json["scopes"][0]["status"], "missing");
        assert_eq!(json["scopes"][0]["missing"][0], "git");
        assert_eq!(json["scopes"][0]["missing"][2], "cc|gcc|clang");
    }

    #[test]
    fn install_core_is_rejected() {
        let err = install_deps(Path::new("."), DepsScope::Core).expect_err("core install");
        assert!(err.to_string().contains("does not install core toolchains"));
    }

    #[test]
    fn install_scope_install_is_rejected() {
        let err = install_deps(Path::new("."), DepsScope::Install).expect_err("install scope");
        assert!(
            err.to_string()
                .contains("does not install source-install prerequisites")
        );
    }

    #[test]
    fn os_release_parsing_detects_debian_like() {
        let ubuntu = "ID=ubuntu\nID_LIKE=\"debian\"\n";
        assert_eq!(os_release_field(ubuntu, "ID").as_deref(), Some("ubuntu"));
        assert_eq!(
            os_release_field(ubuntu, "ID_LIKE").as_deref(),
            Some("debian")
        );
    }
}
