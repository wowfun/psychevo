from __future__ import annotations

import json
import math
from copy import deepcopy
from dataclasses import dataclass
from pathlib import Path
from typing import Any

MAX_NOTE_BYTES = 1024 * 1024
PEVAL_PY_CONFIG = "peval-py.toml"
ANALYSIS_JSON_FILENAME = "analysis.json"
ANALYSIS_MD_FILENAME = "analysis.md"
MERGEABLE_ANALYSIS_LIST_FIELDS = (
    "findings",
    "recommendations",
    "limitations",
    "commands",
)
ANALYSIS_REPORT_FIELDS = (
    "summary",
    "analysis_status",
    "subject",
    "findings",
    "recommendations",
    "limitations",
    "commands",
    "analysis_metrics",
    "confidence",
)
ANALYSIS_INPUT_FIELDS = (
    "summary",
    "status",
    "findings",
    "recommendations",
    "limitations",
    "confidence",
)
ANALYSIS_INPUT_EXTRA_FIELD = "extra"
ANALYSIS_IMPORT_HINT_FIELDS = (
    "subject",
    "metrics",
    "commands",
    "analysis_status",
    "analysis_metrics",
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


def cached_analysis_report(
    *,
    workspace_root: str | None,
    eval_slug: str,
    agent_id: str | None,
    session_id: str | None,
    trial_key: str,
) -> dict[str, Any] | None:
    roots = cell_root_for(
        workspace_root=workspace_root,
        eval_slug=eval_slug,
        agent_id=agent_id,
        session_id=session_id,
        cell_key=trial_key,
    )
    if roots is None:
        return None
    root, cell_dir = roots

    relative_paths: dict[str, str] = {}
    report: dict[str, Any] = {
        "trial_key": str(trial_key),
        "status": "cached",
    }

    json_path = cell_dir / "analysis.json"
    json_relative = read_json_analysis(json_path, root)
    if json_relative is not None:
        relative_paths["json"] = json_relative[0]
        report["relative_path"] = json_relative[0]
        report.update(json_relative[1])

    md_path = cell_dir / "analysis.md"
    md_relative = read_markdown_report(md_path, root)
    if md_relative is not None:
        relative_paths["md"] = md_relative[0]
        if "relative_path" not in report:
            report["relative_path"] = md_relative[0]
        if md_relative[1]:
            report["md_report"] = md_relative[1]

    if relative_paths:
        report["relative_paths"] = relative_paths
    if not report.get("relative_path"):
        return None
    return report


def cached_note_report(
    *,
    workspace_root: str | None,
    eval_slug: str,
    agent_id: str | None,
    session_id: str | None,
    trial_key: str,
) -> dict[str, Any] | None:
    roots = cell_root_for(
        workspace_root=workspace_root,
        eval_slug=eval_slug,
        agent_id=agent_id,
        session_id=session_id,
        cell_key=trial_key,
    )
    if roots is None:
        return None
    root, cell_dir = roots
    return read_note_report(cell_dir / "notes.md", root, trial_key)


def save_cell_note(
    *,
    workspace_root: str | None,
    eval_slug: str,
    agent_id: str | None,
    session_id: str | None,
    cell_key: str | None,
    markdown: str,
) -> str:
    if not isinstance(markdown, str):
        raise ValueError("markdown must be a string")
    if len(markdown.encode("utf-8")) > MAX_NOTE_BYTES:
        raise ValueError("notes.md exceeds 1 MiB limit")
    roots = cell_root_for(
        workspace_root=workspace_root,
        eval_slug=eval_slug,
        agent_id=agent_id,
        session_id=session_id,
        cell_key=cell_key,
    )
    if roots is None:
        raise ValueError("cannot locate peval workspace cell for notes.md")
    root, cell_dir = roots
    return write_note_file(cell_dir / "notes.md", root, markdown)


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


def write_note_file(path: Path, root: Path, markdown: str) -> str:
    if not isinstance(markdown, str):
        raise ValueError("markdown must be a string")
    if len(markdown.encode("utf-8")) > MAX_NOTE_BYTES:
        raise ValueError("notes.md exceeds 1 MiB limit")
    target = path
    target.parent.mkdir(parents=True, exist_ok=True)
    target.write_text(markdown, encoding="utf-8")
    try:
        return target.relative_to(root).as_posix()
    except ValueError as exc:
        raise ValueError("notes.md target is outside the workspace root") from exc


def task_root_for(
    *,
    workspace_root: str | None,
    eval_slug: str,
    agent_id: str | None,
    session_id: str | None,
) -> tuple[Path, Path] | None:
    root = safe_root(workspace_root)
    eval_part = safe_segment(eval_slug)
    agent_part = safe_segment(agent_id)
    session_part = safe_segment(session_id)
    if root is None or eval_part is None or agent_part is None or session_part is None:
        return None
    return root, root / "runs" / eval_part / agent_part / session_part


def cell_root_for(
    *,
    workspace_root: str | None,
    eval_slug: str,
    agent_id: str | None,
    session_id: str | None,
    cell_key: str | None,
) -> tuple[Path, Path] | None:
    roots = task_root_for(
        workspace_root=workspace_root,
        eval_slug=eval_slug,
        agent_id=agent_id,
        session_id=session_id,
    )
    cell_part = safe_segment(cell_key)
    if roots is None or cell_part is None:
        return None
    root, task_root = roots
    return root, task_root / cell_part


def read_json_analysis(path: Path, root: Path) -> tuple[str, dict[str, Any]] | None:
    if not path.is_file():
        return None
    try:
        payload = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError, UnicodeDecodeError):
        return None
    if not isinstance(payload, dict):
        return None
    try:
        relative_path = path.relative_to(root).as_posix()
    except ValueError:
        return None
    return relative_path, analysis_fields_from_json(payload)


def analysis_fields_from_json(payload: dict[str, Any]) -> dict[str, Any]:
    fields: dict[str, Any] = {}
    summary = payload.get("summary")
    if isinstance(summary, str) and summary.strip():
        fields["summary"] = summary
    status = payload.get("status")
    if isinstance(status, str) and status.strip():
        fields["analysis_status"] = status
    subject = payload.get("subject")
    if isinstance(subject, dict) and subject:
        fields["subject"] = deepcopy(subject)
    for key in MERGEABLE_ANALYSIS_LIST_FIELDS:
        value = payload.get(key)
        if isinstance(value, list) and value:
            fields[key] = deepcopy(value)
    metrics = payload.get("metrics")
    if isinstance(metrics, dict) and metrics:
        fields["analysis_metrics"] = deepcopy(metrics)
    confidence = payload.get("confidence")
    if isinstance(confidence, str) and confidence.strip():
        fields["confidence"] = confidence
    elif (
        isinstance(confidence, (int, float))
        and not isinstance(confidence, bool)
        and math.isfinite(float(confidence))
    ):
        fields["confidence"] = confidence
    return fields


def read_markdown_report(path: Path, root: Path) -> tuple[str, str | None] | None:
    if not path.is_file():
        return None
    try:
        text = path.read_text(encoding="utf-8")
    except (OSError, UnicodeDecodeError):
        return None
    try:
        relative_path = path.relative_to(root).as_posix()
    except ValueError:
        return None
    return relative_path, text if text.strip() else None


def read_note_report(path: Path, root: Path, trial_key: str) -> dict[str, Any] | None:
    if not path.is_file():
        return None
    try:
        if path.stat().st_size > MAX_NOTE_BYTES:
            return None
        markdown = path.read_text(encoding="utf-8")
    except (OSError, UnicodeDecodeError):
        return None
    try:
        relative_path = path.relative_to(root).as_posix()
    except ValueError:
        return None
    return {
        "trial_key": str(trial_key),
        "source": "cell",
        "label": "notes.md",
        "markdown": markdown,
        "source_ref": {
            "kind": "note",
            "label": "notes.md",
            "relative_path": relative_path,
        },
    }


def safe_root(value: str | None) -> Path | None:
    if value is None:
        return None
    try:
        root = Path(value).expanduser().resolve()
    except (OSError, RuntimeError):
        return None
    return root if root.is_dir() else None


def safe_segment(value: object) -> str | None:
    if value is None:
        return None
    text = str(value).strip()
    safe = "".join(
        char if char.isalnum() or char in {"-", "_", "."} else "_"
        for char in text
    ).strip("._")
    if not safe:
        return None
    return safe
