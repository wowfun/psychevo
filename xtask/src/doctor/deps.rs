use std::path::Path;

use anyhow::Result;
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
                "sudo apt-get update && sudo apt-get install -y sqlite3".to_string(),
            ],
            Self::Vhs => vec![
                "sudo apt-get update".to_string(),
                "sudo apt-get install -y ca-certificates curl gpg python3 git".to_string(),
                "curl -fsSL https://repo.charm.sh/apt/gpg.key | sudo gpg --dearmor --yes -o /etc/apt/keyrings/charm.gpg".to_string(),
                "echo 'deb [signed-by=/etc/apt/keyrings/charm.gpg] https://repo.charm.sh/apt/ * *' | sudo tee /etc/apt/sources.list.d/charm.list >/dev/null".to_string(),
                "sudo apt-get update && sudo apt-get install -y vhs ttyd ffmpeg".to_string(),
            ],
            Self::Playwright => vec![
                "pnpm exec playwright install chromium".to_string(),
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

pub(crate) fn run(command: DepsCommand, _root: &Path) -> Result<()> {
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
        assert!(row.install_hint.iter().any(|hint| hint.contains("apt-get")));
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
            "sudo apt-get update && sudo apt-get install -y sqlite3"
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
}
