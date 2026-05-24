#[allow(unused_imports)]
pub(crate) use super::*;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TerminalDefaultColors {
    pub(crate) foreground: (u8, u8, u8),
    pub(crate) background: (u8, u8, u8),
}

#[cfg(unix)]
pub(crate) fn query_terminal_default_colors(timeout: Duration) -> Option<TerminalDefaultColors> {
    unix_terminal_probe::query_default_colors(timeout)
        .ok()
        .flatten()
}

#[cfg(not(unix))]
pub(crate) fn query_terminal_default_colors(_timeout: Duration) -> Option<TerminalDefaultColors> {
    None
}

pub(crate) fn parse_terminal_default_colors(buffer: &[u8]) -> Option<TerminalDefaultColors> {
    let text = String::from_utf8_lossy(buffer);
    let foreground = parse_osc_color(&text, "10;")?;
    let background = parse_osc_color(&text, "11;")?;
    Some(TerminalDefaultColors {
        foreground,
        background,
    })
}

pub(crate) fn parse_osc_color(text: &str, slot: &str) -> Option<(u8, u8, u8)> {
    let start = text.find(&format!("]{slot}"))?;
    let after_slot = &text[start + slot.len() + 1..];
    let color_start = after_slot.find("rgb:")?;
    let color = &after_slot[color_start + 4..];
    let end = color.find(['\u{0007}', '\u{001b}']).unwrap_or(color.len());
    let mut parts = color[..end].split('/');
    let r = parse_hex_color_component(parts.next()?)?;
    let g = parse_hex_color_component(parts.next()?)?;
    let b = parse_hex_color_component(parts.next()?)?;
    Some((r, g, b))
}

pub(crate) fn parse_hex_color_component(value: &str) -> Option<u8> {
    let value = value.trim();
    if value.is_empty() || value.len() > 4 {
        return None;
    }
    let parsed = u32::from_str_radix(value, 16).ok()?;
    let max = (1_u32 << (value.len() * 4)) - 1;
    Some(((parsed * 255) / max) as u8)
}

#[cfg(unix)]
pub(crate) mod unix_terminal_probe {
    pub(crate) use super::*;
    use std::fs::File;
    use std::fs::OpenOptions;
    use std::io;
    use std::io::Write;
    use std::os::fd::AsRawFd;
    use std::os::fd::FromRawFd;

    struct Tty {
        reader: File,
        writer: File,
        original_flags: libc::c_int,
    }

    impl Tty {
        fn open() -> io::Result<Self> {
            let stdio_reader = dup_file(libc::STDIN_FILENO);
            let stdio_writer = dup_file(libc::STDOUT_FILENO);
            match (stdio_reader, stdio_writer) {
                (Ok(reader), Ok(writer)) => Self::new(reader, writer),
                _ => {
                    let reader = OpenOptions::new().read(true).open("/dev/tty")?;
                    let writer = OpenOptions::new().write(true).open("/dev/tty")?;
                    Self::new(reader, writer)
                }
            }
        }

        fn new(reader: File, writer: File) -> io::Result<Self> {
            let fd = reader.as_raw_fd();
            let original_flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
            if original_flags == -1 {
                return Err(io::Error::last_os_error());
            }
            let result =
                unsafe { libc::fcntl(fd, libc::F_SETFL, original_flags | libc::O_NONBLOCK) };
            if result == -1 {
                return Err(io::Error::last_os_error());
            }
            Ok(Self {
                reader,
                writer,
                original_flags,
            })
        }

        fn write_all(&mut self, bytes: &[u8]) -> io::Result<()> {
            self.writer.write_all(bytes)?;
            self.writer.flush()
        }

        fn read_available(&mut self, buffer: &mut Vec<u8>) -> io::Result<()> {
            let mut chunk = [0_u8; 256];
            loop {
                let count = unsafe {
                    libc::read(
                        self.reader.as_raw_fd(),
                        chunk.as_mut_ptr().cast::<libc::c_void>(),
                        chunk.len(),
                    )
                };
                if count > 0 {
                    buffer.extend_from_slice(&chunk[..count as usize]);
                    continue;
                }
                if count == 0 {
                    return Ok(());
                }
                let err = io::Error::last_os_error();
                if matches!(
                    err.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
                ) {
                    return Ok(());
                }
                return Err(err);
            }
        }

        fn poll_readable(&self, timeout: Duration) -> io::Result<bool> {
            let timeout_ms = timeout.as_millis().min(libc::c_int::MAX as u128) as libc::c_int;
            let mut fd = libc::pollfd {
                fd: self.reader.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            };
            loop {
                let result = unsafe { libc::poll(&mut fd, 1, timeout_ms) };
                if result > 0 {
                    return Ok((fd.revents & libc::POLLIN) != 0);
                }
                if result == 0 {
                    return Ok(false);
                }
                let err = io::Error::last_os_error();
                if err.kind() != io::ErrorKind::Interrupted {
                    return Err(err);
                }
            }
        }
    }

    impl Drop for Tty {
        fn drop(&mut self) {
            let _ =
                unsafe { libc::fcntl(self.reader.as_raw_fd(), libc::F_SETFL, self.original_flags) };
        }
    }

    fn dup_file(fd: libc::c_int) -> io::Result<File> {
        let duplicated = unsafe { libc::dup(fd) };
        if duplicated == -1 {
            return Err(io::Error::last_os_error());
        }
        Ok(unsafe { File::from_raw_fd(duplicated) })
    }

    pub(crate) fn query_default_colors(
        timeout: Duration,
    ) -> io::Result<Option<TerminalDefaultColors>> {
        let mut tty = Tty::open()?;
        tty.write_all(b"\x1B]10;?\x1B\\\x1B]11;?\x1B\\")?;
        let deadline = Instant::now() + timeout;
        let mut buffer = Vec::new();
        loop {
            tty.read_available(&mut buffer)?;
            if let Some(colors) = parse_terminal_default_colors(&buffer) {
                return Ok(Some(colors));
            }
            let now = Instant::now();
            if now >= deadline {
                return Ok(None);
            }
            if !tty.poll_readable(deadline.saturating_duration_since(now))? {
                return Ok(None);
            }
        }
    }
}
