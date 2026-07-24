use psychevo_agent_core::now_ms;
use rusqlite::{OptionalExtension, params};
use serde_json::json;
use uuid::Uuid;

use crate::error::{Error, Result};

use super::store_undo_helpers::session_tool_call_count;
use super::{NativeSessionForkInput, StateRuntime};

impl StateRuntime {
    /// Copies the durable conversation prefix into a new root Native session.
    ///
    /// The transaction deliberately omits source bindings, activities, live
    /// projections, deliveries, outbox rows, revert state, agent lineage, and
    /// automation ownership. The child remains in the current workspace and
    /// receives a fresh Native runtime identity on its next turn.
    pub fn fork_native_session_history(&self, input: NativeSessionForkInput<'_>) -> Result<String> {
        let child_session_id = Uuid::now_v7().to_string();
        let now = now_ms();
        let metadata_json = serde_json::to_string(&json!({
            "forkedFromThreadId": input.source_session_id,
        }))?;
        let requested_boundary = input.before_session_seq;
        let source_session_id = input.source_session_id;
        self.write_retry(|conn| {
            let eligible = conn
                .query_row(
                    r#"
                    SELECT 1
                    FROM sessions s
                    WHERE s.id = ?1
                      AND s.source IN ('web', 'tui')
                      AND s.parent_session_id IS NULL
                      AND COALESCE(json_extract(s.metadata_json, '$.side_conversation'), 0) = 0
                      AND json_type(s.metadata_json, '$.revert') IS NULL
                      AND NOT EXISTS (
                          SELECT 1 FROM agent_edges e WHERE e.child_session_id = s.id
                      )
                    "#,
                    params![source_session_id],
                    |row| row.get::<_, i64>(0),
                )
                .optional()?
                .is_some();
            if !eligible {
                return Err(rusqlite::Error::InvalidParameterName(format!(
                    "session is not an eligible root interactive Thread: {source_session_id}"
                )));
            }
            let native_binding = conn
                .query_row(
                    "SELECT 1 FROM gateway_runtime_bindings WHERE thread_id = ?1 AND resolution_status = 'resolved' AND backend_kind = 'native'",
                    params![source_session_id],
                    |row| row.get::<_, i64>(0),
                )
                .optional()?
                .is_some();
            if !native_binding {
                return Err(rusqlite::Error::InvalidParameterName(format!(
                    "history fork requires a resolved Native binding: {source_session_id}"
                )));
            }
            let active = conn
                .query_row(
                    "SELECT 1 FROM gateway_activities WHERE thread_id = ?1 AND status IN ('running', 'queued') AND lease_expires_at_ms >= ?2 LIMIT 1",
                    params![source_session_id, now],
                    |row| row.get::<_, i64>(0),
                )
                .optional()?
                .is_some();
            if active {
                return Err(rusqlite::Error::InvalidParameterName(format!(
                    "running Thread cannot be forked: {source_session_id}"
                )));
            }
            let boundary = requested_boundary.unwrap_or(i64::MAX);
            if let Some(boundary) = requested_boundary {
                let valid_boundary = conn
                    .query_row(
                        "SELECT 1 FROM messages WHERE session_id = ?1 AND session_seq = ?2 AND role = 'user'",
                        params![source_session_id, boundary],
                        |row| row.get::<_, i64>(0),
                    )
                    .optional()?
                    .is_some();
                if !valid_boundary {
                    return Err(rusqlite::Error::InvalidParameterName(format!(
                        "fork boundary is not a durable user message: {boundary}"
                    )));
                }
            }

            let inserted = conn.execute(
                r#"
                INSERT INTO sessions (
                    id, source, parent_session_id, cwd, model, provider,
                    started_at_ms, updated_at_ms, ended_at_ms, end_reason, archived_at_ms,
                    message_count, tool_call_count, title, metadata_json
                )
                SELECT ?1, source, NULL, cwd, model, provider,
                       ?2, ?2, NULL, NULL, NULL, 0, 0, title, ?3
                FROM sessions
                WHERE id = ?4
                "#,
                params![child_session_id, now, metadata_json, source_session_id],
            )?;
            if inserted != 1 {
                return Err(rusqlite::Error::ExecuteReturnedResults);
            }

            conn.execute(
                r#"
                INSERT INTO messages (
                    session_id, session_seq, role, timestamp_ms, message_json,
                    content_text, tool_call_id, tool_name, tool_calls_json,
                    finish_reason, outcome, model, provider, usage_json, metadata_json,
                    context_input_tokens, billable_input_tokens, billable_output_tokens,
                    reasoning_tokens, cache_read_tokens, cache_write_tokens,
                    reported_total_tokens, estimated_cost_nanodollars,
                    pricing_source, pricing_tier, cost_status,
                    pricing_missing_reason, pricing_version
                )
                SELECT ?1, session_seq, role, timestamp_ms, message_json,
                       content_text, tool_call_id, tool_name, tool_calls_json,
                       finish_reason, outcome, model, provider, usage_json, metadata_json,
                       context_input_tokens, billable_input_tokens, billable_output_tokens,
                       reasoning_tokens, cache_read_tokens, cache_write_tokens,
                       reported_total_tokens, estimated_cost_nanodollars,
                       pricing_source, pricing_tier, cost_status,
                       pricing_missing_reason, pricing_version
                FROM messages
                WHERE session_id = ?2 AND session_seq < ?3
                ORDER BY session_seq ASC
                "#,
                params![child_session_id, source_session_id, boundary],
            )?;

            conn.execute(
                r#"
                INSERT INTO context_evidence (
                    session_id, prompt_session_seq, context_seq, role, source_kind,
                    source_name, source_path, provider_group, provider_block_index,
                    context_kind, timestamp_ms, content_text, metadata_json
                )
                SELECT ?1, prompt_session_seq, context_seq, role, source_kind,
                       source_name, source_path, provider_group, provider_block_index,
                       context_kind, timestamp_ms, content_text, metadata_json
                FROM context_evidence
                WHERE session_id = ?2 AND prompt_session_seq < ?3
                "#,
                params![child_session_id, source_session_id, boundary],
            )?;

            conn.execute(
                r#"
                INSERT INTO session_prompt_prefixes (
                    session_id, version, created_at_ms, provider, model, prefix_hash,
                    tool_declarations_hash, invalidation_reason, slots_json, metadata_json
                )
                SELECT ?1, p.version, p.created_at_ms, p.provider, p.model, p.prefix_hash,
                       p.tool_declarations_hash, p.invalidation_reason, p.slots_json, p.metadata_json
                FROM session_prompt_prefixes p
                WHERE p.session_id = ?2
                  AND EXISTS (
                      SELECT 1
                      FROM messages m
                      WHERE m.session_id = ?2
                        AND m.session_seq < ?3
                        AND CAST(json_extract(m.metadata_json, '$.prompt_prefix.version') AS INTEGER) = p.version
                  )
                "#,
                params![child_session_id, source_session_id, boundary],
            )?;

            conn.execute(
                r#"
                INSERT INTO session_compactions (
                    session_id, created_at_ms, reason, summary_text,
                    first_kept_session_seq, created_after_session_seq,
                    tokens_before, tokens_after, summary_provider, summary_model,
                    instructions, metadata_json
                )
                SELECT ?1, created_at_ms, reason, summary_text,
                       first_kept_session_seq, created_after_session_seq,
                       tokens_before, tokens_after, summary_provider, summary_model,
                       instructions, metadata_json
                FROM session_compactions
                WHERE session_id = ?2
                  AND created_after_session_seq < ?3
                  AND first_kept_session_seq < ?3
                "#,
                params![child_session_id, source_session_id, boundary],
            )?;

            conn.execute(
                r#"
                INSERT INTO gateway_runtime_bindings (
                    thread_id, resolution_status, agent_ref, agent_fingerprint,
                    agent_definition_json, runtime_ref, backend_kind, native_kind,
                    native_session_id, cwd, profile_fingerprint, profile_revision,
                    profile_config_json, adapter_kind, adapter_revision, ownership,
                    parent_thread_id, binding_revision, thread_preferences_json,
                    runtime_observed_json, control_revision, unresolved_reason,
                    created_at_ms, updated_at_ms
                )
                SELECT ?1, resolution_status, agent_ref, agent_fingerprint,
                       agent_definition_json, runtime_ref, backend_kind, native_kind,
                       NULL, cwd, profile_fingerprint, profile_revision,
                       profile_config_json, adapter_kind, adapter_revision, ownership,
                       NULL, 1, thread_preferences_json,
                       NULL, 1, unresolved_reason, ?2, ?2
                FROM gateway_runtime_bindings
                WHERE thread_id = ?3 AND backend_kind = 'native'
                "#,
                params![child_session_id, now, source_session_id],
            )?;

            conn.execute(
                r#"
                INSERT INTO gateway_turn_terminals (
                    turn_id, thread_id, status, outcome, error_message,
                    started_at_ms, completed_at_ms, metadata_json
                )
                SELECT 'fork:' || ?1 || ':' || turn_id, ?1, status, outcome, error_message,
                       started_at_ms, completed_at_ms, metadata_json
                FROM gateway_turn_terminals
                WHERE thread_id = ?2
                  AND (
                    ?3 = 9223372036854775807
                    OR COALESCE(
                        json_extract(metadata_json, '$.firstCommittedSeq'),
                        json_extract(metadata_json, '$.first_committed_seq')
                    ) < ?3
                  )
                "#,
                params![child_session_id, source_session_id, boundary],
            )?;

            let message_count: i64 = conn.query_row(
                "SELECT COUNT(*) FROM messages WHERE session_id = ?1",
                params![child_session_id],
                |row| row.get(0),
            )?;
            let tool_call_count = session_tool_call_count(conn, &child_session_id)?;
            conn.execute(
                "UPDATE sessions SET message_count = ?1, tool_call_count = ?2 WHERE id = ?3",
                params![message_count, tool_call_count, child_session_id],
            )?;
            Ok(())
        })
        .map_err(|error| match error {
            Error::Sqlite(rusqlite::Error::InvalidParameterName(message)) => {
                Error::Message(message)
            }
            other => other,
        })?;
        Ok(child_session_id)
    }
}
