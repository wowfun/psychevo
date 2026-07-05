from __future__ import annotations

import argparse
from textwrap import dedent

from peval_py.outputs import DEFAULT_OUTPUT

INSPECT_EPILOG = """\
Inspect mode emits a compact fixed JSON digest for triage. Use --steps for
selected step evidence, --max-content-chars to bound previews, or -m raw for
the full peval-compatible JSON or HTML report.
"""


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
        help=(
            "source path: JSONL, report JSON, trajectory artifact, Trial cell, "
            "or descendant; repeatable"
        ),
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
    parser.add_argument(
        "--max-content-chars",
        type=int,
        help="bound source content and inspect preview text",
    )
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
        "--steps",
        action="append",
        metavar="IDS",
        help="show selected step_id evidence only; comma lists and start:end ranges supported",
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
