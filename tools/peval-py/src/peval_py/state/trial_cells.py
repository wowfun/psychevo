from __future__ import annotations

from pathlib import Path

from peval_py._state.annotations import optional_str
from peval_py._state.artifacts import (
    AGENT_DIR,
    DEFAULT_ANALYSIS_EVAL_SLUG,
    artifact_segment,
    read_json_object,
    trial_artifacts,
)
from peval_py.config import ToolConfig
from peval_py.models import LoadedSession


def discover_complete_trial_cell_dirs(path: Path) -> list[Path]:
    start = path.expanduser()
    if start.exists():
        start = start.resolve()
    direct = complete_trial_cell_dir_for_path(start)
    if direct is not None:
        return [direct]
    if not start.is_dir():
        return []
    cells: list[Path] = []
    for agent_dir in sorted(start.rglob(AGENT_DIR)):
        if not agent_dir.is_dir():
            continue
        cell_dir = agent_dir.parent
        artifacts = trial_artifacts(cell_dir)
        if artifacts.trajectory_path.is_file() and artifacts.meta_path.is_file():
            append_unique_path(cells, cell_dir.resolve())
    return cells


def complete_trial_cell_dir_for_path(path: Path) -> Path | None:
    current = path if path.is_dir() else path.parent
    for candidate in [current, *current.parents]:
        artifacts = trial_artifacts(candidate)
        if artifacts.trajectory_path.is_file() and artifacts.meta_path.is_file():
            return candidate.resolve()
    return None


def append_unique_path(paths: list[Path], candidate: Path) -> None:
    resolved = candidate.resolve()
    if not any(path == resolved for path in paths):
        paths.append(resolved)


def loaded_trial_cell_import_session(
    cell_dir: Path,
    config: ToolConfig,
) -> LoadedSession:
    cell_dir = cell_dir.expanduser().resolve()
    artifacts = trial_artifacts(cell_dir)
    trajectory = read_json_object(artifacts.trajectory_path)
    meta = read_json_object(artifacts.meta_path)
    agent = trajectory.get("agent") if isinstance(trajectory.get("agent"), dict) else {}
    adapter = optional_str(meta.get("adapter") or agent.get("name")) or "artifact"
    return LoadedSession(
        records=None,
        input_label=trial_cell_import_label(cell_dir),
        adapter_id=adapter,
        input_path=str(cell_dir),
        session_hint=optional_str(trajectory.get("session_id")),
        agent_name=optional_str(agent.get("name")),
        model=optional_str(agent.get("model_name")),
        source_kind="trial-artifact",
        snapshot_trajectory=trajectory,
        snapshot_meta=meta,
        artifact_eval_slug=(
            infer_eval_slug_from_trial_cell_dir(cell_dir) or config.analysis_eval_slug
        ),
    )


def trial_cell_import_label(cell_dir: Path) -> str:
    parts = cell_dir.parts
    for index, part in enumerate(parts):
        if part == "runs" and len(parts) - index >= 5:
            return Path(*parts[index:]).as_posix()
    return str(cell_dir)


def infer_eval_slug_from_trial_cell_dir(cell_dir: Path) -> str | None:
    parts = cell_dir.parts
    for index in range(len(parts) - 1, -1, -1):
        if parts[index] == "runs" and len(parts) - index >= 5:
            return artifact_segment(parts[index + 1], DEFAULT_ANALYSIS_EVAL_SLUG)
    return None
