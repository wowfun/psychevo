pub(crate) const MAX_PANIC_PAYLOAD_BYTES: usize = 2_048;
pub(crate) const MAX_PANIC_BACKTRACE_BYTES: usize = 8_192;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PanicEvidence {
    pub(crate) payload: String,
    pub(crate) backtrace: String,
}

impl PanicEvidence {
    pub(crate) fn capture(payload: &(dyn std::any::Any + Send)) -> Self {
        Self {
            payload: panic_payload_text(payload),
            backtrace: bounded_text(
                std::backtrace::Backtrace::force_capture().to_string(),
                MAX_PANIC_BACKTRACE_BYTES,
            ),
        }
    }

    pub(crate) fn terminal_message(&self, prefix: &str) -> String {
        format!(
            "{prefix}: {}\nCaptured unwind backtrace:\n{}",
            self.payload, self.backtrace
        )
    }
}

pub(crate) fn bounded_text(mut text: String, max_bytes: usize) -> String {
    if text.len() <= max_bytes {
        return text;
    }
    let mut end = max_bytes.saturating_sub(3).min(text.len());
    while !text.is_char_boundary(end) {
        end = end.saturating_sub(1);
    }
    text.truncate(end);
    text.push_str("...");
    text
}

fn panic_payload_text(payload: &(dyn std::any::Any + Send)) -> String {
    let text = payload
        .downcast_ref::<String>()
        .cloned()
        .or_else(|| {
            payload
                .downcast_ref::<&str>()
                .map(|value| (*value).to_string())
        })
        .unwrap_or_else(|| "non-string panic payload".to_string());
    bounded_text(text, MAX_PANIC_PAYLOAD_BYTES)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn panic_evidence_preserves_kind_and_bounds_utf8() {
        let long = "界".repeat(MAX_PANIC_PAYLOAD_BYTES);
        let evidence = PanicEvidence::capture(&long);
        assert!(evidence.payload.len() <= MAX_PANIC_PAYLOAD_BYTES);
        assert!(evidence.payload.ends_with("..."));

        let evidence = PanicEvidence::capture(&7_u64);
        assert_eq!(evidence.payload, "non-string panic payload");
        assert!(evidence.backtrace.len() <= MAX_PANIC_BACKTRACE_BYTES);
    }
}
