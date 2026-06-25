from __future__ import annotations

import argparse
import re
import sqlite3
from dataclasses import dataclass, replace
from pathlib import Path
from typing import Any

from peval_py.atif import is_atif_json_path
from peval_py.adapters import available_adapter_ids, normalize_adapter_id
from peval_py.config import is_windows_absolute_like_path
from peval_py.input_table import InputTableRow, read_input_tables
from peval_py.session_select import resolve_session_selectors
from peval_py.sources import MessageRecord

ADAPTER_SELECTOR_RE = re.compile(r"^([pd])([1-9][0-9]*)=(.+)$")
SESSION_SELECTOR_RE = re.compile(r"^d([1-9][0-9]*)=(.+)$")
PSEUDO_ADAPTERS = {"atif", "report"}
PATH_TOKEN_RE = re.compile(r"[^a-z0-9]+")
DEFAULT_DB_TOKEN_RE = re.compile(r"^@([A-Za-z0-9_.-]+)$")


@dataclass(frozen=True)
class AdapterAssignments:
    default_adapter: str
    path_adapters: dict[int, str]
    db_adapters: dict[int, str]
    default_explicit: bool = False


@dataclass(frozen=True)
class LoadedSession:
    records: list[MessageRecord] | None
    input_label: str
    adapter_id: str
    input_path: str | None = None
    db_path: str | None = None
    session_hint: str | None = None
    agent_name: str | None = None
    agent_version: str | None = None
    model: str | None = None
    source_alias: str | None = None
    source_kind: str = "path"
    workspace_source_key: str | None = None
    snapshot_trajectory: dict[str, Any] | None = None
    snapshot_meta: dict[str, Any] | None = None
    snapshot_source_report: dict[str, Any] | None = None


@dataclass(frozen=True)
class LoadedInputs:
    sessions: list[LoadedSession]
    notes: list[str]


def parse_adapter_assignments(
    raw_adapters: list[str],
    default_adapter: str,
) -> AdapterAssignments:
    default = normalize_adapter_id(default_adapter)
    path_adapters: dict[int, str] = {}
    db_adapters: dict[int, str] = {}
    default_explicit = False
    for raw in raw_adapters:
        text = str(raw).strip()
        match = ADAPTER_SELECTOR_RE.fullmatch(text)
        if match:
            family, raw_index, raw_adapter = match.groups()
            index = int(raw_index)
            adapter = normalize_adapter_id(raw_adapter)
            assignments = path_adapters if family == "p" else db_adapters
            if index in assignments:
                raise ValueError(f"duplicate adapter selector: {family}{index}")
            assignments[index] = adapter
            continue
        if "=" in text:
            raise ValueError("--adapter selector must use pN=ADAPTER or dN=ADAPTER")
        default = normalize_adapter_id(text)
        default_explicit = True
    return AdapterAssignments(default, path_adapters, db_adapters, default_explicit)


def validate_selected_adapter(adapter: object, available: set[str], source: str) -> str:
    adapter_id = normalize_adapter_id(adapter)
    if adapter_id not in available:
        options = ", ".join(sorted(available)) or "<none>"
        raise ValueError(
            f"unsupported adapter for {source}: {adapter_id}; "
            f"available adapters: {options}"
        )
    return adapter_id


def load_sessions(
    args: argparse.Namespace,
    adapter_assignments: AdapterAssignments,
    *,
    require_sources: bool = True,
    config: object | None = None,
) -> list[LoadedSession]:
    paths = list(getattr(args, "path", None) or [])
    dbs = list(getattr(args, "db", None) or [])
    if (
        require_sources
        and not paths
        and not dbs
        and not getattr(args, "input_table", None)
    ):
        raise ValueError("missing input source; pass --path, --db, or --input-table")
    validate_adapter_selector_range(
        adapter_assignments,
        path_count=len(paths),
        db_count=len(dbs),
    )
    if getattr(args, "session_id", None) and not dbs:
        raise ValueError("--session-id is only valid with --db")

    available = set(available_adapter_ids())
    sessions: list[LoadedSession] = []
    for index, path in enumerate(paths, start=1):
        source_path = Path(path)
        is_atif = is_atif_json_path(str(source_path))
        adapter_id = (
            "atif"
            if is_atif
            else adapter_for_input_path(
                str(source_path),
                index,
                adapter_assignments,
                "path",
                available,
            )
        )
        sessions.append(
            LoadedSession(
                records=None,
                input_label=source_path.name,
                adapter_id=adapter_id,
                input_path=str(source_path),
                session_hint=None if is_atif else source_path.stem or "session",
                source_kind="path",
            )
        )

    session_ids_by_db = parse_db_session_ids(
        getattr(args, "session_id", None) or [],
        db_count=len(dbs),
    )
    for index, db in enumerate(dbs, start=1):
        raw_session_ids = session_ids_by_db.get(index) or []
        if is_workspace_state_db_input(db, config):
            sessions.extend(
                load_workspace_snapshot_sessions(
                    args,
                    index,
                    str(db),
                    raw_session_ids,
                    config,
                )
            )
            continue
        resolved_db, token_adapter = resolve_db_input(db, index, adapter_assignments, config)
        db_path = Path(resolved_db)
        adapter_id = token_adapter or adapter_for_input_path(
            str(db_path),
            index,
            adapter_assignments,
            "db",
            available,
        )
        session_ids = (
            resolve_session_selectors(adapter_id, str(db_path), raw_session_ids)
            if raw_session_ids
            else [None]
        )
        for session_id in session_ids:
            sessions.append(
                LoadedSession(
                    records=None,
                    input_label=(
                        f"{db_path.name}:{session_id}" if session_id else db_path.name
                    ),
                    adapter_id=adapter_id,
                    input_path=str(db_path),
                    db_path=str(db_path),
                    session_hint=session_id,
                    source_kind="db",
                )
            )

    return sessions


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
    workspace_root = getattr(config, "workspace_root", None)
    if not workspace_root:
        raise ValueError(peval_py_state_db_error(raw_db))
    from peval_py.state import (
        meta_with_source_alias,
        open_workspace_state_readonly,
        source_report_with_current_annotations,
        uniquify_trial_keys,
    )

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
                input_path=str(raw_db),
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


def load_inputs(
    args: argparse.Namespace,
    adapter_assignments: AdapterAssignments,
    *,
    require_sources: bool = True,
    config: object | None = None,
) -> LoadedInputs:
    sessions = load_sessions(
        args,
        adapter_assignments,
        require_sources=require_sources,
        config=config,
    )
    notes: list[str] = []
    available = set(available_adapter_ids())
    table_data = read_input_tables(getattr(args, "input_table", None) or [])
    notes.extend(f"0={note}" for note in table_data.report_notes)
    for row in table_data.rows:
        session_index = len(sessions) + 1
        sessions.append(
            loaded_session_from_table_row(row, adapter_assignments, available)
        )
        notes.extend(table_note_for_session(note, session_index) for note in row.notes)
        notes.extend(f"0={note}" for note in row.report_notes)
    sessions = apply_source_aliases(
        sessions,
        getattr(args, "source_alias", None) or [],
    )
    validate_required_adapters(sessions)
    return LoadedInputs(sessions=sessions, notes=notes)


def loaded_session_from_table_row(
    row: InputTableRow,
    adapter_assignments: AdapterAssignments,
    available: set[str],
) -> LoadedSession:
    if row.path is not None:
        source_path = Path(row.path)
        is_atif = is_atif_json_path(str(source_path))
        adapter_id = (
            "atif"
            if is_atif
            else normalize_adapter_id(row.adapter)
            if row.adapter
            else adapter_for_input_path(
                str(source_path),
                None,
                adapter_assignments,
                "path",
                available,
            )
        )
        return LoadedSession(
            records=None,
            input_label=source_path.name,
            adapter_id=adapter_id,
            input_path=str(source_path),
            session_hint=None if is_atif else source_path.stem or "session",
            agent_name=row.agent_name,
            agent_version=row.agent_version,
            model=row.model,
            source_alias=row.source_alias,
            source_kind="path",
        )
    if row.db is None:
        raise ValueError(f"{row.table_path}: row {row.row_number}: missing input source")
    db_path = Path(row.db)
    adapter_id = (
        normalize_adapter_id(row.adapter)
        if row.adapter
        else adapter_for_input_path(
            str(db_path),
            None,
            adapter_assignments,
            "db",
            available,
        )
    )
    session_ids = (
        resolve_session_selectors(adapter_id, str(db_path), [row.session_id])
        if row.session_id
        else [None]
    )
    session_id = session_ids[0]
    return LoadedSession(
        records=None,
        input_label=f"{db_path.name}:{session_id}" if session_id else db_path.name,
        adapter_id=adapter_id,
        input_path=str(db_path),
        db_path=str(db_path),
        session_hint=session_id,
        agent_name=row.agent_name,
        agent_version=row.agent_version,
        model=row.model,
        source_alias=row.source_alias,
        source_kind="db",
    )


def resolve_db_input(
    raw_db: str,
    index: int,
    adapter_assignments: AdapterAssignments,
    config: object | None,
) -> tuple[str, str | None]:
    text = str(raw_db).strip()
    match = DEFAULT_DB_TOKEN_RE.fullmatch(text)
    if not match:
        if is_peval_py_state_db_input(text):
            raise ValueError(peval_py_state_db_error(text))
        return raw_db, None
    adapter_id = normalize_adapter_id(match.group(1))
    selected_adapter = adapter_assignments.db_adapters.get(index)
    if selected_adapter is not None and selected_adapter != adapter_id:
        raise ValueError(
            f"DB input d{index} uses {text} but adapter selector d{index}={selected_adapter}"
        )
    defaults = dict(getattr(config, "adapter_default_db_paths", {}) or {})
    path = defaults.get(adapter_id)
    if not path:
        raise ValueError(f"no default_db_path configured for adapter: {adapter_id}")
    return path, adapter_id


def same_local_path(left: str, right: str) -> bool:
    left_path = resolved_local_path(left)
    right_path = resolved_local_path(right)
    return left_path is not None and right_path is not None and left_path == right_path


def resolved_local_path(value: str) -> Path | None:
    text = str(value).strip()
    if not text or is_windows_absolute_like_path(text):
        return None
    path = Path(text).expanduser()
    if not path.is_absolute():
        path = Path.cwd() / path
    return path.resolve()


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


def apply_source_aliases(
    sessions: list[LoadedSession],
    raw_aliases: list[str],
) -> list[LoadedSession]:
    aliases = parse_source_aliases(raw_aliases, len(sessions))
    if not aliases:
        return sessions
    return [
        replace(session, source_alias=aliases.get(index, session.source_alias))
        for index, session in enumerate(sessions, start=1)
    ]


def parse_source_aliases(raw_aliases: list[str], session_count: int) -> dict[int, str]:
    aliases: dict[int, str] = {}
    for raw in raw_aliases:
        text = str(raw)
        if "=" not in text:
            raise ValueError("--source-alias must use N=TEXT syntax")
        raw_index, alias = text.split("=", 1)
        if not raw_index.isdigit() or int(raw_index) <= 0:
            raise ValueError("--source-alias index must be a positive integer")
        index = int(raw_index)
        if index in aliases:
            raise ValueError(f"duplicate --source-alias index: {index}")
        if index > session_count:
            raise ValueError(
                f"--source-alias index {index} is out of range for {session_count} sessions"
            )
        alias = alias.strip()
        if not alias:
            raise ValueError("--source-alias text must not be empty")
        aliases[index] = alias
    return aliases


def table_note_for_session(note: str, session_index: int) -> str:
    if "=" in note:
        raw_index, _ = note.split("=", 1)
        if raw_index.isdigit():
            return note
    return f"{session_index}={note}"


def validate_adapter_selector_range(
    adapter_assignments: AdapterAssignments,
    path_count: int,
    db_count: int,
) -> None:
    for index in sorted(adapter_assignments.path_adapters):
        if index > path_count:
            raise ValueError(
                f"adapter selector p{index} has no matching --path input "
                f"(path inputs: {path_count})"
            )
    for index in sorted(adapter_assignments.db_adapters):
        if index > db_count:
            raise ValueError(
                f"adapter selector d{index} has no matching --db input "
                f"(DB inputs: {db_count})"
            )


def adapter_for_input_path(
    path: str,
    index: int | None,
    adapter_assignments: AdapterAssignments,
    family: str,
    available: set[str],
) -> str:
    selectors = (
        adapter_assignments.path_adapters
        if family == "path"
        else adapter_assignments.db_adapters
    )
    if index is not None and index in selectors:
        return selectors[index]
    if adapter_assignments.default_explicit:
        return adapter_assignments.default_adapter
    inferred = infer_adapter_from_path(path, available)
    return inferred or adapter_assignments.default_adapter


def infer_adapter_from_path(path: str, available: set[str]) -> str | None:
    candidates = sorted(set(available) - PSEUDO_ADAPTERS)
    tokens = path_tokens(path)
    matches = [adapter_id for adapter_id in candidates if adapter_id in tokens]
    if len(matches) > 1:
        raise ValueError(
            f"ambiguous adapter inference for {path}: {', '.join(matches)}; pass -a"
        )
    return matches[0] if matches else None


def path_tokens(path: str) -> set[str]:
    raw_parts = Path(path).expanduser().parts
    tokens: set[str] = set()
    for raw_part in raw_parts:
        part = raw_part.strip().lower()
        if not part:
            continue
        tokens.add(part)
        stem = Path(part).stem
        if stem:
            tokens.add(stem)
        for value in [part, stem]:
            for token in PATH_TOKEN_RE.split(value):
                if token:
                    tokens.add(token)
    return tokens


def parse_db_session_ids(
    raw_session_ids: list[str],
    db_count: int,
) -> dict[int, list[str]]:
    session_ids_by_db: dict[int, list[str]] = {}
    for raw in raw_session_ids:
        text = str(raw)
        match = SESSION_SELECTOR_RE.fullmatch(text)
        if match:
            index = int(match.group(1))
            session_id = match.group(2)
            if index > db_count:
                raise ValueError(
                    f"--session-id selector d{index} has no matching --db input "
                    f"(DB inputs: {db_count})"
                )
            session_ids_by_db.setdefault(index, []).append(session_id)
            continue
        if "=" in text:
            raise ValueError("--session-id selector must use dN=ID")
        if db_count != 1:
            raise ValueError(
                "bare --session-id is only valid with exactly one --db; "
                "use --session-id dN=ID"
            )
        session_ids_by_db.setdefault(1, []).append(text)
    return session_ids_by_db


def validate_required_adapters(sessions: list[LoadedSession]) -> None:
    required = sorted(
        {
            session.adapter_id
            for session in sessions
            if session.adapter_id not in PSEUDO_ADAPTERS
            and session.snapshot_trajectory is None
        }
    )
    if not required:
        return
    available = set(available_adapter_ids())
    for adapter_id in required:
        validate_selected_adapter(adapter_id, available, "input")
