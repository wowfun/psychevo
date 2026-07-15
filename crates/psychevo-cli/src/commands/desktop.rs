use std::collections::BTreeMap;
use std::env;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use anyhow::{Context, Result, anyhow};
use psychevo_runtime::{
    ExecutableResolveOptions, HostPlatform, canonicalize_cwd, resolve_executable_path,
    tokio_host_process_command,
};

use crate::args::DesktopArgs;
use crate::commands::serve::source_checkout_roots;
use crate::env::{env_value, inherited_env, resolve_explicit_path};
use crate::profiles::{PROFILE_ENV, PROFILE_HOME_ENV};

pub(crate) const DESKTOP_CWD_ENV: &str = "PSYCHEVO_DESKTOP_CWD";
pub(crate) const PEVO_BIN_ENV: &str = "PSYCHEVO_PEVO_BIN";
const LIBGL_ALWAYS_SOFTWARE_ENV: &str = "LIBGL_ALWAYS_SOFTWARE";
const PNPM_RUNTIME_ENV_DEFAULTS: [(&str, &str); 4] = [
    ("COREPACK_ENABLE_PROJECT_SPEC", "0"),
    ("COREPACK_ENABLE_DOWNLOAD_PROMPT", "0"),
    ("COREPACK_ENABLE_STRICT", "0"),
    ("pnpm_config_pm_on_fail", "warn"),
];

pub(crate) async fn run_desktop_command(args: DesktopArgs) -> Result<ExitCode> {
    let env_map = inherited_env();
    let cwd = env::current_dir().context("resolve current directory")?;
    let desktop_cwd = resolve_desktop_cwd(args.dir.as_deref(), &env_map, &cwd)?;
    let source_root = desktop_source_root(&cwd)?;
    let pevo_bin = env::current_exe().context("resolve current pevo executable")?;
    let mut command = desktop_command_for_platform(
        &source_root,
        &desktop_cwd,
        &pevo_bin,
        &env_map,
        HostPlatform::current(),
    )?;
    apply_desktop_runtime_env(command.as_std_mut(), &env_map);

    let status = command
        .status()
        .await
        .with_context(|| "run `pnpm --filter @psychevo/desktop tauri:dev`")?;
    Ok(exit_code_from_status(status))
}

fn desktop_source_root(cwd: &Path) -> Result<PathBuf> {
    select_desktop_source_root(source_checkout_roots(cwd)).ok_or_else(|| {
        anyhow!(
            "Psychevo Desktop source checkout not found; run `pevo desktop` from a source checkout that contains apps/desktop"
        )
    })
}

fn select_desktop_source_root(candidates: Vec<PathBuf>) -> Option<PathBuf> {
    candidates
        .into_iter()
        .find(|candidate| is_desktop_source_root(candidate))
}

fn is_desktop_source_root(root: &Path) -> bool {
    root.join("apps/desktop/package.json").is_file()
        && root
            .join("apps/desktop/src-tauri/tauri.conf.json")
            .is_file()
}

fn desktop_command_for_platform(
    source_root: &Path,
    desktop_cwd: &Path,
    pevo_bin: &Path,
    env_map: &BTreeMap<String, String>,
    platform: HostPlatform,
) -> Result<tokio::process::Command> {
    let pnpm = resolve_executable_path(
        "pnpm",
        source_root,
        &ExecutableResolveOptions {
            platform,
            env: env_map,
        },
    )
    .ok_or_else(|| anyhow!("pnpm not found; install pnpm before running `pevo desktop`"))?;
    let args = ["--filter", "@psychevo/desktop", "tauri:dev"].map(OsString::from);
    let mut command = tokio_host_process_command(&pnpm, &args, platform, env_map)?;
    {
        let command = command.as_std_mut();
        command
            .current_dir(source_root)
            .env(DESKTOP_CWD_ENV, desktop_cwd);
        apply_pevo_bin_env(command, pevo_bin);
        apply_profile_env(command, env_map);
        apply_pnpm_runtime_env(command, env_map);
    }
    Ok(command)
}

fn apply_pnpm_runtime_env(command: &mut Command, env_map: &BTreeMap<String, String>) {
    for (name, value) in PNPM_RUNTIME_ENV_DEFAULTS {
        if psychevo_runtime::env_value_case_insensitive(env_map, name).is_none() {
            command.env(name, value);
        }
    }
}

fn resolve_desktop_cwd(
    dir: Option<&Path>,
    env_map: &BTreeMap<String, String>,
    cwd: &Path,
) -> Result<PathBuf> {
    let path = match dir {
        Some(dir) => resolve_explicit_path(dir, env_map, cwd)?,
        None => cwd.to_path_buf(),
    };
    Ok(canonicalize_cwd(&path)?)
}

fn apply_profile_env(command: &mut Command, env_map: &BTreeMap<String, String>) {
    for name in ["PSYCHEVO_HOME", PROFILE_ENV, PROFILE_HOME_ENV] {
        if let Some(value) = env_map.get(name) {
            command.env(name, value);
        }
    }
}

fn apply_pevo_bin_env(command: &mut Command, pevo_bin: &Path) {
    command.env(PEVO_BIN_ENV, pevo_bin);
}

fn apply_desktop_runtime_env(command: &mut Command, env_map: &BTreeMap<String, String>) {
    apply_wsl_software_gl_env_for(
        command,
        env_map,
        cfg!(target_os = "linux"),
        is_probably_wsl(env_map),
    );
}

fn apply_wsl_software_gl_env_for(
    command: &mut Command,
    env_map: &BTreeMap<String, String>,
    linux: bool,
    wsl: bool,
) {
    if let Some((key, value)) = wsl_software_gl_default_for(env_map, linux, wsl) {
        command.env(key, value);
    }
}

fn wsl_software_gl_default_for(
    env_map: &BTreeMap<String, String>,
    linux: bool,
    wsl: bool,
) -> Option<(&'static str, &'static str)> {
    if linux && wsl && env_value(LIBGL_ALWAYS_SOFTWARE_ENV, env_map).is_none() {
        return Some((LIBGL_ALWAYS_SOFTWARE_ENV, "1"));
    }
    None
}

fn is_probably_wsl(env_map: &BTreeMap<String, String>) -> bool {
    let proc_version = std::fs::read_to_string("/proc/version").ok();
    let os_release = std::fs::read_to_string("/proc/sys/kernel/osrelease").ok();
    is_probably_wsl_from(
        proc_version.as_deref(),
        os_release.as_deref(),
        env_value("WSL_DISTRO_NAME", env_map).is_some(),
        env_value("WSL_INTEROP", env_map).is_some(),
    )
}

fn is_probably_wsl_from(
    proc_version: Option<&str>,
    os_release: Option<&str>,
    distro_env: bool,
    interop_env: bool,
) -> bool {
    proc_version.is_some_and(contains_wsl_marker)
        || os_release.is_some_and(contains_wsl_marker)
        || distro_env
        || interop_env
}

fn contains_wsl_marker(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    lower.contains("microsoft") || lower.contains("wsl")
}

fn exit_code_from_status(status: std::process::ExitStatus) -> ExitCode {
    if let Some(code) = status.code().and_then(|code| u8::try_from(code).ok()) {
        return ExitCode::from(code);
    }
    ExitCode::FAILURE
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::process::Command;

    use tempfile::tempdir;

    use super::*;

    fn command_env_value(command: &Command, name: &str) -> Option<String> {
        command
            .get_envs()
            .find_map(|(key, value)| {
                (key == name).then(|| value.map(|value| value.to_string_lossy().to_string()))
            })
            .flatten()
    }

    #[test]
    fn desktop_source_root_requires_desktop_app_files() {
        let temp = tempdir().expect("temp");
        let missing = temp.path().join("missing");
        let root = temp.path().join("root");
        fs::create_dir_all(root.join("apps/desktop/src-tauri")).expect("desktop dir");
        fs::write(root.join("apps/desktop/package.json"), "{}").expect("package");
        fs::write(root.join("apps/desktop/src-tauri/tauri.conf.json"), "{}").expect("tauri config");

        assert_eq!(
            select_desktop_source_root(vec![missing, root.clone()]),
            Some(root)
        );
    }

    #[test]
    fn resolve_desktop_cwd_uses_invocation_cwd_by_default() {
        let temp = tempdir().expect("temp");
        let cwd = temp.path().join("workspace");
        fs::create_dir_all(&cwd).expect("cwd");
        let env_map = BTreeMap::new();

        assert_eq!(resolve_desktop_cwd(None, &env_map, &cwd).unwrap(), cwd);
    }

    #[test]
    fn resolve_desktop_cwd_resolves_relative_dir_against_invocation_cwd() {
        let temp = tempdir().expect("temp");
        let cwd = temp.path().join("workspace");
        let project = cwd.join("project");
        fs::create_dir_all(&project).expect("project");
        let env_map = BTreeMap::new();

        assert_eq!(
            resolve_desktop_cwd(Some(Path::new("project")), &env_map, &cwd).unwrap(),
            project
        );
    }

    #[test]
    fn desktop_command_reports_missing_pnpm() {
        let temp = tempdir().expect("temp");
        let source_root = temp.path().join("source");
        let desktop_cwd = temp.path().join("workspace");
        fs::create_dir_all(&source_root).expect("source root");
        fs::create_dir_all(&desktop_cwd).expect("desktop cwd");
        let env_map = BTreeMap::from([(
            "PATH".to_string(),
            temp.path().join("missing").display().to_string(),
        )]);

        let error = desktop_command_for_platform(
            &source_root,
            &desktop_cwd,
            Path::new("/tmp/pevo"),
            &env_map,
            psychevo_runtime::HostPlatform::Posix,
        )
        .expect_err("missing pnpm");

        assert_eq!(
            error.to_string(),
            "pnpm not found; install pnpm before running `pevo desktop`"
        );
    }

    #[test]
    fn desktop_command_launches_windows_pnpm_shim_from_source_root() {
        let temp = tempdir().expect("temp");
        let source_root = temp.path().join("source");
        let desktop_cwd = temp.path().join("workspace");
        let bin = temp.path().join("bin");
        fs::create_dir_all(&source_root).expect("source root");
        fs::create_dir_all(&desktop_cwd).expect("desktop cwd");
        fs::create_dir_all(&bin).expect("bin");
        let pnpm = bin.join("pnpm.cmd");
        fs::write(&pnpm, "@echo off\n").expect("pnpm shim");
        let env_map = BTreeMap::from([
            ("PATH".to_string(), bin.display().to_string()),
            ("PATHEXT".to_string(), ".CMD".to_string()),
            (
                "COMSPEC".to_string(),
                r"C:\Windows\System32\cmd.exe".to_string(),
            ),
        ]);

        let command = desktop_command_for_platform(
            &source_root,
            &desktop_cwd,
            Path::new(r"C:\Tools\pevo.exe"),
            &env_map,
            psychevo_runtime::HostPlatform::Windows,
        )
        .expect("desktop command");
        let command = command.as_std();
        let args = command
            .get_args()
            .map(|arg| arg.to_string_lossy().to_string())
            .collect::<Vec<_>>();

        assert_eq!(
            command.get_program(),
            std::ffi::OsStr::new(r"C:\Windows\System32\cmd.exe")
        );
        assert_eq!(&args[..4], ["/D", "/S", "/V:OFF", "/C"]);
        #[cfg(not(windows))]
        {
            assert!(args[4].contains(&pnpm.display().to_string()), "{args:?}");
            assert!(args[4].contains("\"--filter\""), "{args:?}");
            assert!(args[4].contains("\"@psychevo/desktop\""), "{args:?}");
            assert!(args[4].contains("\"tauri:dev\""), "{args:?}");
        }
        assert_eq!(command.get_current_dir(), Some(source_root.as_path()));
    }

    #[test]
    fn desktop_command_launches_posix_pnpm_directly() {
        let temp = tempdir().expect("temp");
        let source_root = temp.path().join("source");
        let desktop_cwd = temp.path().join("workspace");
        let bin = temp.path().join("bin");
        fs::create_dir_all(&source_root).expect("source root");
        fs::create_dir_all(&desktop_cwd).expect("desktop cwd");
        fs::create_dir_all(&bin).expect("bin");
        let pnpm = bin.join("pnpm");
        fs::write(&pnpm, "#!/bin/sh\n").expect("pnpm");
        let env_map = BTreeMap::from([("PATH".to_string(), bin.display().to_string())]);

        let command = desktop_command_for_platform(
            &source_root,
            &desktop_cwd,
            Path::new("/tmp/pevo"),
            &env_map,
            psychevo_runtime::HostPlatform::Posix,
        )
        .expect("desktop command");
        let command = command.as_std();

        assert_eq!(command.get_program(), pnpm.as_os_str());
        assert_eq!(
            command.get_args().collect::<Vec<_>>(),
            ["--filter", "@psychevo/desktop", "tauri:dev"]
                .map(std::ffi::OsStr::new)
                .to_vec()
        );
    }

    #[test]
    fn desktop_command_defaults_corepack_to_installed_pnpm() {
        let temp = tempdir().expect("temp");
        let source_root = temp.path().join("source");
        let desktop_cwd = temp.path().join("workspace");
        let bin = temp.path().join("bin");
        fs::create_dir_all(&source_root).expect("source root");
        fs::create_dir_all(&desktop_cwd).expect("desktop cwd");
        fs::create_dir_all(&bin).expect("bin");
        fs::write(bin.join("pnpm"), "#!/bin/sh\n").expect("pnpm");
        let env_map = BTreeMap::from([("PATH".to_string(), bin.display().to_string())]);

        let command = desktop_command_for_platform(
            &source_root,
            &desktop_cwd,
            Path::new("/tmp/pevo"),
            &env_map,
            psychevo_runtime::HostPlatform::Posix,
        )
        .expect("desktop command");
        let command = command.as_std();

        for (name, expected) in [
            ("COREPACK_ENABLE_PROJECT_SPEC", "0"),
            ("COREPACK_ENABLE_DOWNLOAD_PROMPT", "0"),
            ("COREPACK_ENABLE_STRICT", "0"),
            ("pnpm_config_pm_on_fail", "warn"),
        ] {
            assert_eq!(
                command_env_value(command, name).as_deref(),
                Some(expected),
                "{name}"
            );
        }
    }

    #[test]
    fn desktop_command_preserves_explicit_corepack_settings() {
        let temp = tempdir().expect("temp");
        let source_root = temp.path().join("source");
        let desktop_cwd = temp.path().join("workspace");
        let bin = temp.path().join("bin");
        fs::create_dir_all(&source_root).expect("source root");
        fs::create_dir_all(&desktop_cwd).expect("desktop cwd");
        fs::create_dir_all(&bin).expect("bin");
        fs::write(bin.join("pnpm"), "#!/bin/sh\n").expect("pnpm");
        let env_map = BTreeMap::from([
            ("PATH".to_string(), bin.display().to_string()),
            ("COREPACK_ENABLE_PROJECT_SPEC".to_string(), "1".to_string()),
            (
                "COREPACK_ENABLE_DOWNLOAD_PROMPT".to_string(),
                "1".to_string(),
            ),
            ("COREPACK_ENABLE_STRICT".to_string(), "1".to_string()),
            ("pnpm_config_pm_on_fail".to_string(), "error".to_string()),
        ]);

        let command = desktop_command_for_platform(
            &source_root,
            &desktop_cwd,
            Path::new("/tmp/pevo"),
            &env_map,
            psychevo_runtime::HostPlatform::Posix,
        )
        .expect("desktop command");
        let command = command.as_std();

        for (name, _) in PNPM_RUNTIME_ENV_DEFAULTS {
            assert_eq!(
                command_env_value(command, name),
                None,
                "explicit {name} must remain inherited"
            );
        }
    }

    #[test]
    fn pevo_desktop_passes_current_pevo_binary_to_tauri_child() {
        let mut command = Command::new("pnpm");

        apply_pevo_bin_env(&mut command, Path::new("/tmp/pevo"));

        assert_eq!(
            command_env_value(&command, PEVO_BIN_ENV).as_deref(),
            Some("/tmp/pevo")
        );
    }

    #[test]
    fn detects_wsl_from_proc_version_marker() {
        assert!(is_probably_wsl_from(
            Some("Linux version 6.18.33.1-microsoft-standard-WSL2"),
            None,
            false,
            false,
        ));
    }

    #[test]
    fn detects_wsl_from_os_release_marker() {
        assert!(is_probably_wsl_from(
            None,
            Some("6.18.33.1-microsoft-standard-WSL2"),
            false,
            false,
        ));
    }

    #[test]
    fn detects_wsl_from_environment_markers() {
        assert!(is_probably_wsl_from(None, None, true, false));
        assert!(is_probably_wsl_from(None, None, false, true));
        assert!(!is_probably_wsl_from(None, None, false, false));
    }

    #[test]
    fn desktop_runtime_env_defaults_to_software_gl_on_wsl_linux() {
        let env_map = BTreeMap::new();
        let mut command = Command::new("pnpm");

        apply_wsl_software_gl_env_for(&mut command, &env_map, true, true);

        assert_eq!(
            command_env_value(&command, LIBGL_ALWAYS_SOFTWARE_ENV).as_deref(),
            Some("1")
        );
    }

    #[test]
    fn desktop_runtime_env_preserves_explicit_software_gl_setting() {
        let env_map = BTreeMap::from([(LIBGL_ALWAYS_SOFTWARE_ENV.to_string(), "0".to_string())]);
        let mut command = Command::new("pnpm");

        apply_wsl_software_gl_env_for(&mut command, &env_map, true, true);

        assert_eq!(command_env_value(&command, LIBGL_ALWAYS_SOFTWARE_ENV), None);
    }

    #[test]
    fn desktop_runtime_env_does_not_set_software_gl_on_non_wsl_linux() {
        let env_map = BTreeMap::new();
        let mut command = Command::new("pnpm");

        apply_wsl_software_gl_env_for(&mut command, &env_map, true, false);

        assert_eq!(command_env_value(&command, LIBGL_ALWAYS_SOFTWARE_ENV), None);
    }

    #[test]
    fn desktop_runtime_env_does_not_set_software_gl_off_linux() {
        let env_map = BTreeMap::new();
        let mut command = Command::new("pnpm");

        apply_wsl_software_gl_env_for(&mut command, &env_map, false, true);

        assert_eq!(command_env_value(&command, LIBGL_ALWAYS_SOFTWARE_ENV), None);
    }
}
