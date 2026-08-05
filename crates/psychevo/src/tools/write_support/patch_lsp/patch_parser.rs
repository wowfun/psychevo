use super::super::text_edit::{
    V4aHunk, V4aLine, V4aOperation, V4aOperationKind, fuzzy_find_and_replace, strategy_exact,
};
use crate::error::{Error, Result};

pub(crate) fn parse_v4a_patch(patch: &str) -> Result<Vec<V4aOperation>> {
    let lines = patch.lines().collect::<Vec<_>>();
    let start = lines
        .iter()
        .position(|line| line.trim() == "*** Begin Patch")
        .map(|idx| idx + 1)
        .unwrap_or(0);
    let end = lines
        .iter()
        .position(|line| line.trim() == "*** End Patch")
        .unwrap_or(lines.len());
    let mut operations = Vec::new();
    let mut current: Option<V4aOperation> = None;
    let mut current_hunk: Option<V4aHunk> = None;
    for line in &lines[start..end] {
        if let Some(path) = marker_value(line, "*** Update File:") {
            push_v4a_current(&mut operations, &mut current, &mut current_hunk);
            current = Some(V4aOperation {
                kind: V4aOperationKind::Update,
                file_path: path,
                hunks: Vec::new(),
            });
        } else if let Some(path) = marker_value(line, "*** Add File:") {
            push_v4a_current(&mut operations, &mut current, &mut current_hunk);
            current = Some(V4aOperation {
                kind: V4aOperationKind::Add,
                file_path: path,
                hunks: Vec::new(),
            });
            current_hunk = Some(V4aHunk {
                context_hint: None,
                lines: Vec::new(),
            });
        } else if let Some(path) = marker_value(line, "*** Delete File:") {
            push_v4a_current(&mut operations, &mut current, &mut current_hunk);
            operations.push(V4aOperation {
                kind: V4aOperationKind::Delete,
                file_path: path,
                hunks: Vec::new(),
            });
        } else if marker_value(line, "*** Move File:").is_some()
            || marker_value(line, "*** Move to:").is_some()
        {
            return Err(Error::Message(
                "patch moves are not supported; use Add/Delete or an explicit shell operation"
                    .to_string(),
            ));
        } else if line.starts_with("@@") {
            if let Some(op) = current.as_mut()
                && let Some(hunk) = current_hunk.take()
                && !hunk.lines.is_empty()
            {
                op.hunks.push(hunk);
            }
            current_hunk = Some(V4aHunk {
                context_hint: parse_context_hint(line),
                lines: Vec::new(),
            });
        } else if let Some(op) = current.as_mut() {
            let hunk = current_hunk.get_or_insert_with(|| V4aHunk {
                context_hint: None,
                lines: Vec::new(),
            });
            if let Some(content) = line.strip_prefix('+') {
                hunk.lines.push(V4aLine {
                    prefix: '+',
                    content: content.to_string(),
                });
            } else if let Some(content) = line.strip_prefix('-') {
                hunk.lines.push(V4aLine {
                    prefix: '-',
                    content: content.to_string(),
                });
            } else if let Some(content) = line.strip_prefix(' ') {
                hunk.lines.push(V4aLine {
                    prefix: ' ',
                    content: content.to_string(),
                });
            } else if line.starts_with('\\') {
                continue;
            } else if !line.is_empty() || op.kind == V4aOperationKind::Add {
                hunk.lines.push(V4aLine {
                    prefix: ' ',
                    content: (*line).to_string(),
                });
            }
        }
    }
    push_v4a_current(&mut operations, &mut current, &mut current_hunk);
    if operations.is_empty() {
        return Err(Error::Message("patch contains no operations".to_string()));
    }
    for op in &operations {
        if op.file_path.trim().is_empty() {
            return Err(Error::Message("patch operation has empty path".to_string()));
        }
        if op.kind == V4aOperationKind::Update && op.hunks.is_empty() {
            return Err(Error::Message(format!(
                "update operation has no hunks: {}",
                op.file_path
            )));
        }
    }
    Ok(operations)
}

fn marker_value(line: &str, marker: &str) -> Option<String> {
    line.trim()
        .strip_prefix(marker)
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn push_v4a_current(
    operations: &mut Vec<V4aOperation>,
    current: &mut Option<V4aOperation>,
    current_hunk: &mut Option<V4aHunk>,
) {
    if let Some(mut op) = current.take() {
        if let Some(hunk) = current_hunk.take()
            && !hunk.lines.is_empty()
        {
            op.hunks.push(hunk);
        }
        operations.push(op);
    }
}

fn parse_context_hint(line: &str) -> Option<String> {
    let trimmed = line.trim();
    let inner = trimmed.strip_prefix("@@")?.strip_suffix("@@")?.trim();
    (!inner.is_empty()).then(|| inner.to_string())
}

pub(crate) fn apply_v4a_update_hunks(
    content: &str,
    hunks: &[V4aHunk],
) -> std::result::Result<String, String> {
    let mut updated = content.to_string();
    for hunk in hunks {
        let search_lines = hunk
            .lines
            .iter()
            .filter(|line| line.prefix == ' ' || line.prefix == '-')
            .map(|line| line.content.as_str())
            .collect::<Vec<_>>();
        let replacement_lines = hunk
            .lines
            .iter()
            .filter(|line| line.prefix == ' ' || line.prefix == '+')
            .map(|line| line.content.as_str())
            .collect::<Vec<_>>();
        if search_lines.is_empty() {
            let insert_text = replacement_lines.join("\n");
            updated =
                apply_addition_only_hunk(&updated, hunk.context_hint.as_deref(), &insert_text)?;
            continue;
        }
        let search = search_lines.join("\n");
        let replacement = replacement_lines.join("\n");
        match fuzzy_find_and_replace(&updated, &search, &replacement, false) {
            Ok(outcome) => updated = outcome.content,
            Err(err) => {
                return Err(format!(
                    "hunk {} not found: {err}",
                    hunk.context_hint
                        .as_ref()
                        .map(|hint| format!("{hint:?}"))
                        .unwrap_or_else(|| "(no hint)".to_string())
                ));
            }
        }
    }
    Ok(updated)
}

fn apply_addition_only_hunk(
    content: &str,
    context_hint: Option<&str>,
    insert_text: &str,
) -> std::result::Result<String, String> {
    if insert_text.is_empty() {
        return Ok(content.to_string());
    }
    let Some(hint) = context_hint else {
        return Ok(format!(
            "{}\n{}\n",
            content.trim_end_matches('\n'),
            insert_text
        ));
    };
    let matches = strategy_exact(content, hint);
    if matches.is_empty() {
        return Err(format!(
            "addition-only hunk context hint {hint:?} not found"
        ));
    }
    if matches.len() > 1 {
        return Err(format!(
            "addition-only hunk context hint {hint:?} is ambiguous ({} occurrences)",
            matches.len()
        ));
    }
    let insert_at = content[matches[0].end..]
        .find('\n')
        .map(|idx| matches[0].end + idx + 1)
        .unwrap_or(content.len());
    let mut out = String::new();
    out.push_str(&content[..insert_at]);
    out.push_str(insert_text);
    out.push('\n');
    out.push_str(&content[insert_at..]);
    Ok(out)
}

pub(crate) fn v4a_add_content(op: &V4aOperation) -> String {
    op.hunks
        .iter()
        .flat_map(|hunk| &hunk.lines)
        .filter(|line| line.prefix == '+')
        .map(|line| line.content.as_str())
        .collect::<Vec<_>>()
        .join("\n")
}
