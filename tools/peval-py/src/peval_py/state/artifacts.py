from __future__ import annotations

import shutil
from pathlib import Path
from typing import Any

from peval_py._state.annotations import optional_str
from peval_py._state.artifacts import (
    read_json_object,
    relative_to_root,
    trial_artifacts,
    trial_cell_dir,
    write_json_file,
)
from peval_py.state.constants import TRIAL_CELL_SIDECARS
from peval_py.state.summaries import trial_summary


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
        summary = trial_summary(trajectory, meta)
        self.conn.execute(
            """
            UPDATE peval_py_sources
            SET trial_key = ?, trial_session_id = ?, last_turn_finished_at_ms = ?
            WHERE source_key = ?
            """,
            (
                summary["trial_key"],
                summary["trial_session_id"],
                summary["last_turn_finished_at_ms"],
                source_key,
            ),
        )

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
            "agent_name": optional_str(agent.get("name")),
            "agent_version": None,
            "model": optional_str(agent.get("model_name")),
        }
