#[allow(unused_imports)]
pub(crate) use super::*;

pub(crate) fn highlight_code_line(line: &str, lang: &str) -> Vec<Span<'static>> {
    let theme = tui_theme();
    if is_comment_line(line, lang) {
        return vec![Span::styled(line.to_string(), theme.dim_style())];
    }

    let mut spans = Vec::new();
    let mut chars = line.char_indices().peekable();
    while let Some((start, ch)) = chars.next() {
        if matches!(ch, '"' | '\'' | '`') {
            let quote = ch;
            let mut end = start + ch.len_utf8();
            let mut escaped = false;
            for (index, next) in chars.by_ref() {
                end = index + next.len_utf8();
                if escaped {
                    escaped = false;
                    continue;
                }
                if next == '\\' {
                    escaped = true;
                    continue;
                }
                if next == quote {
                    break;
                }
            }
            spans.push(Span::styled(
                line[start..end].to_string(),
                theme.success_style(),
            ));
            continue;
        }
        if is_identifier_start(ch) {
            let mut end = start + ch.len_utf8();
            while let Some((index, next)) = chars.peek().copied() {
                if !is_identifier_continue(next) {
                    break;
                }
                chars.next();
                end = index + next.len_utf8();
            }
            let token = &line[start..end];
            let style = if is_code_keyword(token, lang) {
                theme.accent_style().add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };
            spans.push(Span::styled(token.to_string(), style));
            continue;
        }
        if ch.is_ascii_digit() {
            let mut end = start + ch.len_utf8();
            while let Some((index, next)) = chars.peek().copied() {
                if !next.is_ascii_alphanumeric() && next != '_' && next != '.' {
                    break;
                }
                chars.next();
                end = index + next.len_utf8();
            }
            spans.push(Span::styled(
                line[start..end].to_string(),
                theme.identity_style(),
            ));
            continue;
        }
        if matches!(
            ch,
            '{' | '}' | '[' | ']' | '(' | ')' | ':' | ';' | ',' | '.'
        ) {
            spans.push(Span::styled(ch.to_string(), theme.dim_style()));
        } else {
            spans.push(Span::raw(ch.to_string()));
        }
    }
    spans
}

pub(crate) fn is_identifier_start(ch: char) -> bool {
    ch == '_' || ch.is_ascii_alphabetic()
}

pub(crate) fn is_identifier_continue(ch: char) -> bool {
    ch == '_' || ch == '-' || ch.is_ascii_alphanumeric()
}

pub(crate) fn normalized_lang(lang: &str) -> &str {
    match lang {
        "rs" => "rust",
        "js" | "jsx" | "ts" | "tsx" => "javascript",
        "sh" | "bash" | "zsh" => "shell",
        other => other,
    }
}

pub(crate) fn is_comment_line(line: &str, lang: &str) -> bool {
    let trimmed = line.trim_start();
    match normalized_lang(lang) {
        "shell" | "python" | "ruby" | "yaml" | "toml" => trimmed.starts_with('#'),
        "sql" => trimmed.starts_with("--"),
        _ => trimmed.starts_with("//"),
    }
}

pub(crate) fn is_code_keyword(token: &str, lang: &str) -> bool {
    match normalized_lang(lang) {
        "rust" => matches!(
            token,
            "as" | "async"
                | "await"
                | "break"
                | "const"
                | "continue"
                | "crate"
                | "else"
                | "enum"
                | "false"
                | "fn"
                | "for"
                | "if"
                | "impl"
                | "in"
                | "let"
                | "loop"
                | "match"
                | "mod"
                | "move"
                | "mut"
                | "pub"
                | "ref"
                | "return"
                | "self"
                | "struct"
                | "trait"
                | "true"
                | "type"
                | "use"
                | "where"
                | "while"
        ),
        "javascript" => matches!(
            token,
            "async"
                | "await"
                | "break"
                | "case"
                | "catch"
                | "class"
                | "const"
                | "else"
                | "export"
                | "false"
                | "for"
                | "from"
                | "function"
                | "if"
                | "import"
                | "let"
                | "new"
                | "return"
                | "switch"
                | "true"
                | "try"
                | "type"
                | "while"
        ),
        "json" | "yaml" | "toml" => matches!(token, "true" | "false" | "null"),
        "shell" => matches!(
            token,
            "case"
                | "do"
                | "done"
                | "elif"
                | "else"
                | "esac"
                | "fi"
                | "for"
                | "function"
                | "if"
                | "in"
                | "then"
                | "while"
        ),
        _ => matches!(token, "true" | "false" | "null"),
    }
}
