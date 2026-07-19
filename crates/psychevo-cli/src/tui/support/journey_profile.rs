use std::fs::{File, OpenOptions};
use std::io::{self, BufWriter, Write};
use std::path::Path;
use std::sync::mpsc::{self, Sender};
use std::thread::{self, JoinHandle};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use serde::Serialize;

use super::{
    GatewayEvent, TranscriptBlock, TranscriptBlockKind, TranscriptEntry, TranscriptEntryRole,
};

pub(crate) const TUI_PROFILE_PATH_ENV: &str = "PSYCHEVO_TUI_PROFILE_PATH";

#[derive(Debug)]
pub(crate) struct TuiJourneyProfileProbe {
    writer: Option<ProfileWriter>,
    input_ready: bool,
    next_sample_index: u64,
    active_sample: Option<ActiveSample>,
}

#[derive(Debug, Default)]
struct ActiveSample {
    sample_index: u64,
    send_feedback_visible: bool,
    first_assistant_received: bool,
    first_assistant_applied: bool,
    first_output_visible: bool,
    turn_completed_received: bool,
    turn_completed_applied: bool,
    turn_settled: bool,
}

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct TuiProfileFrameObservation {
    pub(crate) input_ready: bool,
    pub(crate) send_feedback_ready: bool,
    pub(crate) turn_running: bool,
    pub(crate) composer_focused: bool,
    pub(crate) compaction_running: bool,
}

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct GatewayEventProfileKind {
    first_assistant: bool,
    turn_completed: bool,
}

impl TuiJourneyProfileProbe {
    pub(crate) fn from_env(env: &std::collections::BTreeMap<String, String>) -> io::Result<Self> {
        let Some(path) = env
            .get(TUI_PROFILE_PATH_ENV)
            .map(String::as_str)
            .filter(|path| !path.trim().is_empty())
        else {
            return Ok(Self::disabled());
        };
        Self::for_path(Path::new(path))
    }

    pub(crate) fn disabled() -> Self {
        Self {
            writer: None,
            input_ready: false,
            next_sample_index: 0,
            active_sample: None,
        }
    }

    fn for_path(path: &Path) -> io::Result<Self> {
        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            std::fs::create_dir_all(parent)?;
        }
        let file = OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(path)?;
        let mut probe = Self {
            writer: Some(ProfileWriter::new(file)?),
            input_ready: false,
            next_sample_index: 0,
            active_sample: None,
        };
        probe.mark("process_started", None);
        Ok(probe)
    }

    pub(crate) fn is_enabled(&self) -> bool {
        self.writer.is_some()
    }

    pub(crate) fn mark_ui_constructed(&mut self) {
        self.mark("ui_constructed", None);
    }

    pub(crate) fn mark_history_loaded(&mut self) {
        self.mark("history_loaded", None);
    }

    pub(crate) fn mark_send_committed(&mut self) {
        if self.writer.is_none() {
            return;
        }
        let sample_index = self.next_sample_index;
        self.next_sample_index = self.next_sample_index.saturating_add(1);
        self.active_sample = Some(ActiveSample {
            sample_index,
            ..ActiveSample::default()
        });
        self.mark("send_committed", Some(sample_index));
    }

    pub(crate) fn observe_gateway_event_received(
        &mut self,
        event: &GatewayEvent,
    ) -> GatewayEventProfileKind {
        let kind = GatewayEventProfileKind::from_event(event);
        self.observe_gateway_event(kind, false);
        kind
    }

    pub(crate) fn observe_gateway_event_applied(&mut self, event_kind: GatewayEventProfileKind) {
        self.observe_gateway_event(event_kind, true);
    }

    fn observe_gateway_event(&mut self, kind: GatewayEventProfileKind, applied: bool) {
        let Some(sample) = self.active_sample.as_mut() else {
            return;
        };
        let sample_index = sample.sample_index;
        let first_assistant = kind.first_assistant
            && if applied {
                !std::mem::replace(&mut sample.first_assistant_applied, true)
            } else {
                !std::mem::replace(&mut sample.first_assistant_received, true)
            };
        let turn_completed = kind.turn_completed
            && if applied {
                !std::mem::replace(&mut sample.turn_completed_applied, true)
            } else {
                !std::mem::replace(&mut sample.turn_completed_received, true)
            };
        if first_assistant {
            self.mark(
                if applied {
                    "gateway_first_assistant_event_applied"
                } else {
                    "gateway_first_assistant_event_received"
                },
                Some(sample_index),
            );
        }
        if turn_completed {
            self.mark(
                if applied {
                    "turn_completed_applied"
                } else {
                    "turn_completed_received"
                },
                Some(sample_index),
            );
        }
    }

    pub(crate) fn observe_frame(&mut self, observation: TuiProfileFrameObservation) {
        if !self.input_ready && observation.input_ready {
            self.input_ready = true;
            self.mark("input_ready", None);
        }

        let Some(sample) = self.active_sample.as_mut() else {
            return;
        };
        let sample_index = sample.sample_index;
        let feedback_visible = !sample.send_feedback_visible && observation.send_feedback_ready;
        if feedback_visible {
            sample.send_feedback_visible = true;
        }
        let output_visible = !sample.first_output_visible && sample.first_assistant_applied;
        if output_visible {
            sample.first_output_visible = true;
        }
        let settled = !sample.turn_settled
            && sample.turn_completed_applied
            && !observation.turn_running
            && observation.composer_focused
            && !observation.compaction_running;
        if settled {
            sample.turn_settled = true;
        }

        if feedback_visible {
            self.mark("send_feedback_surface_committed", Some(sample_index));
        }
        if output_visible {
            self.mark("first_output_surface_committed", Some(sample_index));
        }
        if settled {
            self.mark("turn_settled_surface_committed", Some(sample_index));
        }
    }

    pub(crate) fn finish(&mut self) -> io::Result<()> {
        match self.writer.as_mut() {
            Some(writer) => writer.finish(),
            None => Ok(()),
        }
    }

    fn mark(&mut self, event: &'static str, sample_index: Option<u64>) {
        if let Some(writer) = self.writer.as_mut() {
            writer.mark(event, sample_index);
        }
    }
}

impl Default for TuiJourneyProfileProbe {
    fn default() -> Self {
        Self::disabled()
    }
}

impl GatewayEventProfileKind {
    fn from_event(event: &GatewayEvent) -> Self {
        match event {
            GatewayEvent::EntryStarted { entry, .. }
            | GatewayEvent::EntryUpdated { entry, .. }
            | GatewayEvent::EntryCompleted { entry, .. } => Self {
                first_assistant: entry_has_nonempty_assistant_text(entry),
                turn_completed: false,
            },
            GatewayEvent::TurnCompleted {
                committed_entries, ..
            } => Self {
                first_assistant: committed_entries
                    .iter()
                    .any(entry_has_nonempty_assistant_text),
                turn_completed: true,
            },
            _ => Self::default(),
        }
    }
}

fn entry_has_nonempty_assistant_text(entry: &TranscriptEntry) -> bool {
    entry.role == TranscriptEntryRole::Assistant && entry.blocks.iter().any(block_has_nonempty_text)
}

fn block_has_nonempty_text(block: &TranscriptBlock) -> bool {
    block.kind == TranscriptBlockKind::Text
        && block
            .body
            .as_deref()
            .or(block.detail.as_deref())
            .or(block.preview.as_deref())
            .is_some_and(|text| !text.trim().is_empty())
}

#[derive(Debug)]
struct ProfileWriter {
    origin: Instant,
    epoch_unix_ms: u64,
    clock_domain_id: String,
    next_seq: u64,
    sender: Option<Sender<String>>,
    writer_thread: Option<JoinHandle<io::Result<()>>>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ProfileRecord<'a> {
    schema_version: u8,
    surface: &'static str,
    clock_domain_id: &'a str,
    epoch_unix_ms: u64,
    monotonic_ns: u64,
    seq: u64,
    sample_index: Option<u64>,
    event: &'static str,
}

impl ProfileWriter {
    fn new(file: File) -> io::Result<Self> {
        let (sender, receiver) = mpsc::channel::<String>();
        let writer_thread = thread::Builder::new()
            .name("pevo-tui-profile-writer".to_string())
            .spawn(move || {
                let mut writer = BufWriter::new(file);
                while let Ok(line) = receiver.recv() {
                    writer.write_all(line.as_bytes())?;
                    writer.write_all(b"\n")?;
                    writer.flush()?;
                }
                writer.flush()
            })?;
        Ok(Self {
            origin: Instant::now(),
            epoch_unix_ms: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis()
                .try_into()
                .unwrap_or(u64::MAX),
            clock_domain_id: format!("tui:{}", uuid::Uuid::now_v7()),
            next_seq: 1,
            sender: Some(sender),
            writer_thread: Some(writer_thread),
        })
    }

    fn mark(&mut self, event: &'static str, sample_index: Option<u64>) {
        let monotonic_ns = self
            .origin
            .elapsed()
            .as_nanos()
            .try_into()
            .unwrap_or(u64::MAX);
        let record = ProfileRecord {
            schema_version: 1,
            surface: "tui",
            clock_domain_id: &self.clock_domain_id,
            epoch_unix_ms: self.epoch_unix_ms,
            monotonic_ns,
            seq: self.next_seq,
            sample_index,
            event,
        };
        self.next_seq = self.next_seq.saturating_add(1);
        let Ok(line) = serde_json::to_string(&record) else {
            return;
        };
        if let Some(sender) = &self.sender {
            let _ = sender.send(line);
        }
    }

    fn finish(&mut self) -> io::Result<()> {
        self.sender.take();
        let Some(writer_thread) = self.writer_thread.take() else {
            return Ok(());
        };
        writer_thread
            .join()
            .map_err(|_| io::Error::other("TUI profile writer thread panicked"))?
    }
}

impl Drop for ProfileWriter {
    fn drop(&mut self) {
        let _ = self.finish();
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use serde_json::Value;

    use super::*;

    #[test]
    fn gateway_classification_ignores_blank_user_and_reasoning_content() {
        let assistant = GatewayEvent::EntryUpdated {
            turn_id: "turn-1".to_string(),
            entry: transcript_entry(
                TranscriptEntryRole::Assistant,
                TranscriptBlockKind::Text,
                Some("visible"),
            ),
        };
        assert!(GatewayEventProfileKind::from_event(&assistant).first_assistant);

        for event in [
            GatewayEvent::EntryUpdated {
                turn_id: "turn-1".to_string(),
                entry: transcript_entry(
                    TranscriptEntryRole::Assistant,
                    TranscriptBlockKind::Text,
                    Some("  "),
                ),
            },
            GatewayEvent::EntryUpdated {
                turn_id: "turn-1".to_string(),
                entry: transcript_entry(
                    TranscriptEntryRole::Assistant,
                    TranscriptBlockKind::Reasoning,
                    Some("hidden reasoning"),
                ),
            },
            GatewayEvent::EntryUpdated {
                turn_id: "turn-1".to_string(),
                entry: transcript_entry(
                    TranscriptEntryRole::User,
                    TranscriptBlockKind::Text,
                    Some("prompt"),
                ),
            },
        ] {
            assert!(!GatewayEventProfileKind::from_event(&event).first_assistant);
        }
    }

    #[test]
    fn probe_writes_ordered_content_free_jsonl_for_two_samples() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("tui-profile.jsonl");
        let mut probe = TuiJourneyProfileProbe::for_path(&path).expect("probe");

        probe.mark_ui_constructed();
        probe.mark_history_loaded();
        probe.observe_frame(TuiProfileFrameObservation {
            input_ready: true,
            ..TuiProfileFrameObservation::default()
        });
        exercise_sample(&mut probe);
        exercise_sample(&mut probe);
        probe.finish().expect("finish probe");

        let records = std::fs::read_to_string(path)
            .expect("profile JSONL")
            .lines()
            .map(|line| serde_json::from_str::<Value>(line).expect("valid JSON record"))
            .collect::<Vec<_>>();
        let events = records
            .iter()
            .map(|record| record["event"].as_str().expect("event"))
            .collect::<Vec<_>>();
        assert_eq!(
            events,
            [
                "process_started",
                "ui_constructed",
                "history_loaded",
                "input_ready",
                "send_committed",
                "send_feedback_surface_committed",
                "gateway_first_assistant_event_received",
                "gateway_first_assistant_event_applied",
                "first_output_surface_committed",
                "turn_completed_received",
                "turn_completed_applied",
                "turn_settled_surface_committed",
                "send_committed",
                "send_feedback_surface_committed",
                "gateway_first_assistant_event_received",
                "gateway_first_assistant_event_applied",
                "first_output_surface_committed",
                "turn_completed_received",
                "turn_completed_applied",
                "turn_settled_surface_committed",
            ]
        );

        let allowed_keys = BTreeSet::from([
            "clockDomainId",
            "epochUnixMs",
            "event",
            "monotonicNs",
            "sampleIndex",
            "schemaVersion",
            "seq",
            "surface",
        ]);
        let clock_domain = records[0]["clockDomainId"].as_str().expect("clock domain");
        for (index, record) in records.iter().enumerate() {
            assert_eq!(record["schemaVersion"], 1);
            assert_eq!(record["surface"], "tui");
            assert_eq!(record["clockDomainId"], clock_domain);
            assert_eq!(record["seq"], (index + 1) as u64);
            assert_eq!(
                record
                    .as_object()
                    .expect("record object")
                    .keys()
                    .map(String::as_str)
                    .collect::<BTreeSet<_>>(),
                allowed_keys
            );
        }
        assert!(
            records[..4]
                .iter()
                .all(|record| record["sampleIndex"].is_null())
        );
        assert!(
            records[4..12]
                .iter()
                .all(|record| record["sampleIndex"] == 0)
        );
        assert!(
            records[12..]
                .iter()
                .all(|record| record["sampleIndex"] == 1)
        );
        assert!(records.windows(2).all(|pair| {
            pair[0]["monotonicNs"].as_u64().expect("monotonic")
                <= pair[1]["monotonicNs"].as_u64().expect("monotonic")
        }));
    }

    fn exercise_sample(probe: &mut TuiJourneyProfileProbe) {
        probe.mark_send_committed();
        probe.observe_frame(TuiProfileFrameObservation {
            input_ready: true,
            send_feedback_ready: true,
            turn_running: true,
            composer_focused: true,
            compaction_running: false,
        });
        let assistant = GatewayEventProfileKind {
            first_assistant: true,
            turn_completed: false,
        };
        probe.observe_gateway_event(assistant, false);
        probe.observe_gateway_event(assistant, false);
        probe.observe_gateway_event(assistant, true);
        probe.observe_frame(TuiProfileFrameObservation {
            input_ready: true,
            send_feedback_ready: true,
            turn_running: true,
            composer_focused: true,
            compaction_running: false,
        });
        let completed = GatewayEventProfileKind {
            first_assistant: false,
            turn_completed: true,
        };
        probe.observe_gateway_event(completed, false);
        probe.observe_gateway_event(completed, true);
        probe.observe_frame(TuiProfileFrameObservation {
            input_ready: true,
            send_feedback_ready: false,
            turn_running: false,
            composer_focused: true,
            compaction_running: false,
        });
    }

    fn transcript_entry(
        role: TranscriptEntryRole,
        kind: TranscriptBlockKind,
        body: Option<&str>,
    ) -> TranscriptEntry {
        TranscriptEntry {
            id: "entry-1".to_string(),
            thread_id: "thread-1".to_string(),
            turn_id: Some("turn-1".to_string()),
            message_seq: Some(1),
            role,
            status: psychevo_gateway::TranscriptBlockStatus::Running,
            source: "runtime.stream".to_string(),
            blocks: vec![TranscriptBlock {
                id: "block-1".to_string(),
                kind,
                status: psychevo_gateway::TranscriptBlockStatus::Running,
                order: 0,
                phase_ordinal: None,
                source: "runtime.stream".to_string(),
                title: None,
                body: body.map(str::to_string),
                preview: None,
                detail: None,
                artifact_ids: Vec::new(),
                metadata: None,
                result: None,
                created_at_ms: 0,
                updated_at_ms: 0,
            }],
            metadata: None,
            usage: None,
            accounting: None,
            created_at_ms: 0,
            updated_at_ms: 0,
        }
    }
}
