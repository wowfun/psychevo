from __future__ import annotations

import argparse
import json
import re
import sys
from dataclasses import dataclass, replace
from pathlib import Path

from peval_py.atif import convert_db, convert_path, convert_records, is_atif_json_path
from peval_py.adapters import available_adapter_ids, normalize_adapter_id
from peval_py.adapters.base import ConversionResult
from peval_py.config import ToolConfig, apply_overrides, config_for_adapter, load_config
from peval_py.html import render_html
from peval_py.input_table import InputTableRow, read_input_tables
from peval_py.report import NoteInput, ReportSession, build_multi_report
from peval_py.sources import MessageRecord

DEFAULT_OUTPUT = object()
FILENAME_PART_RE = re.compile(r"[^A-Za-z0-9._-]+")
ADAPTER_SELECTOR_RE = re.compile(r"^([pd])([1-9][0-9]*)=(.+)$")
SESSION_SELECTOR_RE = re.compile(r"^d([1-9][0-9]*)=(.+)$")


@dataclass(frozen=True)
class AdapterAssignments:
    default_adapter: str
    path_adapters: dict[int, str]
    db_adapters: dict[int, str]


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


@dataclass(frozen=True)
class LoadedInputs:
    sessions: list[LoadedSession]
    notes: list[str]


def main(argv: list[str] | None = None) -> int:
    parser = build_parser()
    args = parser.parse_args(argv)
    try:
        config = apply_overrides(load_config(args.config), args)
        adapter_assignments = parse_adapter_assignments(
            getattr(args, "adapter", None) or [],
            config.adapter,
        )
        config = config_for_adapter(config, adapter_assignments.default_adapter)
        loaded_inputs = load_inputs(args, adapter_assignments)
        sessions = loaded_inputs.sessions
        if args.command == "export":
            session_config = config_for_session(sessions[0], config)
            conversion = convert_session(sessions[0], config)
            payload = conversion.trajectory
            write_json(
                payload,
                resolve_export_output(args, conversion.trajectory, session_config),
            )
            return 0
        report_sessions = [
            ReportSession(
                conversion=convert_session(session, config),
                input_label=session.input_label,
                input_path=session.input_path,
                session_hint=session.session_hint,
                adapter_id=session.adapter_id,
            )
            for session in sessions
        ]
        notes = parse_notes(
            [*(getattr(args, "note", None) or []), *loaded_inputs.notes],
            len(report_sessions),
        )
        report = build_multi_report(report_sessions, config, notes)
        fmt = resolve_report_format(args)
        output = resolve_report_output(args, fmt, report, config)
        if fmt == "json":
            write_json(report, output)
        elif fmt == "html":
            write_text(render_html(report, locale=config.locale), output)
        else:
            raise ValueError(f"unsupported report format: {fmt}")
        return 0
    except Exception as exc:  # noqa: BLE001 - CLI boundary.
        print(f"peval-py: {exc}", file=sys.stderr)
        return 1


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        prog="peval-py",
        description=(
            "Lightweight standalone Python peval. Current scenarios export or "
            "view retained agent trajectories from JSONL paths or adapter-owned "
            "SQLite databases."
        ),
    )
    verbs = parser.add_subparsers(dest="command", required=True)

    view = verbs.add_parser(
        "view",
        help="render a report",
        description="Render a peval-style report for a supported scenario.",
    )
    view_scenarios = view.add_subparsers(dest="scenario", required=True)
    view_trajectory = view_scenarios.add_parser(
        "trajectory",
        aliases=["tr"],
        help="view one or more retained agent trajectories",
        description=(
            "Build an offline trajectory report. Repeat -p/--path for JSONL "
            "session comparison, repeat -d/--db for adapter-owned DB comparison, "
            "or mix paths and DBs in one report."
        ),
    )
    add_shared_args(view_trajectory)
    view_trajectory.add_argument(
        "-f",
        "--format",
        choices=["json", "html"],
        help="report format; defaults from output suffix, bare -o uses html",
    )
    view_trajectory.add_argument(
        "-n",
        "--note",
        action="append",
        default=[],
        metavar="N=TEXT",
        help="add a report note at 0 or a one-based session note; repeatable",
    )

    export = verbs.add_parser(
        "export",
        help="export normalized data",
        description="Export normalized data for a supported scenario.",
    )
    export_scenarios = export.add_subparsers(dest="scenario", required=True)
    export_trajectory = export_scenarios.add_parser(
        "trajectory",
        aliases=["tr"],
        help="export one retained agent trajectory as ATIF JSON",
        description=(
            "Export one session as an ATIF v1.7 trajectory. Unlike view, export "
            "accepts exactly one effective -p/--path or -d/--db session. Some DB "
            "adapters can select a default session when -s/--session-id is omitted."
        ),
    )
    add_shared_args(export_trajectory)

    return parser


def add_shared_args(parser: argparse.ArgumentParser) -> None:
    parser.add_argument("-c", "--config", help="TOML config path")
    parser.add_argument(
        "-a",
        "--adapter",
        action="append",
        help="input adapter id; defaults to config or psychevo",
    )
    parser.add_argument(
        "-p",
        "--path",
        action="append",
        metavar="PATH",
        help="JSONL session path; repeatable for view trajectory",
    )
    parser.add_argument(
        "-d",
        "--db",
        action="append",
        metavar="PATH",
        help="adapter-owned SQLite state database; repeatable for view trajectory",
    )
    parser.add_argument(
        "-s",
        "--session-id",
        action="append",
        metavar="ID",
        help="DB session id; use dN=ID when multiple DB inputs are present",
    )
    parser.add_argument(
        "-i",
        "--input-table",
        action="append",
        metavar="PATH",
        help="CSV, JSON, or .xlsx input manifest; repeatable",
    )
    parser.add_argument("--agent-name", help="override ATIF agent name")
    parser.add_argument("--agent-version", help="override ATIF agent version")
    parser.add_argument("--model", help="override ATIF agent model name")
    parser.add_argument("--trajectory-id", help="override ATIF trajectory id")
    parser.add_argument("--max-content-chars", type=int, help="truncate large content")
    parser.add_argument("--no-redact", action="store_true", help="disable secret redaction")
    parser.add_argument(
        "-o",
        "--output",
        nargs="?",
        const=DEFAULT_OUTPUT,
        help=(
            "write to PATH; bare -o writes an adapter/session-based "
            "default filename"
        ),
    )


def parse_adapter_assignments(
    raw_adapters: list[str],
    default_adapter: str,
) -> AdapterAssignments:
    default = normalize_adapter_id(default_adapter)
    path_adapters: dict[int, str] = {}
    db_adapters: dict[int, str] = {}
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
    return AdapterAssignments(default, path_adapters, db_adapters)


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
) -> list[LoadedSession]:
    paths = list(getattr(args, "path", None) or [])
    dbs = list(getattr(args, "db", None) or [])
    if not paths and not dbs and not getattr(args, "input_table", None):
        raise ValueError("missing input source; pass --path, --db, or --input-table")
    validate_adapter_selector_range(
        adapter_assignments,
        path_count=len(paths),
        db_count=len(dbs),
    )
    if getattr(args, "session_id", None) and not dbs:
        raise ValueError("--session-id is only valid with --db")

    sessions: list[LoadedSession] = []
    for index, path in enumerate(paths, start=1):
        source_path = Path(path)
        is_atif = is_atif_json_path(str(source_path))
        sessions.append(
            LoadedSession(
                records=None,
                input_label=source_path.name,
                adapter_id=(
                    "atif"
                    if is_atif
                    else adapter_assignments.path_adapters.get(
                        index,
                        adapter_assignments.default_adapter,
                    )
                ),
                input_path=str(source_path),
                session_hint=None if is_atif else source_path.stem or "session",
            )
        )

    session_ids_by_db = parse_db_session_ids(
        getattr(args, "session_id", None) or [],
        db_count=len(dbs),
    )
    for index, db in enumerate(dbs, start=1):
        db_path = Path(db)
        adapter_id = adapter_assignments.db_adapters.get(
            index,
            adapter_assignments.default_adapter,
        )
        session_ids = session_ids_by_db.get(index) or [None]
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
                )
            )

    return sessions


def load_inputs(
    args: argparse.Namespace,
    adapter_assignments: AdapterAssignments,
) -> LoadedInputs:
    sessions = load_sessions(args, adapter_assignments)
    notes: list[str] = []
    table_data = read_input_tables(getattr(args, "input_table", None) or [])
    notes.extend(f"0={note}" for note in table_data.report_notes)
    for row in table_data.rows:
        session_index = len(sessions) + 1
        sessions.append(
            loaded_session_from_table_row(row, adapter_assignments.default_adapter)
        )
        notes.extend(table_note_for_session(note, session_index) for note in row.notes)
        notes.extend(f"0={note}" for note in row.report_notes)
    if args.command == "export" and len(sessions) != 1:
        raise ValueError("export trajectory accepts exactly one input session")
    validate_required_adapters(sessions)
    return LoadedInputs(sessions=sessions, notes=notes)


def loaded_session_from_table_row(
    row: InputTableRow,
    default_adapter: str,
) -> LoadedSession:
    if row.path is not None:
        source_path = Path(row.path)
        is_atif = is_atif_json_path(str(source_path))
        adapter_id = (
            "atif"
            if is_atif
            else normalize_adapter_id(row.adapter or default_adapter)
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
        )
    if row.db is None:
        raise ValueError(f"{row.table_path}: row {row.row_number}: missing input source")
    db_path = Path(row.db)
    return LoadedSession(
        records=None,
        input_label=f"{db_path.name}:{row.session_id}" if row.session_id else db_path.name,
        adapter_id=normalize_adapter_id(row.adapter or default_adapter),
        input_path=str(db_path),
        db_path=str(db_path),
        session_hint=row.session_id,
        agent_name=row.agent_name,
        agent_version=row.agent_version,
        model=row.model,
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
        {session.adapter_id for session in sessions if session.adapter_id != "atif"}
    )
    if not required:
        return
    available = set(available_adapter_ids())
    for adapter_id in required:
        validate_selected_adapter(adapter_id, available, "input")


def config_for_session(session: LoadedSession, config: ToolConfig) -> ToolConfig:
    session_config = config_for_adapter(config, session.adapter_id)
    updates: dict[str, str] = {}
    if session.agent_name is not None:
        updates["agent_name"] = session.agent_name
    if session.agent_version is not None:
        updates["agent_version"] = session.agent_version
    if session.model is not None:
        updates["model"] = session.model
    return replace(session_config, **updates) if updates else session_config


def convert_session(session: LoadedSession, config: ToolConfig) -> ConversionResult:
    session_config = config_for_session(session, config)
    if session.db_path is not None:
        return convert_db(session.db_path, session.session_hint, session_config)
    if session.records is None:
        if not session.input_path:
            raise ValueError("path input is missing a source path")
        return convert_path(session.input_path, session_config)
    return convert_records(session.records, session_config)


def parse_notes(raw_notes: list[str], session_count: int) -> list[NoteInput]:
    notes: list[NoteInput] = []
    for raw in raw_notes:
        if "=" not in raw:
            raise ValueError("--note must use N=TEXT syntax")
        raw_index, markdown = raw.split("=", 1)
        if not raw_index.isdigit():
            raise ValueError("--note index must be a non-negative integer")
        index = int(raw_index)
        if index > session_count:
            raise ValueError(
                f"--note index {index} is out of range for {session_count} sessions"
            )
        notes.append(NoteInput(index=index, markdown=markdown))
    return notes


def resolve_report_format(args: argparse.Namespace) -> str:
    if getattr(args, "format", None):
        return args.format
    if args.output is DEFAULT_OUTPUT:
        return "html"
    if args.output:
        suffix = Path(args.output).suffix.lower()
        if suffix == ".html":
            return "html"
        if suffix == ".json":
            return "json"
    return "json"


def resolve_export_output(
    args: argparse.Namespace,
    trajectory: dict,
    config,
) -> str | None:
    if args.output is DEFAULT_OUTPUT:
        return default_output_name("trajectory", "json", trajectory, config)
    return args.output


def resolve_report_output(
    args: argparse.Namespace,
    fmt: str,
    report: dict,
    config: ToolConfig,
) -> str | None:
    if args.output is DEFAULT_OUTPUT:
        trajectories = report.get("trajectory", [])
        adapter = default_report_adapter(report, config)
        if len(trajectories) > 1:
            return default_multi_output_name(
                "report",
                fmt,
                len(trajectories),
                adapter,
            )
        trajectory = trajectories[0] if trajectories else {}
        return default_output_name("report", fmt, trajectory, config, adapter=adapter)
    return args.output


def default_report_adapter(report: dict, config: ToolConfig) -> str:
    adapters = {
        str(meta.get("adapter"))
        for meta in report.get("trajectory_meta", [])
        if meta.get("adapter")
    }
    if len(adapters) == 1:
        return next(iter(adapters))
    if len(adapters) > 1:
        return "multi-adapter"
    return config.adapter


def default_output_name(
    kind: str,
    ext: str,
    trajectory: dict,
    config: ToolConfig,
    *,
    adapter: str | None = None,
) -> str:
    adapter_part = filename_part(adapter or config.adapter, "adapter")
    session = filename_part(trajectory.get("session_id"), "session")
    return f"{kind}-{adapter_part}-{session}.{ext}"


def default_multi_output_name(kind: str, ext: str, count: int, adapter: str) -> str:
    adapter_part = filename_part(adapter, "adapter")
    return f"{kind}-{adapter_part}-sessions-{count}.{ext}"


def filename_part(value: object, fallback: str) -> str:
    text = str(value or "").strip() or fallback
    safe = FILENAME_PART_RE.sub("-", text).strip(".-")
    return safe or fallback


def write_json(payload: dict, output: str | None) -> None:
    write_text(json.dumps(payload, indent=2, ensure_ascii=False) + "\n", output)


def write_text(payload: str, output: str | None) -> None:
    if output:
        Path(output).write_text(payload, encoding="utf-8")
    else:
        sys.stdout.write(payload)


if __name__ == "__main__":
    raise SystemExit(main())
