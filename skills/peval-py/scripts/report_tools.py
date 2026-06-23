#!/usr/bin/env python3
"""Small peval-py report helpers for skill workflows."""

from __future__ import annotations

import argparse
import json
from pathlib import Path
from typing import Any

DEFAULT_EVAL_SLUG = "default"


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


def safe_segment(value: object, fallback: str | None = None) -> str | None:
    if value is None:
        return fallback
    text = str(value).strip()
    safe = "".join(
        char if char.isalnum() or char in {"-", "_", "."} else "_"
        for char in text
    ).strip("._")
    return safe or fallback


def report_trajectories(report: dict[str, Any]) -> list[dict[str, Any]]:
    trajectories = report.get("trajectory") or []
    if isinstance(trajectories, dict):
        trajectories = [trajectories]
    if not isinstance(trajectories, list):
        raise SystemExit("report trajectory field is not a list or object")
    return [item for item in trajectories if isinstance(item, dict)]


def report_metas(report: dict[str, Any]) -> list[dict[str, Any]]:
    metas = report.get("trajectory_meta") or []
    if not isinstance(metas, list):
        return []
    return [item if isinstance(item, dict) else {} for item in metas]


def cmd_subjects(args: argparse.Namespace) -> int:
    report = load_report(args.report_json)
    trajectories = report_trajectories(report)
    metas = report_metas(report)
    eval_segment = safe_segment(args.eval_slug, DEFAULT_EVAL_SLUG) or DEFAULT_EVAL_SLUG
    workspace = args.workspace.expanduser() if args.workspace else None

    for index, trajectory in enumerate(trajectories, start=1):
        meta = metas[index - 1] if index - 1 < len(metas) else {}
        agent = trajectory.get("agent") or {}
        if not isinstance(agent, dict):
            agent = {}
        trial_key = meta.get("trial_key")
        agent_segment = safe_segment(agent.get("name")) or safe_segment(
            meta.get("adapter")
        )
        session_segment = safe_segment(trajectory.get("session_id")) or safe_segment(
            trial_key
        )
        cell_segment = safe_segment(trial_key)
        item = {
            "index": index,
            "session_id": trajectory.get("session_id"),
            "agent_name": agent.get("name"),
            "adapter": meta.get("adapter"),
            "trial_key": trial_key,
            "eval_segment": eval_segment,
            "agent_segment": agent_segment,
            "session_segment": session_segment,
            "cell_segment": cell_segment,
        }
        if agent_segment and session_segment and cell_segment:
            item["run_path"] = (
                Path("runs")
                / eval_segment
                / agent_segment
                / session_segment
                / cell_segment
            ).as_posix()
        if (
            workspace is not None
            and agent_segment
            and session_segment
            and cell_segment
        ):
            cell_dir = (
                workspace
                / "runs"
                / eval_segment
                / agent_segment
                / session_segment
                / cell_segment
            )
            item.update(
                {
                    "cell_dir": str(cell_dir),
                    "analysis_json_path": str(cell_dir / "analysis.json"),
                    "analysis_md_path": str(cell_dir / "analysis.md"),
                }
            )
        print(
            json.dumps(
                item,
                ensure_ascii=False,
            )
        )
    return 0


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        description="Inspect peval-py report JSON files for Trial cell identities.",
    )
    subcommands = parser.add_subparsers(dest="command", required=True)

    subjects = subcommands.add_parser(
        "subjects",
        help="list session, agent, adapter, and trial ids",
    )
    subjects.add_argument("report_json", type=Path)
    subjects.add_argument(
        "--workspace",
        type=Path,
        help="workspace root; include full Trial cell artifact paths when provided",
    )
    subjects.add_argument(
        "--eval-slug",
        default=DEFAULT_EVAL_SLUG,
        help="analysis eval slug for emitted artifact paths (default: default)",
    )
    subjects.set_defaults(func=cmd_subjects)

    return parser


def main() -> int:
    args = build_parser().parse_args()
    return args.func(args)


if __name__ == "__main__":
    raise SystemExit(main())
