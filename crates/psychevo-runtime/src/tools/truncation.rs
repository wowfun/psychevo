#[derive(Debug)]
struct Truncated {
    content: String,
    truncated: bool,
    lines: usize,
}

fn truncate_head(input: &str, max_bytes: usize, max_lines: usize) -> Truncated {
    let mut out = String::new();
    let mut lines = 0usize;
    let mut bytes = 0usize;
    let mut truncated = false;
    for (idx, line) in input.split('\n').enumerate() {
        let addition = if idx == 0 {
            line.to_string()
        } else {
            format!("\n{line}")
        };
        if lines >= max_lines || bytes + addition.len() > max_bytes {
            truncated = true;
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
    }
}

fn truncate_tail(input: &str, max_bytes: usize, max_lines: usize) -> Truncated {
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
    }
}

fn dominant_line_ending(text: &str) -> &'static str {
    let crlf = text.matches("\r\n").count();
    let lf = text.matches('\n').count();
    if crlf > 0 && crlf >= lf.saturating_sub(crlf) {
        "\r\n"
    } else {
        "\n"
    }
}

fn normalize_lf(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\r', "\n")
}

fn restore_line_endings(text: &str, line_ending: &str) -> String {
    if line_ending == "\n" {
        text.to_string()
    } else {
        text.replace('\n', line_ending)
    }
}

fn unified_diff(path: &str, old: &str, new: &str) -> String {
    TextDiff::from_lines(old, new)
        .unified_diff()
        .header(&format!("a/{path}"), &format!("b/{path}"))
        .to_string()
}
