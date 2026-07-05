from __future__ import annotations

import hashlib
import os
from pathlib import Path
from typing import Any


def data_ref_for_input(label: str, input_path: str | None) -> dict[str, Any]:
    relative = label
    size: int | None = None
    digest: str | None = None
    modified_ms: int | None = None
    if input_path:
        path = Path(input_path)
        if path.exists():
            session_db_ref = path.suffix in {".db", ".sqlite", ".sqlite3"} and ":" in label
            if not session_db_ref:
                stat = path.stat()
                size = stat.st_size
                modified_ms = int(stat.st_mtime * 1000)
                digest = file_hash(path)
            try:
                relative_path = Path(os.path.relpath(path, Path.cwd()))
                relative = str(relative_path) if not str(relative_path).startswith("..") else path.name
            except ValueError:
                relative = path.name
    ref = {
        "kind": "input",
        "label": label,
        "relative_path": relative,
        "mime": "application/jsonl" if label.endswith(".jsonl") else "application/octet-stream",
    }
    if size is not None:
        ref["size_bytes"] = size
    if digest:
        ref["content_hash"] = digest
    if modified_ms is not None:
        ref["modified_ms"] = modified_ms
    return ref


def file_hash(path: Path) -> str:
    hasher = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 64), b""):
            hasher.update(chunk)
    return f"sha256:{hasher.hexdigest()}"
