use serde_json::Value;

#[cfg(test)]
use super::FrameworkTurnTerminalOutcome;
use super::{FrameworkTurnTerminalStatus, Thread};
use crate::Result;
use crate::state::{GatewayTurnTerminalRecord, SessionCompactionRecord};

/// Compaction checkpoints and non-success terminal facts positioned around a
/// Thread's message sequence.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ThreadStructuralHistory {
    pub compactions: Vec<ThreadCompaction>,
    pub turn_terminals: Vec<ThreadTurnTerminal>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ThreadCompaction {
    pub checkpoint_id: i64,
    pub created_at_ms: i64,
    pub reason: String,
    pub summary: String,
    pub first_kept_session_seq: i64,
    pub boundary_session_seq: i64,
    pub tokens_before: Option<u64>,
    pub tokens_after: Option<u64>,
    pub summary_provider: String,
    pub summary_model: String,
    pub instructions: Option<String>,
    pub metadata: Option<Value>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThreadTurnTerminalStatus {
    Failed,
    Interrupted,
}

impl ThreadTurnTerminalStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Failed => "failed",
            Self::Interrupted => "interrupted",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ThreadTurnTerminal {
    pub turn_id: String,
    pub status: ThreadTurnTerminalStatus,
    pub outcome: Option<String>,
    pub error_message: Option<String>,
    pub first_committed_session_seq: Option<i64>,
    pub boundary_session_seq: i64,
    pub completed_at_ms: i64,
    pub metadata: Option<Value>,
}

impl Thread {
    /// Read one checkpoint by its indexed identity and enforce Thread
    /// ownership without scanning the complete structural history.
    pub async fn compaction(&self, checkpoint_id: i64) -> Result<Option<ThreadCompaction>> {
        self.client.ensure_open()?;
        let Some(record) = self
            .client
            .inner
            .state
            .session_compaction(checkpoint_id)
            .await?
        else {
            return Ok(None);
        };
        if record.session_id != self.id {
            return Err(crate::Error::structured(
                "Compaction checkpoint does not belong to this Thread.",
                serde_json::json!({
                    "kind": "compaction_thread_mismatch",
                    "threadId": self.id,
                    "checkpointId": checkpoint_id,
                }),
            ));
        }
        Ok(Some(ThreadCompaction::from(record)))
    }

    /// Read all structural history still valid for this Thread.
    pub async fn structural_history(&self) -> Result<ThreadStructuralHistory> {
        let state = &self.client.inner.state;
        let (compactions, terminals) = tokio::try_join!(
            state.list_valid_session_compactions(&self.id),
            state.list_valid_gateway_turn_terminals_for_thread(&self.id),
        )?;
        Ok(structural_history(compactions, terminals))
    }

    /// Read structural facts whose message boundary is in `[lower, before)`.
    ///
    /// The limit is applied independently to compactions and Turn terminals.
    /// Zero returns an empty history without acquiring storage.
    pub async fn structural_history_window(
        &self,
        lower_session_seq: i64,
        before_session_seq: Option<i64>,
        before_structural_entry: Option<(i64, &str)>,
        limit: usize,
    ) -> Result<(ThreadStructuralHistory, bool)> {
        if limit == 0 {
            return Ok((ThreadStructuralHistory::default(), false));
        }
        let state = &self.client.inner.state;
        let query_limit = limit.saturating_add(1);
        let (mut compactions, mut terminals) = tokio::try_join!(
            state.list_valid_session_compactions_between(
                &self.id,
                lower_session_seq,
                before_session_seq,
                before_structural_entry,
                query_limit,
            ),
            state.list_valid_gateway_turn_terminals_for_thread_window(
                &self.id,
                lower_session_seq,
                before_session_seq,
                before_structural_entry,
                query_limit,
            ),
        )?;
        let has_older = compactions.len() > limit || terminals.len() > limit;
        if compactions.len() > limit {
            compactions.remove(0);
        }
        if terminals.len() > limit {
            terminals.remove(0);
        }
        Ok((structural_history(compactions, terminals), has_older))
    }
}

fn structural_history(
    compactions: Vec<SessionCompactionRecord>,
    terminals: Vec<GatewayTurnTerminalRecord>,
) -> ThreadStructuralHistory {
    ThreadStructuralHistory {
        compactions: compactions
            .into_iter()
            .map(ThreadCompaction::from)
            .collect(),
        turn_terminals: terminals
            .into_iter()
            .filter_map(ThreadTurnTerminal::from_record)
            .collect(),
    }
}

impl From<SessionCompactionRecord> for ThreadCompaction {
    fn from(record: SessionCompactionRecord) -> Self {
        Self {
            checkpoint_id: record.id,
            created_at_ms: record.created_at_ms,
            reason: record.reason,
            summary: record.summary_text,
            first_kept_session_seq: record.first_kept_session_seq,
            boundary_session_seq: record.created_after_session_seq,
            tokens_before: record.tokens_before,
            tokens_after: record.tokens_after,
            summary_provider: record.summary_provider,
            summary_model: record.summary_model,
            instructions: record.instructions,
            metadata: record.metadata,
        }
    }
}

impl ThreadTurnTerminal {
    fn from_record(record: GatewayTurnTerminalRecord) -> Option<Self> {
        let status = match record.status {
            FrameworkTurnTerminalStatus::Failed => ThreadTurnTerminalStatus::Failed,
            FrameworkTurnTerminalStatus::Interrupted => ThreadTurnTerminalStatus::Interrupted,
            FrameworkTurnTerminalStatus::Completed => return None,
        };
        let first_committed_session_seq = terminal_metadata_sequence(
            record.metadata.as_ref(),
            "firstCommittedSeq",
            "first_committed_seq",
        );
        Some(Self {
            turn_id: record.turn_id,
            status,
            outcome: record.outcome.map(|outcome| outcome.as_str().to_string()),
            error_message: record.error_message,
            first_committed_session_seq,
            boundary_session_seq: record.boundary_session_seq,
            completed_at_ms: record.completed_at_ms,
            metadata: record.metadata,
        })
    }
}

fn terminal_metadata_sequence(metadata: Option<&Value>, camel: &str, snake: &str) -> Option<i64> {
    metadata
        .and_then(|metadata| metadata.get(camel).or_else(|| metadata.get(snake)))
        .and_then(Value::as_i64)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::application::{Application, StartThreadRequest};
    use crate::state::{
        GatewayTurnTerminalInput, SessionCompactionInput, SessionCompactionRecord,
        SessionRevertState, StateRuntime,
    };

    async fn application_with_thread() -> (tempfile::TempDir, Application, Thread) {
        let temp = tempfile::tempdir().expect("tempdir");
        let cwd = temp.path().join("workspace");
        std::fs::create_dir_all(&cwd).expect("workspace");
        let application = Application::builder()
            .home(temp.path())
            .database_path(":memory:")
            .build()
            .await
            .expect("application");
        let thread = application
            .client()
            .start_thread(StartThreadRequest::new(cwd))
            .await
            .expect("thread");
        (temp, application, thread)
    }

    async fn append_compaction(
        state: &StateRuntime,
        thread_id: &str,
        boundary_session_seq: i64,
        metadata: Value,
    ) -> SessionCompactionRecord {
        state
            .append_session_compaction(SessionCompactionInput {
                session_id: thread_id.to_string(),
                reason: format!("reason-{boundary_session_seq}"),
                summary_text: format!("summary-{boundary_session_seq}"),
                first_kept_session_seq: boundary_session_seq + 1,
                created_after_session_seq: boundary_session_seq,
                tokens_before: Some(boundary_session_seq as u64 + 100),
                tokens_after: Some(boundary_session_seq as u64 + 10),
                summary_provider: "test-provider".to_string(),
                summary_model: "test-model".to_string(),
                instructions: Some(format!("instructions-{boundary_session_seq}")),
                metadata: Some(metadata),
            })
            .await
            .expect("compaction")
    }

    async fn append_terminal(
        state: &StateRuntime,
        thread_id: &str,
        turn_id: &str,
        status: FrameworkTurnTerminalStatus,
        boundary_session_seq: i64,
        completed_at_ms: i64,
        metadata: Value,
    ) {
        state
            .upsert_gateway_turn_terminal(GatewayTurnTerminalInput {
                turn_id,
                thread_id,
                status,
                outcome: Some(match status {
                    FrameworkTurnTerminalStatus::Completed => FrameworkTurnTerminalOutcome::Normal,
                    FrameworkTurnTerminalStatus::Failed => FrameworkTurnTerminalOutcome::Failed,
                    FrameworkTurnTerminalStatus::Interrupted => {
                        FrameworkTurnTerminalOutcome::Aborted
                    }
                }),
                error_message: Some("terminal error"),
                started_at_ms: Some(completed_at_ms - 1),
                completed_at_ms,
                boundary_session_seq: Some(boundary_session_seq),
                metadata: Some(metadata),
            })
            .await
            .expect("terminal");
    }

    #[tokio::test]
    async fn structural_history_maps_complete_compaction_and_terminal_metadata() {
        let (_temp, application, thread) = application_with_thread().await;
        let state = application.inner.state.clone();
        let compaction_metadata = json!({
            "checkpoint": {"nested": [1, {"kept": true}]},
            "projection_hint": "complete"
        });
        let raw_compaction =
            append_compaction(&state, thread.id(), 14, compaction_metadata.clone()).await;
        let terminal_metadata = json!({
            "firstCommittedSeq": 12,
            "first_committed_seq": 112,
            "lastCommittedSeq": 14,
            "last_committed_seq": 114,
            "terminal": {"nested": ["kept", 3]}
        });
        append_terminal(
            &state,
            thread.id(),
            "turn-failed",
            FrameworkTurnTerminalStatus::Failed,
            14,
            200,
            terminal_metadata.clone(),
        )
        .await;
        append_terminal(
            &state,
            thread.id(),
            "turn-completed",
            FrameworkTurnTerminalStatus::Completed,
            15,
            201,
            json!({"lastCommittedSeq": 15}),
        )
        .await;

        let history = thread
            .structural_history()
            .await
            .expect("structural history");

        assert_eq!(
            history,
            ThreadStructuralHistory {
                compactions: vec![ThreadCompaction {
                    checkpoint_id: raw_compaction.id,
                    created_at_ms: raw_compaction.created_at_ms,
                    reason: "reason-14".to_string(),
                    summary: "summary-14".to_string(),
                    first_kept_session_seq: 15,
                    boundary_session_seq: 14,
                    tokens_before: Some(114),
                    tokens_after: Some(24),
                    summary_provider: "test-provider".to_string(),
                    summary_model: "test-model".to_string(),
                    instructions: Some("instructions-14".to_string()),
                    metadata: Some(compaction_metadata),
                }],
                turn_terminals: vec![ThreadTurnTerminal {
                    turn_id: "turn-failed".to_string(),
                    status: ThreadTurnTerminalStatus::Failed,
                    outcome: Some("failed".to_string()),
                    error_message: Some("terminal error".to_string()),
                    first_committed_session_seq: Some(12),
                    boundary_session_seq: 14,
                    completed_at_ms: 200,
                    metadata: Some(terminal_metadata),
                }],
            }
        );
        application.shutdown().await.expect("shutdown");
    }

    #[tokio::test]
    async fn staged_revert_hides_terminal_projection_without_deleting_durable_evidence() {
        let (_temp, application, thread) = application_with_thread().await;
        let state = application.inner.state.clone();
        for (turn_id, boundary) in [("turn-before", 4), ("turn-at", 5), ("turn-after", 6)] {
            append_terminal(
                &state,
                thread.id(),
                turn_id,
                FrameworkTurnTerminalStatus::Failed,
                boundary,
                1_000 + boundary,
                json!({"boundary": boundary}),
            )
            .await;
        }
        state
            .set_session_revert_state(
                thread.id(),
                SessionRevertState::workspace_undo(5, "snapshot".to_string()),
            )
            .await
            .expect("stage revert");

        let raw = state
            .list_gateway_turn_terminals_for_thread(thread.id())
            .await
            .expect("raw terminal evidence");
        assert_eq!(
            raw.iter()
                .map(|terminal| terminal.turn_id.as_str())
                .collect::<Vec<_>>(),
            ["turn-before", "turn-at", "turn-after"]
        );
        assert!(
            state
                .gateway_turn_terminal("turn-at")
                .await
                .expect("exact terminal evidence")
                .is_some()
        );

        let history = thread
            .structural_history()
            .await
            .expect("current structural projection");
        assert_eq!(
            history
                .turn_terminals
                .iter()
                .map(|terminal| terminal.turn_id.as_str())
                .collect::<Vec<_>>(),
            ["turn-before"]
        );
        let (window, has_older) = thread
            .structural_history_window(i64::MIN, None, None, 10)
            .await
            .expect("current structural window");
        assert!(!has_older);
        assert_eq!(
            window
                .turn_terminals
                .iter()
                .map(|terminal| terminal.turn_id.as_str())
                .collect::<Vec<_>>(),
            ["turn-before"]
        );

        state
            .clear_session_revert_state(thread.id())
            .await
            .expect("clear revert");
        assert_eq!(
            thread
                .structural_history()
                .await
                .expect("restored structural projection")
                .turn_terminals
                .iter()
                .map(|terminal| terminal.turn_id.as_str())
                .collect::<Vec<_>>(),
            ["turn-before", "turn-at", "turn-after"]
        );

        application.shutdown().await.expect("shutdown");
    }

    #[tokio::test]
    async fn exact_compaction_lookup_returns_only_the_owning_thread_checkpoint() {
        let (temp, application, thread) = application_with_thread().await;
        let other_cwd = temp.path().join("other-workspace");
        std::fs::create_dir_all(&other_cwd).expect("other workspace");
        let other = application
            .client()
            .start_thread(StartThreadRequest::new(other_cwd))
            .await
            .expect("other Thread");
        let raw = append_compaction(
            &application.inner.state,
            thread.id(),
            17,
            json!({"checkpoint": "exact"}),
        )
        .await;

        assert_eq!(
            thread.compaction(raw.id).await.expect("exact compaction"),
            Some(ThreadCompaction::from(raw.clone()))
        );
        assert!(
            thread
                .compaction(raw.id + 1)
                .await
                .expect("missing compaction")
                .is_none()
        );
        let error = other
            .compaction(raw.id)
            .await
            .expect_err("cross-Thread checkpoint");
        let data = error
            .structured_data()
            .expect("structured cross-Thread checkpoint error");
        assert_eq!(data["kind"], "compaction_thread_mismatch");
        assert_eq!(data["threadId"], other.id());
        assert_eq!(data["checkpointId"], raw.id);

        application.shutdown().await.expect("shutdown");
    }

    #[tokio::test]
    async fn structural_history_window_is_half_open_and_per_kind_bounded() {
        let (_temp, application, thread) = application_with_thread().await;
        let state = application.inner.state.clone();
        let mut compaction_metadata = Vec::new();
        let mut terminal_metadata = Vec::new();
        for boundary in [5, 10, 20, 30] {
            let checkpoint = json!({"kind": "checkpoint", "boundary": boundary});
            append_compaction(&state, thread.id(), boundary, checkpoint.clone()).await;
            compaction_metadata.push(checkpoint);

            let terminal = match boundary {
                5 => json!({"lastCommittedSeq": boundary, "boundary": boundary}),
                10 => json!({"last_committed_seq": boundary, "boundary": boundary}),
                20 => json!({"firstCommittedSeq": boundary + 1, "boundary": boundary}),
                30 => json!({"first_committed_seq": boundary + 1, "boundary": boundary}),
                _ => unreachable!(),
            };
            append_terminal(
                &state,
                thread.id(),
                &format!("turn-{boundary}"),
                if boundary % 20 == 10 {
                    FrameworkTurnTerminalStatus::Interrupted
                } else {
                    FrameworkTurnTerminalStatus::Failed
                },
                boundary,
                1_000 + boundary,
                terminal.clone(),
            )
            .await;
            terminal_metadata.push(terminal);
        }

        let full = thread
            .structural_history()
            .await
            .expect("full structural history");
        assert_eq!(
            full.compactions
                .iter()
                .map(|record| record.boundary_session_seq)
                .collect::<Vec<_>>(),
            [5, 10, 20, 30]
        );
        assert_eq!(
            full.turn_terminals
                .iter()
                .map(|record| record.boundary_session_seq)
                .collect::<Vec<_>>(),
            [5, 10, 20, 30]
        );
        assert_eq!(
            full.compactions
                .iter()
                .map(|record| record.metadata.clone().expect("compaction metadata"))
                .collect::<Vec<_>>(),
            compaction_metadata
        );
        assert_eq!(
            full.turn_terminals
                .iter()
                .map(|record| record.metadata.clone().expect("terminal metadata"))
                .collect::<Vec<_>>(),
            terminal_metadata
        );

        let (window, _) = thread
            .structural_history_window(10, Some(30), None, 10)
            .await
            .expect("structural window");
        assert_eq!(
            window
                .compactions
                .iter()
                .map(|record| record.boundary_session_seq)
                .collect::<Vec<_>>(),
            [10, 20]
        );
        assert_eq!(
            window
                .turn_terminals
                .iter()
                .map(|record| record.boundary_session_seq)
                .collect::<Vec<_>>(),
            [10, 20]
        );
        assert_eq!(
            window
                .turn_terminals
                .iter()
                .map(|record| record.first_committed_session_seq)
                .collect::<Vec<_>>(),
            [None, Some(21)]
        );

        let (latest_in_window, has_older) = thread
            .structural_history_window(10, Some(30), None, 1)
            .await
            .expect("bounded structural window");
        assert!(has_older);
        assert_eq!(latest_in_window.compactions[0].boundary_session_seq, 20);
        assert_eq!(latest_in_window.turn_terminals[0].boundary_session_seq, 20);
        assert_eq!(
            thread
                .structural_history_window(i64::MIN, None, None, 0)
                .await
                .expect("zero structural window"),
            (ThreadStructuralHistory::default(), false)
        );

        application.shutdown().await.expect("shutdown");
    }
}
