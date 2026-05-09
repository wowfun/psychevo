#[derive(Debug)]
struct SseParser {
    buffer: Vec<u8>,
    current_event: SseEvent,
    saw_data: bool,
    bom_checked: bool,
    done_seen: bool,
}

#[derive(Debug)]
struct SseEvent {
    event: String,
    data: String,
}

impl Default for SseEvent {
    fn default() -> Self {
        Self {
            event: "message".to_string(),
            data: String::new(),
        }
    }
}

impl SseParser {
    fn new() -> Self {
        Self {
            buffer: Vec::new(),
            current_event: SseEvent::default(),
            saw_data: false,
            bom_checked: false,
            done_seen: false,
        }
    }

    fn push(&mut self, chunk: &[u8]) -> Result<Vec<ChatCompletionChunk>> {
        self.buffer.extend_from_slice(chunk);
        let mut events = Vec::new();
        self.drain_complete_lines(false, &mut events)?;
        Ok(events)
    }

    fn finish(&mut self) -> Result<Vec<ChatCompletionChunk>> {
        let mut events = Vec::new();
        self.drain_complete_lines(true, &mut events)?;
        if self.saw_data {
            self.dispatch_current(&mut events)?;
        }
        Ok(events)
    }

    fn done_seen(&self) -> bool {
        self.done_seen
    }

    fn drain_complete_lines(
        &mut self,
        finish: bool,
        events: &mut Vec<ChatCompletionChunk>,
    ) -> Result<()> {
        if !self.strip_bom_if_ready(finish) {
            return Ok(());
        }

        loop {
            let Some((line_end, consumed)) = next_sse_line(&self.buffer, finish) else {
                break;
            };
            let line = std::str::from_utf8(&self.buffer[..line_end])
                .map_err(|err| Error::Provider(format!("SSE line is not UTF-8: {err}")))?
                .to_string();
            self.buffer.drain(..consumed);
            self.process_line(&line, events)?;
        }
        Ok(())
    }

    fn strip_bom_if_ready(&mut self, finish: bool) -> bool {
        if self.bom_checked {
            return true;
        }
        const BOM: &[u8] = &[0xEF, 0xBB, 0xBF];
        if self.buffer.len() >= BOM.len() {
            self.bom_checked = true;
            if self.buffer.starts_with(BOM) {
                self.buffer.drain(..BOM.len());
            }
            return true;
        }
        if !finish && BOM.starts_with(&self.buffer) {
            return false;
        }
        self.bom_checked = true;
        true
    }

    fn process_line(&mut self, line: &str, events: &mut Vec<ChatCompletionChunk>) -> Result<()> {
        if line.is_empty() {
            if self.saw_data {
                self.dispatch_current(events)?;
            }
            self.current_event = SseEvent::default();
            self.saw_data = false;
            return Ok(());
        }
        if line.starts_with(':') {
            return Ok(());
        }
        let (field, value) = line.split_once(':').map_or((line, ""), |(field, value)| {
            (field, value.strip_prefix(' ').unwrap_or(value))
        });
        match field {
            "event" => self.current_event.event = value.to_string(),
            "data" => {
                if self.saw_data {
                    self.current_event.data.push('\n');
                }
                self.current_event.data.push_str(value);
                self.saw_data = true;
            }
            _ => {}
        }
        Ok(())
    }

    fn dispatch_current(&mut self, events: &mut Vec<ChatCompletionChunk>) -> Result<()> {
        let data = self.current_event.data.trim();
        if data.is_empty() {
            return Ok(());
        }
        if data == "[DONE]" {
            self.done_seen = true;
            return Ok(());
        }
        if let Ok(raw) = serde_json::from_str::<Value>(data)
            && let Some(error) = raw.get("error")
        {
            let message = error
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("provider returned an error in stream");
            return Err(Error::Provider(message.to_string()));
        }
        events.push(serde_json::from_str(data)?);
        Ok(())
    }
}

fn next_sse_line(buffer: &[u8], finish: bool) -> Option<(usize, usize)> {
    let pos = buffer
        .iter()
        .position(|byte| *byte == b'\n' || *byte == b'\r');
    match pos {
        Some(index) => {
            if buffer[index] == b'\r' && buffer.get(index + 1).is_none() && !finish {
                return None;
            }
            let consumed =
                if buffer[index] == b'\r' && buffer.get(index + 1).copied() == Some(b'\n') {
                    index + 2
                } else {
                    index + 1
                };
            Some((index, consumed))
        }
        None if finish && !buffer.is_empty() => Some((buffer.len(), buffer.len())),
        None => None,
    }
}
