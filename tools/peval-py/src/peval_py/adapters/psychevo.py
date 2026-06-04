from __future__ import annotations

import sqlite3
from pathlib import Path

from peval_py.adapters.common import CommonMessageAdapter
from peval_py.config import ToolConfig
from peval_py.sources import MessageRecord, read_jsonl, read_sqlite_messages


class PsychevoAdapter(CommonMessageAdapter):
    agent_id = "psychevo"
    default_agent_name = "psychevo"

    def convert_path(self, path: str, config: ToolConfig):
        source = Path(path).expanduser()
        if source.is_dir() or source.suffix in {".db", ".sqlite", ".sqlite3"}:
            return self.convert_db(str(source), None, config)
        return self.convert(read_jsonl(path), config)

    def convert_db(
        self,
        path: str,
        session_id: str | None,
        config: ToolConfig,
    ):
        records = read_psychevo_db(path, session_id, config)
        return self.convert(records, config)


def read_psychevo_db(
    path: str,
    session_id: str | None,
    config: ToolConfig,
) -> list[MessageRecord]:
    db_path = resolve_psychevo_db(path)
    selected_session_id = select_session_id(db_path, session_id)
    return read_sqlite_messages(str(db_path), selected_session_id, config.db)


def resolve_psychevo_db(path: str) -> Path:
    source = Path(path).expanduser()
    if source.is_dir():
        source = source / "state.db"
    if not source.exists():
        raise ValueError(f"Psychevo DB not found: {source}")
    return source


def select_session_id(path: Path, session_id: str | None) -> str:
    conn = sqlite3.connect(f"file:{path}?mode=ro", uri=True)
    try:
        if session_id:
            row = conn.execute(
                """
                SELECT id
                FROM sessions
                WHERE id = ?
                """,
                (session_id,),
            ).fetchone()
            if row is None:
                raise ValueError(f"Psychevo session not found: {session_id}")
            return str(row[0])
        row = conn.execute(
            """
            SELECT id
            FROM sessions
            ORDER BY updated_at_ms DESC, ended_at_ms DESC, started_at_ms DESC, id DESC
            LIMIT 1
            """
        ).fetchone()
    except sqlite3.Error as exc:
        raise ValueError(f"failed to read Psychevo DB: {exc}") from exc
    finally:
        conn.close()
    if row is None:
        raise ValueError("Psychevo DB contains no sessions")
    return str(row[0])
