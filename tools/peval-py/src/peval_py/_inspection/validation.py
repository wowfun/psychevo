from __future__ import annotations

import argparse
from pathlib import Path
from typing import Any

from peval_py.outputs import DEFAULT_OUTPUT, unique_timestamped_name

def resolve_inspect_output(args: argparse.Namespace) -> str | None:
    if args.output is DEFAULT_OUTPUT:
        return unique_timestamped_name("inspect.json")
    return args.output


def validate_inspect_args(args: argparse.Namespace) -> None:
    validate_inspect_raw_only_args(args)
    if getattr(args, "format", None) == "html":
        raise ValueError("view tr inspect mode supports only JSON output; use -m raw for HTML reports")
    output = getattr(args, "output", None)
    if output and output is not DEFAULT_OUTPUT and Path(str(output)).suffix.lower() == ".html":
        raise ValueError("view tr inspect mode writes JSON; use -m raw for HTML reports")
    parse_step_selectors(getattr(args, "steps", None) or [])


def validate_inspect_raw_only_args(args: argparse.Namespace) -> None:
    raw_flags = [
        ("agent_name", getattr(args, "agent_name", None) is not None),
        ("agent_version", getattr(args, "agent_version", None) is not None),
        ("model", getattr(args, "model", None) is not None),
        ("no_redact", bool(getattr(args, "no_redact", False))),
    ]
    raw_used = [name for name, was_used in raw_flags if was_used]
    if raw_used:
        raise ValueError(
            "raw-only option(s) require -m raw: "
            + ", ".join(f"--{name.replace('_', '-')}" for name in raw_used)
        )


def validate_raw_args(args: argparse.Namespace) -> None:
    inspect_flags = [
        ("head", getattr(args, "head", None) is not None),
        ("tail", getattr(args, "tail", None) is not None),
        ("top", getattr(args, "top", None) is not None),
        ("steps", getattr(args, "steps", None) is not None),
        ("tool_call", getattr(args, "tool_call", None) is not None),
        ("source", getattr(args, "source", None) is not None),
    ]
    used = [name for name, was_used in inspect_flags if was_used]
    if used:
        raise ValueError(
            "inspect-only option(s) cannot be used with -m raw: "
            + ", ".join(f"--{name.replace('_', '-')}" for name in used)
        )


def positive_int(value: Any, default: int) -> int:
    if value is None:
        return default
    result = int(value)
    if result < 0:
        raise ValueError("inspect count options must be non-negative")
    return result


def parse_step_selectors(values: list[Any]) -> list[str]:
    selectors: list[str] = []
    seen: set[str] = set()
    for value in values:
        text = str(value)
        parts = text.split(",")
        if any(not part.strip() for part in parts):
            raise ValueError("--steps selectors cannot contain empty segments")
        for raw_part in parts:
            part = raw_part.strip()
            expanded = expand_step_selector(part)
            for selector in expanded:
                if selector not in seen:
                    selectors.append(selector)
                    seen.add(selector)
    return selectors


def expand_step_selector(value: str) -> list[str]:
    if ":" not in value:
        return [value]
    parts = value.split(":")
    if len(parts) != 2 or not parts[0].strip() or not parts[1].strip():
        raise ValueError(f"invalid --steps range {value!r}; use start:end")
    start = parse_step_range_endpoint(parts[0], value)
    end = parse_step_range_endpoint(parts[1], value)
    if start > end:
        raise ValueError(f"invalid descending --steps range: {value}")
    return [str(index) for index in range(start, end + 1)]


def parse_step_range_endpoint(value: str, raw_range: str) -> int:
    text = value.strip()
    if not text.isdigit():
        raise ValueError(
            f"invalid --steps range {raw_range!r}; "
            "range endpoints must be positive integers"
        )
    number = int(text)
    if number <= 0:
        raise ValueError(
            f"invalid --steps range {raw_range!r}; "
            "range endpoints must be positive integers"
        )
    return number


def positive_list(values: list[Any], label: str) -> list[int]:
    parsed = []
    for value in values:
        number = int(value)
        if number <= 0:
            raise ValueError(f"{label} values must be positive one-based indexes")
        parsed.append(number)
    return parsed
