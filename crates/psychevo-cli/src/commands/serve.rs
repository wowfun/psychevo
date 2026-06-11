use std::collections::BTreeSet;
use std::env;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Result, anyhow};
use psychevo_gateway::{Gateway, GatewayWebServerConfig, bind_gateway_web_server};
use psychevo_runtime::{StateRuntime, canonicalize_workdir};
use serde_json::json;

use crate::args::ServeArgs;
use crate::env::{
    ensure_home_initialized, env_path, env_value, inherited_env, resolve_explicit_path,
    resolve_psychevo_home, resolve_state_db,
};

pub(crate) async fn run_serve_command(args: ServeArgs) -> Result<ExitCode> {
    let env_map = inherited_env();
    let cwd = env::current_dir()?;
    let home = resolve_psychevo_home(&env_map, &cwd)?;
    let config_path = env_path("PSYCHEVO_CONFIG", &env_map, &cwd)?;
    let db_path = resolve_state_db(&env_map, &home, &cwd)?;
    let bypass_home = config_path.is_some() && env_value("PSYCHEVO_DB", &env_map).is_some();
    if !bypass_home {
        ensure_home_initialized(&home)?;
    }

    let requested_workdir = match &args.dir {
        Some(dir) => resolve_explicit_path(dir, &env_map, &cwd)?,
        None => cwd.clone(),
    };
    let workdir = canonicalize_workdir(&requested_workdir)?;
    let token = serve_token(&args, &env_map, &cwd)?;
    let static_dir = args
        .static_dir
        .as_deref()
        .map(|path| resolve_explicit_path(path, &env_map, &cwd))
        .transpose()?;
    let managed_state = args
        .managed_state
        .as_deref()
        .map(|path| resolve_explicit_path(path, &env_map, &cwd))
        .transpose()?;

    let profile_home = home.clone();
    let state = StateRuntime::open(&db_path)?;
    let gateway = Gateway::new(state);
    let profile_name = env_value(crate::profiles::PROFILE_ENV, &env_map)
        .unwrap_or_else(|| crate::profiles::DEFAULT_PROFILE.to_string());
    let mut config =
        GatewayWebServerConfig::headless(gateway, home, workdir, config_path, env_map, token);
    config.bind_addr = args.bind;
    config.bind_port_fallbacks = args.bind_fallbacks;
    config.static_dir = static_dir;
    config.managed_state_path = managed_state;

    let bound = bind_gateway_web_server(config).await?;
    println!(
        "{}",
        serde_json::to_string(&json!({
            "ready": true,
            "baseUrl": bound.url(),
            "readyzUrl": format!("{}/readyz", bound.url()),
            "pid": std::process::id(),
            "version": env!("CARGO_PKG_VERSION"),
            "profile": profile_name,
            "profileHome": profile_home,
        }))?
    );
    bound.run().await?;
    Ok(ExitCode::SUCCESS)
}

fn serve_token(
    args: &ServeArgs,
    env_map: &std::collections::BTreeMap<String, String>,
    cwd: &Path,
) -> Result<String> {
    if let Some(path) = &args.token_file {
        let path = resolve_explicit_path(path, env_map, cwd)?;
        let token = std::fs::read_to_string(&path)?;
        let token = token.trim().to_string();
        if token.is_empty() {
            return Err(anyhow!("token file is empty: {}", path.display()));
        }
        return Ok(token);
    }
    env_value("PSYCHEVO_SERVE_TOKEN", env_map)
        .filter(|token| !token.is_empty())
        .ok_or_else(|| anyhow!("pevo serve requires PSYCHEVO_SERVE_TOKEN or --token-file"))
}

#[allow(dead_code)]
pub(crate) fn resolve_static_dir(
    explicit: Option<&Path>,
    env_map: &std::collections::BTreeMap<String, String>,
    cwd: &Path,
) -> Result<PathBuf> {
    Ok(resolve_static_dir_diagnostic(explicit, env_map, cwd)?.path)
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub(crate) struct StaticDirResolution {
    pub(crate) path: PathBuf,
    pub(crate) source: &'static str,
    pub(crate) searched: Vec<PathBuf>,
}

impl StaticDirResolution {
    pub(crate) fn found(&self) -> bool {
        self.path.join("index.html").exists()
    }
}

#[allow(dead_code)]
pub(crate) fn resolve_static_dir_diagnostic(
    explicit: Option<&Path>,
    env_map: &std::collections::BTreeMap<String, String>,
    cwd: &Path,
) -> Result<StaticDirResolution> {
    if let Some(path) = explicit {
        let path = resolve_explicit_path(path, env_map, cwd)?;
        return Ok(StaticDirResolution {
            searched: vec![path.clone()],
            path,
            source: "explicit",
        });
    }
    if let Some(path) = env_value("PSYCHEVO_WEB_DIST", env_map) {
        let path = resolve_explicit_path(Path::new(&path), env_map, cwd)?;
        return Ok(StaticDirResolution {
            searched: vec![path.clone()],
            path,
            source: "env",
        });
    }
    let candidates = static_dir_candidates(cwd);
    for candidate in &candidates {
        if candidate.join("index.html").exists() {
            return Ok(StaticDirResolution {
                path: candidate.clone(),
                source: static_dir_source(candidate, cwd),
                searched: candidates,
            });
        }
    }
    Ok(StaticDirResolution {
        path: cwd.join("apps/workbench/dist"),
        source: "missing",
        searched: candidates,
    })
}

#[allow(dead_code)]
pub(crate) fn static_dir_candidates(cwd: &Path) -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    let mut seen = BTreeSet::new();
    if let Some(path) = static_install_share_dir() {
        push_unique(&mut candidates, &mut seen, path);
    }
    for root in source_checkout_roots(cwd) {
        push_unique(&mut candidates, &mut seen, root.join("apps/workbench/dist"));
    }
    push_unique(&mut candidates, &mut seen, cwd.join("apps/workbench/dist"));
    candidates
}

#[allow(dead_code)]
pub(crate) fn static_dir_build_command() -> &'static str {
    "pnpm --filter @psychevo/workbench build"
}

#[allow(dead_code)]
pub(crate) fn static_dir_install_command() -> &'static str {
    "scripts/install.sh"
}

fn push_unique(candidates: &mut Vec<PathBuf>, seen: &mut BTreeSet<String>, path: PathBuf) {
    let key = path.to_string_lossy().replace('\\', "/");
    if seen.insert(key) {
        candidates.push(path);
    }
}

fn static_dir_source(candidate: &Path, cwd: &Path) -> &'static str {
    if candidate == cwd.join("apps/workbench/dist") {
        return "cwd";
    }
    if static_install_share_dir().as_deref() == Some(candidate) {
        return "install-share";
    }
    "source-checkout"
}

#[allow(dead_code)]
pub(crate) fn static_install_share_dir() -> Option<PathBuf> {
    env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|dir| dir.join("../share/psychevo/web")))
}

#[allow(dead_code)]
pub(crate) fn source_checkout_roots(cwd: &Path) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    let mut seen = BTreeSet::new();
    for start in [
        Some(PathBuf::from(env!("CARGO_MANIFEST_DIR"))),
        env::current_exe().ok(),
        Some(cwd.to_path_buf()),
    ]
    .into_iter()
    .flatten()
    {
        if let Some(root) = source_checkout_root_from(&start) {
            push_unique(&mut roots, &mut seen, root);
        }
    }
    roots
}

fn source_checkout_root_from(start: &Path) -> Option<PathBuf> {
    let mut current = if start.is_file() {
        start.parent()?.to_path_buf()
    } else {
        start.to_path_buf()
    };
    loop {
        if current.join("Cargo.toml").exists()
            && current.join("crates/psychevo-cli/Cargo.toml").exists()
            && current.join("apps/workbench").is_dir()
        {
            return Some(current);
        }
        if !current.pop() {
            return None;
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use tempfile::tempdir;

    use super::*;

    #[test]
    fn explicit_static_dir_wins() {
        let temp = tempdir().expect("temp");
        let cwd = temp.path();
        let resolution =
            resolve_static_dir_diagnostic(Some(Path::new("custom-dist")), &BTreeMap::new(), cwd)
                .expect("resolution");
        assert_eq!(resolution.source, "explicit");
        assert_eq!(resolution.path, cwd.join("custom-dist"));
        assert_eq!(resolution.searched, vec![cwd.join("custom-dist")]);
    }

    #[test]
    fn env_static_dir_wins() {
        let temp = tempdir().expect("temp");
        let cwd = temp.path();
        let mut env = BTreeMap::new();
        env.insert("PSYCHEVO_WEB_DIST".to_string(), "env-dist".to_string());
        let resolution = resolve_static_dir_diagnostic(None, &env, cwd).expect("resolution");
        assert_eq!(resolution.source, "env");
        assert_eq!(resolution.path, cwd.join("env-dist"));
        assert_eq!(resolution.searched, vec![cwd.join("env-dist")]);
    }

    #[test]
    fn source_checkout_root_is_recognized() {
        let temp = tempdir().expect("temp");
        let root = temp.path();
        std::fs::write(root.join("Cargo.toml"), "[workspace]\n").expect("root cargo");
        std::fs::create_dir_all(root.join("crates/psychevo-cli")).expect("cli dir");
        std::fs::write(root.join("crates/psychevo-cli/Cargo.toml"), "[package]\n")
            .expect("cli cargo");
        std::fs::create_dir_all(root.join("apps/workbench")).expect("workbench");
        let nested = root.join("crates/psychevo-cli/src");
        std::fs::create_dir_all(&nested).expect("nested");

        assert_eq!(source_checkout_root_from(&nested), Some(root.to_path_buf()));
    }

    #[test]
    fn missing_static_dir_keeps_diagnostic_search_list() {
        let temp = tempdir().expect("temp");
        let cwd = temp.path();
        let mut env = BTreeMap::new();
        env.insert(
            "PSYCHEVO_WEB_DIST".to_string(),
            "missing-env-dist".to_string(),
        );
        let resolution = resolve_static_dir_diagnostic(None, &env, cwd).expect("resolution");
        assert_eq!(resolution.source, "env");
        assert!(!resolution.found());
        assert_eq!(resolution.path, cwd.join("missing-env-dist"));
        assert_eq!(resolution.searched, vec![cwd.join("missing-env-dist")]);
        assert_eq!(
            static_dir_build_command(),
            "pnpm --filter @psychevo/workbench build"
        );
    }
}
