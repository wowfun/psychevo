from __future__ import annotations

import contextlib
import io
import json
import re
import shutil
import sqlite3
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from types import SimpleNamespace
from unittest.mock import patch

from peval_py.atif import convert_db, convert_path, convert_records
from peval_py.adapters import adapter_for, available_adapter_ids
from peval_py.adapters.base import ConversionResult, StepMeta
from peval_py.config import ToolConfig, apply_overrides, config_for_adapter, load_config
from peval_py.html import load_asset_text, render_html, render_serve_html
from peval_py.input_table import read_input_table
from peval_py.report import NoteInput, ReportSession, build_multi_report, build_report
from peval_py.sources import (
    ACCOUNTING_COLUMNS,
    MessageRecord,
    read_jsonl,
    read_sqlite_messages,
)

FIXTURES = Path(__file__).parent / "fixtures"


def script_json(html: str, element_id: str):
    match = re.search(
        rf'<script type="application/json" id="{re.escape(element_id)}">(.*?)</script>',
        html,
        re.S,
    )
    if not match:
        raise AssertionError(f"missing script json: {element_id}")
    return json.loads(match.group(1))


class FakeEntryPoint:
    def __init__(self, name: str, value) -> None:
        self.name = name
        self.value = value
        self.load_count = 0

    def load(self):
        self.load_count += 1
        return self.value


class FakeEntryPoints:
    def __init__(self, entries: list[FakeEntryPoint]) -> None:
        self.entries = entries

    def select(self, group: str) -> list[FakeEntryPoint]:
        if group == "peval_py.adapters":
            return self.entries
        return []


class BrokenEntryPoint(FakeEntryPoint):
    def load(self):
        self.load_count += 1
        raise AssertionError(f"{self.name} should not be loaded")


class CustomPathAdapter:
    agent_id = "custom"

    def convert_path(self, path: str, config: ToolConfig) -> ConversionResult:
        source = Path(path)
        prefix = str(config.adapter_options.get("label_prefix", "custom"))
        session_id = f"{prefix}:{source.stem}"
        return ConversionResult(
            trajectory={
                "schema_version": "ATIF-v1.7",
                "trajectory_id": f"custom:{source.stem}",
                "session_id": session_id,
                "agent": {
                    "name": config.agent_name or "custom",
                    "version": config.agent_version,
                },
                "steps": [
                    {
                        "step_id": 1,
                        "source": "user",
                        "message": source.read_text(encoding="utf-8").strip(),
                    }
                ],
                "final_metrics": {
                    "total_steps": 1,
                    "total_turns": 1,
                    "total_tool_calls": 0,
                    "total_tool_errors": 0,
                },
            },
            steps_meta=[StepMeta(step_id=1, source="user", timestamp_ms=100)],
            warnings=[],
            total_events=1,
            unmapped_events=0,
            started_at_ms=100,
            finished_at_ms=100,
        )


def create_messages_db(path: Path) -> None:
    conn = sqlite3.connect(path)
    columns = ", ".join(f"{name} INTEGER" for name in ACCOUNTING_COLUMNS[:-2])
    conn.execute(
        """
        CREATE TABLE sessions (
            id TEXT PRIMARY KEY,
            source TEXT NOT NULL,
            parent_session_id TEXT,
            workdir TEXT NOT NULL,
            model TEXT NOT NULL,
            provider TEXT NOT NULL,
            started_at_ms INTEGER NOT NULL,
            updated_at_ms INTEGER NOT NULL,
            ended_at_ms INTEGER,
            end_reason TEXT,
            archived_at_ms INTEGER,
            message_count INTEGER NOT NULL DEFAULT 0,
            tool_call_count INTEGER NOT NULL DEFAULT 0,
            title TEXT,
            metadata_json TEXT
        )
        """
    )
    conn.execute(
        f"""
        CREATE TABLE messages (
            session_id TEXT,
            session_seq INTEGER,
            message_json TEXT,
            usage_json TEXT,
            metadata_json TEXT,
            {columns},
            pricing_source TEXT,
            pricing_tier TEXT
        )
        """
    )
    conn.executemany(
        """
        INSERT INTO sessions
        (id, source, parent_session_id, workdir, model, provider,
         started_at_ms, updated_at_ms, ended_at_ms, end_reason, archived_at_ms,
         message_count, tool_call_count, title, metadata_json)
        VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        """,
        [
            (
                "db-a",
                "cli",
                None,
                "/tmp/a",
                "db-model-a",
                "test-provider",
                100,
                200,
                None,
                None,
                None,
                2,
                0,
                "DB A",
                None,
            ),
            (
                "db-b",
                "cli",
                None,
                "/tmp/b",
                "db-model-b",
                "test-provider",
                300,
                450,
                None,
                None,
                None,
                2,
                0,
                "DB B",
                None,
            ),
        ],
    )
    rows = [
        (
            "db-a",
            1,
            {"role": "user", "content": "hello a", "timestamp_ms": 100},
            None,
            None,
        ),
        (
            "db-a",
            2,
            {
                "role": "assistant",
                "content": "done a",
                "timestamp_ms": 200,
                "model": "db-model-a",
            },
            {"input_tokens": 2, "output_tokens": 3},
            None,
        ),
        (
            "db-b",
            1,
            {"role": "user", "content": "hello b", "timestamp_ms": 300},
            None,
            None,
        ),
        (
            "db-b",
            2,
            {
                "role": "assistant",
                "content": "done b",
                "timestamp_ms": 450,
                "model": "db-model-b",
            },
            {"input_tokens": 5, "output_tokens": 7},
            None,
        ),
    ]
    for session_id, seq, message, usage, metadata in rows:
        conn.execute(
            """
            INSERT INTO messages
            (session_id, session_seq, message_json, usage_json, metadata_json)
            VALUES (?, ?, ?, ?, ?)
            """,
            (
                session_id,
                seq,
                json.dumps(message),
                json.dumps(usage) if usage else None,
                json.dumps(metadata) if metadata else None,
            ),
        )
    conn.commit()
    conn.close()


def create_opencode_db(path: Path) -> None:
    conn = sqlite3.connect(path)
    conn.execute(
        """
        CREATE TABLE session (
            id TEXT PRIMARY KEY,
            title TEXT NOT NULL,
            directory TEXT NOT NULL,
            agent TEXT,
            model TEXT,
            time_created INTEGER NOT NULL,
            time_updated INTEGER NOT NULL
        )
        """
    )
    conn.execute(
        """
        CREATE TABLE message (
            id TEXT PRIMARY KEY,
            session_id TEXT NOT NULL,
            time_created INTEGER NOT NULL,
            time_updated INTEGER NOT NULL,
            data TEXT NOT NULL
        )
        """
    )
    conn.execute(
        """
        CREATE TABLE part (
            id TEXT PRIMARY KEY,
            message_id TEXT NOT NULL,
            session_id TEXT NOT NULL,
            time_created INTEGER NOT NULL,
            time_updated INTEGER NOT NULL,
            data TEXT NOT NULL
        )
        """
    )
    sessions = [
        ("ses-old", "Old session", "/tmp/old", None, None, 1000, 1100),
        (
            "ses-latest",
            "Latest session",
            "/tmp/latest",
            "build",
            json.dumps({"id": "oc-session-model"}),
            2000,
            2600,
        ),
    ]
    conn.executemany(
        """
        INSERT INTO session
        (id, title, directory, agent, model, time_created, time_updated)
        VALUES (?, ?, ?, ?, ?, ?, ?)
        """,
        sessions,
    )
    messages = [
        (
            "msg-old-user",
            "ses-old",
            1000,
            1000,
            json.dumps({"role": "user"}),
        ),
        (
            "msg-latest-user",
            "ses-latest",
            2000,
            2000,
            json.dumps({"role": "user"}),
        ),
        (
            "msg-latest-agent",
            "ses-latest",
            2100,
            2500,
            json.dumps(
                {
                    "role": "assistant",
                    "modelID": "oc-message-model",
                    "tokens": {"input": 2, "output": 3, "total": 5},
                    "cost": 0.000001,
                }
            ),
        ),
    ]
    conn.executemany(
        """
        INSERT INTO message
        (id, session_id, time_created, time_updated, data)
        VALUES (?, ?, ?, ?, ?)
        """,
        messages,
    )
    parts = [
        (
            "part-old-text",
            "msg-old-user",
            "ses-old",
            1000,
            1000,
            {"type": "text", "text": "old prompt"},
        ),
        (
            "part-latest-text",
            "msg-latest-user",
            "ses-latest",
            2000,
            2000,
            {"type": "text", "text": "latest prompt"},
        ),
        (
            "part-latest-reasoning",
            "msg-latest-agent",
            "ses-latest",
            2110,
            2110,
            {"type": "reasoning", "text": "thinking"},
        ),
        (
            "part-latest-tool",
            "msg-latest-agent",
            "ses-latest",
            2200,
            2300,
            {
                "type": "tool",
                "tool": "read",
                "callID": "call-read",
                "state": {
                    "status": "completed",
                    "input": {"file": "README.md"},
                    "output": "file contents",
                },
            },
        ),
        (
            "part-latest-finish",
            "msg-latest-agent",
            "ses-latest",
            2500,
            2500,
            {
                "type": "step-finish",
                "tokens": {"input": 2, "output": 3, "reasoning": 1, "total": 6},
                "cost": 0.000001,
            },
        ),
    ]
    conn.executemany(
        """
        INSERT INTO part
        (id, message_id, session_id, time_created, time_updated, data)
        VALUES (?, ?, ?, ?, ?, ?)
        """,
        [
            (id_, message_id, session_id, created, updated, json.dumps(data))
            for id_, message_id, session_id, created, updated, data in parts
        ],
    )
    conn.commit()
    conn.close()


def create_hermes_db(path: Path) -> None:
    conn = sqlite3.connect(path)
    conn.execute(
        """
        CREATE TABLE sessions (
            id TEXT PRIMARY KEY,
            source TEXT,
            title TEXT,
            model TEXT,
            system_prompt TEXT,
            started_at REAL,
            ended_at REAL,
            cwd TEXT,
            input_tokens INTEGER DEFAULT 0,
            output_tokens INTEGER DEFAULT 0,
            cache_read_tokens INTEGER DEFAULT 0,
            cache_write_tokens INTEGER DEFAULT 0,
            reasoning_tokens INTEGER DEFAULT 0,
            estimated_cost_usd REAL,
            actual_cost_usd REAL,
            pricing_version TEXT,
            cost_source TEXT,
            billing_provider TEXT
        )
        """
    )
    conn.execute(
        """
        CREATE TABLE messages (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            session_id TEXT NOT NULL,
            role TEXT NOT NULL,
            content TEXT,
            tool_call_id TEXT,
            tool_calls TEXT,
            tool_name TEXT,
            timestamp REAL NOT NULL,
            token_count INTEGER,
            finish_reason TEXT,
            reasoning TEXT,
            reasoning_content TEXT,
            platform_message_id TEXT,
            active INTEGER NOT NULL DEFAULT 1
        )
        """
    )
    conn.executemany(
        """
        INSERT INTO sessions
        (id, source, title, model, system_prompt, started_at, ended_at, cwd,
         input_tokens, output_tokens, cache_read_tokens, cache_write_tokens,
         reasoning_tokens, estimated_cost_usd, actual_cost_usd, pricing_version,
         cost_source, billing_provider)
        VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        """,
        [
            (
                "hermes-old",
                "cli",
                "Old Hermes",
                "hermes-old-model",
                None,
                100.0,
                120.0,
                "/tmp/old",
                1,
                2,
                0,
                0,
                0,
                0.000001,
                None,
                "old-prices",
                None,
                "test",
            ),
            (
                "hermes-latest",
                "cli",
                "Latest Hermes",
                "hermes-session-model",
                "Hermes system prompt",
                200.0,
                260.0,
                "/tmp/latest",
                11,
                13,
                2,
                3,
                5,
                0.00002,
                None,
                "test-prices",
                None,
                "test",
            ),
        ],
    )
    conn.executemany(
        """
        INSERT INTO messages
        (session_id, role, content, tool_call_id, tool_calls, tool_name,
         timestamp, token_count, finish_reason, reasoning, reasoning_content,
         platform_message_id, active)
        VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        """,
        [
            (
                "hermes-old",
                "user",
                "old prompt",
                None,
                None,
                None,
                100.0,
                None,
                None,
                None,
                None,
                "old-user",
                1,
            ),
            (
                "hermes-old",
                "assistant",
                "inactive old answer",
                None,
                None,
                None,
                9999.0,
                None,
                None,
                None,
                None,
                "old-inactive",
                0,
            ),
            (
                "hermes-latest",
                "user",
                "latest prompt",
                None,
                None,
                None,
                210.0,
                None,
                None,
                None,
                None,
                "latest-user",
                1,
            ),
            (
                "hermes-latest",
                "assistant",
                "latest answer",
                None,
                json.dumps(
                    [
                        {
                            "id": "call-lookup",
                            "function": {
                                "name": "lookup",
                                "arguments": json.dumps({"query": "state"}),
                            },
                        }
                    ]
                ),
                None,
                220.0,
                99,
                "tool_calls",
                "legacy reasoning",
                "latest reasoning",
                "latest-assistant",
                1,
            ),
            (
                "hermes-latest",
                "tool",
                "lookup result",
                "call-lookup",
                None,
                "lookup",
                230.0,
                None,
                None,
                None,
                None,
                "latest-tool",
                1,
            ),
        ],
    )
    conn.commit()
    conn.close()
