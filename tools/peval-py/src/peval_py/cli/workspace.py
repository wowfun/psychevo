from __future__ import annotations

import argparse
from pathlib import Path

from peval_py.config import is_windows_absolute_like_path
from peval_py.inputs import (
    canonical_trial_cell_paths_for_inputs,
    infer_workspace_root_from_trial_cell_paths,
    resolved_local_path,
    same_local_path,
)

def rewrite_trial_cell_path_args(args: argparse.Namespace) -> argparse.Namespace:
    if getattr(args, "command", None) not in {"view", "export"}:
        return args
    cell_paths = canonical_trial_cell_paths_for_inputs(
        list(getattr(args, "path", None) or [])
    )
    if not cell_paths:
        return args
    values = vars(args).copy()
    values.update(
        {
            "path": [str(path) for path in cell_paths],
            "root": None,
            "adapter": None,
            "db": None,
            "session_id": None,
            "input_table": None,
        }
    )
    if getattr(args, "command", None) == "view":
        values["list_sessions"] = False
        values["list_interactive"] = False
    return argparse.Namespace(**values)


def validated_workspace_root(args: argparse.Namespace) -> str | None:
    root = getattr(args, "root", None)
    if root and getattr(args, "command", None) in {"view", "export", "import"}:
        from peval_py.state import ensure_workspace_root

        root_text = str(root).strip()
        root_path = resolved_local_path(root_text)
        if root_path is None:
            if is_windows_absolute_like_path(root_text):
                raise ValueError(f"workspace root is not accessible: {root_text}")
            root_path = Path(root_text).expanduser()
        return str(ensure_workspace_root(root_path))
    return root


def workspace_root_for_args(args: argparse.Namespace) -> tuple[str | None, bool]:
    root = validated_workspace_root(args)
    if getattr(args, "command", None) not in {"view", "export"}:
        return root, False
    inferred = infer_workspace_root_from_trial_cell_paths(
        list(getattr(args, "path", None) or [])
    )
    if inferred is None:
        return root, False
    if root:
        if not same_local_path(root, str(inferred)):
            raise ValueError(
                f"explicit workspace root {root} conflicts with inferred workspace "
                f"root {inferred} from Trial cell artifact path"
            )
        return root, False
    from peval_py.state import ensure_workspace_root

    return str(ensure_workspace_root(inferred)), True
