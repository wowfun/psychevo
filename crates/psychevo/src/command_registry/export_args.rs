#[allow(unused_imports)]
pub(crate) use super::*;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedSessionExportCommand {
    pub path: Option<String>,
    pub format: SessionExportFormat,
    pub include: SessionExportIncludeSet,
}

pub fn parse_session_export_command_args(
    args: &str,
    artifact_kind: SessionArtifactKind,
    usage: &str,
) -> crate::Result<ParsedSessionExportCommand> {
    let tokens = split_slash_argument_tokens(args)?;
    let allow_format = artifact_kind == SessionArtifactKind::Export;
    let mut path = None;
    let mut format = SessionExportFormat::Markdown;
    let mut include = None;
    let mut index = 0usize;
    while index < tokens.len() {
        let token = &tokens[index];
        match token.as_str() {
            "--include" | "-i" => {
                index += 1;
                let Some(value) = tokens.get(index) else {
                    return Err(usage_error(usage));
                };
                include = Some(parse_session_export_include(value, artifact_kind, usage)?);
            }
            "--format" | "-f" if allow_format => {
                index += 1;
                let Some(value) = tokens.get(index) else {
                    return Err(usage_error(usage));
                };
                format = parse_session_export_format(value).ok_or_else(|| usage_error(usage))?;
            }
            value if allow_format && value.starts_with("--format=") => {
                let value = value.trim_start_matches("--format=");
                format = parse_session_export_format(value).ok_or_else(|| usage_error(usage))?;
            }
            value if allow_format && value.starts_with("-f=") => {
                let value = value.trim_start_matches("-f=");
                format = parse_session_export_format(value).ok_or_else(|| usage_error(usage))?;
            }
            value if value.starts_with("--include=") => {
                let value = value.trim_start_matches("--include=");
                include = Some(parse_session_export_include(value, artifact_kind, usage)?);
            }
            value if value.starts_with('-') => return Err(usage_error(usage)),
            value => {
                if path.is_some() {
                    return Err(usage_error(usage));
                }
                path = Some(value.to_string());
            }
        }
        index += 1;
    }
    Ok(ParsedSessionExportCommand {
        path,
        format,
        include: include.unwrap_or_else(|| SessionExportIncludeSet::default_for(artifact_kind)),
    })
}

pub fn parse_session_export_include(
    value: &str,
    artifact_kind: SessionArtifactKind,
    usage: &str,
) -> crate::Result<SessionExportIncludeSet> {
    SessionExportIncludeSet::parse(value, artifact_kind).map_err(|_| usage_error(usage))
}

pub fn parse_session_export_format(value: &str) -> Option<SessionExportFormat> {
    match value {
        "markdown" | "md" => Some(SessionExportFormat::Markdown),
        "json" => Some(SessionExportFormat::Json),
        _ => None,
    }
}

pub fn split_slash_argument_tokens(input: &str) -> crate::Result<Vec<String>> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut quote = None;
    let mut chars = input.chars().peekable();
    while let Some(ch) = chars.next() {
        match (quote, ch) {
            (Some(active), value) if value == active => quote = None,
            (None, '"' | '\'') => quote = Some(ch),
            (None, value) if value.is_whitespace() => {
                if !current.is_empty() {
                    tokens.push(std::mem::take(&mut current));
                }
            }
            (_, '\\') => {
                if let Some(next) = chars.next() {
                    current.push(next);
                }
            }
            (_, value) => current.push(value),
        }
    }
    if quote.is_some() {
        return Err(crate::Error::Message(
            "unterminated quoted argument".to_string(),
        ));
    }
    if !current.is_empty() {
        tokens.push(current);
    }
    Ok(tokens)
}

fn usage_error(usage: &str) -> crate::Error {
    crate::Error::Message(format!("usage: {usage}"))
}
