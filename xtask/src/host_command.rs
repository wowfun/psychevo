use std::env;
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result};

pub(crate) fn exists(program: &str) -> bool {
    resolve(program).is_some()
}

pub(crate) fn resolve(program: &str) -> Option<PathBuf> {
    resolve_from(
        program,
        env::var_os("PATH").as_deref(),
        env::var_os("PATHEXT").as_deref(),
        cfg!(windows),
    )
}

pub(crate) fn pnpm<I, S>(args: I) -> Result<Command>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let program = resolve("pnpm").context("resolve pnpm from PATH")?;
    let args = args
        .into_iter()
        .map(|arg| arg.as_ref().to_os_string())
        .collect::<Vec<_>>();

    #[cfg(windows)]
    if is_windows_command_script(&program) {
        return windows_command_script(&program, &args);
    }

    let mut command = Command::new(program);
    command.args(args);
    Ok(command)
}

fn resolve_from(
    program: &str,
    path: Option<&OsStr>,
    path_ext: Option<&OsStr>,
    windows: bool,
) -> Option<PathBuf> {
    let program_path = Path::new(program);
    let extensions = executable_extensions(program_path, path_ext, windows);
    if program_path.components().count() > 1 {
        return resolve_candidate(program_path, &extensions);
    }
    let path = path?;
    env::split_paths(path)
        .find_map(|directory| resolve_candidate(&directory.join(program_path), &extensions))
}

fn resolve_candidate(candidate: &Path, extensions: &[OsString]) -> Option<PathBuf> {
    extensions.iter().find_map(|extension| {
        let mut path = candidate.as_os_str().to_os_string();
        path.push(extension);
        let path = PathBuf::from(path);
        path.is_file().then(|| absolute_path(&path))
    })
}

fn executable_extensions(program: &Path, path_ext: Option<&OsStr>, windows: bool) -> Vec<OsString> {
    if !windows || program.extension().is_some() {
        return vec![OsString::new()];
    }

    let mut extensions = Vec::new();
    let configured = path_ext
        .map(|value| value.to_string_lossy())
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| ".COM;.EXE;.BAT;.CMD".into());
    extensions.extend(configured.split(';').filter_map(|extension| {
        let extension = extension.trim();
        if extension.is_empty() {
            None
        } else if extension.starts_with('.') {
            Some(OsString::from(extension))
        } else {
            Some(OsString::from(format!(".{extension}")))
        }
    }));
    extensions
}

fn absolute_path(path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        env::current_dir()
            .map(|current| current.join(path))
            .unwrap_or_else(|_| path.to_path_buf())
    }
}

#[cfg(windows)]
fn is_windows_command_script(program: &Path) -> bool {
    program
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            extension.eq_ignore_ascii_case("cmd") || extension.eq_ignore_ascii_case("bat")
        })
}

#[cfg(windows)]
fn windows_command_script(program: &Path, args: &[OsString]) -> Result<Command> {
    use std::os::windows::process::CommandExt;

    let command_processor = env::var_os("COMSPEC")
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| OsString::from("cmd.exe"));
    let mut command = Command::new(command_processor);
    command.args(["/D", "/S", "/V:OFF", "/C"]);
    command.raw_arg(windows_command_script_line(program, args)?);
    Ok(command)
}

#[cfg(windows)]
fn windows_command_script_line(program: &Path, args: &[OsString]) -> Result<String> {
    let mut command_line = String::from("\"");
    command_line.push_str(&windows_command_script_token(program.as_os_str())?);
    for arg in args {
        command_line.push(' ');
        command_line.push_str(&windows_command_script_token(arg)?);
    }
    command_line.push('"');
    Ok(command_line)
}

#[cfg(windows)]
fn windows_command_script_token(value: &OsStr) -> Result<String> {
    let value = value
        .to_str()
        .context("Windows command-script paths and arguments must be Unicode")?;
    if value.contains(['\0', '\r', '\n', '"', '%']) {
        anyhow::bail!(
            "Windows command-script paths and arguments cannot contain NUL, newlines, quotes, or percent expansion"
        );
    }
    Ok(format!("\"{value}\""))
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    static NEXT_TEMP_ROOT: AtomicUsize = AtomicUsize::new(0);

    #[cfg(windows)]
    #[test]
    fn windows_resolution_uses_pathext_for_command_shims() {
        let root = test_root("pathext");
        fs::create_dir_all(&root).expect("create test root");
        let shim = root.join("pnpm.CMD");
        fs::write(root.join("pnpm"), "#!/bin/sh\n").expect("write POSIX shim decoy");
        fs::write(&shim, "@echo off\r\n").expect("write command shim");
        let path = env::join_paths([&root]).expect("join test PATH");

        let resolved = resolve_from("pnpm", Some(&path), Some(OsStr::new(".EXE;.CMD")), true)
            .expect("resolve pnpm command shim");

        assert_eq!(resolved, absolute_path(&shim));
        fs::remove_dir_all(root).expect("remove test root");
    }

    #[cfg(windows)]
    #[test]
    fn windows_command_scripts_use_one_quoted_command_line() {
        let line = windows_command_script_line(
            Path::new(r"C:\Program Files\nodejs\pnpm.cmd"),
            &[OsString::from("exec"), OsString::from("tsx")],
        )
        .expect("build command line");

        assert_eq!(line, r#"""C:\Program Files\nodejs\pnpm.cmd" "exec" "tsx"""#);
    }

    #[cfg(unix)]
    #[test]
    fn unix_resolution_uses_extensionless_executables() {
        use std::os::unix::fs::PermissionsExt;

        let root = test_root("unix");
        fs::create_dir_all(&root).expect("create test root");
        let executable = root.join("pnpm");
        fs::write(&executable, "#!/bin/sh\n").expect("write executable");
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o755))
            .expect("make executable");
        let path = env::join_paths([&root]).expect("join test PATH");

        let resolved = resolve_from("pnpm", Some(&path), None, false)
            .expect("resolve extensionless executable");

        assert_eq!(resolved, absolute_path(&executable));
        fs::remove_dir_all(root).expect("remove test root");
    }

    fn test_root(label: &str) -> PathBuf {
        let id = NEXT_TEMP_ROOT.fetch_add(1, Ordering::Relaxed);
        env::temp_dir().join(format!(
            "psychevo-xtask-host-command-{label}-{}-{id}",
            std::process::id()
        ))
    }
}
