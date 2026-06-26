from __future__ import annotations

import argparse
import json
import re
import sys
from datetime import datetime
from pathlib import Path
from typing import Any

from peval_py.config import ToolConfig

DEFAULT_OUTPUT = object()
FILENAME_PART_RE = re.compile(r"[^A-Za-z0-9._-]+")


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
    trajectory: dict[str, Any],
    config: ToolConfig,
) -> str | None:
    if args.output is DEFAULT_OUTPUT:
        return default_output_name("trajectory", "json", trajectory, config)
    return args.output


def resolve_report_output(
    args: argparse.Namespace,
    fmt: str,
    report: dict[str, Any],
    config: ToolConfig,
    *,
    timestamped: bool = False,
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
                timestamped=timestamped,
            )
        trajectory = trajectories[0] if trajectories else {}
        return default_output_name(
            "report",
            fmt,
            trajectory,
            config,
            adapter=adapter,
            timestamped=timestamped,
        )
    return args.output


def default_report_adapter(report: dict[str, Any], config: ToolConfig) -> str:
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
    trajectory: dict[str, Any],
    config: ToolConfig,
    *,
    adapter: str | None = None,
    timestamped: bool = False,
) -> str:
    adapter_part = filename_part(adapter or config.adapter, "adapter")
    session = filename_part(trajectory.get("session_id"), "session")
    name = f"{kind}-{adapter_part}-{session}.{ext}"
    return unique_timestamped_name(name) if timestamped else name


def default_multi_output_name(
    kind: str,
    ext: str,
    count: int,
    adapter: str,
    *,
    timestamped: bool = False,
) -> str:
    adapter_part = filename_part(adapter, "adapter")
    name = f"{kind}-{adapter_part}-sessions-{count}.{ext}"
    return unique_timestamped_name(name) if timestamped else name


def unique_timestamped_name(name: str) -> str:
    path = Path(name)
    suffix = path.suffix
    stem = path.name[: -len(suffix)] if suffix else path.name
    timestamp = timestamp_part()
    timestamped = path.with_name(f"{stem}-{timestamp}{suffix}")
    if not timestamped.exists():
        return str(timestamped)
    counter = 2
    while True:
        candidate = path.with_name(f"{stem}-{timestamp}-{counter}{suffix}")
        if not candidate.exists():
            return str(candidate)
        counter += 1


def timestamp_part() -> str:
    return datetime.now().strftime("%Y%m%d-%H%M%S-%f")


def filename_part(value: object, fallback: str) -> str:
    text = str(value or "").strip() or fallback
    safe = FILENAME_PART_RE.sub("-", text).strip(".-")
    return safe or fallback


def write_json(payload: dict[str, Any], output: str | None) -> str | None:
    return write_text(json.dumps(payload, indent=2, ensure_ascii=False) + "\n", output)


def write_text(payload: str, output: str | None) -> str | None:
    if output:
        Path(output).write_text(payload, encoding="utf-8")
        return str(output)
    else:
        sys.stdout.write(payload)
        return None


def announce_written(path: str | None, *, label: str = "report") -> None:
    if path:
        print(f"wrote {label}: {path}")
