use std::path::PathBuf;

use anyhow::{Context, Result, bail};

pub(crate) fn repo_root() -> Result<PathBuf> {
    let mut dir = std::env::current_dir().context("read current directory")?;
    loop {
        if dir.join("Cargo.toml").is_file() && dir.join("packages").is_dir() {
            return Ok(dir);
        }
        if !dir.pop() {
            bail!("could not find repository root");
        }
    }
}
