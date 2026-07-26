CREATE TABLE IF NOT EXISTS framework_interactions (
    interaction_id TEXT NOT NULL,
    thread_id TEXT NOT NULL,
    turn_id TEXT NOT NULL,
    kind TEXT NOT NULL,
    status TEXT NOT NULL CHECK(status IN ('pending', 'resolved', 'cancelled')),
    payload_json TEXT NOT NULL,
    resolution_json TEXT,
    requested_at_ms INTEGER NOT NULL,
    resolved_at_ms INTEGER,
    PRIMARY KEY(turn_id, interaction_id),
    FOREIGN KEY(thread_id) REFERENCES sessions(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_framework_interactions_thread_pending
    ON framework_interactions(thread_id, status, requested_at_ms);
CREATE INDEX IF NOT EXISTS idx_framework_interactions_turn
    ON framework_interactions(turn_id, requested_at_ms);

PRAGMA user_version = 29;
