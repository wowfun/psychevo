ALTER TABLE gateway_turn_terminals
    ADD COLUMN boundary_session_seq INTEGER NOT NULL DEFAULT 0;

DROP INDEX IF EXISTS idx_gateway_turn_terminals_thread;
CREATE INDEX idx_gateway_turn_terminals_thread
    ON gateway_turn_terminals(
        thread_id, boundary_session_seq, completed_at_ms, turn_id
    );

CREATE INDEX idx_gateway_turn_terminals_visible_history
    ON gateway_turn_terminals(
        thread_id, boundary_session_seq, completed_at_ms, turn_id
    )
    WHERE status IN ('failed', 'interrupted');

CREATE INDEX idx_messages_visible_history
    ON messages(session_id, session_seq DESC)
    WHERE json_type(metadata_json, '$.side_inherited.hidden') IS NOT 'true';

PRAGMA user_version = 32;
