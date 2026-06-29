from __future__ import annotations

import argparse
import sys
from dataclasses import replace
from pathlib import Path
from textwrap import dedent

from peval_py.config import (
    apply_overrides,
    config_for_adapter,
    is_windows_absolute_like_path,
    load_config,
)
from peval_py.html import render_html
from peval_py.inputs import (
    infer_workspace_root_from_trial_cell_paths,
    load_inputs,
    parse_adapter_assignments,
    resolved_local_path,
    same_local_path,
)
from peval_py.outputs import (
    DEFAULT_OUTPUT,
    announce_written,
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

INSPECT_EPILOG = """\
Inspect mode emits a compact fixed JSON digest for triage. Use -m raw for the
full peval-compatible JSON or HTML report.
"""


def main(argv: list[str] | None = None) -> int:
    parser = build_parser()
    args = parser.parse_args(argv)
    try:
        if args.command == "init":
            from peval_py.workspace import run_init_command

            run_init_command(args)
            return 0
        if args.command == "import":
            from peval_py.analysis import run_import_analysis_command

            workspace_root = validated_workspace_root(args)
            run_import_analysis_command(args, workspace_root)
            return 0
        workspace_root, inferred_workspace_root = workspace_root_for_args(args)
        config = apply_overrides(
            load_config(args.config, workspace_root=workspace_root),
            args,
        )
        adapter_assignments = parse_adapter_assignments(
            getattr(args, "adapter", None) or [],
            config.adapter,
        )
        config = config_for_adapter(config, adapter_assignments.default_adapter)
        if (
            args.command in {"view", "export"}
            and workspace_root
            and (getattr(args, "root", None) or inferred_workspace_root)
        ):
            from peval_py.state import workspace_paths

            config = replace(
                config,
                workspace_state_db_path=str(
                    workspace_paths(Path(workspace_root)).state_db_path
                ),
            )
        if args.command == "serve":
            from peval_py.serve import run_serve_command

            run_serve_command(args, config, adapter_assignments)
            return 0
        if args.command == "view" and getattr(args, "mode", "inspect") == "inspect":
            from peval_py.inspection import validate_inspect_raw_only_args

            validate_inspect_raw_only_args(args)
        if args.command == "view" and getattr(args, "list_sessions", False):
            print_session_lists(args, adapter_assignments, config)
            return 0
        if args.command == "view" and getattr(args, "list_interactive", False):
            selected = interactive_session_selection(args, adapter_assignments, config)
            if not selected:
                return 0
            args = argparse.Namespace(**{**vars(args), "session_id": selected})
        if args.command == "view" and getattr(args, "mode", "inspect") == "inspect":
            from peval_py.inspection import (
                build_inspect_payload,
                resolve_inspect_output,
                validate_inspect_args,
            )

            validate_inspect_args(args)
            announce_written(
                write_json(
                    build_inspect_payload(args, adapter_assignments, config),
                    resolve_inspect_output(args),
                )
            )
            return 0
        if args.command == "view" and getattr(args, "mode", "inspect") == "raw":
            from peval_py.inspection import validate_raw_args

            validate_raw_args(args)
        loaded_inputs = load_inputs(args, adapter_assignments, config=config)
        if args.command == "export":
            if len(loaded_inputs.sessions) != 1:
                raise ValueError("export trajectory accepts exactly one input session")
            session = loaded_inputs.sessions[0]
            session_config = config_for_session(session, config)
            if session.snapshot_trajectory is not None:
                write_json(
                    session.snapshot_trajectory,
                    resolve_export_output(args, session.snapshot_trajectory, session_config),
                )
                return 0
            conversion = convert_session(session, config)
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
        output = resolve_report_output(args, fmt, report, config, timestamped=True)
        if fmt == "json":
            written = write_json(report, output)
        elif fmt == "html":
            written = write_text(render_html(report, locale=config.locale), output)
        else:
            raise ValueError(f"unsupported report format: {fmt}")
        if args.command == "view":
            announce_written(written)
        return 0
    except Exception as exc:  # noqa: BLE001 - CLI boundary.
        print(f"peval-py: {exc}", file=sys.stderr)
        return 1


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        prog="peval-py",
        description=(
            "Lightweight standalone Python peval. Current scenarios init, "
            "export, import, view, or serve retained agent trajectories and "
            "analysis artifacts."
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
            "Inspect retained agent trajectories by default. Use -m raw only "
            "when a full peval-compatible JSON or HTML report is needed."
        ),
        epilog=dedent(INSPECT_EPILOG),
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    add_shared_args(view_trajectory)
    add_raw_report_override_args(
        view_trajectory.add_argument_group("raw report options")
    )
    add_root_arg(view_trajectory)
    view_trajectory.add_argument(
        "-m",
        "--mode",
        choices=["inspect", "raw"],
        default="inspect",
        help="view mode: inspect emits bounded JSON; raw emits a full report",
    )
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
    add_source_alias_arg(view_trajectory)
    add_inspect_args(view_trajectory)

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
    add_conversion_override_args(export_trajectory)
    add_root_arg(export_trajectory)

    import_cmd = verbs.add_parser(
        "import",
        help="import workspace artifacts",
        description="Import analysis files into peval-py workspace artifacts.",
    )
    import_scenarios = import_cmd.add_subparsers(dest="scenario", required=True)
    import_analysis = import_scenarios.add_parser(
        "analysis",
        help="import Trial analysis reports",
        description=(
            "Import JSON or Markdown analysis reports into a selected workspace "
            "Trial cell."
        ),
    )
    import_analysis.add_argument(
        "-r",
        "--root",
        required=True,
        metavar="DIR",
        help="existing peval-py workspace root",
    )
    import_analysis.add_argument(
        "--run-path",
        required=True,
        metavar="PATH",
        help=(
            "Trial cell path such as "
            "runs/default/psychevo/<session-id>/<cell-key>"
        ),
    )
    import_analysis.add_argument(
        "-p",
        "--path",
        action="append",
        required=True,
        metavar="PATH",
        help="analysis report path; repeat once for JSON and once for Markdown",
    )
    import_analysis.add_argument(
        "--json",
        action="store_true",
        help="print machine-readable import results",
    )

    serve = verbs.add_parser(
        "serve",
        help="serve the local saved trajectory workspace UI",
        description=(
            "Start the local peval-py saved workspace UI. Source flags persist "
            "and refresh sources before serving."
        ),
    )
    add_shared_args(serve, include_output=False)
    add_conversion_override_args(serve)
    add_note_arg(serve)
    add_source_alias_arg(serve)
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


def validated_workspace_root(args: argparse.Namespace) -> str | None:
    root = getattr(args, "root", None)
    if root and getattr(args, "command", None) in {"view", "export", "import"}:
        from peval_py.state import ensure_workspace_root

        root_text = str(root).strip()
        root_path = resolved_local_path(root_text)
        if root_path is None:
            if is_windows_absolute_like_path(root_text):
                raise ValueError(f"workspace root is not accessible: {root_text}")
            root_path = Path(root_text).expanduser()
        return str(ensure_workspace_root(root_path))
    return root


def workspace_root_for_args(args: argparse.Namespace) -> tuple[str | None, bool]:
    root = validated_workspace_root(args)
    if getattr(args, "command", None) not in {"view", "export"}:
        return root, False
    inferred = infer_workspace_root_from_trial_cell_paths(
        list(getattr(args, "path", None) or [])
    )
    if inferred is None:
        return root, False
    if root:
        if not same_local_path(root, str(inferred)):
            raise ValueError(
                f"explicit workspace root {root} conflicts with inferred workspace "
                f"root {inferred} from Trial cell artifact path"
            )
        return root, False
    from peval_py.state import ensure_workspace_root

    return str(ensure_workspace_root(inferred)), True


def add_root_arg(parser: argparse.ArgumentParser) -> None:
    parser.add_argument(
        "-r",
        "--root",
        metavar="DIR",
        help=(
            "existing peval-py workspace root for config discovery; "
            "run peval-py init -r DIR first"
        ),
    )


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
    parser.add_argument("--max-content-chars", type=int, help="truncate large content")
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


def add_conversion_override_args(parser: argparse.ArgumentParser) -> None:
    parser.add_argument("--agent-name", help="override ATIF agent name")
    parser.add_argument("--agent-version", help="override ATIF agent version")
    parser.add_argument("--model", help="override ATIF agent model name")
    parser.add_argument("--no-redact", action="store_true", help="disable secret redaction")


def add_raw_report_override_args(parser: argparse._ArgumentGroup) -> None:
    parser.add_argument(
        "--agent-name",
        help="raw mode only: override ATIF agent name",
    )
    parser.add_argument(
        "--agent-version",
        help="raw mode only: override ATIF agent version",
    )
    parser.add_argument(
        "--model",
        help="raw mode only: override ATIF agent model name",
    )
    parser.add_argument(
        "--no-redact",
        action="store_true",
        help="raw mode only: disable secret redaction",
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


def add_source_alias_arg(parser: argparse.ArgumentParser) -> None:
    parser.add_argument(
        "--source-alias",
        action="append",
        default=[],
        metavar="N=TEXT",
        help="display alias for a one-based input session; repeatable",
    )


def add_inspect_args(parser: argparse.ArgumentParser) -> None:
    parser.add_argument("--head", type=int, help="inspect first N steps per source; defaults to 2")
    parser.add_argument("--tail", type=int, help="inspect last N steps per source; defaults to 2")
    parser.add_argument("--top", type=int, help="inspect top N ranked rows per source; defaults to 5")
    parser.add_argument(
        "--step",
        action="append",
        metavar="ID",
        help="include compact evidence for a trajectory step_id; repeatable",
    )
    parser.add_argument(
        "--tool-call",
        action="append",
        metavar="ID",
        help="include a tool call and its matching result by tool_call_id; repeatable",
    )
    parser.add_argument(
        "--source",
        action="append",
        type=int,
        metavar="N",
        help="inspect only a one-based source index; repeatable",
    )
    parser.add_argument(
        "--preview-chars",
        type=int,
        help="maximum characters per preview field in inspect output",
    )


def print_session_lists(
    args: argparse.Namespace,
    adapter_assignments,
    config,
) -> None:
    for input_db in db_inputs_with_adapters(args, adapter_assignments, config):
        if len(input_db["all"]) > 1:
            print(f"d{input_db['index']} {input_db['path']} ({input_db['adapter']})")
        if input_db.get("kind") == "workspace-state":
            print(format_workspace_source_table(input_db["sources"]), end="")
        else:
            print(format_session_table(input_db["sessions"]), end="")


def interactive_session_selection(
    args: argparse.Namespace,
    adapter_assignments,
    config,
) -> list[str]:
    if getattr(args, "session_id", None):
        raise ValueError("--list-interactive cannot be combined with --session-id")
    if not sys.stdin.isatty():
        raise ValueError("--list-interactive requires an interactive terminal")
    inputs = db_inputs_with_adapters(args, adapter_assignments, config)
    if len(inputs) != 1:
        raise ValueError(
            "--list-interactive requires exactly one --db; "
            "use repeated -s dN=ID for multiple DB inputs"
        )
    input_db = inputs[0]
    if input_db.get("kind") == "workspace-state":
        print(format_workspace_source_table(input_db["sources"]), end="")
        raw = input("Select saved sources (for example 1,3-5 or all; blank cancels): ")
        indexes = parse_session_selection(raw, len(input_db["sources"]))
        return [
            str(input_db["sources"][index - 1]["source_key"])
            for index in indexes
        ]
    print(format_session_table(input_db["sessions"]), end="")
    raw = input("Select sessions (for example 1,3-5 or all; blank cancels): ")
    indexes = parse_session_selection(raw, len(input_db["sessions"]))
    return [input_db["sessions"][index - 1].session_id for index in indexes]


def db_inputs_with_adapters(
    args: argparse.Namespace,
    adapter_assignments,
    config,
) -> list[dict]:
    from peval_py.inputs import (
        adapter_for_input_path,
        is_workspace_state_db_input,
        resolve_db_input,
        workspace_snapshot_sources_for_input,
    )
    from peval_py.adapters import available_adapter_ids

    dbs = list(getattr(args, "db", None) or [])
    if not dbs:
        raise ValueError("--list requires at least one --db")
    available = set(available_adapter_ids())
    inputs = []
    for index, path in enumerate(dbs, start=1):
        if is_workspace_state_db_input(path, config):
            inputs.append(
                {
                    "index": index,
                    "path": path,
                    "adapter": "workspace",
                    "kind": "workspace-state",
                    "sources": workspace_snapshot_sources_for_input(path, config),
                    "all": dbs,
                }
            )
            continue
        resolved_path, token_adapter = resolve_db_input(path, index, adapter_assignments, config)
        adapter = token_adapter or adapter_for_input_path(
            resolved_path,
            index,
            adapter_assignments,
            "db",
            available,
        )
        inputs.append(
            {
                "index": index,
                "path": resolved_path,
                "adapter": adapter,
                "kind": "adapter-db",
                "sessions": list_adapter_sessions(adapter, resolved_path),
                "all": dbs,
            }
        )
    return inputs


def format_workspace_source_table(sources: list[dict]) -> str:
    headers = [
        "#",
        "source_key",
        "session_id",
        "trial_key",
        "active",
        "kind",
        "adapter",
        "alias/name",
    ]
    rows = [
        [
            str(index),
            value_or_dash(source.get("source_key")),
            value_or_dash(source.get("trial_session_id") or source.get("session_id")),
            value_or_dash(source.get("trial_key")),
            "yes" if source.get("active") else "no",
            value_or_dash(source.get("kind")),
            value_or_dash(source.get("adapter")),
            value_or_dash(source.get("source_alias") or source.get("label")),
        ]
        for index, source in enumerate(sources, start=1)
    ]
    widths = [
        max(len(row[column]) for row in [headers, *rows])
        for column in range(len(headers))
    ]
    lines = [format_table_row(headers, widths)]
    lines.extend(format_table_row(row, widths) for row in rows)
    return "\n".join(lines) + "\n"


def format_table_row(row: list[str], widths: list[int]) -> str:
    return "  ".join(value.ljust(widths[index]) for index, value in enumerate(row))


def value_or_dash(value: object) -> str:
    if value is None:
        return "-"
    text = str(value)
    return text if text else "-"


if __name__ == "__main__":
    raise SystemExit(main())
