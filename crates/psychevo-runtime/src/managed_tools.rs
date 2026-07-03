use std::collections::BTreeMap;
use std::env;
use std::ffi::OsString;
use std::fs;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Deserialize;

use crate::error::{Error, Result};

pub(crate) const RIPGREP_LATEST_RELEASE_URL: &str =
    "https://api.github.com/repos/BurntSushi/ripgrep/releases/latest";

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct ManagedToolPaths {
    pub(crate) path_prefixes: Vec<PathBuf>,
}

pub(crate) async fn ensure_rg(env_map: &BTreeMap<String, String>) -> Result<ManagedToolPaths> {
    ensure_rg_with_downloader(env_map, |tools_dir| async move {
        download_latest_rg(&tools_dir).await
    })
    .await
}

pub(crate) async fn ensure_rg_with_downloader<F, Fut>(
    env_map: &BTreeMap<String, String>,
    downloader: F,
) -> Result<ManagedToolPaths>
where
    F: FnOnce(PathBuf) -> Fut,
    Fut: Future<Output = Result<()>>,
{
    let home = resolve_psychevo_home(env_map)?;
    let tools_dir = home.join("tools");
    let managed_rg = tools_dir.join(rg_executable_name());
    if is_executable_file(&managed_rg) {
        return Ok(managed_paths(tools_dir));
    }
    if find_on_path("rg", env_path_value(env_map), env_map).is_some() {
        return Ok(ManagedToolPaths::default());
    }
    fs::create_dir_all(&tools_dir)?;
    downloader(tools_dir.clone()).await.map_err(|err| {
        Error::Message(format!(
            "rg is required before agent start, but no managed or PATH rg was found and download failed: {err}"
        ))
    })?;
    if !is_executable_file(&managed_rg) {
        return Err(Error::Message(format!(
            "rg is required before agent start, but managed install did not create {}",
            managed_rg.display()
        )));
    }
    Ok(managed_paths(tools_dir))
}

pub(crate) fn managed_paths(tools_dir: PathBuf) -> ManagedToolPaths {
    ManagedToolPaths {
        path_prefixes: vec![tools_dir],
    }
}

pub(crate) fn env_path_value(env_map: &BTreeMap<String, String>) -> Option<OsString> {
    env_map
        .get("PATH")
        .or_else(|| {
            env_map
                .iter()
                .find(|(key, _)| key.eq_ignore_ascii_case("PATH"))
                .map(|(_, value)| value)
        })
        .map(OsString::from)
        .or_else(|| env::var_os("PATH"))
}

pub(crate) fn find_on_path(
    name: &str,
    path: Option<OsString>,
    env_map: &BTreeMap<String, String>,
) -> Option<PathBuf> {
    let path = path?;
    let command_has_extension = Path::new(name).extension().is_some();
    for dir in env::split_paths(&path) {
        for candidate in crate::process_env::executable_path_candidates(
            &dir.join(name),
            command_has_extension,
            env_map,
            cfg!(windows),
        ) {
            if is_executable_file(&candidate) {
                return Some(candidate);
            }
        }
    }
    None
}

pub(crate) fn resolve_psychevo_home(env_map: &BTreeMap<String, String>) -> Result<PathBuf> {
    let raw = env_map
        .get("PSYCHEVO_HOME")
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .unwrap_or("~/.psychevo");
    resolve_configured_path(raw, env_map)
}

pub(crate) fn resolve_configured_path(
    raw: &str,
    env_map: &BTreeMap<String, String>,
) -> Result<PathBuf> {
    let path = if raw == "~" {
        home_path(env_map)?
    } else if let Some(rest) = raw.strip_prefix("~/") {
        home_path(env_map)?.join(rest)
    } else {
        PathBuf::from(raw)
    };
    Ok(if path.is_absolute() {
        path
    } else {
        env::current_dir()?.join(path)
    })
}

pub(crate) fn home_path(env_map: &BTreeMap<String, String>) -> Result<PathBuf> {
    env_map
        .get("HOME")
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .ok_or_else(|| Error::Config("HOME is required to expand ~".to_string()))
}

pub(crate) async fn download_latest_rg(tools_dir: &Path) -> Result<()> {
    let client = reqwest::Client::builder()
        .user_agent("psychevo-runtime")
        .build()?;
    let release: GithubRelease = client
        .get(RIPGREP_LATEST_RELEASE_URL)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    let tag = release.tag_name.trim();
    let version = tag.trim_start_matches('v');
    let asset_name = ripgrep_asset_name(version)?;
    let asset_url =
        format!("https://github.com/BurntSushi/ripgrep/releases/download/{tag}/{asset_name}");
    let bytes = client
        .get(asset_url)
        .send()
        .await?
        .error_for_status()?
        .bytes()
        .await?;
    fs::create_dir_all(tools_dir)?;
    let install_id = install_id();
    let archive_path = tools_dir.join(format!(".{asset_name}.{install_id}.download"));
    let extract_dir = tools_dir.join(format!(".rg-install-{install_id}"));
    fs::write(&archive_path, bytes.as_ref())?;
    if extract_dir.exists() {
        fs::remove_dir_all(&extract_dir)?;
    }
    fs::create_dir_all(&extract_dir)?;
    let result = (|| {
        extract_archive(&archive_path, &extract_dir)?;
        let extracted = find_extracted_rg(&extract_dir)?;
        let target = tools_dir.join(rg_executable_name());
        let temp_target = tools_dir.join(format!(".{}.{install_id}.tmp", rg_executable_name()));
        fs::copy(&extracted, &temp_target)?;
        make_executable(&temp_target)?;
        fs::rename(&temp_target, &target)?;
        Ok(())
    })();
    let _ = fs::remove_file(&archive_path);
    let _ = fs::remove_dir_all(&extract_dir);
    result
}

#[derive(Deserialize)]
pub(crate) struct GithubRelease {
    pub(crate) tag_name: String,
}

pub(crate) fn ripgrep_asset_name(version: &str) -> Result<String> {
    let arch = match env::consts::ARCH {
        "aarch64" => "aarch64",
        "x86_64" => "x86_64",
        other => {
            return Err(Error::Message(format!(
                "unsupported architecture for managed rg download: {other}"
            )));
        }
    };
    match env::consts::OS {
        "macos" => Ok(format!("ripgrep-{version}-{arch}-apple-darwin.tar.gz")),
        "linux" if arch == "aarch64" => Ok(format!(
            "ripgrep-{version}-aarch64-unknown-linux-gnu.tar.gz"
        )),
        "linux" => Ok(format!(
            "ripgrep-{version}-x86_64-unknown-linux-musl.tar.gz"
        )),
        "windows" => Ok(format!("ripgrep-{version}-{arch}-pc-windows-msvc.zip")),
        other => Err(Error::Message(format!(
            "unsupported operating system for managed rg download: {other}"
        ))),
    }
}

pub(crate) fn extract_archive(archive_path: &Path, extract_dir: &Path) -> Result<()> {
    let status = Command::new("tar")
        .arg("-xf")
        .arg(archive_path)
        .arg("-C")
        .arg(extract_dir)
        .status()
        .map_err(|err| Error::Message(format!("failed to start tar for rg install: {err}")))?;
    if !status.success() {
        return Err(Error::Message(format!(
            "tar failed while extracting managed rg archive with status {status}"
        )));
    }
    Ok(())
}

pub(crate) fn find_extracted_rg(root: &Path) -> Result<PathBuf> {
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in fs::read_dir(&dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            if path.file_name().and_then(|name| name.to_str()) == Some(rg_executable_name()) {
                return Ok(path);
            }
        }
    }
    Err(Error::Message(
        "managed rg archive did not contain an rg executable".to_string(),
    ))
}

pub(crate) fn rg_executable_name() -> &'static str {
    if cfg!(windows) { "rg.exe" } else { "rg" }
}

pub(crate) fn is_executable_file(path: &Path) -> bool {
    if !path.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        fs::metadata(path)
            .map(|metadata| metadata.permissions().mode() & 0o111 != 0)
            .unwrap_or(false)
    }
    #[cfg(not(unix))]
    {
        true
    }
}

#[cfg(unix)]
pub(crate) fn make_executable(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    let mut permissions = fs::metadata(path)?.permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions)?;
    Ok(())
}

#[cfg(not(unix))]
pub(crate) fn make_executable(_path: &Path) -> Result<()> {
    Ok(())
}

pub(crate) fn install_id() -> String {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default();
    format!("{}-{millis}", std::process::id())
}

#[cfg(test)]
pub(crate) mod tests {
    pub(crate) use super::*;
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };

    use tempfile::tempdir;

    #[cfg(unix)]
    fn write_fake_rg(path: &Path, output: &str) {
        use std::os::unix::fs::PermissionsExt;

        fs::write(path, format!("#!/bin/sh\nprintf '%s\\n' {output:?}\n")).expect("write fake rg");
        let mut permissions = fs::metadata(path).expect("metadata").permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(path, permissions).expect("chmod");
    }

    #[cfg(windows)]
    fn write_fake_rg(path: &Path, output: &str) {
        fs::write(path, format!("@echo {output}\r\n")).expect("write fake rg");
    }

    fn env_for(home: &Path, path: &Path) -> BTreeMap<String, String> {
        BTreeMap::from([
            ("HOME".to_string(), home.display().to_string()),
            ("PSYCHEVO_HOME".to_string(), home.display().to_string()),
            ("PATH".to_string(), path.display().to_string()),
        ])
    }

    #[tokio::test]
    async fn managed_rg_preferred_over_path_rg() {
        let temp = tempdir().expect("temp");
        let home = temp.path().join("home");
        let tools_dir = home.join("tools");
        let path_dir = temp.path().join("path");
        fs::create_dir_all(&tools_dir).expect("tools");
        fs::create_dir_all(&path_dir).expect("path");
        write_fake_rg(&tools_dir.join(rg_executable_name()), "managed");
        write_fake_rg(&path_dir.join(rg_executable_name()), "path");
        let called = Arc::new(AtomicBool::new(false));
        let called_for_downloader = Arc::clone(&called);

        let result = ensure_rg_with_downloader(&env_for(&home, &path_dir), move |_| {
            called_for_downloader.store(true, Ordering::SeqCst);
            async { Err(Error::Message("unexpected download".to_string())) }
        })
        .await
        .expect("ensure rg");

        assert_eq!(result.path_prefixes, vec![tools_dir]);
        assert!(!called.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn path_rg_skips_download_when_managed_missing() {
        let temp = tempdir().expect("temp");
        let home = temp.path().join("home");
        let path_dir = temp.path().join("path");
        fs::create_dir_all(&home).expect("home");
        fs::create_dir_all(&path_dir).expect("path");
        write_fake_rg(&path_dir.join(rg_executable_name()), "path");
        let called = Arc::new(AtomicBool::new(false));
        let called_for_downloader = Arc::clone(&called);

        let result = ensure_rg_with_downloader(&env_for(&home, &path_dir), move |_| {
            called_for_downloader.store(true, Ordering::SeqCst);
            async { Err(Error::Message("unexpected download".to_string())) }
        })
        .await
        .expect("ensure rg");

        assert!(result.path_prefixes.is_empty());
        assert!(!called.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn missing_rg_reports_download_failure_before_agent_start() {
        let temp = tempdir().expect("temp");
        let home = temp.path().join("home");
        let empty_path = temp.path().join("empty-path");
        fs::create_dir_all(&home).expect("home");
        fs::create_dir_all(&empty_path).expect("path");

        let error = ensure_rg_with_downloader(&env_for(&home, &empty_path), |_| async {
            Err(Error::Message("offline".to_string()))
        })
        .await
        .expect_err("download failure");
        let message = error.to_string();

        assert!(message.contains("before agent start"), "{message}");
        assert!(message.contains("offline"), "{message}");
    }

    #[tokio::test]
    async fn fake_download_install_adds_managed_path_prefix() {
        let temp = tempdir().expect("temp");
        let home = temp.path().join("home");
        let empty_path = temp.path().join("empty-path");
        fs::create_dir_all(&home).expect("home");
        fs::create_dir_all(&empty_path).expect("path");

        let result =
            ensure_rg_with_downloader(&env_for(&home, &empty_path), |tools_dir| async move {
                write_fake_rg(&tools_dir.join(rg_executable_name()), "downloaded");
                Ok(())
            })
            .await
            .expect("ensure rg");

        assert_eq!(result.path_prefixes, vec![home.join("tools")]);
    }
}
