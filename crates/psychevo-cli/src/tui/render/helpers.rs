#[allow(unused_imports)]
pub(crate) use super::*;
pub(crate) fn bottom_panel_height(height: u16) -> u16 {
    16.min(height.saturating_sub(6)).max(8)
}

pub(crate) fn rect_contains(rect: Rect, column: u16, row: u16) -> bool {
    column >= rect.x
        && column < rect.x.saturating_add(rect.width)
        && row >= rect.y
        && row < rect.y.saturating_add(rect.height)
}

pub(crate) fn sidebar_heading(label: &'static str) -> Line<'static> {
    Line::from(Span::styled(
        label,
        Style::default().add_modifier(Modifier::BOLD),
    ))
}

pub(crate) fn short_session(id: &str) -> &str {
    &id[..id.len().min(8)]
}

pub(crate) fn truncate_chars(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_string();
    }
    let keep = max_chars.saturating_sub(1);
    format!("{}…", value.chars().take(keep).collect::<String>())
}

pub(crate) fn short_fetch_error(value: &str) -> String {
    let value = value
        .trim()
        .replace(['\r', '\n', '\t'], " ")
        .trim_start_matches("config failed: ")
        .trim_start_matches("HTTP request failed: ")
        .trim_start_matches("error: ")
        .to_string();
    if value == "timeout" {
        return value;
    }
    truncate_chars(&value, 120)
}

pub(crate) fn format_session_date(timestamp_ms: i64) -> String {
    let days = timestamp_ms.div_euclid(86_400_000);
    let (year, month, day) = civil_from_days(days);
    format!("{year:04}-{month:02}-{day:02}")
}

pub(crate) fn format_session_time(timestamp_ms: i64) -> String {
    let millis = timestamp_ms.rem_euclid(86_400_000);
    let minutes = millis / 60_000;
    let hour = minutes / 60;
    let minute = minutes % 60;
    format!("{hour:02}:{minute:02}")
}

pub(crate) fn civil_from_days(days_since_epoch: i64) -> (i64, u32, u32) {
    let z = days_since_epoch + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = mp + if mp < 10 { 3 } else { -9 };
    let year = y + if m <= 2 { 1 } else { 0 };
    (year, m as u32, d as u32)
}

pub(crate) fn on_off(value: bool) -> &'static str {
    if value { "on" } else { "off" }
}
