from __future__ import annotations

import json
import math
from copy import deepcopy
from dataclasses import dataclass
from pathlib import Path
from typing import Any

from peval_py.analysis.constants import (
    ANALYSIS_IMPORT_HINT_FIELDS,
    ANALYSIS_INPUT_EXTRA_FIELD,
    ANALYSIS_INPUT_FIELDS,
    ANALYSIS_JSON_FILENAME,
    ANALYSIS_MD_FILENAME,
    PEVAL_PY_CONFIG,
)

@dataclass(frozen=True)
class AnalysisImportResult:
    run_path: str
    absolute_run_path: Path
    written: dict[str, str]
    warnings: list[dict[str, str]]

    def to_jsonable(self) -> dict[str, Any]:
        return {
            "run_path": self.run_path,
            "absolute_run_path": str(self.absolute_run_path),
            "written": self.written,
            "warnings": self.warnings,
        }


def import_analysis_artifacts(
    *,
    workspace_root: str | Path,
    run_path: str,
    input_paths: list[str],
) -> AnalysisImportResult:
    root = ensure_import_workspace_root(workspace_root)
    cell = resolve_import_run_path(root, run_path)
    inputs = read_analysis_import_inputs(input_paths)
    written_payloads: dict[str, str] = {}
    warnings: list[dict[str, str]] = []

    if inputs.get("json") is not None:
        warnings.extend(analysis_input_warnings(inputs["json"]))
        write_json_artifact(
            cell.absolute / ANALYSIS_JSON_FILENAME,
            compile_analysis_json_input(
                inputs["json"],
                eval_slug=cell.eval_slug,
                agent_id=cell.agent_id,
                session_id=cell.session_id,
                cell_key=cell.cell_key,
            ),
        )
        written_payloads["analysis_json"] = (
            cell.relative_path / ANALYSIS_JSON_FILENAME
        ).as_posix()
    if inputs.get("md") is not None:
        write_text_artifact(cell.absolute / ANALYSIS_MD_FILENAME, inputs["md"])
        written_payloads["analysis_md"] = (
            cell.relative_path / ANALYSIS_MD_FILENAME
        ).as_posix()

    return AnalysisImportResult(
        run_path=cell.relative_path.as_posix(),
        absolute_run_path=cell.absolute,
        written=written_payloads,
        warnings=warnings,
    )


@dataclass(frozen=True)
class ImportRunCell:
    absolute: Path
    relative_path: Path
    eval_slug: str
    agent_id: str
    session_id: str
    cell_key: str


def ensure_import_workspace_root(value: str | Path) -> Path:
    root = Path(value).expanduser().resolve()
    config_path = root / PEVAL_PY_CONFIG
    if not config_path.is_file():
        raise ValueError(
            f"{root} is not an initialized peval-py workspace; "
            f"run `peval-py init -r {root}`"
        )
    return root


def resolve_import_run_path(root: Path, run_path: str) -> ImportRunCell:
    raw = Path(run_path).expanduser()
    target = raw if raw.is_absolute() else root / raw
    absolute = target.resolve()
    try:
        relative = absolute.relative_to(root)
    except ValueError as exc:
        raise ValueError("--run-path must resolve inside the workspace root") from exc
    parts = relative.parts
    if not parts or parts[0] != "runs":
        raise ValueError("--run-path must resolve under the workspace runs/ directory")
    if len(parts) != 5:
        raise ValueError(
            "--run-path must have form "
            "runs/<analysis_eval_slug>/<agent-id>/<session-id>/<cell-key>"
        )
    _, eval_slug, agent_id, session_id, cell_key = parts
    for label, value in (
        ("analysis_eval_slug", eval_slug),
        ("agent_id", agent_id),
        ("session_id", session_id),
        ("cell_key", cell_key),
    ):
        if value in {"", ".", ".."} or "/" in value or "\\" in value:
            raise ValueError(f"--run-path contains an invalid {label} segment")
    return ImportRunCell(
        absolute=absolute,
        relative_path=Path(*parts),
        eval_slug=eval_slug,
        agent_id=agent_id,
        session_id=session_id,
        cell_key=cell_key,
    )


def read_analysis_import_inputs(input_paths: list[str]) -> dict[str, Any]:
    if not input_paths:
        raise ValueError(
            "import analysis requires at least one --path/-p analysis input"
        )
    inputs: dict[str, Any] = {}
    for raw_path in input_paths:
        path = Path(raw_path).expanduser()
        suffix = path.suffix.lower()
        if suffix == ".json":
            if "json" in inputs:
                raise ValueError(
                    "import analysis accepts at most one JSON analysis input"
                )
            inputs["json"] = read_analysis_json_input(path)
        elif suffix in {".md", ".markdown"}:
            if "md" in inputs:
                raise ValueError(
                    "import analysis accepts at most one Markdown analysis input"
                )
            inputs["md"] = read_analysis_markdown_input(path)
        else:
            raise ValueError(
                f"unsupported analysis input suffix for {path}; "
                "expected .json, .md, or .markdown"
            )
    return inputs


def read_analysis_json_input(path: Path) -> dict[str, Any]:
    try:
        payload = json.loads(path.read_text(encoding="utf-8"))
    except FileNotFoundError as exc:
        raise ValueError(f"analysis JSON input not found: {path}") from exc
    except OSError as exc:
        raise ValueError(f"cannot read analysis JSON input {path}: {exc}") from exc
    except json.JSONDecodeError as exc:
        raise ValueError(f"failed to parse analysis JSON input {path}: {exc}") from exc
    if not isinstance(payload, dict):
        raise ValueError(f"analysis JSON input {path} must contain a JSON object")
    validate_analysis_json_input(payload)
    return payload


def read_analysis_markdown_input(path: Path) -> str:
    try:
        return path.read_text(encoding="utf-8")
    except FileNotFoundError as exc:
        raise ValueError(f"analysis Markdown input not found: {path}") from exc
    except UnicodeDecodeError as exc:
        raise ValueError(
            f"analysis Markdown input is not valid UTF-8: {path}"
        ) from exc
    except OSError as exc:
        raise ValueError(f"cannot read analysis Markdown input {path}: {exc}") from exc


def validate_analysis_json_input(payload: dict[str, Any]) -> None:
    allowed = set(ANALYSIS_INPUT_FIELDS)
    if not payload:
        raise ValueError("analysis JSON input must include at least one analysis field")
    for key, value in payload.items():
        if key == ANALYSIS_INPUT_EXTRA_FIELD:
            if not isinstance(value, dict):
                raise ValueError("analysis JSON input field 'extra' must be an object")
            continue
        if key not in allowed:
            continue
        if key in {"summary", "status"} and not isinstance(value, str):
            raise ValueError(f"analysis JSON input field {key!r} must be a string")
        if key in {"findings", "recommendations", "limitations"} and not isinstance(
            value,
            list,
        ):
            raise ValueError(f"analysis JSON input field {key!r} must be a list")
        if key == "confidence" and not valid_confidence(value):
            raise ValueError(
                "analysis JSON input field 'confidence' must be a string or "
                "finite number"
            )


def valid_confidence(value: Any) -> bool:
    if isinstance(value, str):
        return True
    return (
        isinstance(value, (int, float))
        and not isinstance(value, bool)
        and math.isfinite(float(value))
    )


def compile_analysis_json_input(
    payload: dict[str, Any],
    *,
    eval_slug: str,
    agent_id: str,
    session_id: str,
    cell_key: str,
) -> dict[str, Any]:
    allowed = set(ANALYSIS_INPUT_FIELDS)
    compiled = {key: deepcopy(value) for key, value in payload.items() if key in allowed}
    status = compiled.get("status")
    if not isinstance(status, str) or not status.strip():
        compiled["status"] = "analyzed"
    compiled["subject"] = {
        "eval_slug": eval_slug,
        "agent_id": agent_id,
        "session_id": session_id,
        "cell_key": cell_key,
    }
    extra = analysis_input_extra(payload)
    if extra:
        compiled["extra"] = extra
    return compiled


def analysis_input_extra(payload: dict[str, Any]) -> dict[str, Any]:
    extra: dict[str, Any] = {}
    raw_extra = payload.get(ANALYSIS_INPUT_EXTRA_FIELD)
    if isinstance(raw_extra, dict):
        extra.update(deepcopy(raw_extra))
    allowed = {*ANALYSIS_INPUT_FIELDS, ANALYSIS_INPUT_EXTRA_FIELD}
    for key, value in payload.items():
        if key not in allowed:
            extra[key] = deepcopy(value)
    return extra


def analysis_input_warnings(payload: dict[str, Any]) -> list[dict[str, str]]:
    warnings: list[dict[str, str]] = []
    for field in ANALYSIS_IMPORT_HINT_FIELDS:
        if field in payload:
            warnings.append(
                analysis_import_warning(
                    code="field_preserved_in_extra",
                    field=field,
                    location="top_level",
                    stored_as=f"extra.{field}",
                    message=(
                        f"analysis JSON input field {field!r} is not compiled as "
                        f"a standard top-level analysis field; it was preserved "
                        f"under extra.{field}"
                    ),
                )
            )

    raw_extra = payload.get(ANALYSIS_INPUT_EXTRA_FIELD)
    if isinstance(raw_extra, dict):
        for field in ANALYSIS_INPUT_FIELDS:
            if field in raw_extra:
                warnings.append(
                    analysis_import_warning(
                        code="standard_field_nested_in_extra",
                        field=field,
                        location="extra",
                        stored_as=f"extra.{field}",
                        message=(
                            f"analysis JSON input field {field!r} is nested under "
                            f"extra and was not compiled as a top-level analysis "
                            f"field; place it at the top level to compile it"
                        ),
                    )
                )
    return warnings


def analysis_import_warning(
    *,
    code: str,
    field: str,
    location: str,
    stored_as: str,
    message: str,
) -> dict[str, str]:
    return {
        "code": code,
        "field": field,
        "location": location,
        "stored_as": stored_as,
        "message": message,
    }


def write_json_artifact(path: Path, value: Any) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    tmp = path.with_name(path.name + ".tmp")
    tmp.write_text(
        json.dumps(value, ensure_ascii=False, indent=2) + "\n",
        encoding="utf-8",
    )
    tmp.replace(path)


def write_text_artifact(path: Path, value: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    tmp = path.with_name(path.name + ".tmp")
    tmp.write_text(value, encoding="utf-8")
    tmp.replace(path)


def run_import_analysis_command(args: Any, workspace_root: str | Path) -> None:
    result = import_analysis_artifacts(
        workspace_root=workspace_root,
        run_path=args.run_path,
        input_paths=args.path or [],
    )
    if getattr(args, "json", False):
        print(json.dumps(result.to_jsonable(), ensure_ascii=False, indent=2))
        return
    print(f"imported analysis artifacts for {result.run_path}")
    for label, path in result.written.items():
        print(f"{label}: {path}")
