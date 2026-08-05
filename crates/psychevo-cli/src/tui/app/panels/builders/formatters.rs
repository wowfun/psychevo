pub(super) fn is_missing_session_usage_error(error: &psychevo::Error, session_id: &str) -> bool {
    matches!(
        error,
        psychevo::Error::Message(message)
            if message == &format!("session not found: {session_id}")
    )
}

pub(super) fn format_cache_read_percent(value: Option<f64>) -> String {
    value
        .map(|percent| format!("{percent:.0}%"))
        .unwrap_or_else(|| "-".to_string())
}

pub(super) fn format_effective_token_total(tokens: Option<u64>, status: &str) -> String {
    match tokens {
        Some(tokens) if status == "partial" => format!("≥{tokens}"),
        Some(tokens) => tokens.to_string(),
        None => "unavailable".to_string(),
    }
}
