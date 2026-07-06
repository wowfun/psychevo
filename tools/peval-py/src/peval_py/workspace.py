from __future__ import annotations

import json
from dataclasses import dataclass
from pathlib import Path
from typing import Any

from peval_py.state import STATE_SCHEMA_VERSION, ServeStateStore, workspace_paths


@dataclass(frozen=True)
class InitWorkspaceResult:
    schema_version: int
    root: Path
    peval_py_config: Path
    log_path: Path

    def to_jsonable(self) -> dict[str, Any]:
        return {
            "schema_version": self.schema_version,
            "root": str(self.root),
            "peval_py_config": str(self.peval_py_config),
            "log_path": str(self.log_path),
        }


def init_workspace(root: str | None = None) -> InitWorkspaceResult:
    root_path = Path(root).expanduser() if root else Path.cwd()
    paths = workspace_paths(root_path)
    store = ServeStateStore(paths)
    store.close()
    return InitWorkspaceResult(
        schema_version=STATE_SCHEMA_VERSION,
        root=paths.root,
        peval_py_config=paths.config_path,
        log_path=paths.log_path,
    )


def render_init_text(result: InitWorkspaceResult) -> str:
    return (
        f"peval-py workspace: {result.root}\n"
        f"peval-py config: {result.peval_py_config}\n"
        f"serve log: {result.log_path}\n"
    )


def run_init_command(args: Any) -> None:
    result = init_workspace(getattr(args, "root", None))
    if getattr(args, "json", False):
        print(json.dumps(result.to_jsonable(), ensure_ascii=False, indent=2))
    else:
        print(render_init_text(result), end="")
