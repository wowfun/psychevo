from __future__ import annotations

import argparse
import re
from dataclasses import dataclass
from pathlib import Path

from peval_py.atif import is_atif_json_path
from peval_py.adapters import available_adapter_ids, normalize_adapter_id
from peval_py.input_table import InputTableRow, read_input_tables
from peval_py.session_select import resolve_session_selectors
from peval_py.sources import MessageRecord

ADAPTER_SELECTOR_RE = re.compile(r"^([pd])([1-9][0-9]*)=(.+)$")
SESSION_SELECTOR_RE = re.compile(r"^d([1-9][0-9]*)=(.+)$")
PSEUDO_ADAPTERS = {"atif", "report"}
PATH_TOKEN_RE = re.compile(r"[^a-z0-9]+")


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
    source_kind: str = "path"


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
        db_path = Path(db)
        adapter_id = adapter_for_input_path(
            str(db_path),
            index,
            adapter_assignments,
            "db",
            available,
        )
        raw_session_ids = session_ids_by_db.get(index) or []
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


def load_inputs(
    args: argparse.Namespace,
    adapter_assignments: AdapterAssignments,
    *,
    require_sources: bool = True,
) -> LoadedInputs:
    sessions = load_sessions(
        args,
        adapter_assignments,
        require_sources=require_sources,
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
        source_kind="db",
    )


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
        {session.adapter_id for session in sessions if session.adapter_id not in PSEUDO_ADAPTERS}
    )
    if not required:
        return
    available = set(available_adapter_ids())
    for adapter_id in required:
        validate_selected_adapter(adapter_id, available, "input")
