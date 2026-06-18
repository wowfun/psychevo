#!/usr/bin/env python3
"""Small peval-py report helpers for skill workflows."""

from __future__ import annotations

import argparse
import json
from pathlib import Path
from typing import Any


def load_report(path: Path) -> dict[str, Any]:
    try:
        payload = json.loads(path.read_text(encoding="utf-8"))
    except OSError as exc:
        raise SystemExit(f"cannot read {path}: {exc}") from exc
    except json.JSONDecodeError as exc:
        raise SystemExit(f"{path} is not valid JSON: {exc}") from exc
    if not isinstance(payload, dict):
        raise SystemExit(f"{path} is not a JSON object")
    return payload


def cmd_subjects(args: argparse.Namespace) -> int:
    report = load_report(args.report_json)
    trajectories = report.get("trajectory") or []
    if isinstance(trajectories, dict):
        trajectories = [trajectories]
    if not isinstance(trajectories, list):
        raise SystemExit("report trajectory field is not a list or object")

    metas = report.get("trajectory_meta") or []
    if not isinstance(metas, list):
        metas = []

    for index, trajectory in enumerate(trajectories, start=1):
        if not isinstance(trajectory, dict):
            continue
        meta = metas[index - 1] if index - 1 < len(metas) else {}
        if not isinstance(meta, dict):
            meta = {}
        agent = trajectory.get("agent") or {}
        if not isinstance(agent, dict):
            agent = {}
        print(
            json.dumps(
                {
                    "index": index,
                    "session_id": trajectory.get("session_id"),
                    "agent_name": agent.get("name"),
                    "adapter": meta.get("adapter"),
                    "trial_key": meta.get("trial_key"),
                },
                ensure_ascii=False,
            )
        )
    return 0


def analysis_items(report: dict[str, Any]) -> list[dict[str, Any]]:
    annotations = report.get("annotations") or {}
    if not isinstance(annotations, dict):
        return []
    raw_items = annotations.get("analysis") or []
    if not isinstance(raw_items, list):
        return []
    return [item for item in raw_items if isinstance(item, dict)]


def cmd_check(args: argparse.Namespace) -> int:
    report = load_report(args.report_json)
    require_summary = args.require_summary or args.require_analysis
    require_md_report = args.require_md_report or args.require_analysis
    if require_summary or require_md_report:
        items = analysis_items(report)
        if not items:
            raise SystemExit("missing annotations.analysis")
        if require_summary and not items[0].get("summary"):
            raise SystemExit("missing annotations.analysis[0].summary")
        if require_md_report and not items[0].get("md_report"):
            raise SystemExit("missing annotations.analysis[0].md_report")
        print("report json ok; requested analysis fields recognized")
    else:
        print("report json ok")
    return 0


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        description="Inspect or validate peval-py report JSON files.",
    )
    subcommands = parser.add_subparsers(dest="command", required=True)

    subjects = subcommands.add_parser(
        "subjects",
        help="list session, agent, adapter, and trial ids",
    )
    subjects.add_argument("report_json", type=Path)
    subjects.set_defaults(func=cmd_subjects)

    check = subcommands.add_parser(
        "check",
        help="validate report JSON and optional analysis recognition",
    )
    check.add_argument("report_json", type=Path)
    check.add_argument(
        "--require-analysis",
        action="store_true",
        help="require both annotations.analysis[0].summary and md_report",
    )
    check.add_argument(
        "--require-summary",
        action="store_true",
        help="require annotations.analysis[0].summary",
    )
    check.add_argument(
        "--require-md-report",
        action="store_true",
        help="require annotations.analysis[0].md_report",
    )
    check.set_defaults(func=cmd_check)

    return parser


def main() -> int:
    args = build_parser().parse_args()
    return args.func(args)


if __name__ == "__main__":
    raise SystemExit(main())
