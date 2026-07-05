from __future__ import annotations

from typing import Any

from peval_py._state.annotations import optional_int, optional_str


def now_ms() -> int:
    import time

    return int(time.time() * 1000)


def trial_summary(
    trajectory: dict[str, Any] | None,
    meta: dict[str, Any] | None,
) -> dict[str, Any]:
    trajectory = trajectory or {}
    meta = meta or {}
    return {
        "trial_key": optional_str(meta.get("trial_key") or trajectory.get("trajectory_id")),
        "trial_session_id": optional_str(trajectory.get("session_id")),
        "last_turn_finished_at_ms": optional_int(meta.get("finished_at_ms")),
    }
