use super::snapshot::{
    ADVICE_LIMIT, CATEGORY_ADVICE_PERCENT, CONTEXT_BAR_MAX_CELLS, CONTEXT_BAR_MIN_CELLS,
    ContextAdvice, ContextScope, ContextSnapshot, TOTAL_CRITICAL_PERCENT, TOTAL_WARNING_PERCENT,
};

pub(crate) fn context_advice(snapshot: &ContextSnapshot) -> Vec<ContextAdvice> {
    let mut advice = Vec::new();
    let Some(limit) = snapshot.context_limit else {
        advice.push(ContextAdvice {
            category: "context_limit".to_string(),
            severity: "info".to_string(),
            message: "Context limit unknown; configure or refresh model metadata to show remaining space.".to_string(),
        });
        return advice;
    };
    if limit > 0 {
        let total_percent = snapshot.total.tokens as f64 / limit as f64 * 100.0;
        if total_percent >= TOTAL_CRITICAL_PERCENT {
            advice.push(ContextAdvice {
                category: "total".to_string(),
                severity: "critical".to_string(),
                message: "Context usage is above 90%; reduce message history or tool/skill surface before continuing.".to_string(),
            });
        } else if total_percent >= TOTAL_WARNING_PERCENT {
            advice.push(ContextAdvice {
                category: "total".to_string(),
                severity: "warning".to_string(),
                message: "Context usage is above 70%; consider reducing history, tools, or enabled skills.".to_string(),
            });
        }
    }
    for (key, message) in [
        (
            "system_tools",
            "System tools are a large share; switch to plan mode or reduce the tool surface when possible.",
        ),
        (
            "developer_prompt",
            "Developer prompt is a large share; prune enabled skills or narrow configured agent and skill paths.",
        ),
        (
            "history",
            "History dominates context; shorten the conversation or start a fresh session when practical.",
        ),
        (
            "turn_context",
            "Turn context is a large share; reduce selected skill bodies or prompt-scoped attachments.",
        ),
        (
            "current_prompt",
            "Current prompt is a large share; shorten prompt-scoped input where practical.",
        ),
    ] {
        if advice.len() >= ADVICE_LIMIT {
            break;
        }
        let Some(category) = snapshot.categories.get(key) else {
            continue;
        };
        let percent = category.tokens as f64 / limit as f64 * 100.0;
        if percent > CATEGORY_ADVICE_PERCENT {
            advice.push(ContextAdvice {
                category: key.to_string(),
                severity: "warning".to_string(),
                message: message.to_string(),
            });
        }
    }
    advice.truncate(ADVICE_LIMIT);
    advice
}

pub(crate) fn percent(tokens: u64, limit: Option<u64>) -> Option<f64> {
    let limit = limit?;
    (limit > 0).then(|| tokens as f64 / limit as f64 * 100.0)
}

pub(crate) fn format_token_count(tokens: u64, estimated: bool) -> String {
    format!("{} tokens", format_compact_count(tokens, estimated))
}

pub(crate) fn format_compact_count(tokens: u64, estimated: bool) -> String {
    let prefix = if estimated { "~" } else { "" };
    if tokens < 1_000 {
        format!("{prefix}{tokens}")
    } else if tokens < 1_000_000 {
        let value = tokens as f64 / 1_000.0;
        format!("{prefix}{value:.1}k")
    } else {
        let value = tokens as f64 / 1_000_000.0;
        format!("{prefix}{value:.1}M")
    }
}

pub(crate) fn context_bar(snapshot: &ContextSnapshot, requested_width: usize) -> Option<String> {
    let limit = snapshot.context_limit?;
    let bar_cells = normalize_context_bar_width(requested_width);
    let order = [
        ("base_policy", 'B'),
        ("developer_prompt", 'D'),
        ("project_context", 'P'),
        ("history", 'H'),
        ("turn_context", 'C'),
        ("current_prompt", 'U'),
        ("system_tools", 'T'),
        ("free_space", '.'),
    ];
    let total = order
        .iter()
        .map(|(key, _)| {
            if *key == "free_space" {
                limit.saturating_sub(snapshot.total.estimated_tokens)
            } else {
                snapshot
                    .categories
                    .get(*key)
                    .map(|category| category.tokens)
                    .unwrap_or(0)
            }
        })
        .sum::<u64>();
    if total == 0 {
        return None;
    }
    let mut cells = String::new();
    let mut used = 0usize;
    for (index, (key, marker)) in order.iter().enumerate() {
        let tokens = if *key == "free_space" {
            limit.saturating_sub(snapshot.total.estimated_tokens)
        } else {
            snapshot
                .categories
                .get(*key)
                .map(|category| category.tokens)
                .unwrap_or(0)
        };
        let remaining = bar_cells.saturating_sub(used);
        let width = if index + 1 == order.len() {
            remaining
        } else {
            ((tokens as f64 / total as f64) * bar_cells as f64).round() as usize
        }
        .min(remaining);
        cells.extend(std::iter::repeat_n(*marker, width));
        used = used.saturating_add(width);
    }
    while cells.len() < bar_cells {
        cells.push('.');
    }
    Some(format!("[{cells}]"))
}

pub fn normalize_context_bar_width(requested_width: usize) -> usize {
    let clamped = requested_width.clamp(CONTEXT_BAR_MIN_CELLS, CONTEXT_BAR_MAX_CELLS);
    (clamped / 5 * 5).max(CONTEXT_BAR_MIN_CELLS)
}

pub(crate) fn scope_label(scope: ContextScope) -> &'static str {
    match scope {
        ContextScope::LastProviderRequest => "last provider request",
        ContextScope::SessionEstimate => "session estimate",
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::super::{BTreeMap, OpenAiChatTokenCount, snapshot::*};
    use psychevo_ai::{OpenAiChatRoleTokenCount, OpenAiChatSkillTokenCount};

    fn count(system: u64, tools: u64, skills: u64, messages: u64) -> OpenAiChatTokenCount {
        let mut role_counts = BTreeMap::new();
        role_counts.insert(
            "user".to_string(),
            OpenAiChatRoleTokenCount {
                count: 2,
                tokens: messages,
            },
        );
        OpenAiChatTokenCount {
            encoding: "o200k_base".to_string(),
            encoding_source: "fallback".to_string(),
            encoding_fallback: true,
            base_policy_tokens: system,
            developer_prompt_tokens: skills,
            project_context_tokens: 0,
            history_tokens: messages,
            turn_context_tokens: 0,
            current_prompt_tokens: 0,
            system_prompt_tokens: system + skills,
            system_tools_tokens: tools,
            skills_tokens: 0,
            messages_tokens: messages,
            total_estimated_tokens: system + tools + skills + messages,
            tool_count: 4,
            role_counts,
            project_instruction_context_tokens: 0,
            project_instruction_context_count: 0,
            selected_skill_context_tokens: 0,
            selected_skill_context_count: 0,
            skill_names: vec!["alpha".to_string(), "beta".to_string()],
            skill_entries: vec![
                OpenAiChatSkillTokenCount {
                    name: "beta".to_string(),
                    tokens: 8,
                },
                OpenAiChatSkillTokenCount {
                    name: "alpha".to_string(),
                    tokens: 13,
                },
            ],
        }
    }

    #[test]
    fn snapshot_categories_use_estimates_and_provider_usage_overrides_headline() {
        let mut snapshot = snapshot_from_count(
            ContextScope::LastProviderRequest,
            Some("session".to_string()),
            "openai".to_string(),
            "gpt-4o".to_string(),
            Some("default".to_string()),
            Some(200),
            count(10, 20, 5, 45),
        );

        assert_eq!(snapshot.total.tokens, 80);
        assert!(snapshot.total.estimated);
        assert_eq!(snapshot.categories["system_tools"].tokens, 20);

        snapshot.apply_provider_total(
            crate::accounting::effective_usage_total(Some(
                &serde_json::json!({ "total_tokens": 90 }),
            )),
            None,
        );

        assert_eq!(snapshot.total.tokens, 90);
        assert!(!snapshot.total.estimated);
        assert_eq!(snapshot.total.source, "reported");
        assert_eq!(snapshot.categories["free_space"].tokens, 110);
        assert_eq!(
            snapshot.categories["history"].details["roles"]["user"]["count"],
            2
        );
    }

    #[test]
    fn snapshot_text_reports_project_context_bucket() {
        let mut count = count(10, 20, 5, 45);
        count.project_instruction_context_count = 2;
        count.project_instruction_context_tokens = 17;
        count.project_context_tokens = 17;
        let snapshot = snapshot_from_count(
            ContextScope::LastProviderRequest,
            Some("session".to_string()),
            "openai".to_string(),
            "gpt-4o".to_string(),
            Some("default".to_string()),
            Some(200),
            count,
        );

        let text = format_context_snapshot_text(&snapshot, false);

        assert!(text.contains("project_context: ~17 tokens"));
        assert!(text.contains("project_context: 2 input msgs"));
        assert!(!text.contains("\nmessages:"));
    }

    #[test]
    fn context_text_uses_compact_layout_and_skill_entry_order() {
        let mut snapshot = snapshot_from_count(
            ContextScope::LastProviderRequest,
            Some("session".to_string()),
            "openai".to_string(),
            "gpt-4o".to_string(),
            Some("default".to_string()),
            Some(1_000_000),
            count(350, 674, 341, 31_000),
        );
        snapshot.apply_provider_total(
            crate::accounting::effective_usage_total(Some(
                &serde_json::json!({ "total_tokens": 34_000 }),
            )),
            None,
        );

        let text = format_context_snapshot_text_with_options(
            &snapshot,
            ContextFormatOptions {
                heading: false,
                bar_width: Some(55),
            },
        );

        assert!(text.starts_with("["));
        assert!(text.contains(
            "\nB base  D developer  P project  H history  C turn  U prompt  T tools  . free\n\n"
        ));
        assert!(text.contains("tokens: 34.0k/1.0M (3.4%)\n"));
        let token_line = text
            .lines()
            .find(|line| line.starts_with("tokens:"))
            .expect("token line");
        assert_eq!(
            format_context_total_value(&snapshot),
            token_line.strip_prefix("tokens: ").expect("token value")
        );
        assert_eq!(
            format_context_total_value_parts(34_000, false, Some(1_000_000), Some(3.4)),
            "34.0k/1.0M (3.4%)"
        );
        assert!(!token_line.contains("provider"));
        assert!(text.contains("\n  alpha: ~13 tokens\n  beta: ~8 tokens\n"));
        assert!(text.contains("input_history: ~31.0k tokens"));
        assert!(!text.contains("\nmessages:"));
        assert!(text.contains("user: 2 input msgs, ~31.0k tokens"));
        assert!(text.contains("free_space: 966.0k tokens (96.6%)\n\nscope: last provider request\nmodel: openai/gpt-4o"));

        let value = serde_json::to_value(&snapshot).expect("snapshot json");
        assert!(value["categories"].get("history").is_some());
        assert!(value["categories"].get("input_messages").is_none());
    }

    #[test]
    fn context_text_uses_singular_input_msg_count() {
        assert_eq!(input_message_unit(1), "input msg");
        assert_eq!(input_message_unit(2), "input msgs");
    }

    #[test]
    fn estimated_context_text_marks_only_estimated_headline() {
        let snapshot = snapshot_from_count(
            ContextScope::SessionEstimate,
            Some("session".to_string()),
            "mock".to_string(),
            "model".to_string(),
            None,
            Some(1_000_000),
            count(1_000, 2_000, 3_000, 28_000),
        );
        let text = format_context_snapshot_text(&snapshot, false);

        assert!(text.contains("tokens: ~34.0k/1.0M (3.4%) estimated"));
    }

    #[test]
    fn unknown_context_limit_omits_free_space_and_reports_metadata_advice() {
        let snapshot = snapshot_from_count(
            ContextScope::SessionEstimate,
            Some("session".to_string()),
            "mock".to_string(),
            "model".to_string(),
            None,
            None,
            count(1, 2, 3, 4),
        );

        assert!(!snapshot.categories.contains_key("free_space"));
        assert_eq!(snapshot.total.percent, None);
        assert_eq!(snapshot.advice[0].category, "context_limit");

        let text = format_context_snapshot_text(&snapshot, false);
        let token_line = text
            .lines()
            .find(|line| line.starts_with("tokens:"))
            .expect("token line");
        assert_eq!(
            format_context_total_value(&snapshot),
            token_line.strip_prefix("tokens: ").expect("token value")
        );
    }

    #[test]
    fn advice_is_thresholded_and_bounded() {
        let snapshot = snapshot_from_count(
            ContextScope::LastProviderRequest,
            None,
            "mock".to_string(),
            "model".to_string(),
            Some("default".to_string()),
            Some(100),
            count(5, 30, 25, 35),
        );

        assert_eq!(snapshot.advice.len(), 3);
        assert_eq!(snapshot.advice[0].category, "total");
        assert!(
            snapshot
                .advice
                .iter()
                .any(|advice| advice.category == "system_tools")
        );
        assert!(
            snapshot
                .advice
                .iter()
                .any(|advice| advice.category == "developer_prompt")
        );
    }

    #[test]
    fn recorder_publishes_latest_completed_request() {
        let recorder = ContextRecorder::default();
        {
            let mut state = recorder.state.lock().expect("state");
            state.latest_started_sequence = 2;
        }
        let old = snapshot_from_count(
            ContextScope::LastProviderRequest,
            None,
            "mock".to_string(),
            "old".to_string(),
            None,
            Some(100),
            count(1, 1, 1, 1),
        );
        let latest = snapshot_from_count(
            ContextScope::LastProviderRequest,
            None,
            "mock".to_string(),
            "latest".to_string(),
            None,
            Some(100),
            count(2, 2, 2, 2),
        );

        recorder.finish_count(1, old);
        assert!(recorder.latest_snapshot().is_none());

        recorder.finish_count(2, latest);
        assert!(recorder.latest_snapshot().is_none());
        recorder.record_provider_usage(None);
        assert_eq!(
            recorder.latest_snapshot().expect("snapshot").model,
            "latest"
        );
    }
}
