pub fn user_text_message(text: impl Into<String>) -> Message {
    Message::User {
        content: vec![UserContentBlock::text(text)],
        timestamp_ms: now_ms(),
    }
}

pub fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

fn duration_ms_u64(duration: Duration) -> u64 {
    duration.as_millis().min(u128::from(u64::MAX)) as u64
}

pub struct NoopEventSink;

impl EventSink for NoopEventSink {
    fn emit(&self, _event: AgentEvent) -> BoxFuture<'static, Result<()>> {
        Box::pin(async { Ok(()) })
    }
}
