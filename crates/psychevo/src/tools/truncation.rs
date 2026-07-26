#[allow(unused_imports)]
pub(crate) use super::*;
#[derive(Debug)]
pub(crate) struct Truncated {
    pub(crate) content: String,
    pub(crate) truncated: bool,
    pub(crate) lines: usize,
    pub(crate) bytes: usize,
    pub(crate) truncated_by: Option<&'static str>,
    pub(crate) first_line_exceeds_limit: bool,
}

pub(crate) fn truncate_head(input: &str, max_bytes: usize, max_lines: usize) -> Truncated {
    let mut out = String::new();
    let mut lines = 0usize;
    let mut bytes = 0usize;
    let mut truncated = false;
    let mut truncated_by = None;
    let mut first_line_exceeds_limit = false;
    for (idx, line) in input.split('\n').enumerate() {
        let addition = if idx == 0 {
            line.to_string()
        } else {
            format!("\n{line}")
        };
        if lines >= max_lines {
            truncated = true;
            truncated_by = Some("lines");
            break;
        }
        if bytes + addition.len() > max_bytes {
            truncated = true;
            truncated_by = Some("bytes");
            first_line_exceeds_limit = idx == 0 && lines == 0;
            break;
        }
        bytes += addition.len();
        out.push_str(&addition);
        lines += 1;
    }
    Truncated {
        content: out,
        truncated,
        lines,
        bytes,
        truncated_by,
        first_line_exceeds_limit,
    }
}

pub(crate) fn truncate_tail(input: &str, max_bytes: usize, max_lines: usize) -> Truncated {
    let all = input.split('\n').collect::<Vec<_>>();
    let mut selected = Vec::new();
    let mut bytes = 0usize;
    for line in all.iter().rev() {
        let addition = line.len() + usize::from(!selected.is_empty());
        if selected.len() >= max_lines || bytes + addition > max_bytes {
            break;
        }
        bytes += addition;
        selected.push(*line);
    }
    selected.reverse();
    Truncated {
        content: selected.join("\n"),
        truncated: selected.len() < all.len(),
        lines: selected.len(),
        bytes,
        truncated_by: if selected.len() < all.len() {
            if selected.len() >= max_lines {
                Some("lines")
            } else {
                Some("bytes")
            }
        } else {
            None
        },
        first_line_exceeds_limit: false,
    }
}

pub(crate) fn dominant_line_ending(text: &str) -> &'static str {
    let crlf = text.matches("\r\n").count();
    let lf = text.matches('\n').count();
    if crlf > 0 && crlf >= lf.saturating_sub(crlf) {
        "\r\n"
    } else {
        "\n"
    }
}

pub(crate) fn normalize_lf(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\r', "\n")
}

pub(crate) fn restore_line_endings(text: &str, line_ending: &str) -> String {
    if line_ending == "\n" {
        text.to_string()
    } else {
        text.replace('\n', line_ending)
    }
}
