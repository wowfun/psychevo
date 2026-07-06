from __future__ import annotations

import json
import shutil
from pathlib import Path
from typing import Any

from peval_py._state.annotations import optional_str
from peval_py._state.artifacts import (
    read_json_object,
    relative_to_root,
    source_key_for_trial_cell_components,
    source_key_for_trial,
    trial_artifacts,
    trial_cell_dir,
    write_json_file,
)
from peval_py.state.constants import (
    SOURCE_STATE_DIR,
    SOURCE_STATE_FILENAME,
    SOURCE_STATE_SCHEMA_VERSION,
    SOURCE_STATUS_MISSING,
    SOURCE_STATUS_OK,
    TRIAL_CELL_SIDECARS,
)
from peval_py.state.summaries import now_ms, trial_summary


SOURCE_PROVENANCE_FIELDS = (
    "kind",
    "adapter",
    "label",
    "input_path",
    "db_path",
    "session_id",
    "agent_name",
    "agent_version",
    "model",
)
SOURCE_PROVENANCE_CONTROL_FIELDS = ("refreshable", "snapshot")


class StateArtifactMixin:
    def store_trial(
        self,
        trajectory: dict[str, Any],
        meta: dict[str, Any],
        eval_slug: str,
        *,
        source: dict[str, Any],
    ) -> str:
        return self.write_trial_artifacts(
            source=source,
            trajectory=trajectory,
            meta=meta,
            eval_slug=eval_slug,
        )

    def write_trial_artifacts(
        self,
        *,
        source: dict[str, Any],
        trajectory: dict[str, Any],
        meta: dict[str, Any],
        eval_slug: str,
    ) -> str:
        artifact_dir = trial_cell_dir(
            self.paths.root,
            eval_slug=eval_slug,
            source=source,
            trajectory=trajectory,
            meta=meta,
        )
        artifacts = trial_artifacts(artifact_dir)
        write_json_file(artifacts.trajectory_path, trajectory)
        write_json_file(artifacts.meta_path, meta)
        return relative_to_root(self.paths.root, artifact_dir)

    def copy_trial_sidecars(self, source_cell_dir: Path, artifact_dir: str) -> None:
        source_cell_dir = source_cell_dir.expanduser().resolve()
        target_cell_dir = self.resolve_artifact_dir(artifact_dir)
        for filename in TRIAL_CELL_SIDECARS:
            source_path = source_cell_dir / filename
            if not source_path.is_file():
                continue
            target_path = target_cell_dir / filename
            if source_path.resolve() == target_path.resolve():
                continue
            target_path.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(source_path, target_path)

    def read_trial_artifacts(self, row: dict[str, Any]) -> dict[str, dict[str, Any]]:
        artifact_dir = row.get("artifact_dir")
        if not artifact_dir:
            raise ValueError(f"trial has no artifact directory: {row.get('source_key')}")
        if self.artifact_missing(row):
            raise ValueError(self.missing_artifact_message(row))
        artifacts = trial_artifacts(self.resolve_artifact_dir(str(artifact_dir)))
        return {
            "trajectory": read_json_object(artifacts.trajectory_path),
            "meta": read_json_object(artifacts.meta_path),
        }

    def resolve_artifact_dir(self, artifact_dir: str) -> Path:
        path = Path(artifact_dir)
        if not path.is_absolute():
            path = self.paths.root / path
        return path.resolve()

    def artifact_missing(self, row: dict[str, Any]) -> bool:
        artifact_dir = row.get("artifact_dir")
        if not artifact_dir:
            return False
        artifacts = trial_artifacts(self.resolve_artifact_dir(str(artifact_dir)))
        return not (artifacts.trajectory_path.is_file() and artifacts.meta_path.is_file())

    def missing_artifact_message(self, row: dict[str, Any]) -> str:
        artifact_dir = row.get("artifact_dir")
        return f"Trial cell artifacts not found: {artifact_dir or row.get('source_key')}"

    def update_source_summary(
        self,
        source_key: str,
        trajectory: dict[str, Any],
        meta: dict[str, Any],
    ) -> None:
        del trajectory, meta
        row = self.source_by_key(source_key)
        state = self.read_source_state(self.resolve_artifact_dir(str(row["artifact_dir"])))
        state["updated_at_ms"] = now_ms()
        self.write_source_state(self.resolve_artifact_dir(str(row["artifact_dir"])), state)

    def discover_trial_cell_dirs(self, eval_slug: str) -> list[Path]:
        run_root = self.paths.root / "runs" / eval_slug
        if not run_root.is_dir():
            return []
        cells: list[Path] = []
        for agent_dir in sorted(run_root.iterdir()):
            if not agent_dir.is_dir():
                continue
            for session_dir in sorted(agent_dir.iterdir()):
                if not session_dir.is_dir():
                    continue
                for cell_dir in sorted(session_dir.iterdir()):
                    if not cell_dir.is_dir():
                        continue
                    artifacts = trial_artifacts(cell_dir)
                    if artifacts.trajectory_path.is_file() and artifacts.meta_path.is_file():
                        cells.append(cell_dir)
        return cells

    def discover_source_cell_dirs(self) -> list[Path]:
        run_root = self.paths.root / "runs"
        if not run_root.is_dir():
            return []
        cells: list[Path] = []
        for agent_dir in sorted(run_root.rglob("agent")):
            if not agent_dir.is_dir():
                continue
            cell_dir = agent_dir.parent.resolve()
            artifacts = trial_artifacts(cell_dir)
            if artifacts.trajectory_path.is_file() and artifacts.meta_path.is_file():
                append_unique_path(cells, cell_dir)
        for state_path in sorted(run_root.rglob(f"{SOURCE_STATE_DIR}/{SOURCE_STATE_FILENAME}")):
            append_unique_path(cells, state_path.parent.parent.resolve())
        return cells

    def source_row_for_artifact_cell(
        self,
        cell_dir: Path,
        trajectory: dict[str, Any],
        meta: dict[str, Any],
    ) -> dict[str, Any]:
        agent = trajectory.get("agent") if isinstance(trajectory.get("agent"), dict) else {}
        adapter = optional_str(meta.get("adapter") or agent.get("name")) or "artifact"
        return {
            "kind": "trial-artifact",
            "adapter": adapter,
            "label": relative_to_root(self.paths.root, cell_dir),
            "input_path": str(cell_dir.resolve()),
            "db_path": None,
            "session_id": optional_str(trajectory.get("session_id")),
            "source_alias": None,
            "source_tags": [],
            "agent_name": optional_str(agent.get("name")),
            "agent_version": None,
            "model": optional_str(agent.get("model_name")),
        }

    def source_row_from_cell_dir(self, cell_dir: Path) -> dict[str, Any]:
        cell_dir = cell_dir.expanduser().resolve()
        state = self.read_source_state(cell_dir)
        stored_source = self.source_provenance_from_state(state)
        artifact_dir = relative_to_root(self.paths.root, cell_dir)
        artifacts = trial_artifacts(cell_dir)
        has_artifacts = artifacts.trajectory_path.is_file() and artifacts.meta_path.is_file()
        trajectory: dict[str, Any] | None = None
        meta: dict[str, Any] | None = None
        if has_artifacts:
            trajectory = read_json_object(artifacts.trajectory_path)
            meta = read_json_object(artifacts.meta_path)
            source = self.source_row_for_artifact_cell(cell_dir, trajectory, meta)
            source.update(
                {
                    key: stored_source[key]
                    for key in SOURCE_PROVENANCE_FIELDS
                    if key in stored_source
                }
            )
            source["source_alias"] = optional_str(state.get("source_alias"))
            source["source_tags"] = self.source_tags_from_state(state)
            eval_slug = self.eval_slug_for_cell_dir(cell_dir)
            source_key = source_key_for_trial(eval_slug, source, trajectory, meta)
            summary = trial_summary(trajectory, meta)
            status = optional_str(state.get("last_status")) or SOURCE_STATUS_OK
            error = state.get("last_error")
        else:
            identity = self.cell_path_identity(cell_dir)
            source = self.missing_source_row(
                artifact_dir,
                identity,
                stored_source,
                state,
            )
            source_key = (
                self.source_key_for_cell_identity(identity)
                or optional_str(state.get("source_key"))
                or ""
            )
            summary = self.missing_trial_summary(identity, source, state)
            status = SOURCE_STATUS_MISSING
            error = self.missing_artifact_message({"artifact_dir": artifact_dir, **state})
        timestamp = self.artifact_updated_at_ms(cell_dir)
        refreshable = self.refreshable_from_state(state, stored_source)
        row = {
            "source_key": source_key,
            **source,
            "artifact_dir": artifact_dir,
            "artifact_updated_at_ms": timestamp,
            **summary,
            "refreshable": refreshable,
            "active": bool(state.get("active", True)),
            "snapshot": self.snapshot_from_state(state, stored_source, refreshable),
            "created_at_ms": int(state.get("created_at_ms") or timestamp),
            "updated_at_ms": int(state.get("updated_at_ms") or timestamp),
            "last_status": status,
            "last_error": error,
            "last_refreshed_at_ms": state.get("last_refreshed_at_ms"),
        }
        return row

    def source_provenance_from_state(self, state: dict[str, Any]) -> dict[str, Any]:
        source = state.get("source")
        if isinstance(source, dict):
            return {
                key: source.get(key)
                for key in [*SOURCE_PROVENANCE_FIELDS, *SOURCE_PROVENANCE_CONTROL_FIELDS]
                if source.get(key) is not None
            }
        return {}

    def refreshable_from_state(
        self,
        state: dict[str, Any],
        source: dict[str, Any],
    ) -> bool:
        if source.get("refreshable") is not None:
            return bool(source["refreshable"])
        return False

    def snapshot_from_state(
        self,
        state: dict[str, Any],
        source: dict[str, Any],
        refreshable: bool,
    ) -> bool:
        if source.get("snapshot") is not None:
            return bool(source["snapshot"])
        return not refreshable

    def cell_path_identity(self, cell_dir: Path) -> dict[str, str]:
        try:
            relative = cell_dir.resolve().relative_to(self.paths.root.resolve())
        except ValueError:
            return {}
        parts = relative.parts
        if len(parts) < 5 or parts[0] != "runs":
            return {}
        return {
            "eval_slug": parts[1],
            "agent_id": parts[2],
            "session_id": parts[3],
            "cell_key": parts[4],
        }

    def source_key_for_cell_identity(self, identity: dict[str, str]) -> str | None:
        if not identity:
            return None
        return source_key_for_trial_cell_components(
            eval_slug=identity["eval_slug"],
            agent_id=identity["agent_id"],
            session_id=identity["session_id"],
            cell_key=identity["cell_key"],
        )

    def missing_source_row(
        self,
        artifact_dir: str,
        identity: dict[str, str],
        source: dict[str, Any],
        state: dict[str, Any],
    ) -> dict[str, Any]:
        adapter = optional_str(source.get("adapter") or identity.get("agent_id")) or "artifact"
        session_id = optional_str(source.get("session_id") or identity.get("session_id"))
        agent_name = optional_str(source.get("agent_name") or identity.get("agent_id"))
        return {
            "kind": source.get("kind") or "trial-artifact",
            "adapter": adapter,
            "label": source.get("label") or artifact_dir,
            "input_path": source.get("input_path"),
            "db_path": source.get("db_path"),
            "session_id": session_id,
            "source_alias": optional_str(state.get("source_alias")),
            "source_tags": self.source_tags_from_state(state),
            "agent_name": agent_name,
            "agent_version": source.get("agent_version"),
            "model": source.get("model"),
        }

    def source_tags_from_state(self, state: dict[str, Any]) -> list[str]:
        raw_tags = state.get("source_tags")
        if not isinstance(raw_tags, list):
            return []
        tags: list[str] = []
        seen: set[str] = set()
        for raw_tag in raw_tags:
            tag = optional_str(raw_tag)
            if not tag or tag in seen:
                continue
            seen.add(tag)
            tags.append(tag)
        return tags

    def missing_trial_summary(
        self,
        identity: dict[str, str],
        source: dict[str, Any],
        state: dict[str, Any],
    ) -> dict[str, Any]:
        return {
            "trial_key": identity.get("cell_key"),
            "trial_session_id": (
                source.get("session_id")
                or identity.get("session_id")
            ),
            "last_turn_finished_at_ms": None,
        }

    def eval_slug_for_cell_dir(self, cell_dir: Path) -> str:
        try:
            relative = cell_dir.resolve().relative_to(self.paths.root.resolve())
        except ValueError:
            return "default"
        parts = relative.parts
        if len(parts) >= 5 and parts[0] == "runs":
            return parts[1]
        return "default"

    def source_state_path(self, cell_dir: Path) -> Path:
        return cell_dir / SOURCE_STATE_DIR / SOURCE_STATE_FILENAME

    def read_source_state(self, cell_dir: Path) -> dict[str, Any]:
        path = self.source_state_path(cell_dir)
        if not path.is_file():
            return {}
        try:
            parsed = json.loads(path.read_text(encoding="utf-8"))
        except json.JSONDecodeError as exc:
            raise ValueError(f"failed to parse {path}: {exc}") from exc
        if not isinstance(parsed, dict):
            raise ValueError(f"{path} must contain a JSON object")
        if parsed.get("schema_version") != SOURCE_STATE_SCHEMA_VERSION:
            return {}
        return parsed

    def write_source_state(self, cell_dir: Path, state: dict[str, Any]) -> None:
        payload = self.compact_source_state(state)
        write_json_file(self.source_state_path(cell_dir), payload)

    def compact_source_state(self, state: dict[str, Any]) -> dict[str, Any]:
        timestamp = self.state_timestamp(
            state.get("updated_at_ms")
            or state.get("last_refreshed_at_ms")
            or state.get("created_at_ms")
            or state.get("artifact_updated_at_ms")
            or now_ms()
        )
        created_at = self.state_timestamp(state.get("created_at_ms") or timestamp)
        payload: dict[str, Any] = {
            "schema_version": SOURCE_STATE_SCHEMA_VERSION,
            "created_at_ms": created_at,
            "updated_at_ms": timestamp,
        }
        source_alias = optional_str(state.get("source_alias"))
        if source_alias:
            payload["source_alias"] = source_alias
        source_tags = self.source_tags_from_state(state)
        if source_tags:
            payload["source_tags"] = source_tags
        if not bool(state.get("active", True)):
            payload["active"] = False
        status = optional_str(state.get("last_status"))
        if status and status != SOURCE_STATUS_OK:
            payload["last_status"] = status
        error = optional_str(state.get("last_error"))
        if error:
            payload["last_error"] = error
        if state.get("last_refreshed_at_ms") is not None:
            payload["last_refreshed_at_ms"] = self.state_timestamp(
                state["last_refreshed_at_ms"]
            )
        source = self.compact_source_provenance(state)
        if source:
            payload["source"] = source
        return payload

    def compact_source_provenance(self, state: dict[str, Any]) -> dict[str, Any]:
        source = self.source_provenance_for_write(state)
        if not self.should_persist_source_provenance(source):
            return {}
        compact: dict[str, Any] = {}
        for key in SOURCE_PROVENANCE_FIELDS:
            value = source.get(key)
            if value is not None:
                compact[key] = value
        if source.get("refreshable"):
            compact["refreshable"] = True
        return compact

    def source_provenance_for_write(self, state: dict[str, Any]) -> dict[str, Any]:
        source = self.source_provenance_from_state(state)
        if source:
            return source
        return {
            key: state.get(key)
            for key in [*SOURCE_PROVENANCE_FIELDS, *SOURCE_PROVENANCE_CONTROL_FIELDS]
            if state.get(key) is not None
        }

    def should_persist_source_provenance(self, source: dict[str, Any]) -> bool:
        kind = optional_str(source.get("kind")) or "trial-artifact"
        if kind != "trial-artifact":
            return True
        if source.get("refreshable"):
            return True
        if source.get("db_path"):
            return True
        return False

    def state_timestamp(self, value: Any) -> int:
        try:
            return int(value)
        except (TypeError, ValueError):
            return now_ms()

    def artifact_updated_at_ms(self, cell_dir: Path) -> int:
        artifacts = trial_artifacts(cell_dir)
        mtimes = [
            path.stat().st_mtime_ns // 1_000_000
            for path in [artifacts.trajectory_path, artifacts.meta_path]
            if path.is_file()
        ]
        return max(mtimes) if mtimes else 0


def append_unique_path(paths: list[Path], candidate: Path) -> None:
    resolved = candidate.resolve()
    if not any(path == resolved for path in paths):
        paths.append(resolved)
