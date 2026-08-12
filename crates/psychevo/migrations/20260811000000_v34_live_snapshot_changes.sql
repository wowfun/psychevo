ALTER TABLE gateway_live_snapshots ADD COLUMN change_version INTEGER;

WITH ranked AS (
    SELECT snapshot_key,
           ROW_NUMBER() OVER (ORDER BY updated_at_ms ASC, snapshot_key ASC) AS version
    FROM gateway_live_snapshots
)
UPDATE gateway_live_snapshots
SET change_version = (
    SELECT ranked.version
    FROM ranked
    WHERE ranked.snapshot_key = gateway_live_snapshots.snapshot_key
);

CREATE TABLE gateway_live_snapshot_clock (
    singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
    current_version INTEGER NOT NULL
);

INSERT INTO gateway_live_snapshot_clock(singleton, current_version)
SELECT 1, COALESCE(MAX(change_version), 0)
FROM gateway_live_snapshots;

CREATE UNIQUE INDEX idx_gateway_live_snapshots_change
    ON gateway_live_snapshots(change_version);
CREATE INDEX idx_gateway_live_events_created_at
    ON gateway_live_events(created_at_ms, seq);
CREATE INDEX idx_gateway_live_snapshots_updated_at
    ON gateway_live_snapshots(updated_at_ms, snapshot_key);

DROP INDEX idx_messages_session_seq;
DROP INDEX idx_context_evidence_prompt;
DROP INDEX idx_gateway_live_events_seq;
DROP INDEX idx_gateway_live_snapshots_owner;

PRAGMA user_version = 34;
