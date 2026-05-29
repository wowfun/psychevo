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
}
