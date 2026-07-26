from __future__ import annotations

import os
from pathlib import Path

__version__ = "0.1.0"


def executable() -> str:
    name = "psychevo-app-server.exe" if os.name == "nt" else "psychevo-app-server"
    path = Path(__file__).parent / "bin" / name
    if not path.is_file():
        raise RuntimeError(f"bundled Psychevo App Server is missing: {path}")
    return str(path)


__all__ = ["__version__", "executable"]
