from __future__ import annotations

import argparse
import sys

from peval_py.config import apply_overrides, config_for_adapter, load_config
from peval_py.html import render_html
from peval_py.inputs import load_inputs, parse_adapter_assignments
from peval_py.outputs import (
    DEFAULT_OUTPUT,
    resolve_export_output,
    resolve_report_format,
    resolve_report_output,
    write_json,
    write_text,
)
from peval_py.pipeline import (
    build_report_from_loaded_inputs,
    config_for_session,
    convert_session,
)
from peval_py.session_select import (
    format_session_table,
    list_adapter_sessions,
    parse_session_selection,
)


def main(argv: list[str] | None = None) -> int:
    parser = build_parser()
    args = parser.parse_args(argv)
    try:
        if args.command == "init":
            from peval_py.workspace import run_init_command

            run_init_command(args)
            return 0
        config = apply_overrides(
            load_config(args.config, workspace_root=getattr(args, "root", None)),
            args,
        )
        adapter_assignments = parse_adapter_assignments(
            getattr(args, "adapter", None) or [],
            config.adapter,
        )
        config = config_for_adapter(config, adapter_assignments.default_adapter)
        if args.command == "serve":
            from peval_py.serve import run_serve_command

            run_serve_command(args, config, adapter_assignments)
            return 0
        if args.command == "view" and getattr(args, "list_sessions", False):
            print_session_lists(args, adapter_assignments)
            return 0
        if args.command == "view" and getattr(args, "list_interactive", False):
            selected = interactive_session_selection(args, adapter_assignments)
            if not selected:
                return 0
            args = argparse.Namespace(**{**vars(args), "session_id": selected})
        loaded_inputs = load_inputs(args, adapter_assignments)
        if args.command == "export":
            if len(loaded_inputs.sessions) != 1:
                raise ValueError("export trajectory accepts exactly one input session")
            session_config = config_for_session(loaded_inputs.sessions[0], config)
            conversion = convert_session(loaded_inputs.sessions[0], config)
            write_json(
                conversion.trajectory,
                resolve_export_output(args, conversion.trajectory, session_config),
            )
            return 0
        report = build_report_from_loaded_inputs(
            loaded_inputs,
            config,
            getattr(args, "note", None) or [],
        )
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
            "Lightweight standalone Python peval. Current scenarios init, "
            "export, view, or serve retained agent trajectories from JSONL "
            "paths or adapter-owned SQLite databases."
        ),
    )
    verbs = parser.add_subparsers(dest="command", required=True)

    init = verbs.add_parser(
        "init",
        help="initialize peval-py serve state",
        description="Create or repair the minimal peval-py serve state.",
    )
    init.add_argument(
        "-r",
        "--root",
        metavar="DIR",
        help="workspace root; defaults to the current directory",
    )
    init.add_argument(
        "--json",
        action="store_true",
        help="print machine-readable init results",
    )

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
        "-l",
        "--list",
        action="store_true",
        dest="list_sessions",
        help="list DB sessions and exit",
    )
    view_trajectory.add_argument(
        "-li",
        "--list-interactive",
        action="store_true",
        dest="list_interactive",
        help="list DB sessions, prompt for selection, and render selected sessions",
    )
    add_note_arg(view_trajectory)

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

    serve = verbs.add_parser(
        "serve",
        help="serve the local saved trajectory workspace UI",
        description=(
            "Start the local peval-py saved workspace UI. Source flags persist "
            "and refresh sources before serving."
        ),
    )
    add_shared_args(serve, include_output=False)
    add_note_arg(serve)
    serve.add_argument(
        "-r",
        "--root",
        metavar="DIR",
        help="peval-py workspace root; otherwise discover peval-py.toml",
    )
    serve.add_argument(
        "--host",
        default="127.0.0.1",
        help="localhost bind address; defaults to 127.0.0.1",
    )
    serve.add_argument(
        "--port",
        type=int,
        default=None,
        help="bind port; omitted tries 58010 through 58029",
    )

    return parser


def add_shared_args(
    parser: argparse.ArgumentParser,
    *,
    include_output: bool = True,
) -> None:
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
    if include_output:
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


def add_note_arg(parser: argparse.ArgumentParser) -> None:
    parser.add_argument(
        "-n",
        "--note",
        action="append",
        default=[],
        metavar="N=TEXT",
        help="add a report note at 0 or a one-based session note; repeatable",
    )


def print_session_lists(
    args: argparse.Namespace,
    adapter_assignments,
) -> None:
    for input_db in db_inputs_with_adapters(args, adapter_assignments):
        if len(input_db["all"]) > 1:
            print(f"d{input_db['index']} {input_db['path']} ({input_db['adapter']})")
        print(format_session_table(input_db["sessions"]), end="")


def interactive_session_selection(
    args: argparse.Namespace,
    adapter_assignments,
) -> list[str]:
    if getattr(args, "session_id", None):
        raise ValueError("--list-interactive cannot be combined with --session-id")
    if not sys.stdin.isatty():
        raise ValueError("--list-interactive requires an interactive terminal")
    inputs = db_inputs_with_adapters(args, adapter_assignments)
    if len(inputs) != 1:
        raise ValueError(
            "--list-interactive requires exactly one --db; "
            "use repeated -s dN=ID for multiple DB inputs"
        )
    input_db = inputs[0]
    print(format_session_table(input_db["sessions"]), end="")
    raw = input("Select sessions (for example 1,3-5 or all; blank cancels): ")
    indexes = parse_session_selection(raw, len(input_db["sessions"]))
    return [input_db["sessions"][index - 1].session_id for index in indexes]


def db_inputs_with_adapters(
    args: argparse.Namespace,
    adapter_assignments,
) -> list[dict]:
    from peval_py.inputs import adapter_for_input_path
    from peval_py.adapters import available_adapter_ids

    dbs = list(getattr(args, "db", None) or [])
    if not dbs:
        raise ValueError("--list requires at least one --db")
    available = set(available_adapter_ids())
    inputs = []
    for index, path in enumerate(dbs, start=1):
        adapter = adapter_for_input_path(
            path,
            index,
            adapter_assignments,
            "db",
            available,
        )
        inputs.append(
            {
                "index": index,
                "path": path,
                "adapter": adapter,
                "sessions": list_adapter_sessions(adapter, path),
                "all": dbs,
            }
        )
    return inputs


if __name__ == "__main__":
    raise SystemExit(main())
