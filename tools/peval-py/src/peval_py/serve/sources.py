from __future__ import annotations

from dataclasses import dataclass, replace
from pathlib import Path
from types import SimpleNamespace
from typing import Any

from peval_py.config import ToolConfig
from peval_py.inputs import (
    AdapterAssignments,
    LoadedInputs,
    LoadedSession,
    load_inputs,
    parse_adapter_assignments,
)
from peval_py.serve.errors import HttpError
from peval_py.serve.payloads import (
    adapter_for_db_inspect,
    adapter_override_payload,
    optional_string,
    source_args_from_payload,
    split_source_path_lines,
    source_path_values,
)
from peval_py.session_select import list_adapter_sessions
from peval_py.state import (
    ServeStateStore,
    discover_complete_trial_cell_dirs,
    loaded_trial_cell_import_session,
)


@dataclass(frozen=True)
class AddSourceResult:
    keys: list[str]
    import_results: list[dict[str, Any]] | None = None


def load_serve_inputs(
    args: Any,
    adapter_assignments: AdapterAssignments,
    config: ToolConfig | None = None,
) -> LoadedInputs:
    return load_inputs(args, adapter_assignments, require_sources=False, config=config)


def add_source_payload(
    store: ServeStateStore,
    config: ToolConfig,
    payload: dict[str, Any],
) -> AddSourceResult:
    path_lines = path_batch_lines(payload)
    if len(path_lines) > 1:
        return add_path_batch_sources(store, config, payload, path_lines)
    source_args = source_args_from_payload(store, payload)
    raw_adapter = adapter_override_payload(payload)
    assignments = parse_adapter_assignments(
        [raw_adapter] if raw_adapter else [],
        config.adapter,
    )
    loaded = load_payload_sources(source_args, assignments, config)
    loaded = apply_payload_alias(loaded, optional_string(payload.get("alias")))
    keys = store.import_loaded_sources(loaded, config)
    return AddSourceResult(keys=keys)


def path_batch_lines(payload: dict[str, Any]) -> list[str]:
    raw = optional_string(payload.get("path"))
    if raw is None:
        return []
    return split_source_path_lines(raw)


def add_path_batch_sources(
    store: ServeStateStore,
    config: ToolConfig,
    payload: dict[str, Any],
    path_lines: list[str],
) -> AddSourceResult:
    raw_adapter = adapter_override_payload(payload)
    assignments = parse_adapter_assignments(
        [raw_adapter] if raw_adapter else [],
        config.adapter,
    )
    all_keys: list[str] = []
    results: list[dict[str, Any]] = []
    for line in path_lines:
        try:
            source_args = source_args_from_payload(store, {**payload, "path": line})
            loaded = load_payload_sources(source_args, assignments, config)
            loaded = apply_payload_alias(loaded, optional_string(payload.get("alias")))
            keys = store.import_loaded_sources(loaded, config)
            all_keys.extend(keys)
            results.append({"path": line, "status": "ok", "source_keys": keys})
        except Exception as exc:  # noqa: BLE001 - per-line batch result.
            results.append({"path": line, "status": "error", "error": str(exc)})
    return AddSourceResult(keys=all_keys, import_results=results)


def load_payload_sources(
    source_args: SimpleNamespace,
    assignments: AdapterAssignments,
    config: ToolConfig,
) -> LoadedInputs:
    if not source_args.path:
        return load_serve_inputs(source_args, assignments, config)
    path_values = list(source_args.path)
    recursive_by_index: dict[int, list[LoadedSession]] = {}
    ordinary_paths: list[tuple[int, str]] = []
    for index, path in enumerate(path_values, start=1):
        recursive_sessions = recursive_trial_cell_sessions(path, config)
        if recursive_sessions:
            recursive_by_index[index] = recursive_sessions
        else:
            ordinary_paths.append((index, path))
    ordinary_loaded = LoadedInputs(sessions=[], notes=[])
    if ordinary_paths:
        ordinary_loaded = load_serve_inputs(
            SimpleNamespace(
                **{
                    **vars(source_args),
                    "path": [path for _, path in ordinary_paths],
                }
            ),
            remap_path_assignments(assignments, [index for index, _ in ordinary_paths]),
            config,
        )
    ordinary_sessions = iter(ordinary_loaded.sessions)
    ordered_sessions: list[LoadedSession] = []
    for index, _path in enumerate(path_values, start=1):
        if index in recursive_by_index:
            ordered_sessions.extend(recursive_by_index[index])
        else:
            ordered_sessions.append(next(ordinary_sessions))
    return LoadedInputs(sessions=ordered_sessions, notes=ordinary_loaded.notes)


def recursive_trial_cell_sessions(
    raw_path: str,
    config: ToolConfig,
) -> list[LoadedSession]:
    path = Path(raw_path).expanduser()
    cells = discover_complete_trial_cell_dirs(path)
    if not cells:
        if path.is_dir():
            raise HttpError(400, f"no complete Trial cells found under: {path}")
        return []
    return [loaded_trial_cell_import_session(cell, config) for cell in cells]


def remap_path_assignments(
    assignments: AdapterAssignments,
    original_indexes: list[int],
) -> AdapterAssignments:
    remapped = {
        next_index: assignments.path_adapters[original_index]
        for next_index, original_index in enumerate(original_indexes, start=1)
        if original_index in assignments.path_adapters
    }
    return AdapterAssignments(
        assignments.default_adapter,
        remapped,
        assignments.db_adapters,
        assignments.default_explicit,
    )


def apply_payload_alias(loaded: LoadedInputs, alias: str | None) -> LoadedInputs:
    if alias is None:
        return loaded
    return LoadedInputs(
        sessions=[replace(session, source_alias=alias) for session in loaded.sessions],
        notes=loaded.notes,
    )


def db_sessions_payload(
    store: ServeStateStore,
    payload: dict[str, Any],
) -> dict[str, Any]:
    db_paths = source_path_values(store, payload, "db")
    if len(db_paths) != 1:
        raise HttpError(400, "DB Inspect requires exactly one DB path")
    db_path = db_paths[0]
    path = Path(db_path)
    if not path.is_file():
        raise HttpError(400, f"DB path does not exist: {path}")
    raw_adapter = adapter_override_payload(payload)
    adapter_id, inferred = adapter_for_db_inspect(str(path), raw_adapter)
    sessions = list_adapter_sessions(adapter_id, str(path))
    return {
        "db": str(path),
        "adapter": adapter_id,
        "inferred": inferred,
        "sessions": [
            {
                "index": index,
                "session_id": session.session_id,
                "name": session.name,
            }
            for index, session in enumerate(sessions, start=1)
        ],
    }
