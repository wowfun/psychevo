use crate::tui::{
    TranscriptBlock, TranscriptBlockKind, TranscriptBlockStatus, TranscriptEntry,
    TranscriptEntryRole,
};

pub(super) fn gateway_test_entry(
    id: &str,
    kind: TranscriptBlockKind,
    status: TranscriptBlockStatus,
    title: Option<&str>,
    text: &str,
) -> TranscriptEntry {
    TranscriptEntry {
        id: id.to_string(),
        thread_id: "session-1".to_string(),
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
            preview: Some(text.to_string()),
            detail: Some(text.to_string()),
            body: Some(text.to_string()),
            artifact_ids: Vec::new(),
            metadata: if title == Some("Preamble") {
                Some(serde_json::json!({"projection": "assistant_preamble"}))
            } else {
                None
            },
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

pub(super) fn numbered_lines(start: usize, end: usize) -> String {
    (start..=end)
        .map(|index| format!("line {index}"))
        .collect::<Vec<_>>()
        .join("\n")
}
