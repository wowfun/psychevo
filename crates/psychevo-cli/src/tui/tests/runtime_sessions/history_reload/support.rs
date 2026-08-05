use crate::tui::{
    TranscriptBlock, TranscriptBlockKind, TranscriptBlockStatus, TranscriptEntry,
    TranscriptEntryRole,
};

pub(super) fn gateway_test_entry(
    id: &str,
    kind: TranscriptBlockKind,
    status: TranscriptBlockStatus,
    title: Option<&str>,
    text: Option<&str>,
    metadata: Option<serde_json::Value>,
) -> TranscriptEntry {
    TranscriptEntry {
        id: id.to_string(),
        thread_id: String::new(),
        turn_id: Some("turn-1".to_string()),
        message_seq: None,
        role: TranscriptEntryRole::Assistant,
        status,
        source: "runtime.stream".to_string(),
        blocks: vec![TranscriptBlock {
            id: format!("{id}:block"),
            kind,
            status,
            order: 0,
            phase_ordinal: None,
            source: "runtime.stream".to_string(),
            title: title.map(str::to_string),
            body: text.map(str::to_string),
            preview: text.map(str::to_string),
            detail: text.map(str::to_string),
            artifact_ids: Vec::new(),
            metadata,
            result: None,
            created_at_ms: 1,
            updated_at_ms: 1,
        }],
        metadata: None,
        usage: None,
        accounting: None,
        created_at_ms: 1,
        updated_at_ms: 1,
    }
}
