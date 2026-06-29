use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command as ProcessCommand;

use anyhow::{Context, Result, bail};
use clap::Subcommand;
use serde::Serialize;

#[derive(Debug, Subcommand)]
pub(crate) enum InitCommand {
    DevEnv {
        #[arg(long)]
        home: Option<PathBuf>,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Debug, Serialize)]
struct DevEnvOutput {
    home: String,
    config: String,
    env_file: String,
    pevo_bin: String,
}

pub(crate) fn run(command: InitCommand, root: &Path) -> Result<()> {
    match command {
        InitCommand::DevEnv { home, json } => {
            let output = init_dev_env(root, home)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&output)?);
            } else {
                println!("dev home: {}", output.home);
                println!("config: {}", output.config);
                println!("env: {}", output.env_file);
                println!("prepare live credentials manually before running cargo xtask live");
            }
            Ok(())
        }
    }
}

fn init_dev_env(root: &Path, home: Option<PathBuf>) -> Result<DevEnvOutput> {
    let home = dev_home_path(root, home);
    fs::create_dir_all(&home).with_context(|| format!("create dev home {}", home.display()))?;
    let pevo_bin = build_pevo(root)?;
    let output = ProcessCommand::new(&pevo_bin)
        .arg("init")
        .current_dir(root)
        .env("PSYCHEVO_HOME", &home)
        .output()
        .with_context(|| format!("run {} init", pevo_bin.display()))?;
    if !output.status.success() {
        let mut stderr = std::io::stderr().lock();
        stderr.write_all(&output.stdout)?;
        stderr.write_all(&output.stderr)?;
        bail!("pevo init failed with status {}", output.status);
    }
    Ok(DevEnvOutput {
        home: home.display().to_string(),
        config: home.join("config.toml").display().to_string(),
        env_file: home.join(".env").display().to_string(),
        pevo_bin: pevo_bin.display().to_string(),
    })
}

fn build_pevo(root: &Path) -> Result<PathBuf> {
    let status = ProcessCommand::new("cargo")
        .args(["build", "-p", "psychevo-cli", "--quiet"])
        .current_dir(root)
        .status()
        .context("build psychevo-cli")?;
    if !status.success() {
        bail!("cargo build -p psychevo-cli failed with status {status}");
    }
    let pevo_bin = root.join("target").join("debug").join(binary_name("pevo"));
    if !pevo_bin.is_file() {
        bail!("built pevo binary is missing: {}", pevo_bin.display());
    }
    Ok(pevo_bin)
}

fn dev_home_path(root: &Path, home: Option<PathBuf>) -> PathBuf {
    home.unwrap_or_else(|| root.join(".local").join(".psychevo-dev"))
}

fn binary_name(base: &str) -> String {
    if cfg!(windows) {
        format!("{base}.exe")
    } else {
        base.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dev_home_defaults_to_repo_local_path() {
        let root = Path::new("/tmp/psychevo");
        assert_eq!(
            dev_home_path(root, None),
            PathBuf::from("/tmp/psychevo/.local/.psychevo-dev")
        );
    }

    #[test]
    fn dev_home_accepts_explicit_cli_path() {
        let root = Path::new("/tmp/psychevo");
        assert_eq!(
            dev_home_path(root, Some(PathBuf::from("/tmp/custom"))),
            PathBuf::from("/tmp/custom")
        );
    }
}
