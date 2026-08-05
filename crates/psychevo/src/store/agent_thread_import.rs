#[cfg(test)]
use std::sync::atomic::Ordering;

use psychevo_agent_core::now_ms;

use crate::error::{Error, Result};
use crate::run::normalize_session_title;

use super::store_message_fields::{message_fields, optional_json_string};
use super::store_runtime_bindings::validate_runtime_binding_input;
use super::{AgentThreadImportCommit, AgentThreadImportCommitInput, StateRuntime};

impl StateRuntime {
    pub(crate) async fn commit_agent_thread_import(
        &self,
        input: AgentThreadImportCommitInput<'_>,
    ) -> Result<AgentThreadImportCommit> {
        validate_agent_thread_import(&input)?;
        let title = input.title.and_then(normalize_session_title);
        let metadata_json = serde_json::to_string(input.metadata)?;
        let mut prepared_messages = Vec::with_capacity(input.messages.len());
        let mut tool_call_count = 0i64;
        for message in input.messages {
            let fields = message_fields(message.message)?;
            tool_call_count = tool_call_count.saturating_add(fields.tool_call_count);
            prepared_messages.push((
                fields,
                serde_json::to_string(message.message)?,
                optional_json_string(message.usage)?,
                optional_json_string(message.metadata)?,
            ));
        }
        let message_count = i64::try_from(prepared_messages.len())
            .map_err(|_| Error::Message("Agent import history is too large".to_string()))?;
        let now = now_ms();

        let mut operation = self.begin_sqlx_operation();
        let result = async {
            let mut tx = self.begin_sqlx_write().await?;
            let native_session_id = input
                .binding
                .native_session_id
                .expect("validated Agent import native session id");
            if let Some(thread_id) = sqlx::query_scalar::<_, String>(
                r#"
                SELECT thread_id
                FROM gateway_runtime_bindings
                WHERE resolution_status = 'resolved'
                  AND runtime_ref = ?1
                  AND native_session_id = ?2
                "#,
            )
            .bind(input.binding.runtime_ref)
            .bind(native_session_id)
            .fetch_optional(&mut *tx)
            .await?
            {
                tx.rollback().await?;
                return Ok(AgentThreadImportCommit::Existing { thread_id });
            }

            sqlx::query(
                r#"
                INSERT INTO sessions (
                    id, source, parent_session_id, cwd, model, provider,
                    started_at_ms, updated_at_ms, ended_at_ms, end_reason,
                    archived_at_ms, message_count, tool_call_count, title,
                    metadata_json
                ) VALUES (
                    ?1, ?2, ?3, ?4, 'agent', ?5,
                    ?6, ?6, NULL, NULL, NULL, ?7, ?8, ?9, ?10
                )
                "#,
            )
            .bind(input.thread_id)
            .bind(input.source)
            .bind(input.parent_thread_id)
            .bind(input.cwd.to_string_lossy().as_ref())
            .bind(input.binding.backend_kind)
            .bind(now)
            .bind(message_count)
            .bind(tool_call_count)
            .bind(title.as_deref())
            .bind(&metadata_json)
            .execute(&mut *tx)
            .await?;

            let binding = &input.binding;
            sqlx::query(
                r#"
                INSERT INTO gateway_runtime_bindings (
                    thread_id, resolution_status, agent_ref, agent_fingerprint,
                    agent_definition_json, runtime_ref, backend_kind, native_kind,
                    native_session_id, cwd, profile_fingerprint, profile_revision,
                    profile_config_json, adapter_kind, adapter_revision, ownership,
                    parent_thread_id, binding_revision, thread_preferences_json,
                    runtime_observed_json, control_revision, unresolved_reason,
                    created_at_ms, updated_at_ms
                ) VALUES (
                    ?1, 'resolved', ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10,
                    ?11, ?12, ?13, ?14, ?15, ?16, 1, NULL, NULL, 1, NULL,
                    ?17, ?17
                )
                "#,
            )
            .bind(input.thread_id)
            .bind(binding.agent_ref)
            .bind(binding.agent_fingerprint)
            .bind(binding.agent_definition_json)
            .bind(binding.runtime_ref)
            .bind(binding.backend_kind)
            .bind(binding.native_kind)
            .bind(native_session_id)
            .bind(binding.cwd)
            .bind(binding.profile_fingerprint)
            .bind(binding.profile_revision)
            .bind(binding.profile_config_json)
            .bind(binding.adapter_kind)
            .bind(binding.adapter_revision)
            .bind(binding.ownership.as_str())
            .bind(binding.parent_thread_id)
            .bind(now)
            .execute(&mut *tx)
            .await?;

            for (index, (fields, message_json, usage_json, message_metadata_json)) in
                prepared_messages.iter().enumerate()
            {
                let session_seq = i64::try_from(index)
                    .map_err(|_| Error::Message("Agent import history is too large".to_string()))?
                    + 1;
                sqlx::query(
                    r#"
                    INSERT INTO messages (
                        session_id, session_seq, role, timestamp_ms, message_json,
                        content_text, tool_call_id, tool_name, tool_calls_json,
                        finish_reason, outcome, model, provider, usage_json,
                        metadata_json
                    ) VALUES (
                        ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11,
                        ?12, ?13, ?14, ?15
                    )
                    "#,
                )
                .bind(input.thread_id)
                .bind(session_seq)
                .bind(&fields.role)
                .bind(fields.timestamp_ms)
                .bind(message_json)
                .bind(&fields.content_text)
                .bind(&fields.tool_call_id)
                .bind(&fields.tool_name)
                .bind(&fields.tool_calls_json)
                .bind(&fields.finish_reason)
                .bind(&fields.outcome)
                .bind(&fields.model)
                .bind(&fields.provider)
                .bind(usage_json)
                .bind(message_metadata_json)
                .execute(&mut *tx)
                .await?;
            }

            #[cfg(test)]
            if self
                .inner
                .fail_next_agent_thread_import_commit
                .swap(0, Ordering::SeqCst)
                > 0
            {
                return Err(Error::Message(
                    "injected Agent Thread import commit failure".to_string(),
                ));
            }

            tx.commit().await?;
            Ok(AgentThreadImportCommit::Published)
        }
        .await;
        operation.finish(&result);
        result
    }

    #[cfg(test)]
    pub(crate) fn fail_next_agent_thread_import_commit(&self) {
        self.inner
            .fail_next_agent_thread_import_commit
            .store(1, Ordering::SeqCst);
    }
}

fn validate_agent_thread_import(input: &AgentThreadImportCommitInput<'_>) -> Result<()> {
    validate_runtime_binding_input(&input.binding)?;
    if input.thread_id != input.binding.thread_id {
        return Err(Error::Message(
            "Agent import binding does not belong to the reserved Thread".to_string(),
        ));
    }
    if input.parent_thread_id != input.binding.parent_thread_id {
        return Err(Error::Message(
            "Agent publication binding parent does not match the Thread parent".to_string(),
        ));
    }
    if input.parent_thread_id == Some(input.thread_id) {
        return Err(Error::Message(
            "Agent publication Thread cannot be its own parent".to_string(),
        ));
    }
    let cwd = input.cwd.to_string_lossy();
    if cwd != input.binding.cwd {
        return Err(Error::Message(
            "Agent import binding cwd does not match the reserved Thread".to_string(),
        ));
    }
    if input.source.trim().is_empty() {
        return Err(Error::Message(
            "Agent import Thread source must not be empty".to_string(),
        ));
    }
    if input.binding.native_session_id.is_none() {
        return Err(Error::Message(
            "Agent import requires a native session id".to_string(),
        ));
    }
    Ok(())
}
