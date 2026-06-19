#!/usr/bin/env python3
"""Verify live pevo-dev-env JSON logs."""

import json
import sys
from pathlib import Path


def load(path):
    rows = []
    for raw in Path(path).read_text(encoding="utf-8").splitlines():
        if raw.strip():
            rows.append(json.loads(raw))
    return rows


def completed_entries(events):
    for event in events:
        if event.get("type") == "entry.completed":
            entry = event.get("entry") or {}
            if isinstance(entry, dict):
                yield entry


def entry_blocks(events):
    for entry in completed_entries(events):
        for block in entry.get("blocks") or []:
            if isinstance(block, dict):
                yield block


def final_text(events):
    text = ""
    for event in events:
        if event.get("type") in {"turn.completed", "turn.failed"}:
            final_answer = event.get("finalAnswer")
            if isinstance(final_answer, str) and final_answer:
                text = final_answer
    if text:
        return text

    parts = []
    for entry in completed_entries(events):
        if entry.get("role") != "assistant":
            continue
        for block in entry.get("blocks") or []:
            if block.get("kind") == "text" and block.get("body"):
                parts.append(block["body"])
    return "\n".join(parts)


def verify(provider, token, first_path, second_path):
    first = load(first_path)
    second = load(second_path)
    combined = first + second

    if not any(
        block.get("kind") == "reasoning" and block.get("body")
        for block in entry_blocks(combined)
    ):
        raise SystemExit(f"{provider}: missing reasoning transcript entry")

    if not any(
        (block.get("metadata") or {}).get("tool_name") == "read"
        and (block.get("metadata") or {}).get("outcome") == "normal"
        for block in entry_blocks(first)
    ):
        raise SystemExit(f"{provider}: first run did not complete read")

    first_session = next(
        (event.get("threadId") for event in first if event.get("type") == "thread.started"),
        None,
    )
    second_session = next(
        (event.get("threadId") for event in second if event.get("type") == "thread.started"),
        None,
    )
    if not first_session or first_session != second_session:
        raise SystemExit(f"{provider}: --continue did not reuse the session")

    if token not in final_text(first):
        raise SystemExit(f"{provider}: first final answer did not contain token {token}")
    if token not in final_text(second):
        raise SystemExit(f"{provider}: continue final answer did not contain token {token}")

    print(f"{provider}: ok ({first_path}, {second_path})")


def main(argv):
    if len(argv) != 5:
        raise SystemExit(
            "usage: pevo-dev-env-verify.py PROVIDER TOKEN FIRST_LOG SECOND_LOG"
        )
    verify(argv[1], argv[2], argv[3], argv[4])


if __name__ == "__main__":
    main(sys.argv)
