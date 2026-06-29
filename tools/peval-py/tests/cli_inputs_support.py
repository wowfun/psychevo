from __future__ import annotations

import os

from peval_py_test_support import *


def write_cli_cached_analysis(
    root: Path,
    *,
    eval_slug: str = "default",
    agent_id: str = "agent-a",
    session_id: str = "common_session",
    cell_key: str = "session_t001",
    summary: str = "Root-selected cached analysis.",
) -> Path:
    path = root / "runs" / eval_slug / agent_id / session_id / cell_key / "analysis.json"
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(
        json.dumps(
            {
                "summary": summary,
                "findings": [{"title": "Root-selected finding."}],
                "metrics": {"review_turns": 2},
            }
        ),
        encoding="utf-8",
    )
    return path


def write_cli_cached_markdown(
    root: Path,
    *,
    eval_slug: str = "default",
    agent_id: str = "agent-a",
    session_id: str = "common_session",
    cell_key: str = "session_t001",
    markdown: str = "## Root selected analysis\n\nCached markdown body.",
) -> Path:
    path = root / "runs" / eval_slug / agent_id / session_id / cell_key / "analysis.md"
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(markdown, encoding="utf-8")
    return path


def write_peval_workspace(root: Path) -> None:
    root.mkdir(parents=True, exist_ok=True)
    (root / "peval-py.toml").write_text(
        'analysis_eval_slug = "default"\n',
        encoding="utf-8",
    )


def written_report_path(stdout: str, cwd: Path) -> Path:
    match = re.fullmatch(r"wrote report: (.+)\n", stdout)
    if not match:
        raise AssertionError(f"missing written report path in stdout: {stdout!r}")
    path = Path(match.group(1))
    return path if path.is_absolute() else cwd / path


def write_trial_cell_artifacts(
    cell_dir: Path,
    *,
    session_id: str = "artifact-session",
    trial_key: str = "session_t001",
    agent_id: str = "psychevo",
    adapter: str = "psychevo",
    tool_error: bool = False,
) -> None:
    agent_dir = cell_dir / "agent"
    agent_dir.mkdir(parents=True, exist_ok=True)
    trajectory = {
        "schema_version": "ATIF-v1.7",
        "trajectory_id": trial_key,
        "session_id": session_id,
        "agent": {"name": agent_id, "version": "test"},
        "steps": [
            {
                "step_id": 1,
                "source": "user",
                "message": "direct artifact prompt",
            },
            {
                "step_id": 2,
                "source": "assistant",
                "message": "direct artifact response",
                **(
                    {
                        "tool_calls": [
                            {
                                "tool_call_id": "call_error",
                                "function_name": "exec_command",
                                "arguments": {"cmd": "false"},
                            }
                        ],
                        "observation": {
                            "results": [
                                {
                                    "tool_call_id": "call_error",
                                    "content": "command failed",
                                }
                            ]
                        },
                    }
                    if tool_error
                    else {}
                ),
            },
        ],
        "final_metrics": {
            "total_steps": 2,
            "extra": {
                "total_turns": 1,
                "total_tool_calls": 1 if tool_error else 0,
                "total_tool_errors": 1 if tool_error else 0,
            },
        },
    }
    meta = {
        "trial_key": trial_key,
        "adapter": adapter,
        "started_at_ms": 1000,
        "finished_at_ms": 1200,
        "wall_duration_ms": 200,
        "duration_ms": 200,
        "status": "passed",
        "score": None,
        "score_message": "",
        "warnings": [],
        "total_events": 2,
        "unmapped_events": 0,
        "prompt_unavailable": False,
        "steps": [
            {
                "step_id": 1,
                "tool_calls": [],
                "observations": [],
                "tool_error": False,
                "truncated": False,
            },
            {
                "step_id": 2,
                "tool_calls": [
                    {
                        "tool_call_id": "call_error",
                        "status": "error",
                        "title": "exec_command",
                    }
                ]
                if tool_error
                else [],
                "observations": [
                    {
                        "tool_call_id": "call_error",
                        "status": "error",
                    }
                ]
                if tool_error
                else [],
                "tool_error": tool_error,
                "truncated": False,
            },
        ],
    }
    (agent_dir / "trajectory.json").write_text(
        json.dumps(trajectory),
        encoding="utf-8",
    )
    (agent_dir / "trajectory_meta.json").write_text(
        json.dumps(meta),
        encoding="utf-8",
    )
