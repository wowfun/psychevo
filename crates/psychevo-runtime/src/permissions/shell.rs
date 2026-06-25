use tree_sitter::{Node, Parser, Tree};
use tree_sitter_bash::LANGUAGE as BASH;

pub(crate) fn shell_command_tokens(command: &str) -> Option<Vec<String>> {
    let commands = shell_word_only_commands(command)?;
    match commands.as_slice() {
        [single] => Some(single.clone()),
        _ => None,
    }
}

pub(crate) fn shell_word_only_commands(src: &str) -> Option<Vec<Vec<String>>> {
    let tree = parse_shell(src)?;
    word_only_commands_sequence(&tree, src)
}

pub(crate) fn shell_lc_word_only_commands(command: &[String]) -> Option<Vec<Vec<String>>> {
    let [shell, flag, script] = command else {
        return None;
    };
    if !matches!(flag.as_str(), "-lc" | "-c")
        || !matches!(
            shell_basename(shell).as_deref(),
            Some("bash" | "zsh" | "sh")
        )
    {
        return None;
    }
    shell_word_only_commands(script)
}

pub(crate) fn shell_command_invocations(src: &str) -> Option<Vec<Vec<String>>> {
    let tree = parse_shell(src)?;
    if tree.root_node().has_error() {
        return None;
    }

    let root = tree.root_node();
    let mut cursor = root.walk();
    let mut stack = vec![root];
    let mut command_nodes = Vec::new();
    while let Some(node) = stack.pop() {
        if node.kind() == "command" {
            command_nodes.push(node);
        }
        for child in node.children(&mut cursor) {
            stack.push(child);
        }
    }

    command_nodes.sort_by_key(Node::start_byte);
    let mut commands = Vec::new();
    for node in command_nodes {
        if let Some(command) = parse_command_invocation(node, src) {
            commands.push(command);
        }
    }
    Some(commands)
}

pub(crate) fn shell_has_untracked_background(src: &str) -> bool {
    let Some(tree) = parse_shell(src) else {
        return legacy_background_scan(src);
    };
    if tree.root_node().has_error() {
        return legacy_background_scan(src);
    }
    if tree_contains_background_operator(tree.root_node()) {
        return true;
    }
    shell_command_invocations(src)
        .unwrap_or_default()
        .iter()
        .any(|command| command_uses_detaching_wrapper(command))
}

pub(crate) fn shell_basename(raw: &str) -> Option<String> {
    std::path::Path::new(raw)
        .file_name()
        .and_then(|name| name.to_str())
        .map(|name| {
            #[cfg(windows)]
            {
                let name = name.to_ascii_lowercase();
                for suffix in [".exe", ".cmd", ".bat", ".com"] {
                    if let Some(stripped) = name.strip_suffix(suffix) {
                        return stripped.to_string();
                    }
                }
                name
            }
            #[cfg(not(windows))]
            {
                name.to_string()
            }
        })
}

fn tree_contains_background_operator(root: Node<'_>) -> bool {
    let mut cursor = root.walk();
    let mut stack = vec![root];
    while let Some(node) = stack.pop() {
        if node.kind() == "&" && !is_non_background_ampersand(node) {
            return true;
        }
        for child in node.children(&mut cursor) {
            stack.push(child);
        }
    }
    false
}

fn is_non_background_ampersand(node: Node<'_>) -> bool {
    let mut current = node.parent();
    while let Some(parent) = current {
        match parent.kind() {
            "binary_expression" | "heredoc_body" | "heredoc_content" => return true,
            _ => current = parent.parent(),
        }
    }
    false
}

fn command_uses_detaching_wrapper(command: &[String]) -> bool {
    for word in command {
        if is_env_assignment(word) || word.starts_with('-') {
            continue;
        }
        let Some(name) = shell_basename(word) else {
            continue;
        };
        if matches!(name.as_str(), "env" | "sudo" | "command" | "exec") {
            continue;
        }
        return matches!(name.as_str(), "nohup" | "setsid" | "disown");
    }
    false
}

fn is_env_assignment(word: &str) -> bool {
    let Some((name, _)) = word.split_once('=') else {
        return false;
    };
    !name.is_empty()
        && name
            .chars()
            .all(|ch| ch == '_' || ch.is_ascii_alphanumeric())
        && name
            .chars()
            .next()
            .is_some_and(|ch| ch == '_' || ch.is_ascii_alphabetic())
}

fn legacy_background_scan(command: &str) -> bool {
    let normalized = command.split_whitespace().collect::<Vec<_>>().join(" ");
    normalized.ends_with(" &")
        || normalized.contains(" & ")
        || normalized.starts_with("nohup ")
        || normalized.contains(" nohup ")
        || normalized.starts_with("disown")
        || normalized.contains("; disown")
        || normalized.contains("&& disown")
        || normalized.starts_with("setsid ")
        || normalized.contains(" setsid ")
}

fn parse_shell(src: &str) -> Option<Tree> {
    let lang = BASH.into();
    let mut parser = Parser::new();
    parser.set_language(&lang).ok()?;
    parser.parse(src, None)
}

fn word_only_commands_sequence(tree: &Tree, src: &str) -> Option<Vec<Vec<String>>> {
    if tree.root_node().has_error() {
        return None;
    }

    const ALLOWED_KINDS: &[&str] = &[
        "program",
        "list",
        "pipeline",
        "command",
        "command_name",
        "word",
        "string",
        "string_content",
        "raw_string",
        "number",
        "concatenation",
    ];
    const ALLOWED_PUNCT_TOKENS: &[&str] = &["&&", "||", ";", "|", "\"", "'"];

    let root = tree.root_node();
    let mut cursor = root.walk();
    let mut stack = vec![root];
    let mut command_nodes = Vec::new();
    while let Some(node) = stack.pop() {
        let kind = node.kind();
        if node.is_named() {
            if !ALLOWED_KINDS.contains(&kind) {
                return None;
            }
            if kind == "command" {
                command_nodes.push(node);
            }
        } else if !(ALLOWED_PUNCT_TOKENS.contains(&kind) || kind.trim().is_empty()) {
            return None;
        }
        for child in node.children(&mut cursor) {
            stack.push(child);
        }
    }

    command_nodes.sort_by_key(Node::start_byte);
    let mut commands = Vec::new();
    for node in command_nodes {
        commands.push(parse_plain_command(node, src)?);
    }
    Some(commands)
}

fn parse_plain_command(cmd: Node<'_>, src: &str) -> Option<Vec<String>> {
    if cmd.kind() != "command" {
        return None;
    }
    let mut words = Vec::new();
    let mut cursor = cmd.walk();
    for child in cmd.named_children(&mut cursor) {
        match child.kind() {
            "command_name" => {
                let word = child.named_child(0)?;
                if word.kind() != "word" {
                    return None;
                }
                words.push(word.utf8_text(src.as_bytes()).ok()?.to_string());
            }
            "word" | "number" => {
                words.push(child.utf8_text(src.as_bytes()).ok()?.to_string());
            }
            "string" => words.push(parse_double_quoted_string(child, src)?),
            "raw_string" => words.push(parse_raw_string(child, src)?),
            "concatenation" => words.push(parse_concatenation(child, src)?),
            _ => return None,
        }
    }
    (!words.is_empty()).then_some(words)
}

fn parse_command_invocation(cmd: Node<'_>, src: &str) -> Option<Vec<String>> {
    if cmd.kind() != "command" {
        return None;
    }
    let mut words = Vec::new();
    let mut cursor = cmd.walk();
    for child in cmd.named_children(&mut cursor) {
        match child.kind() {
            "command_name" => {
                let word = child.named_child(0)?;
                words.push(parse_literal_shell_text(word, src)?);
            }
            "word" | "number" | "string" | "raw_string" | "concatenation" => {
                if let Some(word) = parse_literal_shell_text(child, src) {
                    words.push(word);
                }
            }
            _ => {}
        }
    }
    (!words.is_empty()).then_some(words)
}

fn parse_literal_shell_text(node: Node<'_>, src: &str) -> Option<String> {
    match node.kind() {
        "word" | "number" => node.utf8_text(src.as_bytes()).ok().map(str::to_string),
        "string" => parse_double_quoted_string(node, src),
        "raw_string" => parse_raw_string(node, src),
        "concatenation" => parse_concatenation(node, src),
        _ => None,
    }
}

fn parse_concatenation(node: Node<'_>, src: &str) -> Option<String> {
    let mut out = String::new();
    let mut cursor = node.walk();
    for part in node.named_children(&mut cursor) {
        match part.kind() {
            "word" | "number" => out.push_str(part.utf8_text(src.as_bytes()).ok()?),
            "string" => out.push_str(&parse_double_quoted_string(part, src)?),
            "raw_string" => out.push_str(&parse_raw_string(part, src)?),
            _ => return None,
        }
    }
    (!out.is_empty()).then_some(out)
}

fn parse_double_quoted_string(node: Node<'_>, src: &str) -> Option<String> {
    if node.kind() != "string" {
        return None;
    }
    let mut cursor = node.walk();
    for part in node.named_children(&mut cursor) {
        if part.kind() != "string_content" {
            return None;
        }
    }
    let raw = node.utf8_text(src.as_bytes()).ok()?;
    raw.strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
        .map(str::to_string)
}

fn parse_raw_string(node: Node<'_>, src: &str) -> Option<String> {
    if node.kind() != "raw_string" {
        return None;
    }
    let raw = node.utf8_text(src.as_bytes()).ok()?;
    raw.strip_prefix('\'')
        .and_then(|value| value.strip_suffix('\''))
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_word_only_shell_sequences() {
        let commands = shell_word_only_commands("ls && pwd; echo 'hi there' | wc -l").unwrap();
        assert_eq!(
            commands,
            vec![
                vec!["ls".to_string()],
                vec!["pwd".to_string()],
                vec!["echo".to_string(), "hi there".to_string()],
                vec!["wc".to_string(), "-l".to_string()],
            ]
        );
    }

    #[test]
    fn rejects_redirects_and_expansions() {
        assert!(shell_word_only_commands("ls > out.txt").is_none());
        assert!(shell_word_only_commands(r#"echo "$HOME""#).is_none());
    }

    #[test]
    fn command_invocations_keep_quoted_sql_as_argument() {
        let commands = shell_command_invocations(
            r#"sqlite3 db "UPDATE stories SET content = 'system halted';"; sudo reboot"#,
        )
        .unwrap();
        assert_eq!(
            commands,
            vec![
                vec![
                    "sqlite3".to_string(),
                    "db".to_string(),
                    "UPDATE stories SET content = 'system halted';".to_string(),
                ],
                vec!["sudo".to_string(), "reboot".to_string()],
            ]
        );
    }

    #[test]
    fn detects_real_background_execution() {
        assert!(shell_has_untracked_background("sleep 60 &"));
        assert!(shell_has_untracked_background("nohup sleep 60"));
        assert!(shell_has_untracked_background("setsid sleep 60"));
        assert!(shell_has_untracked_background("disown %1"));
        assert!(shell_has_untracked_background("sudo nohup sleep 60"));
        assert!(shell_has_untracked_background("env X=1 setsid sleep 60"));
    }

    #[test]
    fn ignores_ampersands_in_foreground_content() {
        assert!(!shell_has_untracked_background("printf 'a & b\n'"));
        assert!(!shell_has_untracked_background(
            "cat > /tmp/fixnull.c <<'EOF'\nint flags = value & mask;\nEOF",
        ));
    }
}
