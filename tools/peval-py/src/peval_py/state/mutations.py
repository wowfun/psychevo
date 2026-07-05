from __future__ import annotations

from typing import Any

from peval_py._state.artifacts import remove_artifact_dir
from peval_py.analysis import write_note_file
from peval_py.config import ToolConfig
from peval_py.state.summaries import now_ms, trial_summary


class StateMutationMixin:
    def upsert_source_row(
        self,
        source_key: str,
        source: dict[str, Any],
        artifact_dir: str,
        timestamp: int,
        *,
        trajectory: dict[str, Any] | None = None,
        meta: dict[str, Any] | None = None,
        refreshable: bool,
        snapshot: bool,
        status: str,
        error: str | None = None,
    ) -> None:
        summary = trial_summary(trajectory, meta)
        self.conn.execute(
            """
            INSERT INTO peval_py_sources
            (source_key, kind, adapter, label, input_path, db_path, session_id,
             source_alias, agent_name, agent_version, model,
             artifact_dir, artifact_updated_at_ms,
             trial_key, trial_session_id, last_turn_finished_at_ms,
             refreshable, active, snapshot, created_at_ms, updated_at_ms,
             last_status, last_error, last_refreshed_at_ms)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 1, ?, ?, ?, ?, ?, ?)
            ON CONFLICT(source_key) DO UPDATE SET
                kind = excluded.kind,
                adapter = excluded.adapter,
                label = excluded.label,
                input_path = excluded.input_path,
                db_path = excluded.db_path,
                session_id = excluded.session_id,
                source_alias = COALESCE(excluded.source_alias, peval_py_sources.source_alias),
                agent_name = excluded.agent_name,
                agent_version = excluded.agent_version,
                model = excluded.model,
                artifact_dir = excluded.artifact_dir,
                artifact_updated_at_ms = excluded.artifact_updated_at_ms,
                trial_key = excluded.trial_key,
                trial_session_id = excluded.trial_session_id,
                last_turn_finished_at_ms = excluded.last_turn_finished_at_ms,
                refreshable = excluded.refreshable,
                snapshot = excluded.snapshot,
                active = 1,
                updated_at_ms = excluded.updated_at_ms,
                last_status = excluded.last_status,
                last_error = excluded.last_error,
                last_refreshed_at_ms = excluded.last_refreshed_at_ms
            """,
            (
                source_key,
                source["kind"],
                source["adapter"],
                source["label"],
                source.get("input_path"),
                source.get("db_path"),
                source.get("session_id"),
                source.get("source_alias"),
                source.get("agent_name"),
                source.get("agent_version"),
                source.get("model"),
                artifact_dir,
                timestamp,
                summary["trial_key"],
                summary["trial_session_id"],
                summary["last_turn_finished_at_ms"],
                1 if refreshable else 0,
                1 if snapshot else 0,
                timestamp,
                timestamp,
                status,
                error,
                timestamp,
            ),
        )

    def set_source_active(self, source_key: str, active: bool) -> None:
        cursor = self.conn.execute(
            """
            UPDATE peval_py_sources
            SET active = ?, updated_at_ms = ?
            WHERE source_key = ?
            """,
            (1 if active else 0, now_ms(), source_key),
        )
        if cursor.rowcount == 0:
            raise ValueError(f"unknown source: {source_key}")
        self.conn.commit()

    def set_source_alias(self, source_key: str, alias: str | None) -> None:
        cursor = self.conn.execute(
            """
            UPDATE peval_py_sources
            SET source_alias = ?, updated_at_ms = ?
            WHERE source_key = ?
            """,
            (alias or None, now_ms(), source_key),
        )
        if cursor.rowcount == 0:
            raise ValueError(f"unknown source: {source_key}")
        self.conn.commit()

    def delete_source(self, source_key: str) -> None:
        row = self.conn.execute(
            """
            SELECT source_key, artifact_dir
            FROM peval_py_sources
            WHERE source_key = ?
            """,
            (source_key,),
        ).fetchone()
        if row is None:
            raise ValueError(f"unknown source: {source_key}")
        artifact_dir = row["artifact_dir"]
        self.conn.execute(
            "DELETE FROM peval_py_refresh_log WHERE source_key = ?",
            (source_key,),
        )
        self.conn.execute("DELETE FROM peval_py_sources WHERE source_key = ?", (source_key,))
        self.conn.commit()
        if artifact_dir:
            remove_artifact_dir(
                self.paths.root,
                self.resolve_artifact_dir(str(artifact_dir)),
            )

    def save_source_notes(
        self,
        source_key: str,
        markdown: str,
        config: ToolConfig,
    ) -> None:
        row = self.conn.execute(
            """
            SELECT *
            FROM peval_py_sources
            WHERE source_key = ?
            """,
            (source_key,),
        ).fetchone()
        if row is None:
            raise ValueError(f"unknown source: {source_key}")
        source = dict(row)
        if not source.get("refreshable") or source.get("snapshot"):
            raise ValueError("notes.md can only be saved for refreshable sources")
        if not source.get("artifact_dir"):
            raise ValueError("notes.md requires a persisted Trial cell")
        write_note_file(
            self.resolve_artifact_dir(str(source["artifact_dir"])) / "notes.md",
            self.paths.root,
            markdown,
        )
        self.refresh_source(source, config)
