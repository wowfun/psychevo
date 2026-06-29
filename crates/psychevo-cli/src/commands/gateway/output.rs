use std::process::{Command, ExitCode, Stdio};

use anyhow::Result;
use serde_json::{Value, json};

use crate::commands::serve::{
    StaticDirResolution, static_dir_build_command, static_dir_install_command,
};

pub(super) fn print_json(value: Value) -> Result<ExitCode> {
    println!("{}", serde_json::to_string(&value)?);
    Ok(ExitCode::SUCCESS)
}

pub(super) fn print_json_code(value: Value) -> Result<ExitCode> {
    println!("{}", serde_json::to_string(&value)?);
    Ok(ExitCode::from(1))
}

pub(super) fn workbench_dist_missing(resolution: &StaticDirResolution) -> Value {
    json!({
        "ok": false,
        "error": {
            "code": "workbench_dist_missing",
            "message": format!("Workbench assets not found at {}", resolution.path.display()),
            "path": resolution.path.display().to_string(),
            "source": resolution.source,
            "searched": resolution.searched.iter().map(|path| path.display().to_string()).collect::<Vec<_>>(),
            "envVar": "PSYCHEVO_WEB_DIST",
            "buildCommand": static_dir_build_command(),
            "installCommand": static_dir_install_command(),
        }
    })
}

pub(super) fn open_browser(url: &str) -> Result<()> {
    #[cfg(target_os = "macos")]
    let mut command = {
        let mut command = Command::new("open");
        command.arg(url);
        command
    };
    #[cfg(target_os = "windows")]
    let mut command = {
        let mut command = Command::new("cmd");
        command.args(["/C", "start", "", url]);
        command
    };
    #[cfg(all(unix, not(target_os = "macos")))]
    let mut command = {
        let mut command = Command::new("xdg-open");
        command.arg(url);
        command
    };
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    Ok(())
}
