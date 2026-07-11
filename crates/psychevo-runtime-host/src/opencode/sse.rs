use std::collections::{HashSet, VecDeque};

use serde_json::Value;

use crate::{RetryClass, RuntimeError, RuntimeErrorStage};

use super::types::NativeEvent;

const MAX_LINE_BYTES: usize = 256 * 1024;
const MAX_EVENT_BYTES: usize = 2 * 1024 * 1024;
const DEDUP_CAPACITY: usize = 4096;

#[derive(Debug, Default)]
pub(crate) struct SseDecoder {
    pending: Vec<u8>,
    data: Vec<String>,
    data_bytes: usize,
}

impl SseDecoder {
    pub(crate) fn push(&mut self, chunk: &[u8]) -> Result<Vec<String>, RuntimeError> {
        if self.pending.len().saturating_add(chunk.len()) > MAX_EVENT_BYTES {
            return Err(protocol_error(
                "OpenCode SSE frame exceeded the bounded buffer",
            ));
        }
        self.pending.extend_from_slice(chunk);
        let mut output = Vec::new();
        while let Some(index) = self.pending.iter().position(|byte| *byte == b'\n') {
            let mut line = self.pending.drain(..=index).collect::<Vec<_>>();
            line.pop();
            if line.last() == Some(&b'\r') {
                line.pop();
            }
            if line.len() > MAX_LINE_BYTES {
                return Err(protocol_error(
                    "OpenCode SSE line exceeded the bounded buffer",
                ));
            }
            let line = String::from_utf8(line)
                .map_err(|_| protocol_error("OpenCode SSE contained non-UTF-8 data"))?;
            if line.is_empty() {
                if !self.data.is_empty() {
                    output.push(self.data.join("\n"));
                    self.data.clear();
                    self.data_bytes = 0;
                }
                continue;
            }
            if line.starts_with(':') {
                continue;
            }
            if let Some(value) = line.strip_prefix("data:") {
                let value = value.strip_prefix(' ').unwrap_or(value);
                self.data_bytes = self.data_bytes.saturating_add(value.len());
                if self.data_bytes > MAX_EVENT_BYTES {
                    return Err(protocol_error(
                        "OpenCode SSE event exceeded the bounded buffer",
                    ));
                }
                self.data.push(value.to_string());
            }
        }
        if self.pending.len() > MAX_LINE_BYTES {
            return Err(protocol_error(
                "OpenCode SSE line exceeded the bounded buffer",
            ));
        }
        Ok(output)
    }
}

#[derive(Debug)]
pub(crate) struct EventDeduper {
    seen: HashSet<String>,
    order: VecDeque<String>,
    capacity: usize,
}

impl Default for EventDeduper {
    fn default() -> Self {
        Self {
            seen: HashSet::new(),
            order: VecDeque::new(),
            capacity: DEDUP_CAPACITY,
        }
    }
}

impl EventDeduper {
    pub(crate) fn accept(&mut self, process_epoch: u64, id: Option<&str>) -> bool {
        let Some(id) = id else {
            return true;
        };
        let key = format!("{process_epoch}:{id}");
        if !self.seen.insert(key.clone()) {
            return false;
        }
        self.order.push_back(key);
        while self.order.len() > self.capacity {
            if let Some(expired) = self.order.pop_front() {
                self.seen.remove(&expired);
            }
        }
        true
    }
}

pub(crate) fn decode_native_event(data: &str) -> Result<NativeEvent, RuntimeError> {
    let value: Value = serde_json::from_str(data)
        .map_err(|_| protocol_error("OpenCode SSE event was not valid JSON"))?;
    let wrapper = value.get("payload").unwrap_or(&value);
    let directory = value
        .get("directory")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned);
    let event_type = wrapper
        .get("type")
        .and_then(Value::as_str)
        .ok_or_else(|| protocol_error("OpenCode SSE event did not contain a type"))?;

    if event_type == "sync" {
        let sync = wrapper
            .get("syncEvent")
            .ok_or_else(|| protocol_error("OpenCode sync event did not contain syncEvent"))?;
        let event_type = sync
            .get("type")
            .and_then(Value::as_str)
            .ok_or_else(|| protocol_error("OpenCode sync event did not contain a type"))?;
        return Ok(NativeEvent {
            id: sync
                .get("id")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned),
            directory,
            event_type: unversioned_type(event_type).to_string(),
            properties: sync.get("data").cloned().unwrap_or(Value::Null),
        });
    }

    Ok(NativeEvent {
        id: wrapper
            .get("id")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned),
        directory,
        event_type: event_type.to_string(),
        properties: wrapper.get("properties").cloned().unwrap_or(Value::Null),
    })
}

fn unversioned_type(value: &str) -> &str {
    let Some((head, tail)) = value.rsplit_once('.') else {
        return value;
    };
    if tail.chars().all(|character| character.is_ascii_digit()) {
        head
    } else {
        value
    }
}

fn protocol_error(message: impl Into<String>) -> RuntimeError {
    RuntimeError::new(
        "invalid_event_stream",
        RuntimeErrorStage::Transport,
        RetryClass::Reconnect,
        message,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decoder_handles_fragmented_crlf_and_multiline_data() {
        let mut decoder = SseDecoder::default();
        assert!(decoder.push(b"data: {\"a\":").expect("first").is_empty());
        assert!(
            decoder
                .push(b"1}\r\ndata: tail\r")
                .expect("second")
                .is_empty()
        );
        assert_eq!(
            decoder.push(b"\n\r\n").expect("third"),
            vec!["{\"a\":1}\ntail"]
        );
    }

    #[test]
    fn sync_event_uses_inner_id_and_unversions_type() {
        let event = decode_native_event(
            r#"{"directory":"/tmp/a","payload":{"type":"sync","syncEvent":{"id":"evt_1","type":"session.created.1","data":{"sessionID":"ses_1"}}}}"#,
        )
        .expect("event");
        assert_eq!(event.id.as_deref(), Some("evt_1"));
        assert_eq!(event.event_type, "session.created");
        assert_eq!(event.session_id(), Some("ses_1"));
    }

    #[test]
    fn deduper_collapses_direct_and_sync_copies_per_process_epoch() {
        let mut deduper = EventDeduper::default();
        assert!(deduper.accept(3, Some("evt_1")));
        assert!(!deduper.accept(3, Some("evt_1")));
        assert!(deduper.accept(4, Some("evt_1")));
    }
}
