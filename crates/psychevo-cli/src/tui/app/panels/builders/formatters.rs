fn is_missing_session_usage_error(error: &psychevo_runtime::Error, session_id: &str) -> bool {
    matches!(
        error,
        psychevo_runtime::Error::Message(message)
            if message == &format!("session not found: {session_id}")
    )
}

fn format_cache_read_percent(value: Option<f64>) -> String {
    value
        .map(|percent| format!("{percent:.0}%"))
        .unwrap_or_else(|| "-".to_string())
}
