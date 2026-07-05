from __future__ import annotations

import os
import re
import shlex
from pathlib import Path
from types import SimpleNamespace
from typing import Any
from urllib.parse import unquote

from peval_py.adapters import available_adapter_ids, normalize_adapter_id
from peval_py.inputs import infer_adapter_from_path, validate_selected_adapter
from peval_py.serve.constants import WINDOWS_DRIVE_MOUNT_ROOT, WINDOWS_DRIVE_PATH_RE
from peval_py.serve.errors import HttpError
from peval_py.session_select import list_adapter_sessions
from peval_py.state import ServeStateStore

def adapter_default_db_payload(payload: dict[str, Any]) -> tuple[str, str | None]:
    adapter_id = validate_selected_adapter(
        normalize_adapter_id(required_string(payload, "adapter")),
        set(available_adapter_ids()),
        "adapter default DB",
    )
    return adapter_id, optional_string(payload.get("default_db_path"))


def adapter_for_db_inspect(path: str, raw_adapter: str | None) -> tuple[str, bool]:
    available = set(available_adapter_ids())
    if raw_adapter:
        return validate_selected_adapter(
            normalize_adapter_id(raw_adapter),
            available,
            "DB session inspect",
        ), False
    adapter_id = infer_adapter_from_path(path, available)
    if adapter_id is None:
        options = ", ".join(sorted(available)) or "<none>"
        raise HttpError(
            400,
            f"could not infer adapter for {path}; choose adapter "
            f"(available adapters: {options})",
        )
    return adapter_id, True


def source_args_from_payload(
    store: ServeStateStore,
    payload: dict[str, Any],
) -> SimpleNamespace:
    paths = source_path_values(store, payload, "path")
    dbs = source_path_values(store, payload, "db")
    input_table = optional_string(payload.get("input_table"))
    present = [value for value in [paths, dbs, input_table] if value]
    if len(present) != 1:
        raise HttpError(400, "provide exactly one source: path, db, or input_table")
    session_id = optional_string(payload.get("session_id"))
    session_ids = session_ids_payload(payload)
    if session_id and session_ids:
        raise HttpError(400, "provide either session_id or session_ids, not both")
    if (session_id or session_ids) and not dbs:
        raise HttpError(400, "session_id and session_ids are only valid with db sources")
    if (session_id or session_ids) and len(dbs) != 1:
        raise HttpError(400, "session_id and session_ids require exactly one db source")
    return SimpleNamespace(
        path=paths or None,
        db=dbs or None,
        input_table=[workspace_relative_path(store, input_table)] if input_table else None,
        session_id=([session_id] if session_id and dbs else session_ids if dbs else None),
        adapter=[],
        note=[],
    )


def source_path_values(
    store: ServeStateStore,
    payload: dict[str, Any],
    key: str,
) -> list[str]:
    raw = optional_string(payload.get(key))
    if raw is None:
        return []
    parts = split_source_path_list(raw, key)
    if not parts:
        raise HttpError(400, f"{key} path list is empty")
    return [workspace_relative_path(store, part) for part in parts]


def split_source_path_list(raw: str, key: str) -> list[str]:
    try:
        raw_parts = shlex.split(raw, posix=False)
    except ValueError as exc:
        raise HttpError(400, f"{key} path list is invalid: {exc}") from exc
    return [
        unquote_path_token(part)
        for part in raw_parts
        if unquote_path_token(part)
    ]


def unquote_path_token(raw: object) -> str:
    text = str(raw).strip()
    if len(text) >= 2 and text[0] == text[-1] and text[0] in {"'", '"'}:
        return text[1:-1]
    return text


def adapter_override_payload(payload: dict[str, Any]) -> str | None:
    adapter = optional_string(payload.get("adapter"))
    if adapter is None or adapter.lower() == "auto":
        return None
    return adapter


def session_ids_payload(payload: dict[str, Any]) -> list[str] | None:
    raw = payload.get("session_ids")
    if raw is None:
        return None
    if not isinstance(raw, list):
        raise HttpError(400, "session_ids must be an array")
    session_ids: list[str] = []
    for value in raw:
        text = optional_string(value)
        if text is not None:
            session_ids.append(text)
    if not session_ids:
        raise HttpError(400, "session_ids must include at least one session id")
    return session_ids


def workspace_relative_path(
    store: ServeStateStore,
    raw_path: str | None,
    *,
    windows_mount_root: Path | None = None,
) -> str | None:
    if raw_path is None:
        return None
    text = unquote_path_token(raw_path)
    if not text:
        return None
    if is_windows_absolute_like_path(text):
        return resolve_windows_absolute_like_path(text, windows_mount_root)
    path = Path(text).expanduser()
    if not path.is_absolute():
        path = store.paths.root / path
    return str(path)


def is_windows_absolute_like_path(path: str) -> bool:
    return bool(WINDOWS_DRIVE_PATH_RE.match(path)) or path.startswith("\\\\") or path.startswith("//")


def resolve_windows_absolute_like_path(
    raw_path: str,
    windows_mount_root: Path | None = None,
) -> str:
    if os.name == "nt":
        return str(Path(raw_path).expanduser())
    original = Path(raw_path).expanduser()
    if original.exists():
        return str(original)
    mapped = windows_drive_mount_path(
        raw_path,
        windows_mount_root or patched_windows_drive_mount_root(),
    )
    if mapped is not None and mapped.exists():
        return str(mapped)
    return raw_path


def patched_windows_drive_mount_root() -> Path:
    try:
        from peval_py import serve as serve_facade

        return Path(getattr(serve_facade, "WINDOWS_DRIVE_MOUNT_ROOT"))
    except Exception:  # noqa: BLE001 - optional patch compatibility only.
        return WINDOWS_DRIVE_MOUNT_ROOT


def windows_drive_mount_path(raw_path: str, mount_root: Path) -> Path | None:
    if not WINDOWS_DRIVE_PATH_RE.match(raw_path):
        return None
    drive = raw_path[0].lower()
    rest = raw_path[2:].lstrip("\\/")
    parts = [part for part in re.split(r"[\\/]+", rest) if part]
    return Path(mount_root) / drive / Path(*parts)


def source_keys_payload(payload: dict[str, Any]) -> list[str] | None:
    raw_keys = payload.get("source_keys")
    if raw_keys is None and payload.get("source_key") is not None:
        raw_keys = [payload["source_key"]]
    if raw_keys is None:
        return None
    if not isinstance(raw_keys, list):
        raise HttpError(400, "source_keys must be an array")
    return [str(key) for key in raw_keys]


def required_bool(payload: dict[str, Any], key: str) -> bool:
    value = payload.get(key)
    if not isinstance(value, bool):
        raise HttpError(400, f"{key} must be true or false")
    return value


def source_state_payload(
    value: Any,
    *,
    default: str = "active",
    field: str = "source_state",
) -> str:
    text = str(value if value is not None else default).strip().lower()
    if not text:
        text = default
    if text not in {"active", "archived"}:
        raise HttpError(400, f"{field} must be active or archived")
    return text


def source_action_path(path: str) -> tuple[str, str] | None:
    prefix = "/api/sources/"
    if not path.startswith(prefix):
        return None
    parts = path[len(prefix) :].split("/")
    if len(parts) != 2 or not parts[0] or not parts[1]:
        raise HttpError(404, "unknown source action")
    return unquote(parts[0]), parts[1]


def required_string(payload: dict[str, Any], key: str) -> str:
    value = payload.get(key)
    if not isinstance(value, str) or not value:
        raise HttpError(400, f"{key} is required")
    return value


def markdown_payload(payload: dict[str, Any]) -> str:
    value = payload.get("markdown")
    if not isinstance(value, str):
        raise HttpError(400, "markdown is required")
    return value


def alias_payload(payload: dict[str, Any]) -> str | None:
    value = payload.get("alias")
    if value is None:
        return None
    text = str(value).strip()
    return text or None


def optional_string(value: Any) -> str | None:
    if value is None:
        return None
    text = str(value).strip()
    return text or None
