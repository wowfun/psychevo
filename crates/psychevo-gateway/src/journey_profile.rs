use std::fs::{File, OpenOptions};
use std::io::{BufWriter, Write};
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use serde::Serialize;

use crate::{GatewayEvent, TranscriptEntryRole};

const PROFILE_PATH_ENV: &str = "PSYCHEVO_GATEWAY_PROFILE_PATH";

#[derive(Clone, Copy, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GatewayProfileFields<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) adapter: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) event_type: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) has_visible_assistant_text: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) queue_depth: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) request_method: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) runtime_source: Option<&'a str>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct GatewayProfileObservation<'a> {
    schema_version: u8,
    surface: &'static str,
    clock_domain_id: &'a str,
    sequence: u64,
    epoch_ms: u128,
    monotonic_ns: String,
    event: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    turn_id: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    thread_id: Option<&'a str>,
    #[serde(flatten)]
    fields: GatewayProfileFields<'a>,
}

struct GatewayProfileWriter {
    clock_domain_id: String,
    origin: Instant,
    sequence: u64,
    writer: BufWriter<File>,
}

impl GatewayProfileWriter {
    fn open(path: PathBuf) -> std::io::Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let epoch_ms = epoch_ms();
        let writer = OpenOptions::new().create(true).append(true).open(path)?;
        Ok(Self {
            clock_domain_id: format!("gateway-{}-{epoch_ms}", std::process::id()),
            origin: Instant::now(),
            sequence: 0,
            writer: BufWriter::new(writer),
        })
    }

    fn mark(
        &mut self,
        event: &str,
        turn_id: Option<&str>,
        thread_id: Option<&str>,
        fields: GatewayProfileFields<'_>,
    ) -> std::io::Result<()> {
        self.sequence += 1;
        serde_json::to_writer(
            &mut self.writer,
            &GatewayProfileObservation {
                schema_version: 1,
                surface: "gateway",
                clock_domain_id: &self.clock_domain_id,
                sequence: self.sequence,
                epoch_ms: epoch_ms(),
                monotonic_ns: self.origin.elapsed().as_nanos().to_string(),
                event,
                turn_id,
                thread_id,
                fields,
            },
        )?;
        self.writer.write_all(b"\n")?;
        self.writer.flush()
    }
}

fn epoch_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default()
}

static WRITER: OnceLock<Option<Mutex<GatewayProfileWriter>>> = OnceLock::new();

pub(crate) fn gateway_profile_mark(
    event: &str,
    turn_id: Option<&str>,
    thread_id: Option<&str>,
    fields: GatewayProfileFields<'_>,
) {
    let Some(writer) = WRITER
        .get_or_init(|| {
            std::env::var_os(PROFILE_PATH_ENV)
                .filter(|value| !value.is_empty())
                .map(PathBuf::from)
                .and_then(|path| GatewayProfileWriter::open(path).ok())
                .map(Mutex::new)
        })
        .as_ref()
    else {
        return;
    };
    if let Ok(mut writer) = writer.lock() {
        let _ = writer.mark(event, turn_id, thread_id, fields);
    }
}

pub(crate) fn gateway_profile_event_fields(event: &GatewayEvent) -> GatewayProfileFields<'static> {
    let event_type = match event {
        GatewayEvent::EntryStarted { .. } => "entryStarted",
        GatewayEvent::EntryUpdated { .. } => "entryUpdated",
        GatewayEvent::EntryCompleted { .. } => "entryCompleted",
        GatewayEvent::TurnStarted { .. } => "turnStarted",
        GatewayEvent::TurnCompleted { .. } => "turnCompleted",
        GatewayEvent::TurnQueued { .. } => "turnQueued",
        GatewayEvent::TitleChanged { .. } => "titleChanged",
        _ => "other",
    };
    let has_visible_assistant_text = match event {
        GatewayEvent::EntryStarted { entry, .. }
        | GatewayEvent::EntryUpdated { entry, .. }
        | GatewayEvent::EntryCompleted { entry, .. }
            if entry.role == TranscriptEntryRole::Assistant =>
        {
            Some(entry.blocks.iter().any(|block| {
                block
                    .body
                    .as_deref()
                    .is_some_and(|body| !body.trim().is_empty())
            }))
        }
        _ => None,
    };
    GatewayProfileFields {
        event_type: Some(event_type),
        has_visible_assistant_text,
        ..GatewayProfileFields::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn observation_schema_is_content_free_and_monotonic() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("gateway.jsonl");
        let mut writer = GatewayProfileWriter::open(path.clone()).expect("writer");
        writer
            .mark(
                "gateway_run_turn_entered",
                Some("turn-1"),
                Some("thread-1"),
                GatewayProfileFields {
                    runtime_source: Some("web"),
                    ..GatewayProfileFields::default()
                },
            )
            .expect("first mark");
        writer
            .mark(
                "gateway_turn_completed",
                Some("turn-1"),
                Some("thread-1"),
                GatewayProfileFields::default(),
            )
            .expect("second mark");
        let lines = std::fs::read_to_string(path).expect("trace");
        let values = lines
            .lines()
            .map(|line| serde_json::from_str::<serde_json::Value>(line).expect("json line"))
            .collect::<Vec<_>>();
        assert_eq!(values.len(), 2);
        assert_eq!(values[0]["sequence"], 1);
        assert_eq!(values[1]["sequence"], 2);
        for forbidden in ["prompt", "response", "token", "credential", "requestBody"] {
            assert!(!lines.contains(forbidden), "trace leaked {forbidden}");
        }
    }
}
