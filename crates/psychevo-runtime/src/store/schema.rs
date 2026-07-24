use std::fs;
use std::path::Path;
use std::sync::atomic::AtomicUsize;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use rusqlite::Connection;

use crate::error::{Error, Result};

use super::store_schema_helpers::sqlite_table_exists;
use super::{
    MIN_SUPPORTED_SQLITE_SCHEMA_VERSION, SQLITE_SCHEMA_VERSION, StateRuntime, StateRuntimeInner,
};

impl StateRuntime {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        if path != Path::new(":memory:")
            && let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(path)?;
        conn.busy_timeout(Duration::from_millis(250))?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        let user_version: i64 = conn.pragma_query_value(None, "user_version", |row| row.get(0))?;
        let has_schema =
            sqlite_table_exists(&conn, "sessions")? || sqlite_table_exists(&conn, "messages")?;
        if user_version != 0
            && !(MIN_SUPPORTED_SQLITE_SCHEMA_VERSION..=SQLITE_SCHEMA_VERSION)
                .contains(&user_version)
        {
            return Err(Error::Config(format!(
                "state database schema version {user_version} is not supported; run `pevo init --reset-state` or set PSYCHEVO_DB to a new state database"
            )));
        }
        if user_version == 0 && has_schema {
            return Err(Error::Config(
                "state database has an unknown schema version; run `pevo init --reset-state` or set PSYCHEVO_DB to a new state database".to_string(),
            ));
        }
        conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS sessions (
                id TEXT PRIMARY KEY,
                source TEXT NOT NULL,
                parent_session_id TEXT,
                cwd TEXT NOT NULL,
                model TEXT NOT NULL,
                provider TEXT NOT NULL,
                started_at_ms INTEGER NOT NULL,
                updated_at_ms INTEGER NOT NULL,
                ended_at_ms INTEGER,
                end_reason TEXT,
                archived_at_ms INTEGER,
                message_count INTEGER NOT NULL DEFAULT 0,
                tool_call_count INTEGER NOT NULL DEFAULT 0,
                title TEXT,
                metadata_json TEXT
            );

            CREATE TABLE IF NOT EXISTS messages (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                session_id TEXT NOT NULL REFERENCES sessions(id),
                session_seq INTEGER NOT NULL,
                role TEXT NOT NULL,
                timestamp_ms INTEGER NOT NULL,
                message_json TEXT NOT NULL,
                content_text TEXT,
                tool_call_id TEXT,
                tool_name TEXT,
                tool_calls_json TEXT,
                finish_reason TEXT,
                outcome TEXT,
                model TEXT,
                provider TEXT,
                usage_json TEXT,
                metadata_json TEXT,
                context_input_tokens INTEGER,
                billable_input_tokens INTEGER,
                billable_output_tokens INTEGER,
                reasoning_tokens INTEGER,
                cache_read_tokens INTEGER,
                cache_write_tokens INTEGER,
                reported_total_tokens INTEGER,
                estimated_cost_nanodollars INTEGER,
                pricing_source TEXT,
                pricing_tier TEXT,
                cost_status TEXT,
                pricing_missing_reason TEXT,
                pricing_version TEXT,
                UNIQUE(session_id, session_seq)
            );

            CREATE TABLE IF NOT EXISTS context_evidence (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
                prompt_session_seq INTEGER NOT NULL,
                context_seq INTEGER NOT NULL,
                role TEXT NOT NULL,
                source_kind TEXT NOT NULL,
                source_name TEXT,
                source_path TEXT,
                provider_group TEXT,
                provider_block_index INTEGER,
                context_kind TEXT,
                timestamp_ms INTEGER NOT NULL,
                content_text TEXT NOT NULL,
                metadata_json TEXT,
                UNIQUE(session_id, prompt_session_seq, context_seq),
                FOREIGN KEY (session_id, prompt_session_seq)
                    REFERENCES messages(session_id, session_seq)
                    ON DELETE CASCADE
            );

            CREATE TABLE IF NOT EXISTS agent_edges (
                parent_session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
                child_session_id TEXT PRIMARY KEY REFERENCES sessions(id) ON DELETE CASCADE,
                status TEXT NOT NULL,
                created_at_ms INTEGER NOT NULL,
                updated_at_ms INTEGER NOT NULL,
                metadata_json TEXT
            );

            CREATE TABLE IF NOT EXISTS agent_team_runs (
                id TEXT PRIMARY KEY,
                parent_session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
                mission_run_id TEXT,
                team_name TEXT NOT NULL,
                description TEXT,
                source_path TEXT,
                leader_agent_name TEXT NOT NULL,
                members_json TEXT NOT NULL,
                max_parallel_agents INTEGER NOT NULL,
                status TEXT NOT NULL,
                started_at_ms INTEGER NOT NULL,
                ended_at_ms INTEGER,
                final_summary TEXT,
                metadata_json TEXT
            );

            CREATE TABLE IF NOT EXISTS agent_mission_runs (
                id TEXT PRIMARY KEY,
                parent_session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
                team_run_id TEXT REFERENCES agent_team_runs(id) ON DELETE SET NULL,
                team_name TEXT,
                goal TEXT NOT NULL,
                lead_agent_name TEXT NOT NULL,
                status TEXT NOT NULL,
                started_at_ms INTEGER NOT NULL,
                ended_at_ms INTEGER,
                final_summary TEXT,
                metadata_json TEXT
            );

            CREATE TABLE IF NOT EXISTS session_prompt_prefixes (
                session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
                version INTEGER NOT NULL,
                created_at_ms INTEGER NOT NULL,
                provider TEXT NOT NULL,
                model TEXT NOT NULL,
                prefix_hash TEXT NOT NULL,
                tool_declarations_hash TEXT NOT NULL,
                invalidation_reason TEXT,
                slots_json TEXT NOT NULL,
                metadata_json TEXT,
                PRIMARY KEY (session_id, version)
            );

            CREATE TABLE IF NOT EXISTS agent_mailbox_events (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                parent_session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
                child_session_id TEXT REFERENCES sessions(id) ON DELETE SET NULL,
                agent_id TEXT NOT NULL,
                task_name TEXT,
                agent_name TEXT NOT NULL,
                created_at_ms INTEGER NOT NULL,
                delivered_at_ms INTEGER,
                delivered_prompt_session_seq INTEGER,
                delivered_after_session_seq INTEGER,
                delivered_tool_call_id TEXT,
                content_text TEXT NOT NULL,
                payload_json TEXT NOT NULL,
                metadata_json TEXT
            );

            CREATE TABLE IF NOT EXISTS session_compactions (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
                created_at_ms INTEGER NOT NULL,
                reason TEXT NOT NULL,
                summary_text TEXT NOT NULL,
                first_kept_session_seq INTEGER NOT NULL,
                created_after_session_seq INTEGER NOT NULL,
                tokens_before INTEGER,
                tokens_after INTEGER,
                summary_provider TEXT NOT NULL,
                summary_model TEXT NOT NULL,
                instructions TEXT,
                metadata_json TEXT
            );

            CREATE TABLE IF NOT EXISTS gateway_source_bindings (
                source_key TEXT PRIMARY KEY,
                source_kind TEXT NOT NULL,
                raw_identity_json TEXT NOT NULL,
                visible_name TEXT,
                thread_id TEXT REFERENCES sessions(id) ON DELETE CASCADE,
                backend_kind TEXT,
                backend_native_id TEXT,
                draft_agent_ref TEXT,
                draft_profile_ref TEXT,
                draft_control_values_json TEXT,
                created_at_ms INTEGER NOT NULL,
                updated_at_ms INTEGER NOT NULL,
                lineage_json TEXT
            );

            CREATE TABLE IF NOT EXISTS gateway_runtime_bindings (
                thread_id TEXT PRIMARY KEY REFERENCES sessions(id) ON DELETE CASCADE,
                resolution_status TEXT NOT NULL CHECK (resolution_status IN ('resolved', 'unresolved')),
                agent_ref TEXT,
                agent_fingerprint TEXT,
                agent_definition_json TEXT,
                runtime_ref TEXT,
                backend_kind TEXT,
                native_kind TEXT,
                native_session_id TEXT,
                cwd TEXT NOT NULL,
                profile_fingerprint TEXT,
                profile_revision TEXT,
                profile_config_json TEXT,
                adapter_kind TEXT,
                adapter_revision TEXT,
                ownership TEXT NOT NULL CHECK (ownership IN ('read_write', 'read_only')),
                parent_thread_id TEXT REFERENCES sessions(id) ON DELETE SET NULL,
                binding_revision INTEGER NOT NULL CHECK (binding_revision > 0),
                thread_preferences_json TEXT,
                runtime_observed_json TEXT,
                control_revision INTEGER NOT NULL CHECK (control_revision > 0),
                unresolved_reason TEXT,
                created_at_ms INTEGER NOT NULL,
                updated_at_ms INTEGER NOT NULL,
                CHECK (
                    (resolution_status = 'resolved'
                        AND agent_fingerprint IS NOT NULL
                        AND agent_definition_json IS NOT NULL
                        AND runtime_ref IS NOT NULL
                        AND backend_kind IS NOT NULL
                        AND native_kind IS NOT NULL
                        AND profile_fingerprint IS NOT NULL
                        AND profile_revision IS NOT NULL
                        AND adapter_kind IS NOT NULL
                        AND adapter_revision IS NOT NULL
                        AND unresolved_reason IS NULL)
                    OR
                    (resolution_status = 'unresolved' AND unresolved_reason IS NOT NULL)
                )
            );

            CREATE TABLE IF NOT EXISTS gateway_activities (
                activity_id TEXT PRIMARY KEY,
                thread_id TEXT REFERENCES sessions(id) ON DELETE CASCADE,
                source_key TEXT,
                turn_id TEXT,
                kind TEXT NOT NULL,
                status TEXT NOT NULL,
                owner_id TEXT NOT NULL,
                owner_surface TEXT,
                generation INTEGER NOT NULL,
                started_at_ms INTEGER NOT NULL,
                updated_at_ms INTEGER NOT NULL,
                lease_expires_at_ms INTEGER NOT NULL,
                queued_turns INTEGER NOT NULL DEFAULT 0,
                superseded_activity_id TEXT,
                intent_json TEXT
            );

            CREATE TABLE IF NOT EXISTS gateway_live_events (
                seq INTEGER PRIMARY KEY AUTOINCREMENT,
                activity_id TEXT,
                owner_id TEXT,
                thread_id TEXT REFERENCES sessions(id) ON DELETE CASCADE,
                turn_id TEXT,
                event_json TEXT NOT NULL,
                created_at_ms INTEGER NOT NULL
            );

            CREATE TABLE IF NOT EXISTS gateway_live_snapshots (
                snapshot_key TEXT PRIMARY KEY,
                activity_id TEXT,
                owner_id TEXT,
                thread_id TEXT REFERENCES sessions(id) ON DELETE CASCADE,
                turn_id TEXT,
                event_kind TEXT NOT NULL,
                event_json TEXT NOT NULL,
                revision INTEGER NOT NULL,
                created_at_ms INTEGER NOT NULL,
                updated_at_ms INTEGER NOT NULL
            );

            CREATE TABLE IF NOT EXISTS gateway_control_commands (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                activity_id TEXT NOT NULL,
                owner_id TEXT NOT NULL,
                command_kind TEXT NOT NULL,
                status TEXT NOT NULL,
                payload_json TEXT NOT NULL,
                created_at_ms INTEGER NOT NULL,
                updated_at_ms INTEGER NOT NULL,
                error TEXT
            );

            CREATE TABLE IF NOT EXISTS gateway_turn_terminals (
                turn_id TEXT PRIMARY KEY,
                thread_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
                status TEXT NOT NULL,
                outcome TEXT,
                error_message TEXT,
                started_at_ms INTEGER,
                completed_at_ms INTEGER NOT NULL,
                metadata_json TEXT
            );

            CREATE TABLE IF NOT EXISTS gateway_turn_deliveries (
                turn_id TEXT PRIMARY KEY,
                thread_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
                runtime_ref TEXT NOT NULL,
                status TEXT NOT NULL CHECK (status IN ('not_delivered', 'delivered', 'unknown', 'terminal')),
                input_json TEXT,
                input_hash TEXT NOT NULL,
                created_at_ms INTEGER NOT NULL,
                updated_at_ms INTEGER NOT NULL,
                delivery_confirmed_at_ms INTEGER,
                terminal_at_ms INTEGER
            );

            CREATE TABLE IF NOT EXISTS gateway_channel_outbox (
                delivery_id TEXT PRIMARY KEY,
                thread_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
                turn_id TEXT NOT NULL,
                connection_id TEXT NOT NULL,
                source_key TEXT NOT NULL,
                status TEXT NOT NULL CHECK (status IN ('pending', 'acknowledged', 'failed')),
                payload_text TEXT,
                payload_hash TEXT NOT NULL,
                created_at_ms INTEGER NOT NULL,
                updated_at_ms INTEGER NOT NULL,
                acknowledged_at_ms INTEGER
            );

            CREATE TABLE IF NOT EXISTS automations (
                id TEXT PRIMARY KEY,
                cwd TEXT NOT NULL,
                kind TEXT NOT NULL,
                target_thread_id TEXT REFERENCES sessions(id) ON DELETE SET NULL,
                title TEXT NOT NULL,
                prompt TEXT NOT NULL,
                schedule_json TEXT NOT NULL,
                enabled INTEGER NOT NULL DEFAULT 1,
                execution_json TEXT NOT NULL,
                model TEXT,
                reasoning_effort TEXT,
                source_key TEXT,
                created_at_ms INTEGER NOT NULL,
                updated_at_ms INTEGER NOT NULL,
                last_run_at_ms INTEGER,
                next_run_at_ms INTEGER,
                last_status TEXT,
                last_error TEXT
            );

            CREATE TABLE IF NOT EXISTS automation_runs (
                id TEXT PRIMARY KEY,
                automation_id TEXT NOT NULL REFERENCES automations(id) ON DELETE CASCADE,
                trigger TEXT NOT NULL,
                status TEXT NOT NULL,
                started_at_ms INTEGER NOT NULL,
                completed_at_ms INTEGER,
                thread_id TEXT REFERENCES sessions(id) ON DELETE SET NULL,
                source_key TEXT,
                error TEXT,
                metadata_json TEXT
            );
            CREATE INDEX IF NOT EXISTS idx_messages_session_seq
                ON messages(session_id, session_seq);
            CREATE INDEX IF NOT EXISTS idx_sessions_active_browser
                ON sessions(cwd, updated_at_ms DESC, id ASC)
                WHERE archived_at_ms IS NULL AND parent_session_id IS NULL;
            CREATE INDEX IF NOT EXISTS idx_sessions_archived_browser
                ON sessions(cwd, updated_at_ms DESC, id ASC)
                WHERE archived_at_ms IS NOT NULL AND parent_session_id IS NULL;
            CREATE INDEX IF NOT EXISTS idx_context_evidence_prompt
                ON context_evidence(session_id, prompt_session_seq, context_seq);
            CREATE INDEX IF NOT EXISTS idx_agent_edges_parent
                ON agent_edges(parent_session_id, status, updated_at_ms);
            CREATE INDEX IF NOT EXISTS idx_agent_team_runs_parent
                ON agent_team_runs(parent_session_id, status, started_at_ms);
            CREATE INDEX IF NOT EXISTS idx_agent_mission_runs_parent
                ON agent_mission_runs(parent_session_id, status, started_at_ms);
            CREATE INDEX IF NOT EXISTS idx_session_prompt_prefixes_latest
                ON session_prompt_prefixes(session_id, version DESC);
            CREATE INDEX IF NOT EXISTS idx_agent_mailbox_parent_pending
                ON agent_mailbox_events(parent_session_id, delivered_at_ms, created_at_ms);
            CREATE INDEX IF NOT EXISTS idx_session_compactions_latest
                ON session_compactions(session_id, created_after_session_seq, created_at_ms);
            CREATE INDEX IF NOT EXISTS idx_gateway_source_bindings_thread
                ON gateway_source_bindings(thread_id, updated_at_ms);
            CREATE UNIQUE INDEX IF NOT EXISTS idx_gateway_runtime_bindings_native_session
                ON gateway_runtime_bindings(runtime_ref, native_session_id)
                WHERE native_session_id IS NOT NULL;
            CREATE INDEX IF NOT EXISTS idx_gateway_runtime_bindings_parent
                ON gateway_runtime_bindings(parent_thread_id, updated_at_ms);
            CREATE INDEX IF NOT EXISTS idx_gateway_activities_thread
                ON gateway_activities(thread_id, status, updated_at_ms);
            CREATE INDEX IF NOT EXISTS idx_gateway_activities_source
                ON gateway_activities(source_key, status, updated_at_ms);
            CREATE INDEX IF NOT EXISTS idx_gateway_activities_owner
                ON gateway_activities(owner_id, status, updated_at_ms);
            CREATE INDEX IF NOT EXISTS idx_gateway_live_events_seq
                ON gateway_live_events(seq);
            CREATE INDEX IF NOT EXISTS idx_gateway_live_events_thread
                ON gateway_live_events(thread_id, seq);
            CREATE INDEX IF NOT EXISTS idx_gateway_live_snapshots_thread
                ON gateway_live_snapshots(thread_id, turn_id, updated_at_ms);
            CREATE INDEX IF NOT EXISTS idx_gateway_live_snapshots_activity
                ON gateway_live_snapshots(activity_id, updated_at_ms);
            CREATE INDEX IF NOT EXISTS idx_gateway_live_snapshots_owner
                ON gateway_live_snapshots(owner_id, updated_at_ms);
            CREATE INDEX IF NOT EXISTS idx_gateway_control_commands_owner
                ON gateway_control_commands(owner_id, status, id);
            CREATE INDEX IF NOT EXISTS idx_gateway_turn_terminals_thread
                ON gateway_turn_terminals(thread_id, completed_at_ms);
            CREATE INDEX IF NOT EXISTS idx_gateway_turn_deliveries_thread
                ON gateway_turn_deliveries(thread_id, updated_at_ms);
            CREATE INDEX IF NOT EXISTS idx_gateway_channel_outbox_pending
                ON gateway_channel_outbox(connection_id, source_key, status, updated_at_ms);
            CREATE INDEX IF NOT EXISTS idx_automations_cwd_enabled_next
                ON automations(cwd, enabled, next_run_at_ms);
            CREATE INDEX IF NOT EXISTS idx_automation_runs_task
                ON automation_runs(automation_id, started_at_ms);
            CREATE INDEX IF NOT EXISTS idx_automation_runs_status
                ON automation_runs(automation_id, status, started_at_ms);
            "#,
        )?;
        conn.pragma_update(None, "user_version", SQLITE_SCHEMA_VERSION)?;
        Ok(Self {
            inner: Arc::new(StateRuntimeInner {
                db_path: path.to_path_buf(),
                conn: Mutex::new(conn),
                successful_writes: AtomicUsize::new(0),
                filesystem_grants: Mutex::new(Default::default()),
            }),
        })
    }
}
