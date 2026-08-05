ALTER TABLE gateway_live_events
    ADD COLUMN idempotency_key TEXT;

CREATE UNIQUE INDEX IF NOT EXISTS idx_gateway_live_events_idempotency_key
    ON gateway_live_events(idempotency_key)
    WHERE idempotency_key IS NOT NULL;

PRAGMA user_version = 31;
