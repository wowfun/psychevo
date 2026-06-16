from __future__ import annotations

import json
from pathlib import Path
from typing import Any

MAX_NOTE_BYTES = 1024 * 1024
PEVAL_PY_NOTES_CELL = "peval-py-notes"


def cached_analysis_report(
    *,
    workspace_root: str | None,
    eval_slug: str,
    agent_id: str | None,
    session_id: str | None,
    trial_key: str,
) -> dict[str, Any] | None:
    roots = task_root_for(
        workspace_root=workspace_root,
        eval_slug=eval_slug,
        agent_id=agent_id,
        session_id=session_id,
    )
    if roots is None:
        return None
    root, task_root = roots
    cell_dir = unique_analysis_cell_dir(task_root)
    if cell_dir is None:
        return None

    relative_paths: dict[str, str] = {}
    report: dict[str, Any] = {
        "trial_key": str(trial_key),
        "status": "cached",
    }

    json_path = cell_dir / "analysis.json"
    json_relative = read_json_summary(json_path, root)
    if json_relative is not None:
        relative_paths["json"] = json_relative[0]
        report["relative_path"] = json_relative[0]
        if json_relative[1]:
            report["summary"] = json_relative[1]

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
    roots = task_root_for(
        workspace_root=workspace_root,
        eval_slug=eval_slug,
        agent_id=agent_id,
        session_id=session_id,
    )
    if roots is None:
        return None
    root, task_root = roots
    cell_dir = unique_note_cell_dir(task_root)
    if cell_dir is None:
        return None
    return read_note_report(cell_dir / "notes.md", root, trial_key)


def save_cell_note(
    *,
    workspace_root: str | None,
    eval_slug: str,
    agent_id: str | None,
    session_id: str | None,
    markdown: str,
) -> str:
    if not isinstance(markdown, str):
        raise ValueError("markdown must be a string")
    if len(markdown.encode("utf-8")) > MAX_NOTE_BYTES:
        raise ValueError("notes.md exceeds 1 MiB limit")
    roots = task_root_for(
        workspace_root=workspace_root,
        eval_slug=eval_slug,
        agent_id=agent_id,
        session_id=session_id,
    )
    if roots is None:
        raise ValueError("cannot locate peval workspace task for notes.md")
    root, task_root = roots

    note_cells = matching_cell_dirs(task_root, ("notes.md",))
    if len(note_cells) > 1:
        raise ValueError("cannot save notes.md: multiple notes cells match this source")
    if len(note_cells) == 1:
        target = note_cells[0] / "notes.md"
    else:
        analysis_cells = matching_cell_dirs(task_root, ("analysis.json", "analysis.md"))
        if len(analysis_cells) > 1:
            raise ValueError(
                "cannot save notes.md: multiple analysis cells match this source"
            )
        if len(analysis_cells) == 1:
            target = analysis_cells[0] / "notes.md"
        else:
            target = task_root / PEVAL_PY_NOTES_CELL / "notes.md"

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


def unique_analysis_cell_dir(task_root: Path) -> Path | None:
    matches = matching_cell_dirs(task_root, ("analysis.json", "analysis.md"))
    if len(matches) != 1:
        return None
    return matches[0]


def unique_note_cell_dir(task_root: Path) -> Path | None:
    matches = matching_cell_dirs(task_root, ("notes.md",))
    if len(matches) != 1:
        return None
    return matches[0]


def matching_cell_dirs(task_root: Path, file_names: tuple[str, ...]) -> list[Path]:
    try:
        return sorted(
            path
            for path in task_root.iterdir()
            if path.is_dir()
            and any((path / file_name).is_file() for file_name in file_names)
        )
    except OSError:
        return []


def read_json_summary(path: Path, root: Path) -> tuple[str, str | None] | None:
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
    summary = payload.get("summary")
    if isinstance(summary, str) and summary.strip():
        return relative_path, summary
    return relative_path, None


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
    if not text or text in {".", ".."} or "/" in text or "\\" in text:
        return None
    return text
