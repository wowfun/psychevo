from __future__ import annotations

import csv
import json
from dataclasses import dataclass
from importlib import import_module
from pathlib import Path
from typing import Any


COLUMN_ALIASES = {
    "path": "path",
    "p": "path",
    "db": "db",
    "d": "db",
    "session_id": "session_id",
    "session": "session_id",
    "s": "session_id",
    "adapter": "adapter",
    "a": "adapter",
    "note": "notes",
    "notes": "notes",
    "n": "notes",
    "report_note": "report_notes",
    "report_notes": "report_notes",
    "agent_name": "agent_name",
    "agent_version": "agent_version",
    "model": "model",
}


@dataclass(frozen=True)
class InputTableRow:
    table_path: str
    row_number: int
    path: str | None = None
    db: str | None = None
    session_id: str | None = None
    adapter: str | None = None
    notes: tuple[str, ...] = ()
    report_notes: tuple[str, ...] = ()
    agent_name: str | None = None
    agent_version: str | None = None
    model: str | None = None


@dataclass(frozen=True)
class InputTableData:
    rows: list[InputTableRow]
    report_notes: list[str]


def read_input_tables(paths: list[str]) -> InputTableData:
    rows: list[InputTableRow] = []
    report_notes: list[str] = []
    for path in paths:
        data = read_input_table(path)
        rows.extend(data.rows)
        report_notes.extend(data.report_notes)
    return InputTableData(rows=rows, report_notes=report_notes)


def read_input_table(path: str) -> InputTableData:
    source = Path(path).expanduser()
    suffix = source.suffix.lower()
    if suffix == ".csv":
        return read_csv_input_table(source)
    if suffix == ".json":
        return read_json_input_table(source)
    if suffix == ".xlsx":
        return read_xlsx_input_table(source)
    if suffix == ".xls":
        raise ValueError(f"{source}: .xls input tables are unsupported; use .xlsx or CSV")
    raise ValueError(f"{source}: unsupported input table format; use CSV, JSON, or .xlsx")


def read_csv_input_table(path: Path) -> InputTableData:
    with path.open(newline="", encoding="utf-8-sig") as handle:
        reader = csv.reader(handle)
        try:
            raw_headers = next(reader)
        except StopIteration as exc:
            raise ValueError(f"{path}: row 1: input table is empty") from exc
        headers = normalize_headers(path, raw_headers)
        rows: list[InputTableRow] = []
        for row_index, raw_values in enumerate(reader, start=2):
            if len(raw_values) > len(headers):
                raise ValueError(f"{path}: row {row_index}: too many cells for header")
            values = list(raw_values) + [""] * (len(headers) - len(raw_values))
            row = row_from_mapping(path, row_index, dict(zip(headers, values)))
            if row is not None:
                rows.append(row)
        return InputTableData(rows=rows, report_notes=[])


def read_json_input_table(path: Path) -> InputTableData:
    try:
        payload = json.loads(path.read_text(encoding="utf-8"))
    except json.JSONDecodeError as exc:
        raise ValueError(f"{path}: invalid JSON input table: {exc.msg}") from exc
    report_notes: list[str] = []
    if isinstance(payload, list):
        raw_rows = payload
    elif isinstance(payload, dict):
        raw_rows = payload.get("rows")
        if raw_rows is None:
            raise ValueError(f"{path}: JSON input table object must contain rows")
        report_notes.extend(string_values(payload.get("report_notes")))
    else:
        raise ValueError(f"{path}: JSON input table must be an array or object")
    if not isinstance(raw_rows, list):
        raise ValueError(f"{path}: rows must be an array")
    rows: list[InputTableRow] = []
    for index, item in enumerate(raw_rows, start=1):
        if not isinstance(item, dict):
            raise ValueError(f"{path}: row {index}: JSON input table row must be an object")
        row = row_from_json_mapping(path, index, item)
        if row is not None:
            rows.append(row)
    return InputTableData(rows=rows, report_notes=report_notes)


def read_xlsx_input_table(path: Path) -> InputTableData:
    try:
        openpyxl = import_module("openpyxl")
    except ImportError as exc:
        raise ValueError(
            f"{path}: reading .xlsx input tables requires optional dependency "
            "openpyxl; install openpyxl or save the table as CSV"
        ) from exc
    workbook = openpyxl.load_workbook(path, read_only=True, data_only=True)
    try:
        rows_iter = workbook.active.iter_rows(values_only=True)
        try:
            raw_headers = next(rows_iter)
        except StopIteration as exc:
            raise ValueError(f"{path}: row 1: input table is empty") from exc
        headers = normalize_headers(path, list(raw_headers))
        rows: list[InputTableRow] = []
        for row_index, raw_values in enumerate(rows_iter, start=2):
            values = list(raw_values)
            if len(values) > len(headers) and any(cell_text(value) for value in values[len(headers) :]):
                raise ValueError(f"{path}: row {row_index}: too many cells for header")
            values = values[: len(headers)] + [""] * max(0, len(headers) - len(values))
            row = row_from_mapping(path, row_index, dict(zip(headers, values)))
            if row is not None:
                rows.append(row)
        return InputTableData(rows=rows, report_notes=[])
    finally:
        workbook.close()


def normalize_headers(path: Path, raw_headers: list[Any]) -> list[str]:
    headers: list[str] = []
    seen: dict[str, str] = {}
    for index, raw in enumerate(raw_headers, start=1):
        text = cell_text(raw)
        if not text:
            raise ValueError(f"{path}: row 1: empty header at column {index}")
        canonical = canonical_header(text)
        if canonical is None:
            raise ValueError(f"{path}: row 1: unknown input table column: {text}")
        if canonical in seen:
            raise ValueError(
                f"{path}: row 1: duplicate input table column: {text} "
                f"(duplicates {seen[canonical]})"
            )
        seen[canonical] = text
        headers.append(canonical)
    return headers


def row_from_json_mapping(
    path: Path,
    row_number: int,
    values: dict[str, Any],
) -> InputTableRow | None:
    normalized: dict[str, Any] = {}
    seen: dict[str, str] = {}
    for raw_key, value in values.items():
        canonical = canonical_header(str(raw_key))
        if canonical is None:
            raise ValueError(f"{path}: row {row_number}: unknown input table column: {raw_key}")
        if canonical in seen:
            raise ValueError(
                f"{path}: row {row_number}: duplicate input table column: {raw_key} "
                f"(duplicates {seen[canonical]})"
            )
        seen[canonical] = str(raw_key)
        normalized[canonical] = value
    return row_from_mapping(path, row_number, normalized)


def row_from_mapping(
    path: Path,
    row_number: int,
    values: dict[str, Any],
) -> InputTableRow | None:
    normalized = {key: values.get(key) for key in COLUMN_ALIASES.values()}
    if not any_cell(normalized.values()):
        return None
    input_path = optional_path(path, normalized.get("path"))
    db_path = optional_path(path, normalized.get("db"))
    session_id = optional_text(normalized.get("session_id"))
    if bool(input_path) == bool(db_path):
        raise ValueError(f"{path}: row {row_number}: provide exactly one of path or db")
    if input_path and session_id:
        raise ValueError(f"{path}: row {row_number}: session_id is only valid for db rows")
    return InputTableRow(
        table_path=str(path),
        row_number=row_number,
        path=input_path,
        db=db_path,
        session_id=session_id,
        adapter=optional_text(normalized.get("adapter")),
        notes=tuple(string_values(normalized.get("notes"))),
        report_notes=tuple(string_values(normalized.get("report_notes"))),
        agent_name=optional_text(normalized.get("agent_name")),
        agent_version=optional_text(normalized.get("agent_version")),
        model=optional_text(normalized.get("model")),
    )


def canonical_header(raw: str) -> str | None:
    text = raw.strip()
    while text.startswith("-"):
        text = text[1:]
    text = text.strip().lower().replace("-", "_").replace(" ", "_")
    while "__" in text:
        text = text.replace("__", "_")
    return COLUMN_ALIASES.get(text)


def any_cell(values: Any) -> bool:
    return any(cell_text(value) for value in values)


def optional_text(value: Any) -> str | None:
    text = cell_text(value)
    return text or None


def optional_path(table_path: Path, value: Any) -> str | None:
    text = optional_text(value)
    if not text:
        return None
    candidate = Path(text).expanduser()
    if not candidate.is_absolute():
        candidate = table_path.parent / candidate
    return str(candidate.resolve())


def string_values(value: Any) -> list[str]:
    if isinstance(value, list):
        return [text for item in value if (text := cell_text(item))]
    text = cell_text(value)
    return [text] if text else []


def cell_text(value: Any) -> str:
    if value is None:
        return ""
    return str(value).strip()
