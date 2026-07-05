from __future__ import annotations

from pathlib import Path
from typing import Any

from peval_py.analysis.artifacts import (
    cell_root_for,
    read_json_analysis,
    read_markdown_report,
    read_note_report,
    safe_cell_dir,
    safe_root,
    write_note_file,
)
from peval_py.analysis.constants import (
    ANALYSIS_JSON_FILENAME,
    ANALYSIS_MD_FILENAME,
    MAX_NOTE_BYTES,
)

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
    return cached_analysis_report_for_cell(
        workspace_root=root,
        cell_dir=cell_dir,
        trial_key=trial_key,
    )


def cached_analysis_report_for_cell(
    *,
    workspace_root: str | Path | None,
    cell_dir: str | Path,
    trial_key: str,
) -> dict[str, Any] | None:
    root = safe_root(workspace_root)
    if root is None:
        return None
    cell_path = safe_cell_dir(root, cell_dir)
    if cell_path is None:
        return None
    relative_paths: dict[str, str] = {}
    report: dict[str, Any] = {
        "trial_key": str(trial_key),
        "status": "cached",
    }

    json_path = cell_path / ANALYSIS_JSON_FILENAME
    json_relative = read_json_analysis(json_path, root)
    if json_relative is not None:
        relative_paths["json"] = json_relative[0]
        report["relative_path"] = json_relative[0]
        report.update(json_relative[1])

    md_path = cell_path / ANALYSIS_MD_FILENAME
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
    return cached_note_report_for_cell(
        workspace_root=root,
        cell_dir=cell_dir,
        trial_key=trial_key,
    )


def cached_note_report_for_cell(
    *,
    workspace_root: str | Path | None,
    cell_dir: str | Path,
    trial_key: str,
) -> dict[str, Any] | None:
    root = safe_root(workspace_root)
    if root is None:
        return None
    cell_path = safe_cell_dir(root, cell_dir)
    if cell_path is None:
        return None
    return read_note_report(cell_path / "notes.md", root, trial_key)


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
