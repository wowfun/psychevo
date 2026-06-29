#[allow(unused_imports)]
pub(crate) use super::*;

impl<'a> FullscreenUi<'a> {
    pub(crate) fn reconcile_history_agent_rows(
        &mut self,
        edges: &[AgentEdgeRecord],
        catalog: Option<&AgentCatalog>,
    ) {
        if edges.is_empty() {
            return;
        }
        let mut used_edges = std::collections::BTreeSet::<usize>::new();
        for row in &mut self.transcript {
            if row.tool_name.as_deref() != Some("spawn_agent") || row.agent_target.is_some() {
                continue;
            }
            let row_was_active = active_tool_row(row);
            let Some((edge_index, edge)) = matching_agent_edge(row, edges, &used_edges) else {
                continue;
            };
            used_edges.insert(edge_index);
            row.agent_target = Some(edge.child_session_id.clone());
            if let Some(title) = agent_edge_title(edge, catalog) {
                row.title = title;
            }
            if row_was_active {
                row.text = agent_child_status_text(
                    "Running",
                    row.agent_child_tool_uses,
                    row.agent_child_latest_tokens,
                );
                row.full_text = None;
            }
        }
    }

    pub(crate) fn scroll_to_bottom(&mut self) {
        self.scroll = self.max_transcript_scroll();
        self.auto_follow_transcript = true;
        self.pending_scroll_to_bottom = true;
    }

    pub(crate) fn status_running_elapsed(&self, current_session: Option<&str>) -> Option<Duration> {
        if !self.status_has_running(current_session) {
            return None;
        }
        #[cfg(test)]
        if let Some(elapsed) = self.running_elapsed_override {
            return Some(elapsed);
        }
        if let Some(session_id) = current_session
            && let Some(activity) = self.foreign_gateway_activities.get(session_id)
        {
            return Some(activity.started.elapsed());
        }
        self.visible_turn_started
            .or(self.turn_started)
            .map(|started| started.elapsed())
            .or(Some(Duration::default()))
    }

    pub(crate) fn bottom_panel_activity_elapsed(&self) -> Duration {
        #[cfg(test)]
        if let Some(elapsed) = self.running_elapsed_override {
            return elapsed;
        }
        self.motion_started.elapsed()
    }

    pub(crate) fn background_running_session_ids(
        &self,
        current_session: Option<&str>,
    ) -> BTreeSet<String> {
        let mut sessions = BTreeSet::new();
        if let Some(running) = &self.running
            && let Some(session_id) = running.session_id.as_deref()
            && Some(session_id) != current_session
        {
            sessions.insert(session_id.to_string());
        }
        for agent in &self.auxiliary_agent_tasks {
            if !agent.visible_live {
                continue;
            }
            if let Some(session_id) = agent.session_id.as_deref()
                && Some(session_id) != current_session
            {
                sessions.insert(session_id.to_string());
            }
            if let Some(session_id) = agent.child_session_id.as_deref()
                && Some(session_id) != current_session
            {
                sessions.insert(session_id.to_string());
            }
        }
        for shell in &self.auxiliary_shell_tasks {
            if let Some(session_id) = shell.session_id.as_deref()
                && Some(session_id) != current_session
            {
                sessions.insert(session_id.to_string());
            }
        }
        for session_id in self.foreign_gateway_activities.keys() {
            if Some(session_id.as_str()) != current_session {
                sessions.insert(session_id.clone());
            }
        }
        sessions
    }

    pub(crate) fn status_has_running(&self, current_session: Option<&str>) -> bool {
        self.local_status_has_running(current_session)
            || self.foreign_gateway_activity_matches_current_session(current_session)
    }

    pub(crate) fn local_status_has_running(&self, current_session: Option<&str>) -> bool {
        self.running.as_ref().is_some_and(|running| {
            current_session_matches(running.session_id.as_deref(), current_session)
        }) || self.auxiliary_agent_matches_current_session(current_session)
            || self.auxiliary_shell_matches_current_session(current_session)
    }

    pub(crate) fn observe_foreign_gateway_activity(
        &mut self,
        session_id: &str,
        activity: &GatewayActivity,
    ) {
        if !activity.running {
            self.foreign_gateway_activities.remove(session_id);
            return;
        }
        self.observe_foreign_gateway_activity_values(
            session_id,
            activity.active_turn_id.clone(),
            activity.started_at_ms,
        );
    }

    pub(crate) fn observe_foreign_gateway_activity_values(
        &mut self,
        session_id: &str,
        active_turn_id: Option<String>,
        started_at_ms: Option<i64>,
    ) {
        let started = started_at_ms
            .and_then(instant_from_wall_timestamp_ms)
            .or_else(|| {
                self.foreign_gateway_activities
                    .get(session_id)
                    .map(|activity| activity.started)
            })
            .unwrap_or_else(Instant::now);
        self.foreign_gateway_activities.insert(
            session_id.to_string(),
            ForeignGatewayActivity {
                active_turn_id,
                started,
            },
        );
    }

    pub(crate) fn clear_foreign_gateway_activity(&mut self, session_id: &str) {
        self.foreign_gateway_activities.remove(session_id);
    }

    pub(crate) fn foreign_gateway_activity_matches_current_session(
        &self,
        current_session: Option<&str>,
    ) -> bool {
        let Some(session_id) = current_session else {
            return false;
        };
        self.foreign_gateway_activities.contains_key(session_id)
    }

    pub(crate) fn foreign_gateway_activity_started(&self, session_id: &str) -> Option<Instant> {
        self.foreign_gateway_activities
            .get(session_id)
            .map(|activity| activity.started)
    }

    pub(crate) fn foreign_gateway_activity_turn_id(&self, session_id: &str) -> Option<String> {
        self.foreign_gateway_activities
            .get(session_id)
            .and_then(|activity| activity.active_turn_id.clone())
    }

    pub(crate) fn mark_gateway_live_event_applied(&mut self, seq: i64) -> bool {
        self.applied_gateway_live_event_seqs.insert(seq)
    }

    pub(crate) fn auxiliary_agent_matches_current_session(
        &self,
        current_session: Option<&str>,
    ) -> bool {
        let Some(session_id) = current_session else {
            return false;
        };
        self.auxiliary_agent_tasks
            .iter()
            .any(|agent| auxiliary_agent_live_for_session(agent, session_id))
    }

    pub(crate) fn auxiliary_shell_matches_current_session(
        &self,
        current_session: Option<&str>,
    ) -> bool {
        let Some(session_id) = current_session else {
            return false;
        };
        self.auxiliary_shell_tasks
            .iter()
            .any(|shell| shell.session_id.as_deref() == Some(session_id))
    }

    pub(crate) fn request_interrupt(&mut self, current_session: Option<&str>) -> bool {
        let mut interrupted = false;
        if let Some(running) = &self.running
            && current_session_matches(running.session_id.as_deref(), current_session)
        {
            running.control.abort();
            interrupted = true;
        }
        for agent in &self.auxiliary_agent_tasks {
            if current_session
                .is_some_and(|session_id| auxiliary_agent_live_for_session(agent, session_id))
            {
                agent.control.abort();
                interrupted = true;
            }
        }
        for shell in &self.auxiliary_shell_tasks {
            if current_session
                .is_some_and(|session_id| shell.session_id.as_deref() == Some(session_id))
            {
                shell.control.abort();
                interrupted = true;
            }
        }
        if !interrupted {
            return false;
        }
        for shell in &self.auxiliary_shell_tasks {
            shell.control.abort();
        }
        self.pending_auxiliary_shell_commands.clear();
        self.interrupt_requested = true;
        true
    }
}
