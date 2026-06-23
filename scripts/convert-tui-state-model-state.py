#!/usr/bin/env python3
"""One-time conversion from tui-state.json model fields to model-state.json."""

from __future__ import annotations

import argparse
import json
import shutil
import time
from pathlib import Path
from typing import Any


MODEL_STATE_VERSION = 1
TUI_STATE_VERSION = 5
RECENT_LIMIT = 8


def normalize_reasoning(value: Any) -> str | None:
    if not isinstance(value, str):
        return None
    value = value.strip()
    if not value or value == "none":
        return None
    return value


def normalize_model(value: Any) -> str | None:
    if not isinstance(value, str):
        return None
    value = value.strip()
    return value or None


def read_json(path: Path, default: dict[str, Any]) -> dict[str, Any]:
    if not path.exists():
        return dict(default)
    with path.open("r", encoding="utf-8") as handle:
        value = json.load(handle)
    if not isinstance(value, dict):
        raise ValueError(f"{path} must contain a JSON object")
    return value


def write_json(path: Path, value: dict[str, Any], dry_run: bool) -> None:
    if dry_run:
        return
    path.parent.mkdir(parents=True, exist_ok=True)
    temp = path.with_suffix(path.suffix + ".tmp")
    with temp.open("w", encoding="utf-8") as handle:
        json.dump(value, handle, ensure_ascii=False, indent=2)
        handle.write("\n")
    temp.replace(path)


def recent_entry(model: str, reasoning: str | None, now_ms: int) -> dict[str, Any]:
    entry: dict[str, Any] = {
        "model": model,
        "last_selected_at_ms": now_ms,
    }
    if reasoning:
        entry["reasoning_effort"] = reasoning
    return entry


def push_recent(recent: list[dict[str, Any]], entry: dict[str, Any]) -> None:
    model = entry.get("model")
    if not isinstance(model, str) or not model.strip():
        return
    model = model.strip()
    entry["model"] = model
    recent[:] = [item for item in recent if item.get("model") != model]
    recent.insert(0, entry)
    del recent[RECENT_LIMIT:]


def convert(home: Path, dry_run: bool) -> tuple[int, int]:
    tui_path = home / "tui-state.json"
    model_path = home / "model-state.json"
    if not tui_path.exists():
        print(f"no tui-state.json found at {tui_path}")
        return (0, 0)

    tui_state = read_json(tui_path, {})
    model_state = read_json(model_path, {"version": MODEL_STATE_VERSION})
    model_state["version"] = MODEL_STATE_VERSION
    workdirs = model_state.setdefault("workdirs", {})
    if not isinstance(workdirs, dict):
        workdirs = {}
        model_state["workdirs"] = workdirs
    recent = model_state.setdefault("recent_models", [])
    if not isinstance(recent, list):
        recent = []
        model_state["recent_models"] = recent
    recent[:] = [
        item
        for item in recent
        if isinstance(item, dict) and isinstance(item.get("model"), str) and item["model"].strip()
    ][:RECENT_LIMIT]

    now_ms = int(time.time() * 1000)
    converted_workdirs = 0
    tui_workdirs = tui_state.get("workdirs")
    if isinstance(tui_workdirs, dict):
        empty_workdirs: list[str] = []
        for key, entry in tui_workdirs.items():
            if not isinstance(entry, dict):
                continue
            model = normalize_model(entry.pop("model", None))
            reasoning = normalize_reasoning(entry.pop("variant", None))
            if model:
                workdirs[str(key)] = {
                    "model": model,
                    **({"reasoning_effort": reasoning} if reasoning else {}),
                    "updated_at_ms": now_ms,
                }
                push_recent(recent, recent_entry(model, reasoning, now_ms))
                converted_workdirs += 1
            if not entry:
                empty_workdirs.append(str(key))
        for key in empty_workdirs:
            tui_workdirs.pop(key, None)

    old_recent = tui_state.pop("recent_models", [])
    if isinstance(old_recent, list):
        for value in reversed(old_recent):
            model = normalize_model(value)
            if model:
                push_recent(recent, recent_entry(model, None, now_ms))

    tui_state["version"] = TUI_STATE_VERSION
    if not dry_run:
        backup = tui_path.with_name(f"tui-state.json.bak-{time.strftime('%Y%m%d%H%M%S')}")
        shutil.copy2(tui_path, backup)
        print(f"backup: {backup}")
    write_json(model_path, model_state, dry_run)
    write_json(tui_path, tui_state, dry_run)
    print(f"model-state: {model_path}")
    print(f"converted workdirs: {converted_workdirs}, recent models: {len(recent)}")
    return (converted_workdirs, len(recent))


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--home", type=Path, default=Path.home() / ".psychevo")
    parser.add_argument("--dry-run", action="store_true")
    args = parser.parse_args()
    convert(args.home.expanduser(), args.dry_run)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
