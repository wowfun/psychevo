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


def trial_key_for_index(report: dict[str, Any], index: int) -> str:
    if index < 1:
        raise SystemExit("--index must be a one-based positive integer")
    metas = report_metas(report)
    if index > len(metas):
        raise SystemExit(f"--index {index} is outside trajectory_meta length {len(metas)}")
    trial_key = metas[index - 1].get("trial_key")
    if not trial_key:
        raise SystemExit(f"trajectory_meta[{index - 1}].trial_key is missing")
    return str(trial_key)


def target_trial_key(report: dict[str, Any], args: argparse.Namespace) -> str | None:
    target = str(args.trial_key) if args.trial_key else None
    if args.index is not None:
        indexed = trial_key_for_index(report, args.index)
        if target is not None and target != indexed:
            raise SystemExit(
                f"--trial-key {target!r} does not match --index {args.index} ({indexed!r})"
            )
        target = indexed
    return target


def annotation_items(report: dict[str, Any], key: str) -> list[dict[str, Any]]:
    annotations = report.get("annotations") or {}
    if not isinstance(annotations, dict):
        return []
    raw_items = annotations.get(key) or []
    if not isinstance(raw_items, list):
        return []
    return [item for item in raw_items if isinstance(item, dict)]


def selected_annotation_items(
    report: dict[str, Any],
    key: str,
    args: argparse.Namespace,
) -> tuple[str | None, list[dict[str, Any]]]:
    items = annotation_items(report, key)
    target = target_trial_key(report, args)
    if target is None:
        return None, items[:1]
    return target, [item for item in items if str(item.get("trial_key") or "") == target]


def missing_annotation_message(key: str, target: str | None, field: str | None = None) -> str:
    if target is None:
        label = f"annotations.{key}"
        if field is not None:
            label += f"[0].{field}"
        return f"missing {label}"
    label = f"annotations.{key}"
    if field is not None:
        label += f".{field}"
    return f"missing {label} for trial_key={target}"


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
        agent_segment = safe_segment(agent.get("name")) or safe_segment(meta.get("adapter"))
        session_segment = safe_segment(trajectory.get("session_id")) or safe_segment(trial_key)
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
        if workspace is not None and agent_segment and session_segment and cell_segment:
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
                    "notes_path": str(cell_dir / "notes.md"),
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


def cmd_check(args: argparse.Namespace) -> int:
    report = load_report(args.report_json)
    require_summary = args.require_summary or args.require_analysis
    require_md_report = args.require_md_report or args.require_analysis
    if require_summary or require_md_report or args.require_findings:
        target, items = selected_annotation_items(report, "analysis", args)
        if not items:
            raise SystemExit(missing_annotation_message("analysis", target))
        if require_summary and not any(item.get("summary") for item in items):
            raise SystemExit(missing_annotation_message("analysis", target, "summary"))
        if require_md_report and not any(item.get("md_report") for item in items):
            raise SystemExit(missing_annotation_message("analysis", target, "md_report"))
        if args.require_findings and not any(item.get("findings") for item in items):
            raise SystemExit(missing_annotation_message("analysis", target, "findings"))
    if args.require_notes:
        target, items = selected_annotation_items(report, "notes", args)
        if not items:
            raise SystemExit(missing_annotation_message("notes", target))
        if not any(item.get("markdown") for item in items):
            raise SystemExit(missing_annotation_message("notes", target, "markdown"))
    if require_summary or require_md_report or args.require_notes or args.require_findings:
        print("report json ok; requested annotation fields recognized")
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

    check = subcommands.add_parser(
        "check",
        help="validate report JSON and optional analysis recognition",
    )
    check.add_argument("report_json", type=Path)
    check.add_argument(
        "--require-analysis",
        action="store_true",
        help="require both matching annotations.analysis summary and md_report",
    )
    check.add_argument(
        "--require-summary",
        action="store_true",
        help="require matching annotations.analysis summary",
    )
    check.add_argument(
        "--require-md-report",
        action="store_true",
        help="require matching annotations.analysis md_report",
    )
    check.add_argument(
        "--require-findings",
        action="store_true",
        help="require matching annotations.analysis findings",
    )
    check.add_argument(
        "--require-notes",
        action="store_true",
        help="require matching annotations.notes markdown",
    )
    check.add_argument(
        "--trial-key",
        help="target one rendered trajectory_meta[].trial_key",
    )
    check.add_argument(
        "--index",
        type=int,
        help="target one Trial by one-based report index",
    )
    check.set_defaults(func=cmd_check)

    return parser


def main() -> int:
    args = build_parser().parse_args()
    return args.func(args)


if __name__ == "__main__":
    raise SystemExit(main())
