#[cfg(test)]
fn slash_completion(input: &str) -> Option<String> {
    slash_completion_with_items(input, &crate::tui::slash::base_slash_menu_items())
}

fn slash_completion_with_items(input: &str, items: &[SlashMenuItem]) -> Option<String> {
    if input.contains('\n') {
        return None;
    }
    let leading_len = input.len().saturating_sub(input.trim_start().len());
    let leading = &input[..leading_len];
    let typed = input.trim_start();
    if !typed.starts_with('/') {
        return None;
    }
    let items = slash_prefix_menu_items_from(typed, items);
    if items.is_empty() {
        return None;
    }
    let commands = items
        .iter()
        .map(|item| item.completion.as_str())
        .collect::<Vec<_>>();
    let common = common_prefix(&commands);
    let completed = if common.len() > typed.len() {
        common
    } else if let Some(item) = items.first().filter(|item| item.configured_alias)
        && item.completion.len() > typed.len()
    {
        item.completion.clone()
    } else if commands.contains(&typed) || commands.len() > 1 {
        return None;
    } else {
        commands[0].to_string()
    };
    (completed != typed).then(|| format!("{leading}{completed}"))
}

fn selected_slash_menu_command_with_items(
    input: &str,
    selected_index: usize,
    items: &[SlashMenuItem],
) -> Option<String> {
    if input.contains('\n') {
        return None;
    }
    let typed = input.trim_start();
    slash_menu_items_from(typed, items)
        .get(selected_index)
        .map(|item| item.replacement.clone())
}

fn should_submit_typed_slash(input: &str) -> bool {
    let trimmed = input.trim();
    matches!(trimmed, "/session" | "/thinking")
        || crate::command_registry::slash_command_spec(trimmed).is_some()
}

fn should_parse_slash_command_input(input: &str) -> bool {
    !prompt_starts_with_supported_image_path(input)
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ShellEscapeInput {
    command: String,
    history_text: String,
}

fn parse_shell_escape_input(input: &str) -> Option<ShellEscapeInput> {
    let stripped = input.trim_start().strip_prefix('!')?;
    Some(ShellEscapeInput {
        command: stripped.to_string(),
        history_text: format!("!{stripped}"),
    })
}

fn is_empty_shell_escape_input(input: &str) -> bool {
    parse_shell_escape_input(input).is_some_and(|escape| escape.command.trim().is_empty())
}

fn common_prefix(values: &[&str]) -> String {
    let Some(first) = values.first() else {
        return String::new();
    };
    let mut end = first.len();
    for value in values.iter().skip(1) {
        end = first
            .as_bytes()
            .iter()
            .zip(value.as_bytes())
            .take_while(|(left, right)| left == right)
            .count()
            .min(end);
    }
    first[..end].to_string()
}
