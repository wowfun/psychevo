from __future__ import annotations

from peval_py_test_support import *


def compact_css_text(value: str) -> str:
    return re.sub(r"\s+", "", value).replace(";}", "}")


def write_cached_analysis(
    root: Path,
    *,
    eval_slug: str = "default",
    agent_id: str = "agent-a",
    session_id: str = "common_session",
    cell_key: str = "session_t001",
    summary: str = "Cached analysis summary.",
    extra: dict | None = None,
) -> Path:
    path = root / "runs" / eval_slug / agent_id / session_id / cell_key / "analysis.json"
    path.parent.mkdir(parents=True, exist_ok=True)
    payload = {
        "schema_version": 1,
        "trial_name": session_id,
        "summary": summary,
        "checks": {},
    }
    if extra:
        payload.update(extra)
    path.write_text(
        json.dumps(payload),
        encoding="utf-8",
    )
    return path


def write_cached_markdown(
    root: Path,
    *,
    eval_slug: str = "default",
    agent_id: str = "agent-a",
    session_id: str = "common_session",
    cell_key: str = "session_t001",
    markdown: str = "## Finding\n\n- Cached markdown report.",
) -> Path:
    path = root / "runs" / eval_slug / agent_id / session_id / cell_key / "analysis.md"
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(markdown, encoding="utf-8")
    return path


def write_cached_note(
    root: Path,
    *,
    eval_slug: str = "default",
    agent_id: str = "agent-a",
    session_id: str = "common_session",
    cell_key: str = "session_t001",
    markdown: str = "Manual cell note.",
) -> Path:
    path = root / "runs" / eval_slug / agent_id / session_id / cell_key / "notes.md"
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(markdown, encoding="utf-8")
    return path
