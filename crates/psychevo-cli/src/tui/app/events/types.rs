use crate::tui::TranscriptEntryRole;

#[derive(Debug, Clone, Copy)]
pub(crate) struct GatewayTranscriptEntryMeta<'a> {
    pub(super) role: TranscriptEntryRole,
    pub(super) thread_id: &'a str,
    pub(super) turn_id: Option<&'a str>,
    pub(super) entry_id: &'a str,
    pub(super) message_seq: Option<i64>,
    pub(super) source: &'a str,
}
