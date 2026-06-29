use super::*;

pub(super) fn channel_cwd(default_cwd: &Path, connection: &ChannelRuntimeConnection) -> PathBuf {
    let raw = connection.cwd.as_deref().unwrap_or("");
    if raw.trim().is_empty() {
        return default_cwd.to_path_buf();
    }
    let path = PathBuf::from(raw);
    let path = if path.is_absolute() {
        path
    } else {
        default_cwd.join(path)
    };
    psychevo_runtime::canonicalize_cwd(&path).unwrap_or(path)
}

pub(super) fn wechat_context_store_path(home: &Path, id: &str) -> PathBuf {
    home.join("gateway").join("channels").join(format!(
        "{}-wechat-context.json",
        safe_channel_file_stem(id)
    ))
}

fn safe_channel_file_stem(value: &str) -> String {
    let mut out = value
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_'))
        .collect::<String>();
    if out.is_empty() {
        out = "channel".to_string();
    }
    out
}

pub(in crate::server) fn redact_channel_error(value: &str) -> String {
    let mut out = value.replace("Bearer ", "Bearer [redacted] ");
    for key in ["token=", "access_token=", "bot_token="] {
        while let Some(index) = out.find(key) {
            let start = index + key.len();
            let end = out[start..]
                .find(|ch: char| ch == '&' || ch.is_whitespace())
                .map(|offset| start + offset)
                .unwrap_or(out.len());
            out.replace_range(start..end, "[redacted]");
        }
    }
    if out.len() > 240 {
        out.truncate(240);
        out.push_str("...");
    }
    out
}
