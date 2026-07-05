from __future__ import annotations

from peval_py.state.constants import REFRESH_LOG_LIMIT


class StateSchemaMixin:
    def initialize_schema(self) -> None:
        self.conn.executescript(
            """
            CREATE TABLE IF NOT EXISTS peval_py_sources (
                source_key TEXT PRIMARY KEY,
                kind TEXT NOT NULL,
                adapter TEXT NOT NULL,
                label TEXT NOT NULL,
                input_path TEXT,
                db_path TEXT,
                session_id TEXT,
                source_alias TEXT,
                agent_name TEXT,
                agent_version TEXT,
                model TEXT,
                artifact_dir TEXT,
                artifact_updated_at_ms INTEGER,
                trial_key TEXT,
                trial_session_id TEXT,
                last_turn_finished_at_ms INTEGER,
                refreshable INTEGER NOT NULL,
                active INTEGER NOT NULL,
                snapshot INTEGER NOT NULL,
                created_at_ms INTEGER NOT NULL,
                updated_at_ms INTEGER NOT NULL,
                last_status TEXT,
                last_error TEXT,
                last_refreshed_at_ms INTEGER
            );
            CREATE TABLE IF NOT EXISTS peval_py_refresh_log (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                source_key TEXT,
                status TEXT NOT NULL,
                warning_count INTEGER NOT NULL DEFAULT 0,
                error TEXT,
                refreshed_at_ms INTEGER NOT NULL
            );
            """
        )
        self.ensure_source_columns()
        self.conn.commit()

    def ensure_source_columns(self) -> None:
        existing = {
            str(row["name"])
            for row in self.conn.execute("PRAGMA table_info(peval_py_sources)").fetchall()
        }
        columns = {
            "trial_key": "TEXT",
            "trial_session_id": "TEXT",
            "last_turn_finished_at_ms": "INTEGER",
        }
        for name, sql_type in columns.items():
            if name not in existing:
                self.conn.execute(
                    f"ALTER TABLE peval_py_sources ADD COLUMN {name} {sql_type}"
                )

    def log_refresh(
        self,
        source_key: str,
        status: str,
        warning_count: int,
        error: str | None,
        timestamp: int,
    ) -> None:
        self.conn.execute(
            """
            INSERT INTO peval_py_refresh_log
            (source_key, status, warning_count, error, refreshed_at_ms)
            VALUES (?, ?, ?, ?, ?)
            """,
            (source_key, status, warning_count, error, timestamp),
        )
        self.conn.execute(
            """
            DELETE FROM peval_py_refresh_log
            WHERE id NOT IN (
                SELECT id
                FROM peval_py_refresh_log
                ORDER BY refreshed_at_ms DESC, id DESC
                LIMIT ?
            )
            """,
            (REFRESH_LOG_LIMIT,),
        )
