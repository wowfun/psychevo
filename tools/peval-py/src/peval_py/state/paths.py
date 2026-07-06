from __future__ import annotations

import os
import tomllib
from dataclasses import dataclass
from pathlib import Path

from peval_py.config import default_workspace_config_text
from peval_py.state.constants import SERVE_LOG_RELATIVE_PATH

PEVAL_PY_CONFIG = "peval-py.toml"
PEVAL_ROOT_ENV = "PEVAL_ROOT"


@dataclass(frozen=True)
class WorkspacePaths:
    root: Path
    config_path: Path
    log_path: Path


def resolve_workspace_root(explicit_root: str | None = None) -> Path:
    if explicit_root:
        return Path(explicit_root).expanduser().resolve()
    env_root = os.environ.get(PEVAL_ROOT_ENV)
    if env_root:
        return Path(env_root).expanduser().resolve()
    discovered = discover_workspace_root(Path.cwd())
    if discovered is not None:
        return discovered
    raise ValueError(
        "peval-py workspace is not initialized; run `peval-py init`, "
        f"pass --root/-r, or set {PEVAL_ROOT_ENV}"
    )


def discover_workspace_root(start: Path) -> Path | None:
    current = start.resolve()
    while True:
        candidate = current / PEVAL_PY_CONFIG
        if candidate.is_file():
            return ensure_workspace_root(current)
        if current.parent == current:
            return None
        current = current.parent


def ensure_workspace_root(root: Path) -> Path:
    resolved = root.resolve()
    config_path = resolved / PEVAL_PY_CONFIG
    if not config_path.is_file():
        raise ValueError(
            f"{resolved} is not an initialized peval-py workspace; "
            f"run `peval-py init -r {resolved}`"
        )
    try:
        tomllib.loads(config_path.read_text(encoding="utf-8"))
    except tomllib.TOMLDecodeError as exc:
        raise ValueError(f"failed to parse {config_path}: {exc}") from exc
    return resolved


def workspace_paths(root: Path) -> WorkspacePaths:
    root = root.expanduser().resolve()
    root.mkdir(parents=True, exist_ok=True)
    config_path = root / PEVAL_PY_CONFIG
    if config_path.is_file():
        try:
            tomllib.loads(config_path.read_text(encoding="utf-8"))
        except tomllib.TOMLDecodeError as exc:
            raise ValueError(f"failed to parse {config_path}: {exc}") from exc
    else:
        config_path.write_text(default_workspace_config_text(), encoding="utf-8")
    return WorkspacePaths(
        root=root,
        config_path=config_path,
        log_path=root / SERVE_LOG_RELATIVE_PATH,
    )
