from __future__ import annotations

import argparse
import json
import os
import sqlite3
from pathlib import Path
from typing import Any

import peval_py.config as path_config
from peval_py._state.annotations import (
    meta_with_source_alias,
    source_report_with_current_annotations,
    uniquify_trial_keys,
)
from peval_py.models import LoadedSession

TRIAL_TRAJECTORY_RELATIVE_PATH = Path("agent") / "trajectory.json"
TRIAL_META_RELATIVE_PATH = Path("agent") / "trajectory_meta.json"
TRIAL_CELL_GLOB_SUFFIXES = ("/**/*", "\\**\\*", "/**", "\\**")

def is_workspace_state_db_input(raw_db: str, config: object | None) -> bool:
    state_db_path = getattr(config, "workspace_state_db_path", None)
    return bool(state_db_path and same_local_path(str(raw_db), str(state_db_path)))


def workspace_snapshot_sources_for_input(
    raw_db: str,
    config: object | None,
) -> list[dict[str, Any]]:
    if not is_workspace_state_db_input(raw_db, config):
        raise ValueError(peval_py_state_db_error(raw_db))
    workspace_root = getattr(config, "workspace_root", None)
    if not workspace_root:
        raise ValueError(peval_py_state_db_error(raw_db))
    try:
        from peval_py.state import open_workspace_state_readonly

        store = open_workspace_state_readonly(str(workspace_root))
        try:
            rows = store.source_payload()
        finally:
            store.close()
    except sqlite3.Error as exc:
        raise ValueError(f"failed to read peval-py workspace state DB: {exc}") from exc
    return [row for row in rows if row.get("artifact_dir")]


def loaded_trial_cell_artifact_session(
    raw_path: str,
    config: object | None,
) -> LoadedSession | None:
    cell_path = canonical_trial_cell_path_for_input(raw_path)
    if cell_path is None:
        return None
    artifacts = trial_cell_artifact_paths(cell_path)
    if artifacts is None:
        raise_missing_trial_cell_artifacts(cell_path)

    workspace_row = workspace_snapshot_source_for_artifact_path(cell_path, config)
    if workspace_row is not None:
        return load_workspace_snapshot_sessions_from_rows(
            [workspace_row],
            str(cell_path),
            config,
            artifact_cell_path=cell_path,
        )[0]

    trajectory_path, meta_path = artifacts
    trajectory = read_json_object(trajectory_path)
    meta = read_json_object(meta_path)
    meta = dict(meta)
    meta = meta_with_artifact_ref(meta, cell_path, config)
    meta.setdefault(
        "data_ref",
        {
            "kind": "trial-artifact",
            "label": cell_path.name,
            "path": str(cell_path),
        },
    )
    agent = trajectory.get("agent") if isinstance(trajectory.get("agent"), dict) else {}
    adapter_id = str(meta.get("adapter") or agent.get("name") or "artifact")
    return LoadedSession(
        records=None,
        input_label=cell_path.name,
        adapter_id=adapter_id,
        input_path=str(cell_path),
        session_hint=optional_text(trajectory.get("session_id")),
        source_kind="trial-artifact",
        snapshot_trajectory=trajectory,
        snapshot_meta=meta,
    )


def canonical_trial_cell_paths_for_inputs(raw_paths: list[str]) -> list[Path]:
    cells: list[Path] = []
    for raw_path in raw_paths:
        cell_path = canonical_trial_cell_path_for_input(
            raw_path,
            raise_on_malformed=False,
        )
        if cell_path is None:
            continue
        if not any(same_local_path(str(cell_path), str(item)) for item in cells):
            cells.append(cell_path)
    return cells


def canonical_trial_cell_path_for_input(
    raw_path: str,
    *,
    raise_on_malformed: bool = True,
) -> Path | None:
    path = resolved_local_path(strip_trial_cell_glob_suffix(raw_path))
    if path is None:
        return None
    for candidate in [path, *path.parents]:
        if trial_cell_artifact_paths(candidate) is not None:
            return candidate
    if raise_on_malformed:
        malformed = malformed_trial_cell_candidate(path)
        if malformed is not None:
            raise_missing_trial_cell_artifacts(malformed)
    return None


def strip_trial_cell_glob_suffix(raw_path: str) -> str:
    text = str(raw_path).strip()
    for suffix in TRIAL_CELL_GLOB_SUFFIXES:
        if text.endswith(suffix):
            return text[: -len(suffix)]
    return text


def trial_cell_artifact_paths(cell_path: Path) -> tuple[Path, Path] | None:
    if not cell_path.is_dir():
        return None
    trajectory_path = cell_path / TRIAL_TRAJECTORY_RELATIVE_PATH
    meta_path = cell_path / TRIAL_META_RELATIVE_PATH
    if trajectory_path.is_file() and meta_path.is_file():
        return trajectory_path, meta_path
    return None


def malformed_trial_cell_candidate(path: Path) -> Path | None:
    for candidate in [path, *path.parents]:
        if looks_like_trial_cell_artifact_path(candidate):
            return candidate
    return None


def raise_missing_trial_cell_artifacts(cell_path: Path) -> None:
    required = " and ".join(
        [
            TRIAL_TRAJECTORY_RELATIVE_PATH.as_posix(),
            TRIAL_META_RELATIVE_PATH.as_posix(),
        ]
    )
    raise ValueError(
        f"{cell_path} looks like a Trial cell artifact directory but "
        f"is missing {required}"
    )


def looks_like_trial_cell_artifact_path(path: Path) -> bool:
    if path.exists() and not path.is_dir():
        return False
    parts = path.parts
    return any(
        part == "runs" and len(parts) - index == 5
        for index, part in enumerate(parts)
    )


def infer_workspace_root_from_trial_cell_paths(raw_paths: list[str]) -> Path | None:
    roots: list[Path] = []
    for raw_path in raw_paths:
        root = infer_workspace_root_from_trial_cell_path(raw_path)
        if root is not None and not any(
            same_local_path(str(root), str(item)) for item in roots
        ):
            roots.append(root)
    if len(roots) > 1:
        joined = ", ".join(str(root) for root in roots)
        raise ValueError(
            "path inputs belong to different inferred peval-py workspace roots: "
            f"{joined}; pass one workspace at a time"
        )
    return roots[0] if roots else None


def infer_workspace_root_from_trial_cell_path(raw_path: str) -> Path | None:
    path = canonical_trial_cell_path_for_input(raw_path, raise_on_malformed=False)
    if path is None:
        path = resolved_local_path(strip_trial_cell_glob_suffix(raw_path))
    if path is None:
        return None
    for candidate in [path, *path.parents]:
        config_path = candidate / "peval-py.toml"
        if not config_path.is_file():
            continue
        try:
            relative = path.relative_to(candidate)
        except ValueError:
            continue
        parts = relative.parts
        if len(parts) == 5 and parts[0] == "runs":
            return candidate.resolve()
    return None


def workspace_snapshot_source_for_artifact_path(
    cell_path: Path,
    config: object | None,
) -> dict[str, Any] | None:
    workspace_root = getattr(config, "workspace_root", None)
    state_db_path = getattr(config, "workspace_state_db_path", None)
    if not workspace_root or not state_db_path or not Path(state_db_path).is_file():
        return None
    rows = workspace_snapshot_sources_for_input(str(state_db_path), config)
    matches: list[dict[str, Any]] = []
    for row in rows:
        artifact_path = row_artifact_path(row, workspace_root)
        if artifact_path is not None and same_local_path(
            str(artifact_path),
            str(cell_path),
        ):
            matches.append(row)
    if len(matches) > 1:
        raise ValueError(
            f"multiple saved sources reference Trial cell artifact directory: {cell_path}"
        )
    return matches[0] if matches else None


def row_artifact_path(row: dict[str, Any], workspace_root: object) -> Path | None:
    raw_artifact_dir = row.get("artifact_dir")
    if not raw_artifact_dir:
        return None
    artifact_path = Path(str(raw_artifact_dir)).expanduser()
    if not artifact_path.is_absolute() and not path_config.is_windows_absolute_like_path(
        str(raw_artifact_dir)
    ):
        artifact_path = Path(str(workspace_root)).expanduser() / artifact_path
    return artifact_path


def meta_with_artifact_ref(
    meta: dict[str, Any],
    cell_path: Path,
    config: object | None,
    *,
    source_key: str | None = None,
) -> dict[str, Any]:
    copy = dict(meta)
    copy["artifact_ref"] = artifact_ref_for_cell_path(
        cell_path,
        config,
        source_key=source_key,
    )
    return copy


def artifact_ref_for_cell_path(
    cell_path: Path,
    config: object | None,
    *,
    source_key: str | None = None,
) -> dict[str, Any]:
    ref: dict[str, Any] = {
        "kind": "trial-cell-artifact",
        "path": display_local_path(cell_path),
    }
    workspace_root = getattr(config, "workspace_root", None)
    if workspace_root:
        try:
            ref["workspace_relative_path"] = (
                cell_path.resolve()
                .relative_to(Path(str(workspace_root)).expanduser().resolve())
                .as_posix()
            )
        except ValueError:
            pass
    if source_key:
        ref["source_key"] = str(source_key)
    return ref


def display_local_path(path: Path) -> str:
    try:
        return Path(os.path.relpath(path, Path.cwd())).as_posix()
    except ValueError:
        return str(path)


def read_json_object(path: Path) -> dict[str, Any]:
    try:
        parsed = json.loads(path.read_text(encoding="utf-8"))
    except json.JSONDecodeError as exc:
        raise ValueError(f"failed to parse {path}: {exc}") from exc
    if not isinstance(parsed, dict):
        raise ValueError(f"{path} must contain a JSON object")
    return parsed


def load_workspace_snapshot_sessions(
    args: argparse.Namespace,
    db_index: int,
    raw_db: str,
    selectors: list[str],
    config: object | None,
) -> list[LoadedSession]:
    rows = workspace_snapshot_sources_for_input(raw_db, config)
    selected = select_workspace_snapshot_sources(
        rows,
        selectors,
        command=str(getattr(args, "command", "")),
        db_index=db_index,
    )
    if not selected:
        return []
    return load_workspace_snapshot_sessions_from_rows(selected, raw_db, config)


def load_workspace_snapshot_sessions_from_rows(
    selected: list[dict[str, Any]],
    raw_input: str,
    config: object | None,
    *,
    artifact_cell_path: Path | None = None,
) -> list[LoadedSession]:
    workspace_root = getattr(config, "workspace_root", None)
    if not workspace_root:
        raise ValueError(peval_py_state_db_error(raw_input))
    from peval_py.state import open_workspace_state_readonly

    try:
        store = open_workspace_state_readonly(str(workspace_root))
        try:
            artifacts = [store.read_trial_artifacts(row) for row in selected]
            trajectories = [item["trajectory"] for item in artifacts]
            metas = uniquify_trial_keys(
                [
                    meta_with_source_alias(item["meta"], row.get("source_alias"))
                    for row, item in zip(selected, artifacts, strict=True)
                ]
            )
            if artifact_cell_path is not None:
                metas = [
                    meta_with_artifact_ref(
                        meta,
                        artifact_cell_path,
                        config,
                        source_key=optional_text(row.get("source_key")),
                    )
                    for row, meta in zip(selected, metas, strict=True)
                ]
            reports = [
                source_report_with_current_annotations(
                    row,
                    trajectory,
                    meta,
                    config,
                )
                for row, trajectory, meta in zip(
                    selected,
                    trajectories,
                    metas,
                    strict=True,
                )
            ]
        finally:
            store.close()
    except sqlite3.Error as exc:
        raise ValueError(f"failed to read peval-py workspace state DB: {exc}") from exc

    loaded: list[LoadedSession] = []
    for row, trajectory, meta, source_report in zip(
        selected,
        trajectories,
        metas,
        reports,
        strict=True,
    ):
        source_key = str(row.get("source_key") or "")
        session_id = workspace_snapshot_session_id(row)
        label = str(
            row.get("source_alias")
            or row.get("label")
            or session_id
            or source_key
            or "workspace-snapshot"
        )
        loaded.append(
            LoadedSession(
                records=None,
                input_label=label,
                adapter_id=str(row.get("adapter") or meta.get("adapter") or "snapshot"),
                input_path=str(raw_input),
                session_hint=session_id,
                agent_name=row.get("agent_name"),
                agent_version=row.get("agent_version"),
                model=row.get("model"),
                source_alias=row.get("source_alias"),
                source_kind="workspace-snapshot",
                workspace_source_key=source_key,
                snapshot_trajectory=trajectory,
                snapshot_meta=meta,
                snapshot_source_report=source_report,
            )
        )
    return loaded


def select_workspace_snapshot_sources(
    rows: list[dict[str, Any]],
    selectors: list[str],
    *,
    command: str,
    db_index: int,
) -> list[dict[str, Any]]:
    if not selectors:
        active = [row for row in rows if bool(row.get("active"))]
        if command == "export" and len(active) != 1:
            raise ValueError(
                "export trajectory from a workspace state DB requires exactly one "
                f"active saved source or an explicit -s selector (active saved sources: {len(active)})"
            )
        return active
    return [
        resolve_workspace_snapshot_selector(rows, selector, db_index)
        for selector in selectors
    ]


def resolve_workspace_snapshot_selector(
    rows: list[dict[str, Any]],
    selector: str,
    db_index: int,
) -> dict[str, Any]:
    text = str(selector)
    if text.startswith("#"):
        return workspace_snapshot_by_index(rows, text[1:])
    direct = [row for row in rows if str(row.get("source_key") or "") == text]
    if len(direct) == 1:
        return direct[0]
    matches: list[dict[str, Any]] = []
    for row in rows:
        values = {
            optional_text(row.get("session_id")),
            optional_text(row.get("trial_session_id")),
            optional_text(row.get("trial_key")),
        }
        if text in values:
            matches.append(row)
    if len(matches) == 1:
        return matches[0]
    if len(matches) > 1:
        raise ValueError(
            f"ambiguous saved source selector for d{db_index}: {text}; "
            "use source_key or #N"
        )
    raise ValueError(
        f"unknown saved source selector for d{db_index}: {text}; "
        "use --list to see source_key and #N values"
    )


def workspace_snapshot_by_index(
    rows: list[dict[str, Any]],
    raw_index: str,
) -> dict[str, Any]:
    if not raw_index.isdigit():
        raise ValueError(f"saved source index must be a positive integer: #{raw_index}")
    index = int(raw_index)
    if index < 1 or index > len(rows):
        raise ValueError(
            f"saved source index out of range: #{index} "
            f"(available saved sources: {len(rows)})"
        )
    return rows[index - 1]


def workspace_snapshot_session_id(row: dict[str, Any]) -> str | None:
    return optional_text(row.get("trial_session_id")) or optional_text(row.get("session_id"))


def optional_text(value: Any) -> str | None:
    if value is None:
        return None
    text = str(value)
    return text if text else None


def same_local_path(left: str, right: str) -> bool:
    left_path = resolved_local_path(left)
    right_path = resolved_local_path(right)
    return left_path is not None and right_path is not None and left_path == right_path


def resolved_local_path(value: str) -> Path | None:
    text = str(value).strip()
    if not text:
        return None
    if path_config.is_windows_absolute_like_path(text):
        return resolved_windows_absolute_like_path(text)
    path = Path(text).expanduser()
    if not path.is_absolute():
        path = Path.cwd() / path
    return path.resolve()


def resolved_windows_absolute_like_path(text: str) -> Path | None:
    resolved = path_config.resolve_windows_absolute_like_path(text)
    if os.name == "nt":
        return Path(resolved).expanduser().resolve()
    mapped = path_config.windows_drive_mount_path(
        text,
        path_config.WINDOWS_DRIVE_MOUNT_ROOT,
    )
    if mapped is None or not mapped.exists():
        return None
    return mapped.resolve()


def is_peval_py_state_db_input(raw_db: str) -> bool:
    path = resolved_local_path(raw_db)
    if path is None:
        return False
    if path.is_dir():
        path = path / "state.db"
    if not path.is_file():
        return False
    try:
        conn = sqlite3.connect(f"file:{path}?mode=ro", uri=True)
        try:
            rows = conn.execute(
                """
                SELECT name
                FROM sqlite_master
                WHERE type = 'table'
                  AND name IN ('peval_py_sources', 'peval_py_refresh_log')
                """
            ).fetchall()
        finally:
            conn.close()
    except sqlite3.Error:
        return False
    names = {str(row[0]) for row in rows}
    return "peval_py_sources" in names or "peval_py_refresh_log" in names


def peval_py_state_db_error(raw_db: str) -> str:
    return (
        f"{raw_db} is a peval-py workspace state DB, not an adapter source DB; "
        "pass explicit -r <workspace> with the workspace state DB to read saved "
        "workspace snapshots, pass -d @adapter for a configured adapter default "
        "DB, or pass an adapter DB path directly"
    )
