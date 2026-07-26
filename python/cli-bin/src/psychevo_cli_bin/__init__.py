from __future__ import annotations

import os
import subprocess
from pathlib import Path

__version__ = "0.1.0"


def executable() -> str:
    name = "pevo.exe" if os.name == "nt" else "pevo"
    path = Path(__file__).parent / "bin" / name
    if not path.is_file():
        raise RuntimeError(f"bundled pevo executable is missing: {path}")
    return str(path)


def workbench_dist() -> str:
    path = Path(__file__).parent / "workbench"
    if not (path / "index.html").is_file():
        raise RuntimeError(f"bundled Psychevo Workbench assets are missing: {path}")
    return str(path)


def main() -> None:
    environment = os.environ.copy()
    environment.setdefault("PSYCHEVO_WEB_DIST", workbench_dist())
    if os.name == "posix":
        os.execve(executable(), [executable(), *os.sys.argv[1:]], environment)
    raise SystemExit(
        subprocess.call([executable(), *os.sys.argv[1:]], env=environment)
    )


__all__ = ["__version__", "executable", "main", "workbench_dist"]
