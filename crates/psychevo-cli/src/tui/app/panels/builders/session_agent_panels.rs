impl TuiApp {
    pub(crate) fn session_selection_panel(
        &self,
        view: SessionListView,
    ) -> Result<BottomSelectionPanel> {
        let current_session = self.current_session.as_deref();
        let recent_since_ms = wall_now_ms().saturating_sub(7 * 86_400_000);
        let mut groups: BTreeMap<String, Vec<TuiSessionDisplaySummary>> = BTreeMap::new();
        for session in self.tui_sessions(view)? {
            groups
                .entry(session.summary.cwd.clone())
                .or_default()
                .push(session);
        }
        let mut rows = Vec::new();
        for (cwd, mut sessions) in groups {
            sessions.sort_by(|left, right| {
                right
                    .summary
                    .updated_at_ms
                    .cmp(&left.summary.updated_at_ms)
                    .then_with(|| left.summary.id.cmp(&right.summary.id))
            });
            let limit = self
                .session_browser_limits
                .get(&cwd)
                .copied()
                .unwrap_or(20);
            let mut visible_count = 0usize;
            let mut hidden_count = 0usize;
            let expanded = limit > 20;
            let project_label = sessions
                .first()
                .map(|session| session.project_label.clone())
                .unwrap_or_else(|| session_project_label(&cwd));
            for session in sessions {
                let is_current = current_session.is_some_and(|id| id == session.summary.id);
                let in_recent_window = session.summary.updated_at_ms >= recent_since_ms;
                if is_current {
                    rows.push(tui_session_selection_row(session, current_session));
                    continue;
                }
                let show_normal = visible_count < limit && (expanded || in_recent_window);
                if show_normal {
                    visible_count = visible_count.saturating_add(1);
                    rows.push(tui_session_selection_row(session, current_session));
                } else {
                    hidden_count = hidden_count.saturating_add(1);
                }
            }
            if hidden_count > 0 && view == SessionListView::Active {
                rows.push(BottomSelectionRow {
                    label: "Load older sessions".to_string(),
                    description: Some("Show 20 more sessions in this workspace".to_string()),
                    detail: Some(format!("{hidden_count} hidden")),
                    group: Some(project_label),
                    search_text: format!("load older sessions {cwd}"),
                    is_current: false,
                    is_default: false,
                    style: BottomRowStyle::Action,
                    footer: Some("Enter load  Esc close  Type search".to_string()),
                    value: BottomSelectionValue::LoadOlderSessions(cwd),
                });
            }
        }
        let mut panel = BottomSelectionPanel::new_sessions(view, rows);
        if let Some(session_id) = current_session {
            panel.select_value_key(&format!("session:{session_id}"));
        }
        Ok(panel)
    }

    pub(crate) fn agent_panel(&self) -> AgentPanel {
        AgentPanel::new(self.agent_running_panel(), self.agent_available_panel())
    }

    pub(crate) fn agent_running_panel(&self) -> BottomSelectionPanel {
        let paused = agent_spawn_paused();
        let mut rows = vec![BottomSelectionRow {
            label: if paused {
                "Resume spawning".to_string()
            } else {
                "Pause spawning".to_string()
            },
            description: Some(if paused {
                "New Agent calls are blocked; running children continue".to_string()
            } else {
                "New Agent calls are allowed".to_string()
            }),
            detail: Some(format!(
                "depth cap {}  concurrency cap {}",
                MAX_AGENT_SPAWN_DEPTH_CAP, MAX_TEAM_PARALLEL_AGENTS_CAP
            )),
            group: Some("Controls".to_string()),
            search_text: "pause resume spawning depth cap concurrency".to_string(),
            is_current: paused,
            is_default: false,
            style: BottomRowStyle::Action,
            footer: Some("Enter toggle  P toggle  Esc close  Tab available".to_string()),
            value: BottomSelectionValue::AgentSpawningToggle,
        }];
        let mut live_count = 0usize;
        if let Some(parent) = self.current_session.as_deref() {
            let store = self.state_runtime.store();
            let value = agent_status_value(Some(store), Some(parent), false);
            if let Some(agents) = value.get("agents").and_then(Value::as_array) {
                for agent in agents {
                    let status = agent
                        .get("status")
                        .and_then(Value::as_str)
                        .unwrap_or_default();
                    if !matches!(status, "pending_init" | "running") {
                        continue;
                    }
                    let child_session_id = agent
                        .get("child_session_id")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string();
                    if child_session_id.is_empty() {
                        continue;
                    }
                    live_count = live_count.saturating_add(1);
                    let id = agent
                        .get("id")
                        .and_then(Value::as_str)
                        .unwrap_or(child_session_id.as_str())
                        .to_string();
                    let name = agent
                        .get("agent_name")
                        .and_then(Value::as_str)
                        .unwrap_or("agent");
                    let task = agent
                        .get("task")
                        .and_then(Value::as_str)
                        .unwrap_or_default();
                    let task_name = agent.get("task_name").and_then(Value::as_str);
                    let team_name = agent.get("team_name").and_then(Value::as_str);
                    let mission_run_id = agent.get("mission_run_id").and_then(Value::as_str);
                    let team_member_id = agent.get("team_member_id").and_then(Value::as_str);
                    let labels = [
                        team_name.map(|value| format!("team {value}")),
                        team_member_id.map(|value| format!("member {value}")),
                        mission_run_id.map(|value| format!("mission {value}")),
                    ]
                    .into_iter()
                    .flatten()
                    .collect::<Vec<_>>();
                    rows.push(BottomSelectionRow {
                        label: team_member_id.unwrap_or(name).to_string(),
                        description: Some(truncate_chars(task, 80)),
                        detail: Some(
                            [
                                task_name.unwrap_or(status).to_string(),
                                labels.join("  "),
                            ]
                            .into_iter()
                            .filter(|value| !value.is_empty())
                            .collect::<Vec<_>>()
                            .join("  "),
                        ),
                        group: Some("Live child agents".to_string()),
                        search_text: format!(
                            "{id} {child_session_id} {name} {task} {status} {}",
                            labels.join(" ")
                        ),
                        is_current: self.current_session.as_deref()
                            == Some(child_session_id.as_str()),
                        is_default: false,
                        style: BottomRowStyle::Normal,
                        footer: Some(
                            "Enter open  S stop subtree  P pause/resume  Esc close  Tab available"
                                .to_string(),
                        ),
                        value: BottomSelectionValue::AgentRunning {
                            id,
                            child_session_id,
                        },
                    });
                }
            }
        }
        if live_count == 0 {
            rows.push(BottomSelectionRow {
                label: "No running subagents".to_string(),
                description: Some("Completed children stay reachable from Agent rows".to_string()),
                detail: None,
                group: Some("Live child agents".to_string()),
                search_text: "no running subagents completed reachable agent rows".to_string(),
                is_current: false,
                is_default: false,
                style: BottomRowStyle::Normal,
                footer: Some("P pause/resume  Esc close  Tab available".to_string()),
                value: BottomSelectionValue::AgentDiagnostic("no running subagents".to_string()),
            });
        }
        let mut panel = BottomSelectionPanel::new("Agents", "", "No running subagents", rows);
        panel.footer = if live_count == 0 {
            "P pause/resume  Esc close  Tab available".to_string()
        } else {
            "Enter open  S stop subtree  P pause/resume  Esc close  Tab available  Type search"
                .to_string()
        };
        panel
    }

    pub(crate) fn agent_available_panel(&self) -> BottomSelectionPanel {
        let Some(catalog) = self.current_agent_catalog() else {
            let mut panel = BottomSelectionPanel::new("Agents", "", "Agents disabled", Vec::new());
            panel.footer = "Esc close".to_string();
            return panel;
        };
        let mut rows = vec![
            BottomSelectionRow {
                label: "Default main agent".to_string(),
                description: Some("Use the normal session identity for future turns".to_string()),
                detail: self
                    .current_agent
                    .is_none()
                    .then(|| "Current main".to_string()),
                group: Some("Main session".to_string()),
                search_text: "default main agent normal session identity".to_string(),
                is_current: self.current_agent.is_none(),
                is_default: self.current_agent.is_none(),
                style: BottomRowStyle::Normal,
                footer: Some("Enter use default  Esc close  Tab running".to_string()),
                value: BottomSelectionValue::AgentMainDefault,
            },
            BottomSelectionRow {
                label: "Create agent".to_string(),
                description: Some("project .psychevo/agents".to_string()),
                detail: None,
                group: Some("Actions".to_string()),
                search_text: "create new agent project .psychevo".to_string(),
                is_current: false,
                is_default: false,
                style: BottomRowStyle::Action,
                footer: Some("Enter create  Esc close  Tab running".to_string()),
                value: BottomSelectionValue::AgentCreate,
            },
        ];
        rows.extend(
            catalog
                .agents
                .into_iter()
                .map(|agent| agent_definition_row(agent, false, self.current_agent.as_deref())),
        );
        rows.extend(
            catalog
                .shadowed_agents
                .into_iter()
                .map(|agent| agent_definition_row(agent, true, self.current_agent.as_deref())),
        );
        rows.extend(catalog.diagnostics.into_iter().map(agent_diagnostic_row));
        let mut panel = BottomSelectionPanel::new("Agents", "", "No agents available", rows);
        panel.footer = "Enter actions  Esc close  Tab running  Type search".to_string();
        panel
    }

    pub(crate) fn agent_action_panel(
        &self,
        name: String,
        source: AgentSource,
        path: Option<PathBuf>,
        entrypoints: BTreeSet<AgentEntrypoint>,
        shadowed: bool,
    ) -> BottomSelectionPanel {
        let mut rows = Vec::new();
        if !shadowed && entrypoints.contains(&AgentEntrypoint::Subagent) {
            rows.push(agent_action_row(
                &name,
                source,
                path.clone(),
                shadowed,
                AgentAction::UseAsMain,
            ));
        }
        if entrypoints.contains(&AgentEntrypoint::Subagent) {
            rows.push(agent_action_row(
                &name,
                source,
                path.clone(),
                shadowed,
                AgentAction::Run,
            ));
        }
        rows.push(agent_action_row(
            &name,
            source,
            path.clone(),
            shadowed,
            AgentAction::View,
        ));
        if agent_definition_editable(source, path.as_ref()) {
            rows.push(agent_action_row(
                &name,
                source,
                path.clone(),
                shadowed,
                AgentAction::Update,
            ));
            rows.push(agent_action_row(
                &name,
                source,
                path,
                shadowed,
                AgentAction::Delete,
            ));
        }
        BottomSelectionPanel::new_agent_actions(&name, rows)
    }

    pub(crate) fn stats_panel(&self) -> Result<BottomSelectionPanel> {
        let report = usage_stats(StatsOptions {
            state: self.state_runtime.clone(),
            cwd: self.cwd.clone(),
            all: false,
            days: None,
            limit: 8,
        })?;
        let totals = report.get("totals").unwrap_or(&Value::Null);
        let mut rows = Vec::new();
        if let Some(session_id) = self.current_session.as_ref() {
            let summary = match session_usage_summary(SessionUsageOptions {
                state: self.state_runtime.clone(),
                session_id: session_id.clone(),
            }) {
                Ok(summary) => Some(summary),
                Err(error) if is_missing_session_usage_error(&error, session_id) => None,
                Err(error) => return Err(error.into()),
            };
            if let Some(summary) = summary {
                rows.push(stats_row(
                    "session-current",
                    "Current session",
                    format!(
                        "{} messages  {} assistant",
                        summary.message_count, summary.assistant_message_count
                    ),
                    Some(format!(
                        "{} tokens  {}",
                        summary.reported_total_tokens,
                        format_nanodollars(summary.estimated_cost_nanodollars)
                    )),
                    Some("Current session".to_string()),
                ));
                rows.push(stats_row(
                    "session-breakdown",
                    "Session token breakdown",
                    format!(
                        "{} input  {} output  {} context",
                        summary.billable_input_tokens,
                        summary.billable_output_tokens,
                        summary.context_input_tokens
                    ),
                    Some(format!("{} reasoning", summary.reasoning_tokens)),
                    Some("Current session".to_string()),
                ));
                rows.push(stats_row(
                    "session-cache",
                    "Session cache and cost",
                    format!(
                        "{} cache read  {} cache write  {} hit",
                        summary.cache_read_tokens,
                        summary.cache_write_tokens,
                        format_cache_read_percent(summary.cache_read_percent)
                    ),
                    Some(format!("{} unknown pricing", summary.unknown_pricing_count)),
                    Some("Current session".to_string()),
                ));
            }
        }
        rows.extend([
            stats_row(
                "totals",
                "Totals",
                format!(
                    "{} sessions  {} messages",
                    json_i64(totals, "sessions"),
                    json_i64(totals, "messages")
                ),
                Some(format!(
                    "{} tokens  {}",
                    json_i64(totals, "reported_total_tokens"),
                    format_nanodollars(json_i64(totals, "estimated_cost_nanodollars"))
                )),
                None,
            ),
            stats_row(
                "breakdown",
                "Token breakdown",
                format!(
                    "{} input  {} output  {} context",
                    json_i64(totals, "billable_input_tokens"),
                    json_i64(totals, "billable_output_tokens"),
                    json_i64(totals, "context_input_tokens")
                ),
                Some(format!(
                    "{} reported total",
                    json_i64(totals, "reported_total_tokens")
                )),
                None,
            ),
            stats_row(
                "cache-reasoning",
                "Cache and reasoning",
                format!(
                    "{} reasoning  {} cache read  {} cache write",
                    json_i64(totals, "reasoning_tokens"),
                    json_i64(totals, "cache_read_tokens"),
                    json_i64(totals, "cache_write_tokens")
                ),
                None,
                None,
            ),
        ]);
        let unknown_priced_messages = json_i64(totals, "unknown_priced_messages");
        if unknown_priced_messages > 0 {
            rows.push(stats_row(
                "unknown-pricing",
                "Unknown pricing",
                format!("{unknown_priced_messages} assistant messages had usage without pricing"),
                Some(format!(
                    "estimated cost excludes {}",
                    pluralize_count(unknown_priced_messages, "message")
                )),
                None,
            ));
        }
        if let Some(models) = report.get("provider_models").and_then(Value::as_array) {
            for (index, model) in models.iter().enumerate() {
                rows.push(stats_row(
                    format!("model-{index}"),
                    format!(
                        "{}/{}",
                        model.get("provider").and_then(Value::as_str).unwrap_or("-"),
                        model.get("model").and_then(Value::as_str).unwrap_or("-")
                    ),
                    format!(
                        "{} messages  {} tokens",
                        json_i64(model, "messages"),
                        json_i64(model, "reported_total_tokens")
                    ),
                    Some(format_nanodollars(json_i64(
                        model,
                        "estimated_cost_nanodollars",
                    ))),
                    Some("Provider / model".to_string()),
                ));
            }
        }
        if let Some(tools) = report.get("top_tools").and_then(Value::as_array)
            && !tools.is_empty()
        {
            for (index, tool) in tools.iter().enumerate() {
                rows.push(stats_row(
                    format!("tool-{index}"),
                    tool.get("tool").and_then(Value::as_str).unwrap_or("-"),
                    pluralize_count(json_i64(tool, "calls"), "call"),
                    None,
                    Some("Top tools".to_string()),
                ));
            }
        }
        if let Some(sessions) = report.get("top_sessions").and_then(Value::as_array)
            && !sessions.is_empty()
        {
            for (index, session) in sessions.iter().enumerate() {
                let session_id = session
                    .get("session")
                    .and_then(Value::as_str)
                    .unwrap_or("-");
                let title = session
                    .get("title")
                    .and_then(Value::as_str)
                    .filter(|title| !title.trim().is_empty())
                    .unwrap_or_else(|| short_session(session_id));
                rows.push(stats_row(
                    format!("session-{index}"),
                    title,
                    format!(
                        "{}/{}  {} tokens",
                        session
                            .get("provider")
                            .and_then(Value::as_str)
                            .unwrap_or("-"),
                        session.get("model").and_then(Value::as_str).unwrap_or("-"),
                        json_i64(session, "reported_total_tokens")
                    ),
                    Some(format!(
                        "{}  {}",
                        format_nanodollars(json_i64(session, "estimated_cost_nanodollars")),
                        format_session_time(json_i64(session, "updated_at_ms"))
                    )),
                    Some("Top sessions".to_string()),
                ));
            }
        }
        Ok(BottomSelectionPanel::new("Usage", "", "No usage yet", rows))
    }

    pub(crate) fn toolsets_panel(&self) -> Result<BottomSelectionPanel> {
        let value = toolsets_value(&self.run_options(String::new()), ConfigScope::Effective)?;
        let mode_key = self.current_mode.as_str();
        let mode = &value["modes"][mode_key];
        let enabled_toolsets = mode["enabled_toolsets"]
            .as_array()
            .map(|items| string_values(items))
            .unwrap_or_else(|| {
                value["default_enabled_toolsets"]
                    .as_array()
                    .map(|items| string_values(items))
                    .unwrap_or_default()
            });
        let disabled_toolsets = mode["disabled_toolsets"]
            .as_array()
            .map(|items| string_values(items))
            .unwrap_or_default();
        let mut rows = Vec::new();
        for row in value["toolsets"].as_array().cloned().unwrap_or_default() {
            let Some(name) = row["name"].as_str() else {
                continue;
            };
            let enabled = enabled_toolsets.iter().any(|item| item == name)
                && !disabled_toolsets.iter().any(|item| item == name);
            let source = row["source"].as_str().unwrap_or("-");
            let tools = json_array_strings(&row["tools"]).join(", ");
            let includes = json_array_strings(&row["includes"]).join(", ");
            let description = row["description"].as_str().unwrap_or("").to_string();
            let detail = if includes.is_empty() {
                format!("{source}  tools: {tools}")
            } else {
                format!("{source}  includes: {includes}  tools: {tools}")
            };
            rows.push(BottomSelectionRow {
                label: name.to_string(),
                description: Some(description.clone()),
                detail: Some(detail.clone()),
                group: Some(if enabled { "Enabled" } else { "Disabled" }.to_string()),
                search_text: format!("{name} {description} {detail}"),
                is_current: enabled,
                is_default: false,
                style: BottomRowStyle::Normal,
                footer: Some("Enter toggle".to_string()),
                value: BottomSelectionValue::Toolset {
                    name: name.to_string(),
                    enabled,
                },
            });
        }
        let mut panel = BottomSelectionPanel::new(
            &format!("Toolsets ({mode_key})"),
            "",
            "No toolsets configured",
            rows,
        );
        panel.footer = "Enter toggle  Esc close  Type search".to_string();
        Ok(panel)
    }

}

fn tui_session_selection_row(
    session: TuiSessionDisplaySummary,
    current_session: Option<&str>,
) -> BottomSelectionRow {
    let summary = session.summary;
    let title = summary
        .title
        .clone()
        .filter(|title| !title.trim().is_empty())
        .unwrap_or_else(|| short_session(&summary.id).to_string());
    let provider_model = format!("{}/{}", summary.provider, summary.model);
    let description = Some(format!(
        "{}  {}  messages={}",
        session.project_display_path, provider_model, session.visible_message_count
    ));
    let search_text = format!(
        "{} {} {} {} {} {}",
        summary.id,
        title,
        session.project_label,
        session.project_display_path,
        summary.provider,
        summary.model
    );
    BottomSelectionRow {
        label: title,
        description,
        detail: Some(format!(
            "{} {}",
            format_session_date(summary.updated_at_ms),
            format_session_time(summary.updated_at_ms)
        )),
        group: Some(session.project_label),
        search_text,
        is_current: current_session.is_some_and(|id| id == summary.id),
        is_default: false,
        style: BottomRowStyle::Normal,
        footer: None,
        value: BottomSelectionValue::Session(summary.id),
    }
}
