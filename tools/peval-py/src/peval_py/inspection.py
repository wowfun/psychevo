from __future__ import annotations

import argparse
from typing import Any

from peval_py.inputs import AdapterAssignments
from peval_py._inspection.frames import InspectFrames
from peval_py._inspection.payload import parse_source_indexes, source_payload
from peval_py._inspection.reports import inspect_report_for_args
from peval_py._inspection.validation import (
    parse_step_selectors,
    positive_int,
    resolve_inspect_output,
    validate_inspect_args,
    validate_inspect_raw_only_args,
    validate_raw_args,
)

INSPECT_SCHEMA_VERSION = 2
DEFAULT_INSPECT_PREVIEW_CHARS = 3000


def build_inspect_payload(
    args: argparse.Namespace,
    adapter_assignments: AdapterAssignments,
    config: object,
) -> dict[str, Any]:
    step_ids = parse_step_selectors(getattr(args, "steps", None) or [])
    report = inspect_report_for_args(args, adapter_assignments, config)
    preview_default = (
        getattr(config, "max_content_chars")
        if getattr(config, "max_content_chars_explicit", False)
        else DEFAULT_INSPECT_PREVIEW_CHARS
    )
    preview_chars = positive_int(getattr(args, "max_content_chars", None), preview_default)
    frames = InspectFrames.from_report(report, preview_chars=preview_chars)
    source_indexes = parse_source_indexes(getattr(args, "source", None) or [], frames)
    head = positive_int(getattr(args, "head", None), 2)
    tail = positive_int(getattr(args, "tail", None), 2)
    top = positive_int(getattr(args, "top", None), 5)
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
                steps_only=bool(step_ids),
            )
            for source_index in source_indexes
        ],
    }
