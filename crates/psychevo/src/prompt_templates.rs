use std::collections::{BTreeMap, BTreeSet};

pub(crate) const BASE_MODE_DEFAULT: &str = include_str!("../templates/base/mode.default.md");
pub(crate) const BASE_MODE_PLAN: &str = include_str!("../templates/base/mode.plan.md");
pub(crate) const BASE_MODE_DEFAULT_NO_TOOLS: &str =
    include_str!("../templates/base/mode.default.no_tools.md");
pub(crate) const BASE_MODE_PLAN_NO_TOOLS: &str =
    include_str!("../templates/base/mode.plan.no_tools.md");
pub(crate) const SELECTED_MAIN_AGENT: &str = include_str!("../templates/selected_main_agent.md");
pub(crate) const SELECTED_MAIN_AGENT_WITH_INSTRUCTIONS: &str =
    include_str!("../templates/selected_main_agent.with_instructions.md");
pub(crate) const SELECTED_CHILD_AGENT: &str = include_str!("../templates/selected_child_agent.md");
pub(crate) const SELECTED_CHILD_AGENT_WITH_INSTRUCTIONS: &str =
    include_str!("../templates/selected_child_agent.with_instructions.md");
pub(crate) const SELECTED_SYSTEM_AGENT: &str =
    include_str!("../templates/selected_system_agent.md");
pub(crate) const SELECTED_SYSTEM_AGENT_WITH_INSTRUCTIONS: &str =
    include_str!("../templates/selected_system_agent.with_instructions.md");
pub(crate) const CHILD_AGENT_CONTROL: &str = include_str!("../templates/child_agent_control.md");
pub(crate) const AGENT_CATALOG: &str = include_str!("../templates/agent_catalog.md");
pub(crate) const SKILL_INDEX: &str = include_str!("../templates/skill_index.md");
pub(crate) const PROJECT_CONTEXT: &str = include_str!("../templates/project_context.md");
pub(crate) const REQUIRED_AGENT_CALLS: &str = include_str!("../templates/required_agent_calls.md");
pub(crate) const SESSION_TITLE_INSTRUCTION: &str =
    include_str!("../templates/session_title_instruction.md");
pub(crate) const SESSION_TITLE_REQUEST: &str =
    include_str!("../templates/session_title_request.md");
pub(crate) const SESSION_TITLE_REQUEST_SELECTED_SKILLS: &str =
    include_str!("../templates/session_title_request.selected_skills.md");
pub(crate) const COMPACTION_SUMMARY_SYSTEM: &str =
    include_str!("../templates/compaction_summary_system.md");
pub(crate) const COMPACTION_SUMMARY_USER: &str =
    include_str!("../templates/compaction_summary_user.md");
pub(crate) const COMPACTION_SUMMARY_MANUAL_FOCUS_SECTION: &str =
    include_str!("../templates/compaction_summary_manual_focus_section.md");
pub(crate) const COMPACTION_SUMMARY_PREVIOUS_SECTION: &str =
    include_str!("../templates/compaction_summary_previous_section.md");
pub(crate) const COMPACTION_SUMMARY_PREFIX: &str =
    include_str!("../templates/compaction_summary_prefix.md");
pub(crate) const SIDE_BOUNDARY: &str = include_str!("../templates/side_boundary.md");

pub(crate) fn base_mode_default() -> &'static str {
    template_text(BASE_MODE_DEFAULT)
}

pub(crate) fn base_mode_plan() -> &'static str {
    template_text(BASE_MODE_PLAN)
}

pub(crate) fn base_mode_default_no_tools() -> &'static str {
    template_text(BASE_MODE_DEFAULT_NO_TOOLS)
}

pub(crate) fn base_mode_plan_no_tools() -> &'static str {
    template_text(BASE_MODE_PLAN_NO_TOOLS)
}

pub(crate) fn selected_main_agent(name: &str, description: &str, instructions: &str) -> String {
    selected_agent(
        SELECTED_MAIN_AGENT,
        SELECTED_MAIN_AGENT_WITH_INSTRUCTIONS,
        name,
        description,
        instructions,
    )
}

pub(crate) fn selected_child_agent(name: &str, description: &str, instructions: &str) -> String {
    selected_agent(
        SELECTED_CHILD_AGENT,
        SELECTED_CHILD_AGENT_WITH_INSTRUCTIONS,
        name,
        description,
        instructions,
    )
}

pub(crate) fn selected_system_agent(name: &str, description: &str, instructions: &str) -> String {
    selected_agent(
        SELECTED_SYSTEM_AGENT,
        SELECTED_SYSTEM_AGENT_WITH_INSTRUCTIONS,
        name,
        description,
        instructions,
    )
}

pub(crate) fn child_agent_control() -> &'static str {
    template_text(CHILD_AGENT_CONTROL)
}

pub(crate) fn agent_catalog_intro() -> &'static str {
    template_text(AGENT_CATALOG)
}

pub(crate) fn skill_index_intro() -> &'static str {
    template_text(SKILL_INDEX)
}

pub(crate) fn project_context(content: &str) -> String {
    render(PROJECT_CONTEXT, &[("content", content)])
}

pub(crate) fn required_agent_calls(required_agent_mentions: &str) -> String {
    render(
        REQUIRED_AGENT_CALLS,
        &[("required_agent_mentions", required_agent_mentions)],
    )
}

pub(crate) fn session_title_instruction() -> &'static str {
    template_text(SESSION_TITLE_INSTRUCTION)
}

pub(crate) fn session_title_request(prompt: &str) -> String {
    render(SESSION_TITLE_REQUEST, &[("prompt", prompt)])
}

pub(crate) fn session_title_request_with_selected_skills(
    selected_skills: &str,
    prompt: &str,
) -> String {
    render(
        SESSION_TITLE_REQUEST_SELECTED_SKILLS,
        &[("selected_skills", selected_skills), ("prompt", prompt)],
    )
}

pub(crate) fn compaction_summary_system() -> &'static str {
    template_text(COMPACTION_SUMMARY_SYSTEM)
}

pub(crate) fn compaction_summary_user(
    manual_focus_section: &str,
    previous_summary_section: &str,
    messages: &str,
) -> String {
    render(
        COMPACTION_SUMMARY_USER,
        &[
            ("manual_focus_section", manual_focus_section),
            ("previous_summary_section", previous_summary_section),
            ("messages", messages),
        ],
    )
}

pub(crate) fn compaction_summary_manual_focus_section(instructions: &str) -> String {
    format!(
        "{}\n\n",
        render(
            COMPACTION_SUMMARY_MANUAL_FOCUS_SECTION,
            &[("instructions", instructions)]
        )
    )
}

pub(crate) fn compaction_summary_previous_section(summary: &str) -> String {
    format!(
        "{}\n\n",
        render(COMPACTION_SUMMARY_PREVIOUS_SECTION, &[("summary", summary)])
    )
}

pub(crate) fn compaction_summary_prefix() -> &'static str {
    template_text(COMPACTION_SUMMARY_PREFIX)
}

pub fn side_conversation_boundary_prompt() -> &'static str {
    template_text(SIDE_BOUNDARY)
}

pub(crate) fn selected_agent(
    plain_template: &'static str,
    with_instructions_template: &'static str,
    name: &str,
    description: &str,
    instructions: &str,
) -> String {
    let instructions = instructions.trim();
    if instructions.is_empty() {
        render(
            plain_template,
            &[("name", name), ("description", description)],
        )
    } else {
        render(
            with_instructions_template,
            &[
                ("name", name),
                ("description", description),
                ("instructions", instructions),
            ],
        )
    }
}

pub(crate) fn render(source: &'static str, variables: &[(&str, &str)]) -> String {
    let source = template_text(source);
    let segments = parse_template(source);
    let values = variable_map(variables);
    let placeholders = segments
        .iter()
        .filter_map(|segment| match segment {
            Segment::Placeholder(name) => Some(name.clone()),
            Segment::Literal(_) => None,
        })
        .collect::<BTreeSet<_>>();

    for placeholder in &placeholders {
        assert!(
            values.contains_key(placeholder.as_str()),
            "template placeholder `{placeholder}` is missing a value"
        );
    }
    for name in values.keys() {
        assert!(
            placeholders.contains(*name),
            "template value `{name}` is not used by this template"
        );
    }

    let mut rendered = String::new();
    for segment in segments {
        match segment {
            Segment::Literal(literal) => rendered.push_str(&literal),
            Segment::Placeholder(name) => {
                rendered.push_str(values.get(name.as_str()).expect("placeholder checked"));
            }
        }
    }
    rendered
}

pub(crate) fn template_text(source: &'static str) -> &'static str {
    let mut end = source.len();
    while end > 0 {
        let byte = source.as_bytes()[end - 1];
        if byte == b'\n' || byte == b'\r' {
            end -= 1;
        } else {
            break;
        }
    }
    &source[..end]
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Segment {
    Literal(String),
    Placeholder(String),
}

pub(crate) fn parse_template(source: &str) -> Vec<Segment> {
    let mut segments = Vec::new();
    let mut literal_start = 0usize;
    let mut cursor = 0usize;
    while cursor < source.len() {
        let rest = &source[cursor..];
        if rest.starts_with("{{{{") {
            push_literal(&mut segments, &source[literal_start..cursor]);
            push_literal(&mut segments, "{{");
            cursor += 4;
            literal_start = cursor;
            continue;
        }
        if rest.starts_with("}}}}") {
            push_literal(&mut segments, &source[literal_start..cursor]);
            push_literal(&mut segments, "}}");
            cursor += 4;
            literal_start = cursor;
            continue;
        }
        if rest.starts_with("{{") {
            push_literal(&mut segments, &source[literal_start..cursor]);
            let (placeholder, next_cursor) = parse_placeholder(source, cursor);
            segments.push(Segment::Placeholder(placeholder));
            cursor = next_cursor;
            literal_start = cursor;
            continue;
        }
        assert!(
            !rest.starts_with("}}"),
            "template contains an unmatched `}}` at byte {cursor}"
        );
        let Some(ch) = rest.chars().next() else {
            break;
        };
        cursor += ch.len_utf8();
    }
    push_literal(&mut segments, &source[literal_start..]);
    segments
}

pub(crate) fn parse_placeholder(source: &str, start: usize) -> (String, usize) {
    let content_start = start + 2;
    let Some(relative_end) = source[content_start..].find("}}") else {
        panic!("template placeholder starting at byte {start} is missing `}}`");
    };
    let content_end = content_start + relative_end;
    let raw = &source[content_start..content_end];
    assert!(
        !raw.contains("{{"),
        "template placeholder starting at byte {start} contains a nested `{{`"
    );
    let name = raw.trim();
    assert!(
        !name.is_empty(),
        "template placeholder at byte {start} is empty"
    );
    (name.to_string(), content_end + 2)
}

pub(crate) fn push_literal(segments: &mut Vec<Segment>, literal: &str) {
    if literal.is_empty() {
        return;
    }
    if let Some(Segment::Literal(existing)) = segments.last_mut() {
        existing.push_str(literal);
    } else {
        segments.push(Segment::Literal(literal.to_string()));
    }
}

pub(crate) fn variable_map<'a>(variables: &'a [(&'a str, &'a str)]) -> BTreeMap<&'a str, &'a str> {
    let mut values = BTreeMap::new();
    for (name, value) in variables {
        assert!(
            values.insert(*name, *value).is_none(),
            "template value `{name}` was provided more than once"
        );
    }
    values
}

#[cfg(test)]
pub(crate) mod tests {
    use super::{render, template_text};

    #[test]
    fn render_interpolates_placeholders() {
        assert_eq!(
            render("Hello, {{ name }}.\n", &[("name", "Ada")]),
            "Hello, Ada."
        );
    }

    #[test]
    #[should_panic(expected = "missing a value")]
    fn render_panics_for_missing_values() {
        let _ = render("Hello, {{ name }}.", &[]);
    }

    #[test]
    #[should_panic(expected = "not used by this template")]
    fn render_panics_for_extra_values() {
        let _ = render("Hello.", &[("name", "Ada")]);
    }

    #[test]
    fn render_preserves_literal_braces() {
        assert_eq!(render("{{{{ value }}}}", &[]), "{{ value }}");
    }

    #[test]
    fn template_text_trims_only_file_trailing_newlines() {
        assert_eq!(template_text("one\n\n"), "one");
        assert_eq!(template_text("one  \n"), "one  ");
    }

    #[test]
    fn render_preserves_internal_whitespace() {
        assert_eq!(
            render("A\n\n  B {{ value }} C\n", &[("value", "x")]),
            "A\n\n  B x C"
        );
    }
}
