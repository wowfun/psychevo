from __future__ import annotations

import json
import os
import re
import tomllib
from dataclasses import dataclass, field, replace
from pathlib import Path
from typing import Any

from peval_py.i18n import normalize_locale

IDENTIFIER_RE = re.compile(r"^[A-Za-z_][A-Za-z0-9_]*$")
DEFAULT_DB_PATH_RE = re.compile(r"^\s*default_db_path\s*=")
TABLE_HEADER_RE = re.compile(r"^\s*\[([^\]\n]+)\]\s*(?:#.*)?$")
PEVAL_PY_CONFIG = "peval-py.toml"
WINDOWS_DRIVE_PATH_RE = re.compile(r"^[A-Za-z]:[\\/]")
WINDOWS_DRIVE_MOUNT_ROOT = Path("/mnt")
DEFAULT_ADAPTER_DB_PATHS = {
    "psychevo": "~/.psychevo/state.db",
    "opencode": "~/.local/share/opencode/opencode.db",
    "hermes": "~/.hermes/state.db",
}


@dataclass(frozen=True)
class DbMapping:
    messages_table: str = "messages"
    session_id_column: str = "session_id"
    sequence_column: str = "session_seq"
    message_column: str = "message_json"
    usage_column: str = "usage_json"
    metadata_column: str = "metadata_json"


@dataclass(frozen=True)
class ToolConfig:
    adapter: str = "psychevo"
    locale: str = "en"
    workspace_root: str | None = None
    analysis_eval_slug: str = "default"
    agent_name: str | None = None
    agent_version: str = "0.1.0"
    model: str | None = None
    trajectory_id: str | None = None
    max_content_chars: int = 128 * 1024
    redact: bool = True
    db: DbMapping = DbMapping()
    adapter_options: dict[str, Any] = field(default_factory=dict)
    adapter_options_by_id: dict[str, dict[str, Any]] = field(
        default_factory=dict,
        repr=False,
    )
    adapter_default_db_paths: dict[str, str] = field(default_factory=dict, repr=False)
    workspace_state_db_path: str | None = field(default=None, repr=False)


def default_workspace_config_text() -> str:
    lines = ['state_db = "state.db"\n']
    for adapter_id, default_db_path in DEFAULT_ADAPTER_DB_PATHS.items():
        lines.extend(
            [
                "\n",
                f"[{_adapter_table_key(adapter_id)}]\n",
                f"default_db_path = {json.dumps(default_db_path)}\n",
            ]
        )
    return "".join(lines)


def load_config(path: str | None, *, workspace_root: str | None = None) -> ToolConfig:
    config = ToolConfig()
    workspace_config = discover_peval_py_config(workspace_root)
    if workspace_config is not None:
        data = tomllib.loads(workspace_config.read_text(encoding="utf-8"))
        config = replace(config, workspace_root=str(workspace_config.parent))
        config = apply_toml_config(
            config,
            data,
            top_level_locale=True,
            base_dir=workspace_config.parent,
        )
    if path:
        config_path = Path(path).expanduser()
        data = tomllib.loads(config_path.read_text(encoding="utf-8"))
        config = apply_toml_config(
            config,
            data,
            top_level_locale=True,
            base_dir=config_path.parent,
        )
    return config


def discover_peval_py_config(workspace_root: str | None = None) -> Path | None:
    if workspace_root:
        candidate = Path(workspace_root).expanduser() / PEVAL_PY_CONFIG
        return candidate.resolve() if candidate.is_file() else None
    current = Path.cwd().resolve()
    while True:
        candidate = current / PEVAL_PY_CONFIG
        if candidate.is_file():
            return candidate
        if current.parent == current:
            return None
        current = current.parent


def apply_toml_config(
    config: ToolConfig,
    data: dict[str, Any],
    *,
    top_level_locale: bool = False,
    base_dir: Path | None = None,
) -> ToolConfig:
    if top_level_locale and "locale" in data:
        config = replace(config, locale=normalize_locale(data["locale"]))
    if "analysis_eval_slug" in data:
        config = replace(
            config,
            analysis_eval_slug=_safe_path_segment(data["analysis_eval_slug"]),
        )
    defaults = data.get("defaults", {})
    if defaults:
        if not isinstance(defaults, dict):
            raise ValueError("defaults config must be a TOML table")
        updates: dict[str, Any] = {}
        if "adapter" in defaults or "agent" in defaults:
            updates["adapter"] = str(
                defaults.get(
                    "adapter",
                    defaults.get("agent", config.adapter),
                )
            )
        if "agent_name" in defaults:
            updates["agent_name"] = _optional_string(defaults.get("agent_name"))
        if "agent_version" in defaults:
            updates["agent_version"] = str(defaults.get("agent_version"))
        if "locale" in defaults:
            updates["locale"] = normalize_locale(defaults.get("locale"))
        if "model" in defaults:
            updates["model"] = _optional_string(defaults.get("model"))
        if "trajectory_id" in defaults:
            updates["trajectory_id"] = _optional_string(defaults.get("trajectory_id"))
        if "max_content_chars" in defaults:
            updates["max_content_chars"] = int(defaults.get("max_content_chars"))
        if "redact" in defaults:
            updates["redact"] = bool(defaults.get("redact"))
        config = replace(
            config,
            **updates,
        )
    db = data.get("db", {})
    if db:
        if not isinstance(db, dict):
            raise ValueError("db config must be a TOML table")
        db_updates: dict[str, str] = {}
        for key in [
            "messages_table",
            "session_id_column",
            "sequence_column",
            "message_column",
            "usage_column",
            "metadata_column",
        ]:
            if key in db:
                db_updates[key] = _safe_identifier(db[key])
        if db_updates:
            config = replace(config, db=replace(config.db, **db_updates))
    adapter_options_by_id, adapter_default_db_paths = _adapter_config_by_id(
        data.get("adapters", {}),
        base_dir=base_dir,
    )
    if adapter_default_db_paths:
        merged_default_db_paths = dict(config.adapter_default_db_paths)
        merged_default_db_paths.update(adapter_default_db_paths)
        config = replace(config, adapter_default_db_paths=merged_default_db_paths)
    if adapter_options_by_id:
        merged_options = {
            key: dict(value) for key, value in config.adapter_options_by_id.items()
        }
        for adapter_id, options in adapter_options_by_id.items():
            merged = dict(merged_options.get(adapter_id, {}))
            merged.update(options)
            merged_options[adapter_id] = merged
        config = replace(
            config,
            adapter_options_by_id=merged_options,
            adapter_options=_adapter_options_for(config.adapter, merged_options),
        )
    return config


def apply_overrides(config: ToolConfig, args: Any) -> ToolConfig:
    updates: dict[str, Any] = {}
    adapter = _adapter_override(getattr(args, "adapter", None))
    if adapter is not None:
        updates["adapter"] = adapter
    for field in [
        "agent_name",
        "agent_version",
        "model",
        "trajectory_id",
        "max_content_chars",
    ]:
        value = getattr(args, field, None)
        if value is not None:
            updates[field] = value
    if getattr(args, "no_redact", False):
        updates["redact"] = False
    adapter = str(updates.get("adapter", config.adapter))
    if updates or config.adapter_options_by_id:
        updates["adapter_options"] = _adapter_options_for(
            adapter,
            config.adapter_options_by_id,
        )
    return replace(config, **updates)


def config_for_adapter(config: ToolConfig, adapter: object) -> ToolConfig:
    adapter_id = _normalize_adapter_id(adapter)
    return replace(
        config,
        adapter=adapter_id,
        adapter_options=_adapter_options_for(adapter_id, config.adapter_options_by_id),
    )


def write_workspace_locale(config_path: Path, locale: str) -> None:
    normalized = normalize_locale(locale)
    path = config_path.expanduser()
    text = path.read_text(encoding="utf-8") if path.exists() else ""
    lines = text.splitlines(keepends=True)
    locale_line = f"locale = {json.dumps(normalized)}\n"
    first_table_index = next(
        (
            index
            for index, line in enumerate(lines)
            if line.lstrip().startswith("[")
        ),
        len(lines),
    )
    for index, line in enumerate(lines[:first_table_index]):
        if line.lstrip().startswith("locale") and "=" in line:
            lines[index] = locale_line
            path.write_text("".join(lines), encoding="utf-8")
            return
    lines.insert(first_table_index, locale_line)
    path.write_text("".join(lines), encoding="utf-8")


def write_workspace_adapter_default_db(
    config_path: Path,
    adapter: object,
    default_db_path: str | None,
) -> str | None:
    adapter_id = _normalize_adapter_id(adapter)
    raw_path = str(default_db_path or "").strip()
    path = config_path.expanduser()
    text = path.read_text(encoding="utf-8") if path.exists() else ""
    lines = text.splitlines(keepends=True)
    table_range = _adapter_table_range(lines, adapter_id)

    if not raw_path:
        if table_range is not None:
            start, end = table_range
            existing = _default_db_path_line_index(lines, start + 1, end)
            if existing is not None:
                del lines[existing]
                path.write_text("".join(lines), encoding="utf-8")
        return None

    stored_path = display_config_path(raw_path, base_dir=path.parent)
    config_line = f"default_db_path = {json.dumps(stored_path)}\n"
    if table_range is None:
        if lines and not lines[-1].endswith(("\n", "\r")):
            lines[-1] = lines[-1] + "\n"
        if lines and "".join(lines).strip():
            lines.append("\n")
        lines.extend([f"[{_adapter_table_key(adapter_id)}]\n", config_line])
    else:
        start, end = table_range
        existing = _default_db_path_line_index(lines, start + 1, end)
        if existing is None:
            lines.insert(start + 1, config_line)
        else:
            lines[existing] = config_line
    path.write_text("".join(lines), encoding="utf-8")
    return _resolve_config_path(stored_path, base_dir=path.parent)


def _adapter_table_range(
    lines: list[str],
    adapter_id: str,
) -> tuple[int, int] | None:
    for index, line in enumerate(lines):
        if not _is_adapter_table_header(line, adapter_id):
            continue
        end = next(
            (
                candidate
                for candidate in range(index + 1, len(lines))
                if _is_table_header(lines[candidate])
            ),
            len(lines),
        )
        return index, end
    return None


def _default_db_path_line_index(
    lines: list[str],
    start: int,
    end: int,
) -> int | None:
    for index in range(start, end):
        if DEFAULT_DB_PATH_RE.match(lines[index]):
            return index
    return None


def _is_table_header(line: str) -> bool:
    return bool(TABLE_HEADER_RE.match(line))


def _is_adapter_table_header(line: str, adapter_id: str) -> bool:
    match = TABLE_HEADER_RE.match(line)
    if not match:
        return False
    header = match.group(1).strip()
    try:
        parsed = tomllib.loads(f"[{header}]\n__peval_marker = true\n")
    except tomllib.TOMLDecodeError:
        return False
    adapters = parsed.get("adapters")
    if not isinstance(adapters, dict):
        return False
    adapter_config = adapters.get(adapter_id)
    return isinstance(adapter_config, dict) and adapter_config.get("__peval_marker") is True


def _adapter_table_key(adapter_id: str) -> str:
    if IDENTIFIER_RE.match(adapter_id):
        return f"adapters.{adapter_id}"
    return f"adapters.{json.dumps(adapter_id)}"


def _adapter_override(value: object) -> str | None:
    if value is None:
        return None
    if isinstance(value, (list, tuple)):
        adapter = None
        for item in value:
            text = str(item)
            if "=" in text:
                continue
            adapter = text
        return _normalize_adapter_id(adapter) if adapter is not None else None
    return _normalize_adapter_id(value)


def _adapter_config_by_id(
    value: object,
    *,
    base_dir: Path | None = None,
) -> tuple[dict[str, dict[str, Any]], dict[str, str]]:
    if not value:
        return {}, {}
    if not isinstance(value, dict):
        raise ValueError("adapters config must be a TOML table")
    options: dict[str, dict[str, Any]] = {}
    default_db_paths: dict[str, str] = {}
    for key, raw_options in value.items():
        if not isinstance(raw_options, dict):
            raise ValueError(f"adapter options for {key} must be a TOML table")
        adapter_id = str(key).strip().lower()
        adapter_options = dict(raw_options)
        if "default_db_path" in adapter_options:
            default_db_paths[adapter_id] = _resolve_config_path(
                adapter_options.pop("default_db_path"),
                base_dir=base_dir,
            )
        options[adapter_id] = adapter_options
    return options, default_db_paths


def _resolve_config_path(value: object, *, base_dir: Path | None = None) -> str:
    text = str(value).strip()
    if not text:
        raise ValueError("default_db_path must not be empty")
    if is_windows_absolute_like_path(text):
        return resolve_windows_absolute_like_path(text)
    path = Path(text).expanduser()
    if not path.is_absolute():
        path = (base_dir or Path.cwd()) / path
    return str(path.resolve())


def display_config_path(value: object, *, base_dir: Path | None = None) -> str:
    text = str(value).strip()
    if not text:
        return text
    if text.startswith("~"):
        return text
    if is_windows_absolute_like_path(text):
        return text
    path = Path(text).expanduser()
    if not path.is_absolute():
        return text
    home = Path.home().resolve()
    try:
        relative = path.resolve().relative_to(home)
    except ValueError:
        return text
    return "~" if not relative.parts else "~/" + relative.as_posix()


def is_windows_absolute_like_path(path: str) -> bool:
    return (
        bool(WINDOWS_DRIVE_PATH_RE.match(path))
        or path.startswith("\\\\")
        or path.startswith("//")
    )


def resolve_windows_absolute_like_path(
    raw_path: str,
    *,
    windows_mount_root: Path | None = None,
) -> str:
    if os.name == "nt":
        return str(Path(raw_path).expanduser())
    original = Path(raw_path).expanduser()
    if original.exists():
        return str(original.resolve())
    mapped = windows_drive_mount_path(
        raw_path,
        windows_mount_root or WINDOWS_DRIVE_MOUNT_ROOT,
    )
    if mapped is not None and mapped.exists():
        return str(mapped.resolve())
    return raw_path


def windows_drive_mount_path(raw_path: str, mount_root: Path) -> Path | None:
    if not WINDOWS_DRIVE_PATH_RE.match(raw_path):
        return None
    drive = raw_path[0].lower()
    rest = raw_path[2:].lstrip("\\/")
    parts = [part for part in re.split(r"[\\/]+", rest) if part]
    return Path(mount_root) / drive / Path(*parts)


def _adapter_options_for(
    adapter: str,
    adapter_options_by_id: dict[str, dict[str, Any]],
) -> dict[str, Any]:
    return dict(adapter_options_by_id.get(str(adapter).strip().lower(), {}))


def _normalize_adapter_id(adapter: object) -> str:
    text = str(adapter or "").strip().lower()
    if not text:
        raise ValueError("adapter id is required")
    return text


def _safe_identifier(value: object) -> str:
    text = str(value)
    if not IDENTIFIER_RE.match(text):
        raise ValueError(f"unsafe SQL identifier: {text}")
    return text


def _safe_path_segment(value: object) -> str:
    text = str(value).strip()
    if not text or text in {".", ".."} or "/" in text or "\\" in text:
        raise ValueError(f"unsafe path segment: {text}")
    return text


def _optional_string(value: object) -> str | None:
    if value is None:
        return None
    return str(value)
