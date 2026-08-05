CREATE INDEX IF NOT EXISTS idx_agent_edges_agent_id
    ON agent_edges(json_extract(metadata_json, '$.agent.id'), updated_at_ms DESC)
    WHERE json_extract(metadata_json, '$.agent.id') IS NOT NULL;

CREATE INDEX IF NOT EXISTS idx_agent_edges_task_name
    ON agent_edges(json_extract(metadata_json, '$.agent.task_name'), updated_at_ms DESC)
    WHERE json_extract(metadata_json, '$.agent.task_name') IS NOT NULL;

PRAGMA user_version = 30;
