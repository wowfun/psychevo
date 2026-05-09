fn default_clipboard_sink() -> ClipboardSink {
    Arc::new(copy_text_to_clipboard)
}

fn copy_text_to_clipboard(text: &str) -> io::Result<()> {
    copy_text_to_clipboard_with(
        text,
        local_clipboard_commands(),
        |candidate, text| pipe_to_command(candidate.command, candidate.args, text),
        write_osc52_clipboard,
    )
}

fn copy_text_to_clipboard_with(
    text: &str,
    candidates: Vec<ClipboardCommand>,
    mut local_copy: impl FnMut(ClipboardCommand, &str) -> io::Result<bool>,
    osc52_copy: impl FnOnce(&str) -> io::Result<()>,
) -> io::Result<()> {
    if text.is_empty() {
        return Ok(());
    }
    let mut failures = Vec::new();
    for candidate in candidates {
        match local_copy(candidate, text) {
            Ok(true) => return Ok(()),
            Ok(false) => failures.push(format!("{} unavailable", candidate.command)),
            Err(err) => failures.push(format!("{}: {err}", candidate.command)),
        }
    }
    match osc52_copy(text) {
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

fn clipboard_failure_summary(failures: &[String]) -> String {
    if failures.is_empty() {
        return "no clipboard backend succeeded".to_string();
    }
    let summary = failures.join("; ");
    truncate_chars(&summary, 240)
}

fn write_osc52_clipboard(text: &str) -> io::Result<()> {
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

fn write_osc52_sequence(mut writer: impl Write, sequence: &str) -> io::Result<()> {
    writer.write_all(sequence.as_bytes())?;
    writer.flush()
}

fn osc52_sequence(text: &str) -> io::Result<String> {
    osc52_sequence_with_passthrough(
        text,
        std::env::var_os("TMUX").is_some() || std::env::var_os("STY").is_some(),
    )
}

fn osc52_sequence_with_passthrough(text: &str, passthrough: bool) -> io::Result<String> {
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
struct ClipboardCommand {
    command: &'static str,
    args: &'static [&'static str],
}

const NO_ARGS: &[&str] = &[];
const POWERSHELL_CLIPBOARD_ARGS: &[&str] = &[
    "-NonInteractive",
    "-NoProfile",
    "-Command",
    "[Console]::InputEncoding = [System.Text.Encoding]::UTF8; $ErrorActionPreference = 'Stop'; $text = [Console]::In.ReadToEnd(); Set-Clipboard -Value $text",
];
const XCLIP_CLIPBOARD_ARGS: &[&str] = &["-selection", "clipboard"];
const XSEL_CLIPBOARD_ARGS: &[&str] = &["--clipboard", "--input"];

fn local_clipboard_commands() -> Vec<ClipboardCommand> {
    local_clipboard_commands_for(
        cfg!(target_os = "macos"),
        cfg!(target_os = "windows"),
        is_probably_wsl(),
        is_wayland_session(),
    )
}

fn is_wayland_session() -> bool {
    std::env::var_os("WAYLAND_DISPLAY").is_some()
        || std::env::var("XDG_SESSION_TYPE")
            .is_ok_and(|value| value.eq_ignore_ascii_case("wayland"))
}

fn is_probably_wsl() -> bool {
    let proc_version = std::fs::read_to_string("/proc/version").ok();
    let os_release = std::fs::read_to_string("/proc/sys/kernel/osrelease").ok();
    is_probably_wsl_from(
        proc_version.as_deref(),
        os_release.as_deref(),
        std::env::var_os("WSL_DISTRO_NAME").is_some(),
        std::env::var_os("WSL_INTEROP").is_some(),
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

fn local_clipboard_commands_for(
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

fn pipe_to_command(command: &str, args: &[&str], text: &str) -> io::Result<bool> {
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

fn base64_encode(bytes: &[u8]) -> String {
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

