use std::env;
use std::ffi::OsString;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use serde::Serialize;

const ARTIFACT_ROOT_ENV: &str = "PSYCHEVO_DESKTOP_STARTUP_TRACE_ROOT";
const TRACE_FILENAME: &str = "desktop-startup-rust.jsonl";

static TRACE_CLOCK: OnceLock<Instant> = OnceLock::new();
static TRACE_PATH: OnceLock<Option<PathBuf>> = OnceLock::new();
static PROCESS_START_RECORDED: OnceLock<()> = OnceLock::new();
static WINDOW_READY_RECORDED: OnceLock<()> = OnceLock::new();
static MANAGED_GATEWAY_READY_RECORDED: OnceLock<()> = OnceLock::new();
static WORKBENCH_BRIDGE_CONNECTED_RECORDED: OnceLock<()> = OnceLock::new();
static NEXT_SEQUENCE: AtomicU64 = AtomicU64::new(1);
static TRACE_WRITER: Mutex<()> = Mutex::new(());

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct StartupTraceMark<'a> {
    schema_version: u8,
    id: &'a str,
    sequence: u64,
    source_clock: &'a str,
    epoch_ms: u64,
    monotonic_offset_ms: f64,
    pid: u32,
}

pub(crate) fn record_process_start() {
    record_once(&PROCESS_START_RECORDED, "process_start");
}

pub(crate) fn enabled() -> bool {
    trace_path().is_some()
}

pub(crate) fn record_window_ready() {
    record_once(&WINDOW_READY_RECORDED, "window_ready");
}

pub(crate) fn record_managed_gateway_ready() {
    record_once(&MANAGED_GATEWAY_READY_RECORDED, "managed_gateway_ready");
}

pub(crate) fn record_bridge_connected(connection_id: &str) {
    if !enabled() || connection_label(connection_id) != "workbench" {
        return;
    }
    record_once(&WORKBENCH_BRIDGE_CONNECTED_RECORDED, "bridge_connected");
}

fn record_once(recorded: &OnceLock<()>, id: &'static str) {
    let Some(path) = trace_path() else {
        return;
    };
    if recorded.set(()).is_err() {
        return;
    }
    if let Err(error) = append_mark(path, id) {
        eprintln!("failed to write Desktop startup trace mark {id}: {error}");
    }
}

fn append_mark(
    path: &Path,
    id: &'static str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let clock = TRACE_CLOCK.get_or_init(Instant::now);
    let mark = StartupTraceMark {
        schema_version: 1,
        id,
        sequence: NEXT_SEQUENCE.fetch_add(1, Ordering::Relaxed),
        source_clock: "desktop-rust-monotonic",
        epoch_ms: SystemTime::now()
            .duration_since(UNIX_EPOCH)?
            .as_millis()
            .try_into()?,
        monotonic_offset_ms: clock.elapsed().as_secs_f64() * 1_000.0,
        pid: std::process::id(),
    };
    let line = serde_json::to_string(&mark)?;
    let _writer = TRACE_WRITER
        .lock()
        .map_err(|_| "Desktop startup trace writer lock poisoned")?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    writeln!(file, "{line}")?;
    Ok(())
}

fn trace_path() -> Option<&'static Path> {
    TRACE_PATH
        .get_or_init(|| configured_trace_path(env::var_os(ARTIFACT_ROOT_ENV)))
        .as_deref()
}

fn configured_trace_path(value: Option<OsString>) -> Option<PathBuf> {
    value
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .map(|root| root.join(TRACE_FILENAME))
}

fn connection_label(connection_id: &str) -> &str {
    connection_id
        .split_once(':')
        .map_or(connection_id, |(label, _)| label)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn connection_label_discards_random_correlation_suffix() {
        assert_eq!(connection_label("workbench:private-nonce"), "workbench");
        assert_eq!(connection_label("floating:private-nonce"), "floating");
        assert_eq!(connection_label("desktop"), "desktop");
    }

    #[test]
    fn startup_trace_is_default_off_and_requires_an_explicit_root() {
        assert_eq!(configured_trace_path(None), None);
        assert_eq!(configured_trace_path(Some(OsString::new())), None);
        assert_eq!(
            configured_trace_path(Some(OsString::from("artifacts"))),
            Some(PathBuf::from("artifacts").join(TRACE_FILENAME))
        );
    }

    #[test]
    fn trace_shape_contains_only_bounded_timing_evidence() {
        let value = serde_json::to_value(StartupTraceMark {
            schema_version: 1,
            id: "bridge_connected",
            sequence: 4,
            source_clock: "desktop-rust-monotonic",
            epoch_ms: 123,
            monotonic_offset_ms: 12.5,
            pid: 42,
        })
        .expect("trace mark");
        let fields = value
            .as_object()
            .expect("trace object")
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>();
        assert_eq!(
            fields,
            vec![
                "epochMs",
                "id",
                "monotonicOffsetMs",
                "pid",
                "schemaVersion",
                "sequence",
                "sourceClock",
            ]
        );
    }
}
