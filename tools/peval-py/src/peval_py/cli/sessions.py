from __future__ import annotations

import argparse
import sys

from peval_py.adapters import available_adapter_ids
from peval_py.inputs import (
    adapter_for_input_path,
    resolve_db_input,
)
from peval_py.session_select import (
    format_session_table,
    list_adapter_sessions,
    parse_session_selection,
)

def print_session_lists(
    args: argparse.Namespace,
    adapter_assignments,
    config,
) -> None:
    for input_db in db_inputs_with_adapters(args, adapter_assignments, config):
        if len(input_db["all"]) > 1:
            print(f"d{input_db['index']} {input_db['path']} ({input_db['adapter']})")
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
        resolve_db_input,
    )
    from peval_py.adapters import available_adapter_ids

    dbs = list(getattr(args, "db", None) or [])
    if not dbs:
        raise ValueError("--list requires at least one --db")
    available = set(available_adapter_ids())
    inputs = []
    for index, path in enumerate(dbs, start=1):
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
