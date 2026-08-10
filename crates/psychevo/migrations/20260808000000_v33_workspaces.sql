CREATE TABLE workspaces (
    id TEXT PRIMARY KEY NOT NULL,
    display_name TEXT NOT NULL CHECK (length(trim(display_name)) > 0),
    revision INTEGER NOT NULL DEFAULT 1 CHECK (revision > 0),
    created_at_ms INTEGER NOT NULL,
    updated_at_ms INTEGER NOT NULL
);

CREATE TABLE workspace_roots (
    workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    ordinal INTEGER NOT NULL CHECK (ordinal >= 0),
    canonical_path TEXT NOT NULL UNIQUE,
    PRIMARY KEY (workspace_id, ordinal)
);

CREATE TABLE thread_workspace_bindings (
    thread_id TEXT PRIMARY KEY NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE RESTRICT,
    root_source TEXT NOT NULL CHECK (root_source IN ('direct', 'workspace')),
    created_at_ms INTEGER NOT NULL
);

CREATE INDEX idx_thread_workspace_bindings_workspace
    ON thread_workspace_bindings(workspace_id, thread_id);

CREATE INDEX idx_sessions_cwd ON sessions(cwd);

CREATE TABLE thread_workspace_roots (
    thread_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    ordinal INTEGER NOT NULL CHECK (ordinal >= 0),
    canonical_path TEXT NOT NULL,
    PRIMARY KEY (thread_id, ordinal),
    UNIQUE (thread_id, canonical_path)
);

CREATE TABLE gateway_navigation_state (
    singleton INTEGER PRIMARY KEY NOT NULL CHECK (singleton = 1),
    revision INTEGER NOT NULL DEFAULT 1 CHECK (revision > 0)
);

INSERT INTO gateway_navigation_state(singleton, revision) VALUES (1, 1);

CREATE TABLE gateway_pinned_threads (
    thread_id TEXT PRIMARY KEY NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    position INTEGER NOT NULL UNIQUE CHECK (position >= 0)
);

CREATE TABLE gateway_pinned_workspaces (
    workspace_id TEXT PRIMARY KEY NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    position INTEGER NOT NULL UNIQUE CHECK (position >= 0)
);

PRAGMA user_version = 33;
