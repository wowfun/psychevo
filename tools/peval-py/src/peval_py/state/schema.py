from __future__ import annotations

import json
from typing import Any


class StateSchemaMixin:
    def initialize_schema(self) -> None:
        self.paths.log_path.parent.mkdir(parents=True, exist_ok=True)

    def log_refresh(
        self,
        source_key: str,
        status: str,
        warning_count: int,
        error: str | None,
        timestamp: int,
    ) -> None:
        self.append_log(
            {
                "type": "refresh",
                "source_key": source_key,
                "status": status,
                "warning_count": warning_count,
                "error": error,
                "refreshed_at_ms": timestamp,
            }
        )

    def append_log(self, payload: dict[str, Any]) -> None:
        self.paths.log_path.parent.mkdir(parents=True, exist_ok=True)
        with self.paths.log_path.open("a", encoding="utf-8") as handle:
            handle.write(json.dumps(payload, ensure_ascii=False, sort_keys=True) + "\n")
