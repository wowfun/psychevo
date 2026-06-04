from __future__ import annotations

import re
import tomllib
from dataclasses import dataclass, replace
from pathlib import Path
from typing import Any

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
    agent_name: str | None = None
    agent_version: str = "0.1.0"
    model: str | None = None
    trajectory_id: str | None = None
    max_content_chars: int = 16 * 1024
    redact: bool = True
    db: DbMapping = DbMapping()


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
    return config


def apply_overrides(config: ToolConfig, args: Any) -> ToolConfig:
    updates: dict[str, Any] = {}
    for field in [
        "adapter",
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
    return replace(config, **updates)


def _safe_identifier(value: object) -> str:
    text = str(value)
    if not IDENTIFIER_RE.match(text):
        raise ValueError(f"unsafe SQL identifier: {text}")
    return text


def _optional_string(value: object) -> str | None:
    if value is None:
        return None
    return str(value)
