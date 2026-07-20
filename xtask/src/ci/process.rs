use std::env;
use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::Path;
use std::process::{Child, Command as ProcessCommand, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;

use anyhow::{Context, Result, anyhow};

#[derive(Debug)]
pub(crate) struct ProcessOutcome {
    pub(crate) passed: bool,
    pub(crate) exit_code: Option<i32>,
    pub(crate) mirrored_diagnostics: usize,
    pub(crate) had_suppressed_output: bool,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct CaptureStats {
    pub(crate) mirrored_lines: usize,
    pub(crate) had_suppressed_output: bool,
}

impl CaptureStats {
    pub(crate) fn merge(&mut self, other: Self) {
        self.mirrored_lines += other.mirrored_lines;
        self.had_suppressed_output |= other.had_suppressed_output;
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum OutputStream {
    Stdout,
    Stderr,
}

pub(crate) fn create_step_log(log_path: &Path) -> Result<Arc<Mutex<fs::File>>> {
    Ok(Arc::new(Mutex::new(
        fs::File::create(log_path)
            .with_context(|| format!("create step log {}", log_path.display()))?,
    )))
}

pub(crate) fn run_logged_process(
    label: &str,
    command: &mut ProcessCommand,
    log: Arc<Mutex<fs::File>>,
) -> Result<ProcessOutcome> {
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = command
        .spawn()
        .with_context(|| format!("spawn {label}: {command:?}"))?;

    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| anyhow!("{label} stdout was not captured"))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| anyhow!("{label} stderr was not captured"))?;

    let stdout_handle = spawn_capture_stream(stdout, Arc::clone(&log), OutputStream::Stdout);
    let stderr_handle = spawn_capture_stream(stderr, Arc::clone(&log), OutputStream::Stderr);

    let status = child.wait().with_context(|| format!("wait for {label}"))?;
    let mut stats = join_capture_stream("stdout", stdout_handle)?;
    stats.merge(join_capture_stream("stderr", stderr_handle)?);

    Ok(ProcessOutcome {
        passed: status.success(),
        exit_code: status.code(),
        mirrored_diagnostics: stats.mirrored_lines,
        had_suppressed_output: stats.had_suppressed_output,
    })
}

pub(crate) fn spawn_capture_stream<R>(
    reader: R,
    log: Arc<Mutex<fs::File>>,
    stream: OutputStream,
) -> thread::JoinHandle<Result<CaptureStats>>
where
    R: Read + Send + 'static,
{
    thread::spawn(move || capture_stream(reader, log, stream))
}

fn capture_stream<R>(
    reader: R,
    log: Arc<Mutex<fs::File>>,
    stream: OutputStream,
) -> Result<CaptureStats>
where
    R: Read,
{
    let mut reader = BufReader::new(reader);
    let mut line = Vec::new();
    let mut stats = CaptureStats::default();
    loop {
        line.clear();
        let bytes = reader
            .read_until(b'\n', &mut line)
            .context("read step output")?;
        if bytes == 0 {
            break;
        }

        {
            let mut log = log.lock().map_err(|_| anyhow!("step log lock poisoned"))?;
            log.write_all(&line).context("write step log")?;
        }

        if should_mirror_to_terminal(stream, &line) {
            let mut stderr = std::io::stderr().lock();
            stderr
                .write_all(&line)
                .context("write terminal diagnostic")?;
            stderr.flush().context("flush terminal diagnostic")?;
            stats.mirrored_lines += 1;
        } else {
            stats.had_suppressed_output = true;
        }
    }
    Ok(stats)
}

pub(crate) fn join_capture_stream(
    name: &str,
    handle: thread::JoinHandle<Result<CaptureStats>>,
) -> Result<CaptureStats> {
    handle
        .join()
        .map_err(|_| anyhow!("capture {name} thread panicked"))?
        .with_context(|| format!("capture {name} output"))
}

pub(crate) fn should_mirror_to_terminal(stream: OutputStream, line: &[u8]) -> bool {
    match stream {
        OutputStream::Stdout => line_has_warning(line),
        OutputStream::Stderr => true,
    }
}

fn line_has_warning(line: &[u8]) -> bool {
    let lower = String::from_utf8_lossy(line).to_ascii_lowercase();
    lower.contains("warning:") || lower.contains("warning[")
}

pub(crate) fn write_log_line(log: &Arc<Mutex<fs::File>>, line: &str) -> Result<()> {
    let mut log = log.lock().map_err(|_| anyhow!("step log lock poisoned"))?;
    write_line(&mut *log, line).context("write step log")
}

pub(crate) fn write_mirrored_line(log: &Arc<Mutex<fs::File>>, line: &str) -> Result<()> {
    write_log_line(log, line)?;
    let mut stderr = std::io::stderr().lock();
    write_line(&mut stderr, line).context("write terminal diagnostic")?;
    stderr.flush().context("flush terminal diagnostic")
}

fn write_line(writer: &mut dyn Write, line: &str) -> Result<()> {
    writer.write_all(line.as_bytes())?;
    if !line.ends_with('\n') {
        writer.write_all(b"\n")?;
    }
    Ok(())
}

pub(crate) fn command_exists(command: &str) -> bool {
    let path = Path::new(command);
    if path.components().count() > 1 {
        return path.is_file();
    }
    let Some(paths) = env::var_os("PATH") else {
        return false;
    };
    env::split_paths(&paths).any(|dir| dir.join(command).is_file())
}

pub(crate) struct LoggedChild {
    child: Option<Child>,
    stdout_handle: Option<thread::JoinHandle<Result<CaptureStats>>>,
    stderr_handle: Option<thread::JoinHandle<Result<CaptureStats>>>,
}

impl LoggedChild {
    pub(crate) fn spawn(
        label: &str,
        mut command: ProcessCommand,
        log: Arc<Mutex<fs::File>>,
    ) -> Result<Self> {
        command.stdout(Stdio::piped()).stderr(Stdio::piped());
        let mut child = command
            .spawn()
            .with_context(|| format!("spawn {label}: {command:?}"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| anyhow!("{label} stdout was not captured"))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| anyhow!("{label} stderr was not captured"))?;
        Ok(Self {
            child: Some(child),
            stdout_handle: Some(spawn_capture_stream(
                stdout,
                Arc::clone(&log),
                OutputStream::Stdout,
            )),
            stderr_handle: Some(spawn_capture_stream(stderr, log, OutputStream::Stderr)),
        })
    }

    pub(crate) fn stop(&mut self) -> Result<CaptureStats> {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        let mut stats = CaptureStats::default();
        if let Some(handle) = self.stdout_handle.take() {
            stats.merge(join_capture_stream("stdout", handle)?);
        }
        if let Some(handle) = self.stderr_handle.take() {
            stats.merge(join_capture_stream("stderr", handle)?);
        }
        Ok(stats)
    }
}

impl Drop for LoggedChild {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        if let Some(handle) = self.stdout_handle.take() {
            let _ = handle.join();
        }
        if let Some(handle) = self.stderr_handle.take() {
            let _ = handle.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    static NEXT_TEST_LOG: AtomicUsize = AtomicUsize::new(0);

    fn capture_stats(input: &[u8], stream: OutputStream) -> CaptureStats {
        let id = NEXT_TEST_LOG.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "psychevo-xtask-process-{}-{id}.log",
            std::process::id()
        ));
        let log = create_step_log(&path).expect("create capture test log");
        let stats =
            capture_stream(Cursor::new(input.to_vec()), log, stream).expect("capture test output");
        fs::remove_file(path).expect("remove capture test log");
        stats
    }

    #[test]
    fn normal_stdout_is_not_mirrored_to_terminal() {
        assert!(!should_mirror_to_terminal(
            OutputStream::Stdout,
            b"Compiling psychevo-runtime v0.1.0\n"
        ));
    }

    #[test]
    fn stdout_warnings_are_mirrored_to_terminal() {
        assert!(should_mirror_to_terminal(
            OutputStream::Stdout,
            b"warning: unused import: `Path`\n"
        ));
    }

    #[test]
    fn stdout_errors_are_not_mirrored_to_terminal() {
        assert!(should_mirror_to_terminal(
            OutputStream::Stderr,
            b"error[E0425]: cannot find value `x` in this scope\n"
        ));
        assert!(!should_mirror_to_terminal(
            OutputStream::Stdout,
            b"error[E0425]: cannot find value `x` in this scope\n"
        ));
    }

    #[test]
    fn stderr_is_mirrored_to_terminal() {
        assert!(should_mirror_to_terminal(
            OutputStream::Stderr,
            b"any stderr line\n"
        ));
    }

    #[test]
    fn normal_stdout_marks_suppressed_output() {
        assert_eq!(
            capture_stats(b"assertion failed: left == right\n", OutputStream::Stdout),
            CaptureStats {
                mirrored_lines: 0,
                had_suppressed_output: true,
            }
        );
    }

    #[test]
    fn stdout_warning_and_stderr_do_not_mark_suppressed_output() {
        assert_eq!(
            capture_stats(b"warning: unused import\n", OutputStream::Stdout),
            CaptureStats {
                mirrored_lines: 1,
                had_suppressed_output: false,
            }
        );
        assert_eq!(
            capture_stats(b"error: test failed\n", OutputStream::Stderr),
            CaptureStats {
                mirrored_lines: 1,
                had_suppressed_output: false,
            }
        );
    }

    #[test]
    fn capture_stats_merge_sums_mirrors_and_ors_suppressed_output() {
        let mut stats = CaptureStats {
            mirrored_lines: 1,
            had_suppressed_output: false,
        };
        stats.merge(CaptureStats {
            mirrored_lines: 2,
            had_suppressed_output: true,
        });
        assert_eq!(
            stats,
            CaptureStats {
                mirrored_lines: 3,
                had_suppressed_output: true,
            }
        );
    }
}
