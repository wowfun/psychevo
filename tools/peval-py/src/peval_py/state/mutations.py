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
        preserve_existing_source: bool = False,
    ) -> None:
        cell_dir = self.resolve_artifact_dir(artifact_dir)
        existing = self.read_source_state(cell_dir)
        summary = trial_summary(trajectory, meta)
        source_alias = source.get("source_alias")
        if source_alias is None:
            source_alias = existing.get("source_alias")
        source_tags = source.get("source_tags")
        if source_tags is None:
            source_tags = existing.get("source_tags")
        state = {
            "source_key": source_key,
            "kind": source["kind"],
            "adapter": source["adapter"],
            "label": source["label"],
            "input_path": source.get("input_path"),
            "db_path": source.get("db_path"),
            "session_id": source.get("session_id"),
            "source_alias": source_alias,
            "source_tags": self.source_tags_from_state({"source_tags": source_tags}),
            "agent_name": source.get("agent_name"),
            "agent_version": source.get("agent_version"),
            "model": source.get("model"),
            "artifact_dir": artifact_dir,
            "artifact_updated_at_ms": timestamp,
            "trial_key": summary["trial_key"],
            "trial_session_id": summary["trial_session_id"],
            "last_turn_finished_at_ms": summary["last_turn_finished_at_ms"],
            "refreshable": bool(refreshable),
            "active": bool(existing.get("active", True)),
            "snapshot": bool(snapshot),
            "created_at_ms": int(existing.get("created_at_ms") or timestamp),
            "updated_at_ms": timestamp,
            "last_status": status,
            "last_error": error,
            "last_refreshed_at_ms": (
                timestamp if refreshable else existing.get("last_refreshed_at_ms")
            ),
        }
        if preserve_existing_source and isinstance(existing.get("source"), dict):
            state["source"] = existing["source"]
        self.write_source_state(cell_dir, state)

    def set_source_active(self, source_key: str, active: bool) -> None:
        row = self.source_by_key(source_key)
        cell_dir = self.resolve_artifact_dir(str(row["artifact_dir"]))
        state = {**row, **self.read_source_state(cell_dir)}
        state["active"] = bool(active)
        state["updated_at_ms"] = now_ms()
        self.write_source_state(cell_dir, state)

    def set_source_alias(self, source_key: str, alias: str | None) -> None:
        row = self.source_by_key(source_key)
        cell_dir = self.resolve_artifact_dir(str(row["artifact_dir"]))
        state = {**row, **self.read_source_state(cell_dir)}
        state["source_alias"] = alias or None
        state["updated_at_ms"] = now_ms()
        self.write_source_state(cell_dir, state)

    def set_source_tags(self, source_key: str, tags: list[str]) -> None:
        row = self.source_by_key(source_key)
        cell_dir = self.resolve_artifact_dir(str(row["artifact_dir"]))
        state = {**row, **self.read_source_state(cell_dir)}
        state["source_tags"] = self.source_tags_from_state({"source_tags": tags})
        state["updated_at_ms"] = now_ms()
        self.write_source_state(cell_dir, state)

    def delete_source(self, source_key: str) -> None:
        row = self.source_by_key(source_key)
        artifact_dir = row.get("artifact_dir")
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
        source = self.source_by_key(source_key)
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
