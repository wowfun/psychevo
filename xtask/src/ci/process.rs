use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::Path;
use std::process::{Child, Command as ProcessCommand, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

#[cfg(unix)]
use std::os::unix::process::CommandExt;

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
    run_logged_process_inner(label, command, log, None)
}

pub(crate) fn run_logged_process_with_timeout(
    label: &str,
    command: &mut ProcessCommand,
    log: Arc<Mutex<fs::File>>,
    timeout: Duration,
) -> Result<ProcessOutcome> {
    run_logged_process_inner(label, command, log, Some(timeout))
}

fn run_logged_process_inner(
    label: &str,
    command: &mut ProcessCommand,
    log: Arc<Mutex<fs::File>>,
    timeout: Option<Duration>,
) -> Result<ProcessOutcome> {
    if timeout.is_some() {
        configure_process_tree(command);
    }
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

    let status = match timeout {
        Some(timeout) => wait_with_timeout(label, &mut child, &log, timeout),
        None => child.wait().with_context(|| format!("wait for {label}")),
    };
    let mut stats = join_capture_stream("stdout", stdout_handle)?;
    stats.merge(join_capture_stream("stderr", stderr_handle)?);
    let status = status?;

    Ok(ProcessOutcome {
        passed: status.success(),
        exit_code: status.code(),
        mirrored_diagnostics: stats.mirrored_lines,
        had_suppressed_output: stats.had_suppressed_output,
    })
}

fn wait_with_timeout(
    label: &str,
    child: &mut Child,
    log: &Arc<Mutex<fs::File>>,
    timeout: Duration,
) -> Result<std::process::ExitStatus> {
    let started = Instant::now();
    loop {
        if let Some(status) = child.try_wait().with_context(|| format!("poll {label}"))? {
            return Ok(status);
        }
        let elapsed = started.elapsed();
        if elapsed >= timeout {
            let message = format!(
                "{label} timed out after {} seconds; terminating its process tree",
                timeout.as_secs()
            );
            write_mirrored_line(log, &message)?;
            terminate_process_tree(child)
                .with_context(|| format!("terminate timed-out {label} process tree"))?;
            anyhow::bail!("{message}");
        }
        thread::sleep((timeout - elapsed).min(Duration::from_millis(50)));
    }
}

#[cfg(unix)]
fn configure_process_tree(command: &mut ProcessCommand) {
    command.process_group(0);
}

#[cfg(not(unix))]
fn configure_process_tree(_command: &mut ProcessCommand) {}

#[cfg(unix)]
fn terminate_process_tree(child: &mut Child) -> Result<()> {
    let process_group = -(child.id() as i32);
    // SAFETY: the child was spawned as the leader of a new process group. A
    // negative pid targets exactly that group and never the xtask process.
    unsafe {
        libc::kill(process_group, libc::SIGTERM);
    }
    let grace_started = Instant::now();
    while grace_started.elapsed() < Duration::from_millis(250) {
        if child
            .try_wait()
            .context("poll process after SIGTERM")?
            .is_some()
        {
            break;
        }
        thread::sleep(Duration::from_millis(25));
    }
    // SAFETY: same process-group invariant as the SIGTERM call above.
    unsafe {
        libc::kill(process_group, libc::SIGKILL);
    }
    let _ = child.kill();
    child.wait().context("wait for terminated process tree")?;
    Ok(())
}

#[cfg(windows)]
fn terminate_process_tree(child: &mut Child) -> Result<()> {
    let status = ProcessCommand::new("taskkill")
        .args(["/PID", &child.id().to_string(), "/T", "/F"])
        .status()
        .context("run taskkill for timed-out process tree")?;
    let _ = child.kill();
    let _ = child.wait();
    if !status.success() {
        anyhow::bail!("taskkill failed with {status}");
    }
    Ok(())
}

#[cfg(not(any(unix, windows)))]
fn terminate_process_tree(child: &mut Child) -> Result<()> {
    child.kill().context("kill timed-out process")?;
    child.wait().context("wait for timed-out process")?;
    Ok(())
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
    capture_stream_with_mirror(reader, log, stream, mirror_terminal_diagnostic)
}

fn capture_stream_with_mirror<R, F>(
    reader: R,
    log: Arc<Mutex<fs::File>>,
    stream: OutputStream,
    mut mirror: F,
) -> Result<CaptureStats>
where
    R: Read,
    F: FnMut(&[u8]) -> Result<()>,
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
            mirror(&line)?;
            stats.mirrored_lines += 1;
        } else {
            stats.had_suppressed_output = true;
        }
    }
    Ok(stats)
}

fn mirror_terminal_diagnostic(line: &[u8]) -> Result<()> {
    let mut stderr = std::io::stderr().lock();
    stderr
        .write_all(line)
        .context("write terminal diagnostic")?;
    stderr.flush().context("flush terminal diagnostic")
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
    crate::host_command::exists(command)
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

    fn capture_output(input: &[u8], stream: OutputStream) -> (CaptureStats, Vec<u8>) {
        let id = NEXT_TEST_LOG.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "psychevo-xtask-process-{}-{id}.log",
            std::process::id()
        ));
        let log = create_step_log(&path).expect("create capture test log");
        let mut mirrored = Vec::new();
        let stats = capture_stream_with_mirror(Cursor::new(input.to_vec()), log, stream, |line| {
            mirrored.extend_from_slice(line);
            Ok(())
        })
        .expect("capture test output");
        fs::remove_file(path).expect("remove capture test log");
        (stats, mirrored)
    }

    #[cfg(unix)]
    #[test]
    fn timeout_terminates_descendants_that_inherit_capture_pipes() {
        let id = NEXT_TEST_LOG.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "psychevo-xtask-timeout-{}-{id}.log",
            std::process::id()
        ));
        let log = create_step_log(&path).expect("create timeout test log");
        let mut command = ProcessCommand::new("sh");
        command.args(["-c", "sleep 30 & wait"]);

        let started = Instant::now();
        let error = run_logged_process_with_timeout(
            "descendant timeout fixture",
            &mut command,
            log,
            Duration::from_millis(100),
        )
        .expect_err("fixture must time out");

        assert!(started.elapsed() < Duration::from_secs(5));
        assert!(
            error
                .to_string()
                .contains("descendant timeout fixture timed out")
        );
        assert!(
            fs::read_to_string(&path)
                .expect("read timeout log")
                .contains("terminating its process tree")
        );
        fs::remove_file(path).expect("remove timeout test log");
    }

    #[test]
    fn normal_stdout_is_not_mirrored_to_terminal() {
        assert!(!should_mirror_to_terminal(
            OutputStream::Stdout,
            b"Compiling psychevo v0.1.0\n"
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
            capture_output(b"assertion failed: left == right\n", OutputStream::Stdout),
            (
                CaptureStats {
                    mirrored_lines: 0,
                    had_suppressed_output: true,
                },
                Vec::new(),
            )
        );
    }

    #[test]
    fn stdout_warning_and_stderr_do_not_mark_suppressed_output() {
        assert_eq!(
            capture_output(b"warning: unused import\n", OutputStream::Stdout),
            (
                CaptureStats {
                    mirrored_lines: 1,
                    had_suppressed_output: false,
                },
                b"warning: unused import\n".to_vec(),
            )
        );
        assert_eq!(
            capture_output(b"error: test failed\n", OutputStream::Stderr),
            (
                CaptureStats {
                    mirrored_lines: 1,
                    had_suppressed_output: false,
                },
                b"error: test failed\n".to_vec(),
            )
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
