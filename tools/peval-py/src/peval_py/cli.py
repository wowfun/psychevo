from __future__ import annotations

import argparse
import json
import re
import sys
from dataclasses import dataclass
from pathlib import Path

from peval_py.atif import convert_records
from peval_py.config import apply_overrides, load_config
from peval_py.html import render_html
from peval_py.report import NoteInput, ReportSession, build_multi_report
from peval_py.sources import MessageRecord, read_jsonl, read_sqlite_messages

DEFAULT_OUTPUT = object()
FILENAME_PART_RE = re.compile(r"[^A-Za-z0-9._-]+")


@dataclass(frozen=True)
class LoadedSession:
    records: list[MessageRecord]
    input_label: str
    input_path: str | None = None
    session_hint: str | None = None


def main(argv: list[str] | None = None) -> int:
    parser = build_parser()
    args = parser.parse_args(argv)
    try:
        config = apply_overrides(load_config(args.config), args)
        sessions = load_sessions(args, config)
        if args.command == "export":
            conversion = convert_records(sessions[0].records, config)
            payload = conversion.trajectory
            write_json(payload, resolve_export_output(args, conversion.trajectory, config))
            return 0
        report_sessions = [
            ReportSession(
                conversion=convert_records(session.records, config),
                input_label=session.input_label,
                input_path=session.input_path,
                session_hint=session.session_hint,
            )
            for session in sessions
        ]
        notes = parse_notes(getattr(args, "note", None) or [], len(report_sessions))
        report = build_multi_report(report_sessions, config, notes)
        fmt = resolve_report_format(args)
        output = resolve_report_output(args, fmt, report, config)
        if fmt == "json":
            write_json(report, output)
        elif fmt == "html":
            write_text(render_html(report), output)
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
            "view retained agent trajectories from JSONL paths or Psychevo "
            "SQLite messages."
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
            "session comparison, or use one -d/--db with repeated -s/--session-id "
            "for Psychevo DB session comparison."
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
            "accepts exactly one -p/--path or exactly one -d/--db plus -s/--session-id."
        ),
    )
    add_shared_args(export_trajectory)

    return parser


def add_shared_args(parser: argparse.ArgumentParser) -> None:
    parser.add_argument("-c", "--config", help="TOML config path")
    parser.add_argument(
        "-a",
        "--adapter",
        choices=["psychevo", "opencode", "hermes"],
        help="input adapter; defaults to config or psychevo",
    )
    source = parser.add_mutually_exclusive_group(required=True)
    source.add_argument(
        "-p",
        "--path",
        action="append",
        metavar="PATH",
        help="JSONL session path; repeatable for view trajectory",
    )
    source.add_argument("-d", "--db", help="Psychevo SQLite state database")
    parser.add_argument(
        "-s",
        "--session-id",
        action="append",
        metavar="ID",
        help="DB session id; repeatable for view trajectory",
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


def load_sessions(args: argparse.Namespace, config) -> list[LoadedSession]:
    if args.path:
        if args.session_id:
            raise ValueError("--session-id is only valid with --db")
        if args.command == "export" and len(args.path) != 1:
            raise ValueError("export trajectory accepts exactly one --path")
        return [
            LoadedSession(
                records=read_jsonl(path),
                input_label=Path(path).name,
                input_path=str(Path(path)),
                session_hint=Path(path).stem or "session",
            )
            for path in args.path
        ]
    if args.db:
        session_ids = args.session_id or []
        if not session_ids:
            raise ValueError("--db requires at least one --session-id")
        if args.command == "export" and len(session_ids) != 1:
            raise ValueError("export trajectory accepts exactly one --session-id")
        db_path = Path(args.db)
        return [
            LoadedSession(
                records=read_sqlite_messages(args.db, session_id, config.db),
                input_label=f"{db_path.name}:{session_id}",
                input_path=str(db_path),
                session_hint=session_id,
            )
            for session_id in session_ids
        ]
    raise ValueError("missing input source")


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
    config,
) -> str | None:
    if args.output is DEFAULT_OUTPUT:
        trajectories = report.get("trajectory", [])
        if len(trajectories) > 1:
            return default_multi_output_name("report", fmt, len(trajectories), config)
        trajectory = trajectories[0] if trajectories else {}
        return default_output_name("report", fmt, trajectory, config)
    return args.output


def default_output_name(kind: str, ext: str, trajectory: dict, config) -> str:
    adapter = filename_part(config.adapter, "adapter")
    session = filename_part(trajectory.get("session_id"), "session")
    return f"{kind}-{adapter}-{session}.{ext}"


def default_multi_output_name(kind: str, ext: str, count: int, config) -> str:
    adapter = filename_part(config.adapter, "adapter")
    return f"{kind}-{adapter}-sessions-{count}.{ext}"


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
