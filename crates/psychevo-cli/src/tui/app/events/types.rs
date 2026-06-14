#[allow(unused_imports)]
pub(crate) use super::*;

#[derive(Debug, Clone, Copy)]
pub(crate) struct GatewayTranscriptEntryMeta<'a> {
    role: TranscriptEntryRole,
    thread_id: &'a str,
    turn_id: Option<&'a str>,
    entry_id: &'a str,
    message_seq: Option<i64>,
    source: &'a str,
}
