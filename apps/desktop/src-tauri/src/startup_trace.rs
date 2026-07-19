use std::env;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use serde::Serialize;

const ARTIFACT_ROOT_ENV: &str = "PSYCHEVO_WDIO_ARTIFACT_ROOT";
const TRACE_FILENAME: &str = "desktop-startup-rust.jsonl";

static TRACE_CLOCK: OnceLock<Instant> = OnceLock::new();
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
}

pub(crate) fn record_process_start() {
    TRACE_CLOCK.get_or_init(Instant::now);
    record_once(&PROCESS_START_RECORDED, "process_start");
}

pub(crate) fn record_window_ready() {
    record_once(&WINDOW_READY_RECORDED, "window_ready");
}

pub(crate) fn record_managed_gateway_ready() {
    record_once(&MANAGED_GATEWAY_READY_RECORDED, "managed_gateway_ready");
}

pub(crate) fn record_bridge_connected(connection_id: &str) {
    if connection_label(connection_id) != "workbench" {
        return;
    }
    record_once(&WORKBENCH_BRIDGE_CONNECTED_RECORDED, "bridge_connected");
}

fn record_once(recorded: &OnceLock<()>, id: &'static str) {
    if recorded.set(()).is_err() {
        return;
    }
    if let Err(error) = append_mark(id) {
        eprintln!("failed to write Desktop WDIO startup trace mark {id}: {error}");
    }
}

fn append_mark(id: &'static str) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let Some(path) = trace_path() else {
        return Ok(());
    };
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

fn trace_path() -> Option<PathBuf> {
    env::var_os(ARTIFACT_ROOT_ENV)
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
    fn trace_shape_contains_only_bounded_timing_evidence() {
        let value = serde_json::to_value(StartupTraceMark {
            schema_version: 1,
            id: "bridge_connected",
            sequence: 4,
            source_clock: "desktop-rust-monotonic",
            epoch_ms: 123,
            monotonic_offset_ms: 12.5,
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
                "schemaVersion",
                "sequence",
                "sourceClock",
            ]
        );
    }
}
