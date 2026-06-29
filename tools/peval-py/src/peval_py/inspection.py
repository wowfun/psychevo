from __future__ import annotations

import argparse
from typing import Any

from peval_py.inputs import AdapterAssignments
from peval_py._inspection.frames import InspectFrames
from peval_py._inspection.payload import parse_source_indexes, source_payload
from peval_py._inspection.reports import inspect_report_for_args
from peval_py._inspection.validation import (
    positive_int,
    resolve_inspect_output,
    validate_inspect_args,
    validate_inspect_raw_only_args,
    validate_raw_args,
)

INSPECT_SCHEMA_VERSION = 2


def build_inspect_payload(
    args: argparse.Namespace,
    adapter_assignments: AdapterAssignments,
    config: object,
) -> dict[str, Any]:
    report = inspect_report_for_args(args, adapter_assignments, config)
    preview_chars = positive_int(getattr(args, "preview_chars", None), 240)
    frames = InspectFrames.from_report(report, preview_chars=preview_chars)
    source_indexes = parse_source_indexes(getattr(args, "source", None) or [], frames)
    head = positive_int(getattr(args, "head", None), 2)
    tail = positive_int(getattr(args, "tail", None), 2)
    top = positive_int(getattr(args, "top", None), 5)
    step_ids = [str(value) for value in getattr(args, "step", None) or []]
    tool_call_ids = [str(value) for value in getattr(args, "tool_call", None) or []]
    return {
        "inspect_schema_version": INSPECT_SCHEMA_VERSION,
        "sources": [
            source_payload(
                frames,
                source_index,
                head=head,
                tail=tail,
                top=top,
                step_ids=step_ids,
                tool_call_ids=tool_call_ids,
            )
            for source_index in source_indexes
        ],
    }
