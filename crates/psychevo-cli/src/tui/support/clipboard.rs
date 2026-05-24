#[allow(unused_imports)]
pub(crate) use super::*;
pub(crate) fn default_clipboard_sink() -> ClipboardSink {
    Arc::new(copy_text_to_clipboard)
}

pub(crate) fn copy_text_to_clipboard(text: &str) -> io::Result<()> {
    copy_text_to_clipboard_with(
        text,
        ClipboardEnvironment {
            ssh_session: is_ssh_session(),
            tmux_session: is_tmux_session(),
        },
        local_clipboard_commands(),
        |candidate, text| pipe_to_command(candidate.command, candidate.args, text),
        tmux_clipboard_copy,
        write_osc52_clipboard,
    )
}

pub(crate) fn copy_text_to_clipboard_with(
    text: &str,
    environment: ClipboardEnvironment,
    candidates: Vec<ClipboardCommand>,
    mut local_copy: impl FnMut(ClipboardCommand, &str) -> io::Result<bool>,
    mut tmux_copy: impl FnMut(&str) -> io::Result<()>,
    mut osc52_copy: impl FnMut(&str) -> io::Result<()>,
) -> io::Result<()> {
    if text.is_empty() {
        return Ok(());
    }

    let osc52_result = osc52_copy(text);
    if environment.ssh_session {
        return terminal_clipboard_copy_with(
            text,
            environment.tmux_session,
            osc52_result,
            &mut tmux_copy,
        )
        .map_err(|err| io::Error::other(format!("clipboard copy failed: {err}")));
    }

    let mut failures = Vec::new();
    for candidate in candidates {
        match local_copy(candidate, text) {
            Ok(true) => return Ok(()),
            Ok(false) => failures.push(format!("{} unavailable", candidate.command)),
            Err(err) => failures.push(format!("{}: {err}", candidate.command)),
        }
    }
    match osc52_result {
        Ok(()) => Ok(()),
        Err(err) => {
            failures.push(format!("OSC52: {err}"));
            Err(io::Error::other(format!(
                "clipboard copy failed: {}",
                clipboard_failure_summary(&failures)
            )))
        }
    }
}

pub(crate) fn terminal_clipboard_copy_with(
    text: &str,
    tmux_session: bool,
    osc52_result: io::Result<()>,
    tmux_copy: &mut impl FnMut(&str) -> io::Result<()>,
) -> io::Result<()> {
    if !tmux_session {
        return osc52_result.map_err(|err| io::Error::other(format!("OSC52: {err}")));
    }

    let tmux_result = tmux_copy(text);
    if osc52_result.is_ok() || tmux_result.is_ok() {
        return Ok(());
    }

    let osc52_err = osc52_result.expect_err("checked OSC52 error");
    let tmux_err = tmux_result.expect_err("checked tmux error");
    Err(io::Error::other(format!(
        "OSC52: {osc52_err}; tmux: {tmux_err}"
    )))
}

pub(crate) fn clipboard_failure_summary(failures: &[String]) -> String {
    if failures.is_empty() {
        return "no clipboard backend succeeded".to_string();
    }
    let summary = failures.join("; ");
    truncate_chars(&summary, 240)
}

pub(crate) fn write_osc52_clipboard(text: &str) -> io::Result<()> {
    let sequence = osc52_sequence(text)?;
    #[cfg(unix)]
    {
        if let Ok(tty) = std::fs::OpenOptions::new().write(true).open("/dev/tty")
            && write_osc52_sequence(tty, &sequence).is_ok()
        {
            return Ok(());
        }
    }
    write_osc52_sequence(io::stdout(), &sequence)
}

pub(crate) fn write_osc52_sequence(mut writer: impl Write, sequence: &str) -> io::Result<()> {
    writer.write_all(sequence.as_bytes())?;
    writer.flush()
}

pub(crate) fn osc52_sequence(text: &str) -> io::Result<String> {
    osc52_sequence_with_passthrough(
        text,
        std::env::var_os("TMUX").is_some() || std::env::var_os("STY").is_some(),
    )
}

pub(crate) fn osc52_sequence_with_passthrough(text: &str, passthrough: bool) -> io::Result<String> {
    const OSC52_MAX_RAW_BYTES: usize = 100_000;
    let raw_bytes = text.len();
    if raw_bytes > OSC52_MAX_RAW_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("OSC52 payload too large ({raw_bytes} bytes; max {OSC52_MAX_RAW_BYTES})"),
        ));
    }
    let encoded = base64_encode(text.as_bytes());
    if passthrough {
        Ok(format!("\x1bPtmux;\x1b\x1b]52;c;{encoded}\x07\x1b\\"))
    } else {
        Ok(format!("\x1b]52;c;{encoded}\x07"))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ClipboardCommand {
    pub(crate) command: &'static str,
    pub(crate) args: &'static [&'static str],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ClipboardEnvironment {
    pub(crate) ssh_session: bool,
    pub(crate) tmux_session: bool,
}

pub(crate) const NO_ARGS: &[&str] = &[];
pub(crate) const POWERSHELL_CLIPBOARD_ARGS: &[&str] = &[
    "-NonInteractive",
    "-NoProfile",
    "-Command",
    "[Console]::InputEncoding = [System.Text.Encoding]::UTF8; $ErrorActionPreference = 'Stop'; $text = [Console]::In.ReadToEnd(); Set-Clipboard -Value $text",
];
pub(crate) const XCLIP_CLIPBOARD_ARGS: &[&str] = &["-selection", "clipboard"];
pub(crate) const XSEL_CLIPBOARD_ARGS: &[&str] = &["--clipboard", "--input"];

pub(crate) fn local_clipboard_commands() -> Vec<ClipboardCommand> {
    local_clipboard_commands_for(
        cfg!(target_os = "macos"),
        cfg!(target_os = "windows"),
        is_probably_wsl(),
        is_wayland_session(),
    )
}

pub(crate) fn is_ssh_session() -> bool {
    std::env::var_os("SSH_TTY").is_some() || std::env::var_os("SSH_CONNECTION").is_some()
}

pub(crate) fn is_tmux_session() -> bool {
    std::env::var_os("TMUX").is_some() || std::env::var_os("TMUX_PANE").is_some()
}

pub(crate) fn is_wayland_session() -> bool {
    std::env::var_os("WAYLAND_DISPLAY").is_some()
        || std::env::var("XDG_SESSION_TYPE")
            .is_ok_and(|value| value.eq_ignore_ascii_case("wayland"))
}

pub(crate) fn is_probably_wsl() -> bool {
    let proc_version = std::fs::read_to_string("/proc/version").ok();
    let os_release = std::fs::read_to_string("/proc/sys/kernel/osrelease").ok();
    is_probably_wsl_from(
        proc_version.as_deref(),
        os_release.as_deref(),
        std::env::var_os("WSL_DISTRO_NAME").is_some(),
        std::env::var_os("WSL_INTEROP").is_some(),
    )
}

pub(crate) fn is_probably_wsl_from(
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

pub(crate) fn contains_wsl_marker(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    lower.contains("microsoft") || lower.contains("wsl")
}

pub(crate) fn local_clipboard_commands_for(
    macos: bool,
    windows: bool,
    wsl: bool,
    wayland: bool,
) -> Vec<ClipboardCommand> {
    if macos {
        return vec![ClipboardCommand {
            command: "pbcopy",
            args: NO_ARGS,
        }];
    }
    if windows {
        return vec![ClipboardCommand {
            command: "powershell.exe",
            args: POWERSHELL_CLIPBOARD_ARGS,
        }];
    }

    let mut candidates = Vec::new();
    if wsl {
        candidates.push(ClipboardCommand {
            command: "powershell.exe",
            args: POWERSHELL_CLIPBOARD_ARGS,
        });
        candidates.push(ClipboardCommand {
            command: "clip.exe",
            args: NO_ARGS,
        });
    }
    if wayland {
        candidates.push(ClipboardCommand {
            command: "wl-copy",
            args: NO_ARGS,
        });
    }
    candidates.push(ClipboardCommand {
        command: "xclip",
        args: XCLIP_CLIPBOARD_ARGS,
    });
    candidates.push(ClipboardCommand {
        command: "xsel",
        args: XSEL_CLIPBOARD_ARGS,
    });
    candidates
}

pub(crate) fn pipe_to_command(command: &str, args: &[&str], text: &str) -> io::Result<bool> {
    let mut child = match StdCommand::new(command)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(child) => child,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(err) => return Err(err),
    };
    if let Some(stdin) = child.stdin.as_mut() {
        stdin.write_all(text.as_bytes())?;
    }
    drop(child.stdin.take());
    let status = child.wait()?;
    Ok(status.success())
}

pub(crate) fn tmux_clipboard_copy(text: &str) -> io::Result<()> {
    tmux_clipboard_copy_ready(
        || tmux_command_output(["show-options", "-gv", "set-clipboard"]),
        || tmux_command_output(["info"]),
    )?;
    pipe_to_required_command("tmux", &["load-buffer", "-w", "-"], text)
}

pub(crate) fn tmux_clipboard_copy_ready(
    set_clipboard_fn: impl FnOnce() -> io::Result<String>,
    tmux_info_fn: impl FnOnce() -> io::Result<String>,
) -> io::Result<()> {
    let set_clipboard = set_clipboard_fn()?;
    if set_clipboard.trim() == "off" {
        return Err(io::Error::other("tmux clipboard forwarding is disabled"));
    }

    let tmux_info = tmux_info_fn()?;
    if tmux_info.lines().any(|line| line.contains("Ms: [missing]")) {
        return Err(io::Error::other(
            "tmux clipboard forwarding is unavailable: missing Ms capability",
        ));
    }

    Ok(())
}

pub(crate) fn tmux_command_output<const N: usize>(args: [&str; N]) -> io::Result<String> {
    let output = StdCommand::new("tmux").args(args).output()?;
    if output.status.success() {
        return String::from_utf8(output.stdout)
            .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err));
    }

    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    if stderr.is_empty() {
        Err(io::Error::other(format!(
            "tmux exited with status {}",
            output.status
        )))
    } else {
        Err(io::Error::other(format!("tmux failed: {stderr}")))
    }
}

pub(crate) fn pipe_to_required_command(command: &str, args: &[&str], text: &str) -> io::Result<()> {
    let mut child = StdCommand::new(command)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()?;
    let Some(mut stdin) = child.stdin.take() else {
        let _ = child.kill();
        let _ = child.wait();
        return Err(io::Error::other(format!("failed to open {command} stdin")));
    };
    if let Err(err) = stdin.write_all(text.as_bytes()) {
        let _ = child.kill();
        let _ = child.wait();
        return Err(err);
    }
    drop(stdin);

    let output = child.wait_with_output()?;
    if output.status.success() {
        return Ok(());
    }

    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    if stderr.is_empty() {
        Err(io::Error::other(format!(
            "{command} exited with status {}",
            output.status
        )))
    } else {
        Err(io::Error::other(format!("{command} failed: {stderr}")))
    }
}

pub(crate) fn base64_encode(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b0 = chunk[0];
        let b1 = *chunk.get(1).unwrap_or(&0);
        let b2 = *chunk.get(2).unwrap_or(&0);
        out.push(TABLE[(b0 >> 2) as usize] as char);
        out.push(TABLE[(((b0 & 0b0000_0011) << 4) | (b1 >> 4)) as usize] as char);
        if chunk.len() > 1 {
            out.push(TABLE[(((b1 & 0b0000_1111) << 2) | (b2 >> 6)) as usize] as char);
        } else {
            out.push('=');
        }
        if chunk.len() > 2 {
            out.push(TABLE[(b2 & 0b0011_1111) as usize] as char);
        } else {
            out.push('=');
        }
    }
    out
}
