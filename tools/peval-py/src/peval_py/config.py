from __future__ import annotations

import re
import tomllib
from dataclasses import dataclass, field, replace
from pathlib import Path
from typing import Any

from peval_py.i18n import normalize_locale

IDENTIFIER_RE = re.compile(r"^[A-Za-z_][A-Za-z0-9_]*$")


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


def load_config(path: str | None) -> ToolConfig:
    if not path:
        return ToolConfig()
    data = tomllib.loads(Path(path).read_text(encoding="utf-8"))
    config = ToolConfig()
    defaults = data.get("defaults", {})
    if defaults:
        config = replace(
            config,
            adapter=str(
                defaults.get(
                    "adapter",
                    defaults.get("agent", config.adapter),
                )
            ),
            agent_name=_optional_string(defaults.get("agent_name", config.agent_name)),
            agent_version=str(defaults.get("agent_version", config.agent_version)),
            locale=normalize_locale(defaults.get("locale", config.locale)),
            model=_optional_string(defaults.get("model", config.model)),
            trajectory_id=_optional_string(
                defaults.get("trajectory_id", config.trajectory_id)
            ),
            max_content_chars=int(
                defaults.get("max_content_chars", config.max_content_chars)
            ),
            redact=bool(defaults.get("redact", config.redact)),
        )
    db = data.get("db", {})
    if db:
        mapping = DbMapping(
            messages_table=_safe_identifier(
                db.get("messages_table", config.db.messages_table)
            ),
            session_id_column=_safe_identifier(
                db.get("session_id_column", config.db.session_id_column)
            ),
            sequence_column=_safe_identifier(
                db.get("sequence_column", config.db.sequence_column)
            ),
            message_column=_safe_identifier(
                db.get("message_column", config.db.message_column)
            ),
            usage_column=_safe_identifier(db.get("usage_column", config.db.usage_column)),
            metadata_column=_safe_identifier(
                db.get("metadata_column", config.db.metadata_column)
            ),
        )
        config = replace(config, db=mapping)
    adapter_options_by_id = _adapter_options_by_id(data.get("adapters", {}))
    if adapter_options_by_id:
        config = replace(
            config,
            adapter_options_by_id=adapter_options_by_id,
            adapter_options=_adapter_options_for(config.adapter, adapter_options_by_id),
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


def _adapter_options_by_id(value: object) -> dict[str, dict[str, Any]]:
    if not value:
        return {}
    if not isinstance(value, dict):
        raise ValueError("adapters config must be a TOML table")
    options: dict[str, dict[str, Any]] = {}
    for key, raw_options in value.items():
        if not isinstance(raw_options, dict):
            raise ValueError(f"adapter options for {key} must be a TOML table")
        options[str(key).strip().lower()] = dict(raw_options)
    return options


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


def _optional_string(value: object) -> str | None:
    if value is None:
        return None
    return str(value)
