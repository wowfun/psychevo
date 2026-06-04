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
from peval_py.html import load_asset_text, render_html
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


class PevalPyTests(unittest.TestCase):
    def test_config_uses_adapter_default_and_accepts_legacy_agent_key(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            adapter_config = Path(tmp) / "adapter.toml"
            adapter_config.write_text(
                "[defaults]\nadapter = \"opencode\"\n",
                encoding="utf-8",
            )
            self.assertEqual(load_config(str(adapter_config)).adapter, "opencode")

            legacy_config = Path(tmp) / "legacy.toml"
            legacy_config.write_text(
                "[defaults]\nagent = \"hermes\"\n",
                encoding="utf-8",
            )
            self.assertEqual(load_config(str(legacy_config)).adapter, "hermes")

    def test_config_locale_defaults_aliases_and_invalid_values(self) -> None:
        self.assertEqual(load_config(None).locale, "en")
        with tempfile.TemporaryDirectory() as tmp:
            for value, expected in [
                ("en", "en"),
                ("en-US", "en"),
                ("zh-CN", "zh-CN"),
                ("zh", "zh-CN"),
            ]:
                with self.subTest(value=value):
                    config_path = Path(tmp) / f"{value}.toml"
                    config_path.write_text(
                        f"[defaults]\nlocale = \"{value}\"\n",
                        encoding="utf-8",
                    )
                    self.assertEqual(load_config(str(config_path)).locale, expected)

            invalid_config = Path(tmp) / "invalid.toml"
            invalid_config.write_text(
                "[defaults]\nlocale = \"fr-FR\"\n",
                encoding="utf-8",
            )
            with self.assertRaisesRegex(
                ValueError,
                "unsupported locale: fr-FR; supported locales: en, zh-CN",
            ):
                load_config(str(invalid_config))

    def test_config_passes_selected_adapter_options(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            config_path = Path(tmp) / "adapters.toml"
            config_path.write_text(
                """
[defaults]
adapter = "opencode"

[adapters.custom]
label_prefix = "configured"
enabled = true
""",
                encoding="utf-8",
            )
            config = load_config(str(config_path))
            self.assertEqual(config.adapter, "opencode")
            self.assertEqual(config.adapter_options, {})

            overridden = apply_overrides(
                config,
                SimpleNamespace(adapter="custom", no_redact=False),
            )
            self.assertEqual(overridden.adapter, "custom")
            self.assertEqual(
                overridden.adapter_options,
                {"label_prefix": "configured", "enabled": True},
            )

            list_override = apply_overrides(
                config,
                SimpleNamespace(adapter=["custom", "p1=opencode"], no_redact=False),
            )
            self.assertEqual(list_override.adapter, "custom")
            self.assertEqual(
                list_override.adapter_options,
                {"label_prefix": "configured", "enabled": True},
            )

            selected = config_for_adapter(config, "custom")
            self.assertEqual(
                selected.adapter_options,
                {"label_prefix": "configured", "enabled": True},
            )

    def test_adapter_registry_discovers_builtins_and_entry_points_lazily(self) -> None:
        custom_entry = FakeEntryPoint("custom", CustomPathAdapter)
        unused_entry = BrokenEntryPoint("unused", object())
        with patch(
            "peval_py.adapters.entry_points",
            return_value=FakeEntryPoints([custom_entry, unused_entry]),
        ):
            self.assertEqual(adapter_for("psychevo").agent_id, "psychevo")
            self.assertIn("custom", available_adapter_ids())
            self.assertEqual(custom_entry.load_count, 0)
            self.assertEqual(unused_entry.load_count, 0)

            adapter = adapter_for("custom")
            self.assertEqual(adapter.agent_id, "custom")
            self.assertEqual(custom_entry.load_count, 1)
            self.assertEqual(unused_entry.load_count, 0)

    def test_adapter_registry_accepts_class_factory_and_instance_entry_points(self) -> None:
        values = [CustomPathAdapter, lambda: CustomPathAdapter(), CustomPathAdapter()]
        for value in values:
            with self.subTest(value=type(value).__name__):
                with patch(
                    "peval_py.adapters.entry_points",
                    return_value=FakeEntryPoints([FakeEntryPoint("custom", value)]),
                ):
                    adapter = adapter_for("custom")
                    self.assertTrue(callable(getattr(adapter, "convert_path", None)))

    def test_adapter_registry_reports_duplicate_and_unknown_ids(self) -> None:
        duplicate = FakeEntryPoint("opencode", CustomPathAdapter)
        with patch(
            "peval_py.adapters.entry_points",
            return_value=FakeEntryPoints([duplicate]),
        ):
            with self.assertRaisesRegex(ValueError, "duplicate adapter id: opencode"):
                available_adapter_ids()
            self.assertEqual(duplicate.load_count, 0)

        custom = FakeEntryPoint("custom", CustomPathAdapter)
        with patch(
            "peval_py.adapters.entry_points",
            return_value=FakeEntryPoints([custom]),
        ):
            with self.assertRaisesRegex(ValueError, "unsupported adapter: missing"):
                adapter_for("missing")
            self.assertEqual(custom.load_count, 0)

    def test_cli_uses_custom_path_adapter_and_rejects_db_when_path_only(self) -> None:
        from peval_py.cli import main

        with tempfile.TemporaryDirectory() as tmp:
            tmp_path = Path(tmp)
            first = tmp_path / "first.txt"
            second = tmp_path / "second.txt"
            first.write_text("first prompt\n", encoding="utf-8")
            second.write_text("second prompt\n", encoding="utf-8")
            config_path = tmp_path / "custom.toml"
            config_path.write_text(
                """
[defaults]
adapter = "custom"

[adapters.custom]
label_prefix = "configured"
""",
                encoding="utf-8",
            )
            export_out = tmp_path / "trajectory.json"
            view_out = tmp_path / "report.json"
            entry = FakeEntryPoint("custom", CustomPathAdapter)
            with patch(
                "peval_py.adapters.entry_points",
                return_value=FakeEntryPoints([entry]),
            ):
                result = main(
                    [
                        "export",
                        "tr",
                        "-c",
                        str(config_path),
                        "-p",
                        str(first),
                        "-o",
                        str(export_out),
                    ]
                )
                self.assertEqual(result, 0)
                payload = json.loads(export_out.read_text(encoding="utf-8"))
                self.assertEqual(payload["session_id"], "configured:first")
                self.assertEqual(payload["steps"][0]["message"], "first prompt")

                result = main(
                    [
                        "view",
                        "tr",
                        "-c",
                        str(config_path),
                        "-p",
                        str(first),
                        "-p",
                        str(second),
                        "-f",
                        "json",
                        "-o",
                        str(view_out),
                    ]
                )
                self.assertEqual(result, 0)
                payload = json.loads(view_out.read_text(encoding="utf-8"))
                self.assertEqual(
                    [item["session_id"] for item in payload["trajectory"]],
                    ["configured:first", "configured:second"],
                )

                db_path = tmp_path / "state.db"
                create_messages_db(db_path)
                stderr = io.StringIO()
                with contextlib.redirect_stderr(stderr):
                    result = main(
                        [
                            "view",
                            "tr",
                            "-c",
                            str(config_path),
                            "-d",
                            str(db_path),
                            "-s",
                            "db-a",
                            "-f",
                            "json",
                            "-o",
                            str(tmp_path / "db-report.json"),
                        ]
                )
                self.assertNotEqual(result, 0)
                self.assertIn("does not support DB input", stderr.getvalue())

    def test_cli_adapter_selectors_apply_per_path_input(self) -> None:
        from peval_py.cli import main

        with tempfile.TemporaryDirectory() as tmp:
            tmp_path = Path(tmp)
            custom_path = tmp_path / "custom.txt"
            custom_path.write_text("custom prompt\n", encoding="utf-8")
            config_path = tmp_path / "custom.toml"
            config_path.write_text(
                """
[adapters.custom]
label_prefix = "selected"
""",
                encoding="utf-8",
            )
            out_path = tmp_path / "report.json"
            entry = FakeEntryPoint("custom", CustomPathAdapter)
            with patch(
                "peval_py.adapters.entry_points",
                return_value=FakeEntryPoints([entry]),
            ):
                result = main(
                    [
                        "view",
                        "tr",
                        "-c",
                        str(config_path),
                        "-a",
                        "opencode",
                        "-a",
                        "p2=custom",
                        "-p",
                        str(FIXTURES / "common_session.jsonl"),
                        "-p",
                        str(custom_path),
                        "-f",
                        "json",
                        "-o",
                        str(out_path),
                    ]
                )
                self.assertEqual(result, 0)
                payload = json.loads(out_path.read_text(encoding="utf-8"))
                self.assertEqual(
                    [item["agent"]["name"] for item in payload["trajectory"]],
                    ["opencode", "custom"],
                )
                self.assertEqual(
                    [item["adapter"] for item in payload["trajectory_meta"]],
                    ["opencode", "custom"],
                )
                self.assertEqual(
                    [item["adapter"] for item in payload["comparison"]["leaderboard"]["entries"]],
                    ["opencode", "custom"],
                )
                self.assertEqual(
                    payload["trajectory"][1]["session_id"],
                    "selected:custom",
                )

                for argv, message in [
                    (
                        [
                            "view",
                            "tr",
                            "-a",
                            "p1=custom",
                            "-a",
                            "p1=opencode",
                            "-p",
                            str(custom_path),
                        ],
                        "duplicate adapter selector: p1",
                    ),
                    (
                        [
                            "view",
                            "tr",
                            "-a",
                            "p2=custom",
                            "-p",
                            str(custom_path),
                        ],
                        "no matching --path input",
                    ),
                    (
                        [
                            "view",
                            "tr",
                            "-a",
                            "p1=missing",
                            "-p",
                            str(custom_path),
                        ],
                        "available adapters",
                    ),
                ]:
                    with self.subTest(message=message):
                        stderr = io.StringIO()
                        with contextlib.redirect_stderr(stderr):
                            result = main(argv)
                        self.assertNotEqual(result, 0)
                        self.assertIn(message, stderr.getvalue())

    def test_cli_multi_db_keyed_sessions_and_mixed_sources(self) -> None:
        from peval_py.cli import main

        with tempfile.TemporaryDirectory() as tmp:
            tmp_path = Path(tmp)
            hermes_db = tmp_path / "hermes.db"
            opencode_db = tmp_path / "opencode.db"
            create_hermes_db(hermes_db)
            create_opencode_db(opencode_db)

            multi_db_out = tmp_path / "multi-db.json"
            result = main(
                [
                    "view",
                    "tr",
                    "-d",
                    str(hermes_db),
                    "-d",
                    str(opencode_db),
                    "-a",
                    "d1=hermes",
                    "-a",
                    "d2=opencode",
                    "-s",
                    "d1=hermes-old",
                    "-s",
                    "d2=ses-old",
                    "-f",
                    "json",
                    "-o",
                    str(multi_db_out),
                ]
            )
            self.assertEqual(result, 0)
            payload = json.loads(multi_db_out.read_text(encoding="utf-8"))
            self.assertEqual(
                [item["session_id"] for item in payload["trajectory"]],
                ["hermes-old", "ses-old"],
            )
            self.assertEqual(
                [item["adapter"] for item in payload["trajectory_meta"]],
                ["hermes", "opencode"],
            )
            self.assertEqual(
                [item["adapter"] for item in payload["comparison"]["leaderboard"]["entries"]],
                ["hermes", "opencode"],
            )

            mixed_out = tmp_path / "mixed.json"
            result = main(
                [
                    "view",
                    "tr",
                    "-a",
                    "opencode",
                    "-p",
                    str(FIXTURES / "common_session.jsonl"),
                    "-d",
                    str(opencode_db),
                    "-f",
                    "json",
                    "-o",
                    str(mixed_out),
                ]
            )
            self.assertEqual(result, 0)
            payload = json.loads(mixed_out.read_text(encoding="utf-8"))
            self.assertEqual(len(payload["trajectory"]), 2)
            self.assertEqual(
                [item["adapter"] for item in payload["trajectory_meta"]],
                ["opencode", "opencode"],
            )
            self.assertEqual(payload["trajectory"][1]["session_id"], "ses-latest")

            stderr = io.StringIO()
            with contextlib.redirect_stderr(stderr):
                result = main(
                    [
                        "view",
                        "tr",
                        "-d",
                        str(hermes_db),
                        "-d",
                        str(opencode_db),
                        "-a",
                        "d1=hermes",
                        "-a",
                        "d2=opencode",
                        "-s",
                        "ses-old",
                    ]
                )
            self.assertNotEqual(result, 0)
            self.assertIn("bare --session-id", stderr.getvalue())

    def test_psychevo_sqlite_messages_are_ordered_by_session_seq(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            db_path = Path(tmp) / "state.db"
            conn = sqlite3.connect(db_path)
            columns = ", ".join(f"{name} INTEGER" for name in ACCOUNTING_COLUMNS[:-2])
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
            user = {"role": "user", "content": [{"text": "first"}], "timestamp_ms": 100}
            assistant = {
                "role": "assistant",
                "content": [{"type": "text", "text": "second"}],
                "timestamp_ms": 200,
                "model": "db-model",
            }
            conn.execute(
                "INSERT INTO messages (session_id, session_seq, message_json) VALUES (?, ?, ?)",
                ("s1", 2, json.dumps(assistant)),
            )
            conn.execute(
                "INSERT INTO messages (session_id, session_seq, message_json) VALUES (?, ?, ?)",
                ("s1", 1, json.dumps(user)),
            )
            conn.commit()
            conn.close()

            records = read_sqlite_messages(str(db_path), "s1", ToolConfig().db)
            self.assertEqual([record.session_seq for record in records], [1, 2])
            self.assertEqual([record.source_session_id for record in records], ["s1", "s1"])
            result = convert_records(records, ToolConfig(adapter="psychevo"))
            self.assertEqual(result.trajectory["session_id"], "s1")
            self.assertEqual(result.trajectory["steps"][0]["message"], "first")
            self.assertEqual(result.trajectory["steps"][1]["message"], "second")
            self.assertEqual(result.trajectory["agent"]["model_name"], "db-model")

    def test_psychevo_db_adapter_reads_latest_and_explicit_sessions(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            db_path = Path(tmp) / "state.db"
            create_messages_db(db_path)
            config = ToolConfig(adapter="psychevo")

            latest = convert_db(str(db_path), None, config)
            self.assertEqual(latest.trajectory["session_id"], "db-b")
            self.assertEqual(latest.trajectory["agent"]["name"], "psychevo")
            self.assertEqual(latest.trajectory["agent"]["model_name"], "db-model-b")
            self.assertEqual(latest.trajectory["steps"][0]["message"], "hello b")
            self.assertEqual(latest.trajectory["steps"][1]["message"], "done b")
            self.assertEqual(latest.trajectory["final_metrics"]["usage"]["input_tokens"], 5)
            self.assertEqual(latest.trajectory["final_metrics"]["usage"]["output_tokens"], 7)

            explicit = convert_db(str(db_path), "db-a", config)
            self.assertEqual(explicit.trajectory["session_id"], "db-a")
            self.assertEqual(explicit.trajectory["steps"][0]["message"], "hello a")

    def test_psychevo_jsonl_conversion_shape(self) -> None:
        records = read_jsonl(str(FIXTURES / "psychevo_session.jsonl"))
        result = convert_records(
            records,
            ToolConfig(adapter="psychevo", trajectory_id="trial:psychevo"),
        )
        trajectory = result.trajectory
        self.assertEqual(trajectory["schema_version"], "ATIF-v1.7")
        self.assertEqual(trajectory["trajectory_id"], "trial:psychevo")
        self.assertEqual(trajectory["session_id"], "sess-psychevo")
        self.assertEqual([step["step_id"] for step in trajectory["steps"]], [1, 2, 3])
        self.assertEqual(trajectory["steps"][0]["source"], "user")
        self.assertEqual(trajectory["steps"][1]["reasoning_content"], "Inspect the file first.")
        self.assertEqual(
            trajectory["steps"][1]["tool_calls"][0]["tool_call_id"],
            "call-1",
        )
        self.assertEqual(
            trajectory["steps"][1]["tool_calls"][0]["arguments"]["api_key"],
            "<redacted>",
        )
        self.assertEqual(
            trajectory["steps"][1]["observation"]["results"][0]["source_call_id"],
            "call-1",
        )
        self.assertEqual(trajectory["steps"][2]["message"], "Done.")
        self.assertEqual(trajectory["final_metrics"]["total_prompt_tokens"], 8)
        self.assertEqual(trajectory["final_metrics"]["total_completion_tokens"], 10)
        self.assertEqual(trajectory["final_metrics"]["total_tool_calls"], 1)
        self.assertEqual(trajectory["final_metrics"]["total_tool_errors"], 0)
        self.assertEqual(trajectory["final_metrics"]["total_steps"], 3)
        self.assertEqual(trajectory["final_metrics"]["usage"]["input_tokens"], 8)
        self.assertEqual(trajectory["final_metrics"]["usage"]["output_tokens"], 10)
        self.assertEqual(trajectory["final_metrics"]["usage"]["cache_read_tokens"], 1)

        tool_meta = result.steps_meta[1].tool_calls[0]
        self.assertEqual(tool_meta.status, "completed")
        self.assertEqual(tool_meta.execution_duration_ms, 321)
        self.assertEqual(tool_meta.execution_duration_source, "message_metadata")

    def test_opencode_and_hermes_use_common_jsonl_adapter(self) -> None:
        records = read_jsonl(str(FIXTURES / "common_session.jsonl"))
        for adapter in ["opencode", "hermes"]:
            with self.subTest(adapter=adapter):
                result = convert_records(records, ToolConfig(adapter=adapter))
                self.assertEqual(result.trajectory["agent"]["name"], adapter)
                self.assertEqual(result.trajectory["steps"][0]["source"], "system")
                self.assertEqual(
                    result.trajectory["steps"][2]["tool_calls"][0]["function_name"],
                    "list",
                )
                self.assertEqual(
                    result.trajectory["steps"][2]["observation"]["results"][0][
                        "source_call_id"
                    ],
                    "tool-1",
                )

    def test_path_input_reads_exported_atif_json_directly(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            source = convert_records(
                read_jsonl(str(FIXTURES / "common_session.jsonl")),
                ToolConfig(adapter="opencode"),
            )
            atif_path = Path(tmp) / "trajectory.json"
            atif_path.write_text(
                json.dumps(source.trajectory, ensure_ascii=False),
                encoding="utf-8",
            )

            loaded = convert_path(str(atif_path), ToolConfig(adapter="psychevo"))
            self.assertEqual(loaded.trajectory, source.trajectory)
            self.assertEqual(len(loaded.steps_meta), len(source.trajectory["steps"]))
            self.assertEqual(loaded.total_events, len(source.trajectory["steps"]))

    def test_cli_view_accepts_exported_atif_json_path(self) -> None:
        from peval_py.cli import main

        with tempfile.TemporaryDirectory() as tmp:
            tmp_path = Path(tmp)
            source = convert_records(
                read_jsonl(str(FIXTURES / "common_session.jsonl")),
                ToolConfig(adapter="opencode"),
            )
            atif_path = tmp_path / "trajectory.json"
            out_path = tmp_path / "report.json"
            atif_path.write_text(
                json.dumps(source.trajectory, ensure_ascii=False),
                encoding="utf-8",
            )

            result = main(
                [
                    "view",
                    "tr",
                    "-p",
                    str(atif_path),
                    "-f",
                    "json",
                    "-o",
                    str(out_path),
                ]
            )
            self.assertEqual(result, 0)
            payload = json.loads(out_path.read_text(encoding="utf-8"))
            self.assertEqual(payload["trajectory"][0], source.trajectory)
            self.assertEqual(payload["trajectory_meta"][0]["adapter"], "atif")

            missing_adapter_config = tmp_path / "missing.toml"
            missing_adapter_config.write_text(
                "[defaults]\nadapter = \"missing\"\n",
                encoding="utf-8",
            )
            result = main(
                [
                    "view",
                    "tr",
                    "-c",
                    str(missing_adapter_config),
                    "-p",
                    str(atif_path),
                    "-f",
                    "json",
                    "-o",
                    str(out_path),
                ]
            )
            self.assertEqual(result, 0)
            payload = json.loads(out_path.read_text(encoding="utf-8"))
            self.assertEqual(payload["trajectory_meta"][0]["adapter"], "atif")

            result = main(
                [
                    "export",
                    "tr",
                    "-c",
                    str(missing_adapter_config),
                    "-p",
                    str(atif_path),
                    "-o",
                    str(tmp_path / "exported-again.json"),
                ]
            )
            self.assertEqual(result, 0)

    def test_opencode_db_adapter_reads_latest_and_explicit_sessions(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            db_path = Path(tmp) / "opencode.db"
            create_opencode_db(db_path)
            config = ToolConfig(adapter="opencode")

            latest = convert_db(str(db_path), None, config)
            self.assertEqual(latest.trajectory["session_id"], "ses-latest")
            self.assertEqual(latest.trajectory["agent"]["name"], "opencode")
            self.assertEqual(
                latest.trajectory["agent"]["model_name"],
                "oc-message-model",
            )
            self.assertEqual(latest.trajectory["steps"][0]["message"], "latest prompt")
            self.assertEqual(
                latest.trajectory["steps"][1]["reasoning_content"],
                "thinking",
            )
            self.assertEqual(
                latest.trajectory["steps"][1]["tool_calls"][0]["function_name"],
                "read",
            )
            self.assertEqual(
                latest.trajectory["steps"][1]["observation"]["results"][0][
                    "source_call_id"
                ],
                "call-read",
            )
            self.assertEqual(latest.trajectory["final_metrics"]["total_tool_calls"], 1)
            self.assertEqual(latest.trajectory["final_metrics"]["usage"]["input_tokens"], 2)

            old = convert_db(str(db_path), "ses-old", config)
            self.assertEqual(old.trajectory["session_id"], "ses-old")
            self.assertEqual(old.trajectory["steps"][0]["message"], "old prompt")

    def test_hermes_db_adapter_reads_latest_and_explicit_sessions(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            db_path = Path(tmp) / "state.db"
            create_hermes_db(db_path)
            config = ToolConfig(adapter="hermes")

            latest = convert_db(str(db_path), None, config)
            self.assertEqual(latest.trajectory["session_id"], "hermes-latest")
            self.assertEqual(latest.trajectory["agent"]["name"], "hermes")
            self.assertEqual(
                latest.trajectory["agent"]["model_name"],
                "hermes-session-model",
            )
            self.assertEqual(latest.trajectory["steps"][0]["source"], "system")
            self.assertEqual(
                latest.trajectory["steps"][0]["message"],
                "Hermes system prompt",
            )
            self.assertEqual(latest.trajectory["steps"][1]["message"], "latest prompt")
            self.assertEqual(latest.trajectory["steps"][2]["message"], "latest answer")
            self.assertEqual(
                latest.trajectory["steps"][2]["reasoning_content"],
                "latest reasoning",
            )
            self.assertEqual(
                latest.trajectory["steps"][2]["tool_calls"][0]["function_name"],
                "lookup",
            )
            self.assertEqual(
                latest.trajectory["steps"][2]["tool_calls"][0]["arguments"],
                {"query": "state"},
            )
            self.assertEqual(
                latest.trajectory["steps"][2]["observation"]["results"][0][
                    "source_call_id"
                ],
                "call-lookup",
            )
            self.assertEqual(latest.trajectory["final_metrics"]["total_tool_calls"], 1)
            self.assertEqual(latest.trajectory["final_metrics"]["usage"]["input_tokens"], 11)
            self.assertEqual(latest.trajectory["final_metrics"]["usage"]["output_tokens"], 13)
            self.assertEqual(
                latest.trajectory["final_metrics"]["accounting"]["pricing_source"],
                "test-prices",
            )
            self.assertEqual(latest.warnings, [])

            old = convert_db(str(db_path), "hermes-old", config)
            self.assertEqual(old.trajectory["session_id"], "hermes-old")
            self.assertEqual(old.trajectory["steps"][0]["message"], "old prompt")
            self.assertEqual(len(old.trajectory["steps"]), 1)
            self.assertEqual(old.trajectory["final_metrics"]["usage"]["input_tokens"], 1)

    def test_tool_timing_fallback_and_unmatched_warning(self) -> None:
        records = [
            MessageRecord(
                message={
                    "role": "assistant",
                    "tool_calls": [
                        {
                            "tool_call_id": "call-fallback",
                            "function_name": "read",
                            "arguments": {"path": "add.py"},
                        }
                    ],
                    "timestamp_ms": 1000,
                }
            ),
            MessageRecord(
                message={
                    "role": "tool_result",
                    "tool_call_id": "call-fallback",
                    "tool_name": "read",
                    "content": "ok",
                    "timestamp_ms": 1250,
                }
            ),
        ]
        result = convert_records(records, ToolConfig(adapter="psychevo"))
        self.assertEqual(len(result.trajectory["steps"]), 1)
        self.assertEqual(
            result.trajectory["steps"][0]["observation"]["results"][0][
                "source_call_id"
            ],
            "call-fallback",
        )
        tool_meta = result.steps_meta[0].tool_calls[0]
        self.assertEqual(tool_meta.execution_duration_ms, 250)
        self.assertEqual(tool_meta.execution_duration_source, "event_timestamps")

        unmatched = convert_records(
            [
                MessageRecord(
                    message={
                        "role": "tool_result",
                        "tool_call_id": "missing-call",
                        "tool_name": "read",
                        "content": "orphan",
                        "timestamp_ms": 1500,
                    }
                )
            ],
            ToolConfig(adapter="psychevo"),
        )
        self.assertEqual(len(unmatched.trajectory["steps"]), 1)
        self.assertEqual(
            unmatched.trajectory["steps"][0]["observation"]["results"][0][
                "source_call_id"
            ],
            "missing-call",
        )
        self.assertIn("unmatched tool result: missing-call", unmatched.warnings)

    def test_mid_session_tool_failure_is_nested_and_flow_continues(self) -> None:
        records = [
            MessageRecord(message={"role": "user", "content": "fix add.py", "timestamp_ms": 900}),
            MessageRecord(
                message={
                    "role": "assistant",
                    "content": "I will run tests.",
                    "tool_calls": [
                        {
                            "tool_call_id": "call-test-1",
                            "function_name": "exec_command",
                            "arguments": {"cmd": "pytest"},
                        }
                    ],
                    "timestamp_ms": 1000,
                }
            ),
            MessageRecord(
                message={
                    "role": "tool_result",
                    "tool_call_id": "call-test-1",
                    "tool_name": "exec_command",
                    "content": {"exit_code": 1, "stderr": "AssertionError"},
                    "is_error": True,
                    "timestamp_ms": 1125,
                },
                metadata={"elapsed_ms": 125},
            ),
            MessageRecord(
                message={
                    "role": "assistant",
                    "content": "I will inspect the failure.",
                    "tool_calls": [
                        {
                            "tool_call_id": "call-read-1",
                            "function_name": "read",
                            "arguments": {"path": "add.py"},
                        }
                    ],
                    "timestamp_ms": 1200,
                }
            ),
            MessageRecord(
                message={
                    "role": "tool_result",
                    "tool_call_id": "call-read-1",
                    "tool_name": "read",
                    "content": "def add(a, b): return a - b",
                    "timestamp_ms": 1300,
                }
            ),
            MessageRecord(
                message={
                    "role": "assistant",
                    "content": "The failing test is explained.",
                    "timestamp_ms": 1400,
                }
            ),
        ]

        result = convert_records(records, ToolConfig(adapter="psychevo"))
        trajectory = result.trajectory

        self.assertEqual([step["step_id"] for step in trajectory["steps"]], [1, 2, 3, 4])
        self.assertEqual(
            trajectory["steps"][1]["observation"]["results"][0]["source_call_id"],
            "call-test-1",
        )
        self.assertEqual(
            trajectory["steps"][2]["observation"]["results"][0]["source_call_id"],
            "call-read-1",
        )
        self.assertEqual(trajectory["final_metrics"]["total_tool_calls"], 2)
        self.assertEqual(trajectory["final_metrics"]["total_tool_errors"], 1)
        self.assertEqual(result.steps_meta[1].tool_calls[0].status, "error")
        self.assertTrue(result.steps_meta[1].tool_error)
        self.assertEqual(result.steps_meta[1].tool_calls[0].execution_duration_ms, 125)
        self.assertEqual(result.steps_meta[2].tool_calls[0].status, "completed")
        self.assertFalse(result.steps_meta[2].tool_error)
        self.assertEqual(result.warnings, [])

        config = ToolConfig(adapter="psychevo", trajectory_id="trial:failure")
        report = build_report(result, config, "inline")
        step_meta = report["trajectory_meta"][0]["steps"]
        self.assertEqual(step_meta[1]["duration_ms"], 125)
        self.assertEqual(step_meta[2]["duration_ms"], 100)
        html = render_html(report)
        self.assertIn("tool-error-chip", html)
        self.assertIn("exec_command", html)
        self.assertIn("tool success / total", html)

    def test_malformed_jsonl_reports_line_number(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "bad.jsonl"
            path.write_text('{"role":"user","content":"ok"}\n{bad}\n', encoding="utf-8")
            with self.assertRaisesRegex(ValueError, "line 2"):
                read_jsonl(str(path))

    def test_report_json_subset_and_html_safe_embedding(self) -> None:
        records = read_jsonl(str(FIXTURES / "psychevo_session.jsonl"))
        config = ToolConfig(adapter="psychevo", trajectory_id="trial:html")
        result = convert_records(records, config)
        report = build_report(result, config, "psychevo_session.jsonl")
        self.assertEqual(report["schema_version"], 17)
        self.assertEqual(report["includes"], ["core"])
        self.assertIn("trajectory", report)
        self.assertIn("trajectory_meta", report)
        self.assertEqual(report["trajectory_meta"][0]["adapter"], "psychevo")
        self.assertEqual(report["trajectory_meta"][0]["status"], "passed")

        html = render_html(report)
        self.assertIn("data-step-action=\"toggle\"", html)
        self.assertIn("<h1>Agent Trajectory Report</h1>", html)
        self.assertNotIn("<p class=\"eyebrow\">agent trajectory</p>", html)
        self.assertNotIn("id=\"report-copy\"", html)
        self.assertNotIn("id=\"score-strip\"", html)
        self.assertNotIn("class=\"metric-card\"", html)
        self.assertIn('"run": "Run"', html)
        self.assertIn('t("run", "Run")', html)
        self.assertIn('t("result", "Result")', html)
        self.assertIn('t("evidence", "Evidence")', html)
        self.assertIn('"usage_breakdown": "Usage Breakdown"', html)
        self.assertIn("wall duration", html)
        self.assertIn("tool success / total", html)
        self.assertIn(
            "body{margin:0;background:var(--canvas);color:var(--ink);"
            "font:15px/1.48 var(--sans)}",
            html,
        )
        font_sizes = [
            int(value)
            for value in re.findall(r"font(?:-size)?:[^;}]*?(\d+)px", html)
        ]
        self.assertGreaterEqual(min(font_sizes), 12)
        self.assertIn("\\u003cscript", html)
        self.assertNotIn("<script>alert(1)</script>", html)

    def test_multi_session_jsonl_report_comparison_and_notes(self) -> None:
        config = ToolConfig(adapter="opencode")
        first = convert_records(read_jsonl(str(FIXTURES / "common_session.jsonl")), config)
        second = convert_records(read_jsonl(str(FIXTURES / "psychevo_session.jsonl")), config)

        report = build_multi_report(
            [
                ReportSession(
                    conversion=first,
                    input_label="common_session.jsonl",
                    input_path=str(FIXTURES / "common_session.jsonl"),
                    session_hint="common_session",
                ),
                ReportSession(
                    conversion=second,
                    input_label="psychevo_session.jsonl",
                    input_path=str(FIXTURES / "psychevo_session.jsonl"),
                    session_hint="psychevo_session",
                ),
            ],
            config,
            [
                NoteInput(index=0, markdown="Report <script>note</script>"),
                NoteInput(index=2, markdown="Second session note"),
            ],
        )

        self.assertEqual(report["includes"], ["core", "comparison", "annotations"])
        self.assertEqual(len(report["trajectory"]), 2)
        self.assertEqual(report["trajectory"][0]["session_id"], "common_session")
        self.assertEqual(report["trajectory"][1]["session_id"], "sess-psychevo")
        self.assertEqual(report["comparison"]["summary"]["session_count"], 2)
        self.assertEqual(
            report["comparison"]["selected_trial_key"],
            report["trajectory_meta"][0]["trial_key"],
        )
        self.assertEqual(report["comparison"]["default_metric"], "duration")
        rows = report["comparison"]["session_table"]["rows"]
        self.assertEqual(len(rows), 2)
        entries = report["comparison"]["leaderboard"]["entries"]
        self.assertEqual(len(entries), 2)
        forbidden = {"benchmark", "task", "task_id", "task_set_id", "task_family"}
        for row in [*rows, *entries]:
            self.assertTrue(forbidden.isdisjoint(row))
        self.assertEqual(report["annotations"]["report_notes"][0]["markdown"], "Report <script>note</script>")
        self.assertEqual(
            report["annotations"]["notes"][0]["trial_key"],
            report["trajectory_meta"][1]["trial_key"],
        )

        html = render_html(report)
        self.assertNotIn("<h3>Summary</h3>", html)
        self.assertNotIn("Session Heatmap", html)
        self.assertNotIn("Session Table", html)
        self.assertIn("report-note-list", html)
        self.assertIn("report-note", html)
        self.assertIn("Visible Heatmap", html)
        self.assertNotIn("visible_heatmap_eyebrow", html)
        self.assertIn("session-axis", html)
        self.assertIn("grid-template-columns:minmax(150px,220px) minmax(0,1fr)", html)
        self.assertNotIn("repeat(${Math.max(rows.length, 1)}, minmax(150px, 1fr))", html)
        self.assertIn("metric-button", html)
        self.assertIn('labelKey: "duration"', html)
        self.assertIn('labelKey: "tokens"', html)
        self.assertIn('labelKey: "tool_calls"', html)
        self.assertIn('labelKey: "turns"', html)
        self.assertIn("Leaderboard", html)
        self.assertNotIn("leaderboard_eyebrow", html)
        self.assertIn("data-table-sort", html)
        self.assertIn("selected-row", html)
        self.assertIn("data-trial-key", html)
        self.assertIn("selected trial trajectory", html)
        self.assertIn("note-list", html)
        self.assertIn("note-snippet", html)
        self.assertIn("Second session note", html)
        self.assertIn("Report \\u003cscript", html)
        self.assertNotIn("<script>note</script>", html)

    def test_html_report_locale_localizes_report_chrome_except_steps(self) -> None:
        config = ToolConfig(adapter="opencode")
        first = convert_records(read_jsonl(str(FIXTURES / "common_session.jsonl")), config)
        second = convert_records(read_jsonl(str(FIXTURES / "psychevo_session.jsonl")), config)
        report = build_multi_report(
            [
                ReportSession(
                    conversion=first,
                    input_label="common_session.jsonl",
                    input_path=str(FIXTURES / "common_session.jsonl"),
                    session_hint="common_session",
                ),
                ReportSession(
                    conversion=second,
                    input_label="psychevo_session.jsonl",
                    input_path=str(FIXTURES / "psychevo_session.jsonl"),
                    session_hint="psychevo_session",
                ),
            ],
            config,
            [],
        )

        english_html = render_html(report)
        zh_html = render_html(report, locale="zh-CN")

        self.assertIn('<html lang="en">', english_html)
        self.assertIn("<h1>Agent Trajectory Report</h1>", english_html)
        self.assertIn("Visible Heatmap", english_html)
        self.assertIn("Leaderboard", english_html)
        self.assertNotIn("Agent 轨迹报告", english_html)
        self.assertNotIn("可见热力图", english_html)

        self.assertIn('<html lang="zh-CN">', zh_html)
        self.assertIn("<h1>Agent 轨迹报告</h1>", zh_html)
        self.assertIn('"visible_heatmap": "可见热力图"', zh_html)
        self.assertIn('"leaderboard": "Leaderboard"', zh_html)
        self.assertNotIn("visible_heatmap_eyebrow", zh_html)
        self.assertNotIn("leaderboard_eyebrow", zh_html)
        self.assertIn('"duration": "耗时"', zh_html)
        self.assertIn('"status.passed": "通过"', zh_html)
        self.assertIn('"session": "Session"', zh_html)
        self.assertIn('"result": "Result"', zh_html)
        self.assertIn('"notes": "Notes"', zh_html)
        self.assertIn('"selected_trial_trajectory": "selected trial trajectory"', zh_html)
        self.assertIn('"run": "Run"', zh_html)
        self.assertIn('"variant": "variant"', zh_html)
        self.assertIn('"evaluator": "evaluator"', zh_html)
        self.assertIn('"reasoning": "reasoning"', zh_html)
        self.assertIn('"reasoning_exposed": "reasoning exposed"', zh_html)
        self.assertIn('"steps_events": "steps/events"', zh_html)
        self.assertIn('"turns": "Turns"', zh_html)
        self.assertIn('"tool_calls": "Tool Calls"', zh_html)
        self.assertIn('"tool_success_total": "tool success / total"', zh_html)
        self.assertIn('"evidence": "Evidence"', zh_html)
        self.assertIn('"cache_read": "cache read"', zh_html)
        self.assertIn('"cache_write": "cache write"', zh_html)
        self.assertIn('"usage_breakdown": "用量明细"', zh_html)
        self.assertNotIn('"session": "会话"', zh_html)
        self.assertNotIn('"result": "结果"', zh_html)
        self.assertNotIn('"notes": "备注"', zh_html)
        self.assertNotIn('"selected_trial_trajectory": "选中的 Trial 轨迹"', zh_html)
        self.assertNotIn('"run": "运行"', zh_html)
        self.assertNotIn('"variant": "变体"', zh_html)
        self.assertNotIn('"evaluator": "评估器"', zh_html)
        self.assertNotIn('"reasoning": "推理"', zh_html)
        self.assertNotIn('"reasoning_exposed": "包含推理"', zh_html)
        self.assertNotIn('"steps_events": "步骤/事件"', zh_html)
        self.assertNotIn('"turns": "轮次"', zh_html)
        self.assertNotIn('"tool_calls": "工具调用"', zh_html)
        self.assertNotIn('"tool_success_total": "工具成功 / 总数"', zh_html)
        self.assertNotIn('"evidence": "证据"', zh_html)
        self.assertNotIn('"cache_read": "缓存读取"', zh_html)
        self.assertNotIn('"cache_write": "缓存写入"', zh_html)
        self.assertNotIn('"leaderboard": "排行榜"', zh_html)
        self.assertNotIn(">排行榜<", zh_html)
        self.assertIn("<h3>Steps (${count})</h3>", zh_html)
        self.assertIn("<h4>Tool Calls</h4>", zh_html)

    def test_cli_db_multi_session_view_and_note_validation(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            db_path = Path(tmp) / "state.db"
            out_path = Path(tmp) / "report.json"
            create_messages_db(db_path)

            result = subprocess.run(
                [
                    sys.executable,
                    "-m",
                    "peval_py.cli",
                    "view",
                    "tr",
                    "-d",
                    str(db_path),
                    "-s",
                    "db-a",
                    "-s",
                    "db-b",
                    "-n",
                    "0=DB report",
                    "--note",
                    "2=DB B",
                    "-f",
                    "json",
                    "-o",
                    str(out_path),
                ],
                check=True,
                text=True,
                capture_output=True,
            )
            self.assertEqual(result.stderr, "")
            payload = json.loads(out_path.read_text(encoding="utf-8"))
            self.assertEqual([item["session_id"] for item in payload["trajectory"]], ["db-a", "db-b"])
            self.assertEqual(payload["comparison"]["summary"]["session_count"], 2)
            self.assertEqual(payload["annotations"]["report_notes"][0]["markdown"], "DB report")
            self.assertEqual(payload["annotations"]["notes"][0]["markdown"], "DB B")

            bad_note = subprocess.run(
                [
                    sys.executable,
                    "-m",
                    "peval_py.cli",
                    "view",
                    "tr",
                    "-d",
                    str(db_path),
                    "-s",
                    "db-a",
                    "-n",
                    "2=missing",
                ],
                check=False,
                text=True,
                capture_output=True,
            )
            self.assertNotEqual(bad_note.returncode, 0)
            self.assertIn("out of range", bad_note.stderr)

    def test_cli_multi_path_rules_and_export_single_session_boundary(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            out_path = Path(tmp) / "multi.json"
            result = subprocess.run(
                [
                    sys.executable,
                    "-m",
                    "peval_py.cli",
                    "view",
                    "tr",
                    "-a",
                    "opencode",
                    "-p",
                    str(FIXTURES / "common_session.jsonl"),
                    "-p",
                    str(FIXTURES / "psychevo_session.jsonl"),
                    "-n",
                    "1=First session note",
                    "-f",
                    "json",
                    "-o",
                    str(out_path),
                ],
                check=True,
                text=True,
                capture_output=True,
            )
            self.assertEqual(result.stderr, "")
            payload = json.loads(out_path.read_text(encoding="utf-8"))
            self.assertEqual(len(payload["trajectory"]), 2)
            self.assertIn("comparison", payload)
            subprocess.run(
                [sys.executable, "-m", "json.tool", str(out_path)],
                check=True,
                text=True,
                capture_output=True,
            )

            mixed_db = Path(tmp) / "opencode.db"
            mixed_out = Path(tmp) / "mixed.json"
            create_opencode_db(mixed_db)
            mixed = subprocess.run(
                [
                    sys.executable,
                    "-m",
                    "peval_py.cli",
                    "view",
                    "tr",
                    "-a",
                    "opencode",
                    "-p",
                    str(FIXTURES / "common_session.jsonl"),
                    "-d",
                    str(mixed_db),
                    "-f",
                    "json",
                    "-o",
                    str(mixed_out),
                ],
                check=True,
                text=True,
                capture_output=True,
            )
            self.assertEqual(mixed.stderr, "")
            payload = json.loads(mixed_out.read_text(encoding="utf-8"))
            self.assertEqual(len(payload["trajectory"]), 2)
            self.assertEqual(
                [item["adapter"] for item in payload["trajectory_meta"]],
                ["opencode", "opencode"],
            )

            export_multi = subprocess.run(
                [
                    sys.executable,
                    "-m",
                    "peval_py.cli",
                    "export",
                    "tr",
                    "-p",
                    str(FIXTURES / "common_session.jsonl"),
                    "-p",
                    str(FIXTURES / "psychevo_session.jsonl"),
                ],
                check=False,
                text=True,
                capture_output=True,
            )
            self.assertNotEqual(export_multi.returncode, 0)
            self.assertIn("exactly one input session", export_multi.stderr)

            legacy_jsonl_flag = subprocess.run(
                [
                    sys.executable,
                    "-m",
                    "peval_py.cli",
                    "view",
                    "tr",
                    "-j",
                    str(FIXTURES / "common_session.jsonl"),
                ],
                check=False,
                text=True,
                capture_output=True,
            )
            self.assertNotEqual(legacy_jsonl_flag.returncode, 0)

    def test_html_renders_tool_names_timing_and_nested_observations(self) -> None:
        records = [
            MessageRecord(
                message={
                    "role": "assistant",
                    "content": [
                        {
                            "type": "tool_call",
                            "id": "call-exec",
                            "name": "exec_command",
                            "arguments": {"cmd": "true"},
                        }
                    ],
                    "timestamp_ms": 1000,
                },
                usage={"prompt_tokens": 21460},
            ),
            MessageRecord(
                message={
                    "role": "tool_result",
                    "tool_call_id": "call-exec",
                    "tool_name": "exec_command",
                    "content": {"exit_code": 0},
                    "timestamp_ms": 1110,
                },
                metadata={"elapsed_ms": 101},
            ),
        ]
        config = ToolConfig(adapter="psychevo", trajectory_id="trial:tool-html")
        result = convert_records(records, config)
        report = build_report(result, config, "inline")
        html = render_html(report)

        self.assertEqual(len(report["trajectory"][0]["steps"]), 1)
        self.assertIn("exec_command", html)
        self.assertIn("tool exec", html)
        self.assertIn("rail-summary", html)
        self.assertIn("rail-tool-row", html)
        self.assertIn("function stepTimingStats", html)
        self.assertIn("maxStepDurationMs", html)
        self.assertIn("maxToolExecutionMs", html)
        self.assertIn("elapsedMaxMs", html)
        self.assertIn("timeGradientStyle", html)
        self.assertIn("time-gradient", html)
        self.assertIn("--time-pct", html)
        self.assertIn("slowest step", html)
        self.assertIn("slowest tool", html)
        self.assertIn('timeTitle("elapsed", meta?.elapsed_ms, elapsedRatio, "trajectory")', html)
        self.assertIn("function fmtRailTokens", html)
        self.assertIn("fmtRailTokens(tokenInfo.tokens)", html)
        self.assertIn("fmtNum(tokenInfo.tokens)", html)
        self.assertIn("Tool Calls", html)
        self.assertIn("Observations", html)
        self.assertEqual(
            report["trajectory_meta"][0]["steps"][0]["tool_calls"][0][
                "execution_duration_ms"
            ],
            101,
        )

    def test_html_inlines_css_and_js_package_assets(self) -> None:
        report = {
            "schema_version": 17,
            "includes": ["core"],
            "trajectory": [
                {
                    "trajectory_id": "trial:assets",
                    "session_id": "assets",
                    "agent": {"name": "custom"},
                    "steps": [],
                    "final_metrics": {},
                }
            ],
            "trajectory_meta": [
                {
                    "trial_key": "trial:assets",
                    "status": "passed",
                    "steps": [],
                    "warnings": [],
                }
            ],
        }
        css = load_asset_text("report.css")
        js = load_asset_text("report.js")
        html = render_html(report)

        self.assertIn(".time-gradient", css)
        self.assertIn("function renderTrace()", js)
        self.assertIn("<style>\n:root", html)
        self.assertIn("function renderTrace()", html)
        self.assertNotIn("__CSS__", html)
        self.assertNotIn("__JS__", html)

    def test_html_timing_gradients_ignore_missing_values_without_mutating_report(self) -> None:
        report = {
            "schema_version": 17,
            "includes": ["core"],
            "trajectory": [
                {
                    "trajectory_id": "trial:missing-time",
                    "session_id": "missing-time",
                    "agent": {"name": "custom"},
                    "steps": [
                        {
                            "step_id": 1,
                            "source": "agent",
                            "message": "no timing",
                            "tool_calls": [
                                {
                                    "tool_call_id": "call-1",
                                    "function_name": "exec_command",
                                    "arguments": {"cmd": "true"},
                                }
                            ],
                        }
                    ],
                    "final_metrics": {},
                }
            ],
            "trajectory_meta": [
                {
                    "trial_key": "trial:missing-time",
                    "status": "passed",
                    "steps": [
                        {
                            "step_id": 1,
                            "duration_ms": 0,
                            "elapsed_ms": None,
                            "tool_calls": [
                                {
                                    "tool_call_id": "call-1",
                                    "title": "exec_command",
                                    "execution_duration_ms": None,
                                }
                            ],
                        }
                    ],
                    "warnings": [],
                }
            ],
        }
        before = json.loads(json.dumps(report))
        html = render_html(report)
        payload = script_json(html, "peval-py-data")

        self.assertEqual(report, before)
        self.assertEqual(payload, before)
        self.assertIn("function positiveMetric", html)
        self.assertIn("if (!positiveMetric(value) || !positiveMetric(max)) return null", html)

    def test_html_estimates_missing_step_token_chips_without_mutating_report(self) -> None:
        report = {
            "schema_version": 17,
            "includes": ["core"],
            "trajectory": [
                {
                    "trajectory_id": "trial:estimate",
                    "session_id": "estimate-session",
                    "agent": {"name": "custom", "model_name": "unknown-model"},
                    "steps": [
                        {
                            "step_id": 1,
                            "source": "agent",
                            "message": "abcdefgh",
                            "tool_calls": [
                                {
                                    "tool_call_id": "call-1",
                                    "function_name": "read",
                                    "arguments": {"path": "README.md"},
                                }
                            ],
                        }
                    ],
                    "final_metrics": {"usage": {"total_tokens": 100}},
                }
            ],
            "trajectory_meta": [
                {
                    "trial_key": "trial:estimate",
                    "status": "passed",
                    "steps": [{"step_id": 1, "duration_ms": None}],
                    "warnings": [],
                }
            ],
        }
        before = json.loads(json.dumps(report))
        with patch("peval_py.html.import_module", side_effect=ImportError("missing")):
            html = render_html(report)

        self.assertEqual(report, before)
        estimates = script_json(html, "peval-py-token-estimates")
        self.assertIn("trial:estimate", estimates)
        estimate = estimates["trial:estimate"]["1"]
        self.assertEqual(estimate["method"], "byte_length_div_4")
        self.assertEqual(estimate["source"], "visible_step_text")
        self.assertTrue(estimate["estimated"])
        self.assertGreater(estimate["tokens"], 0)
        self.assertIn("renderStepRail(step, sm, meta?.trial_key, timingStats)", html)
        self.assertIn("stepTokenInfo(step, trialKey)", html)
        self.assertIn("stepTokenEstimate(trialKey, step.step_id)", html)
        self.assertIn("estimated tokens", html)
        self.assertIn("from visible step text", html)
        self.assertIn("≈", html)
        self.assertNotIn("estimated", script_json(html, "peval-py-data")["trajectory"][0]["steps"][0])

    def test_html_preserves_exact_step_tokens_without_estimate(self) -> None:
        report = {
            "schema_version": 17,
            "includes": ["core"],
            "trajectory": [
                {
                    "trajectory_id": "trial:exact",
                    "session_id": "exact-session",
                    "agent": {"name": "custom", "model_name": "unknown-model"},
                    "steps": [
                        {
                            "step_id": 1,
                            "source": "agent",
                            "message": "abcdefgh",
                            "metrics": {"prompt_tokens": 3, "completion_tokens": 4},
                        }
                    ],
                    "final_metrics": {"total_prompt_tokens": 3, "total_completion_tokens": 4},
                }
            ],
            "trajectory_meta": [
                {
                    "trial_key": "trial:exact",
                    "status": "passed",
                    "steps": [{"step_id": 1, "duration_ms": None}],
                    "warnings": [],
                }
            ],
        }
        with patch("peval_py.html.import_module", side_effect=ImportError("missing")):
            html = render_html(report)

        self.assertEqual(script_json(html, "peval-py-token-estimates"), {})
        payload = script_json(html, "peval-py-data")
        self.assertEqual(payload["trajectory"][0]["steps"][0]["metrics"]["prompt_tokens"], 3)

    def test_html_estimated_tokens_can_use_optional_tiktoken(self) -> None:
        class FakeEncoding:
            name = "fake-model-encoding"

            def encode(self, text: str):
                return list(range(7))

        class FakeTiktoken:
            def encoding_for_model(self, model: str):
                self.model = model
                return FakeEncoding()

            def get_encoding(self, name: str):
                raise AssertionError("model encoding should be used")

        report = {
            "schema_version": 17,
            "includes": ["core"],
            "trajectory": [
                {
                    "trajectory_id": "trial:tiktoken",
                    "session_id": "tiktoken-session",
                    "agent": {"name": "custom", "model_name": "fake-model"},
                    "steps": [
                        {
                            "step_id": 1,
                            "source": "agent",
                            "message": "model counted text",
                        }
                    ],
                    "final_metrics": {},
                }
            ],
            "trajectory_meta": [
                {
                    "trial_key": "trial:tiktoken",
                    "status": "passed",
                    "steps": [{"step_id": 1, "duration_ms": None}],
                    "warnings": [],
                }
            ],
        }
        fake = FakeTiktoken()
        with patch("peval_py.html.import_module", return_value=fake):
            html = render_html(report)

        self.assertEqual(fake.model, "fake-model")
        estimate = script_json(html, "peval-py-token-estimates")["trial:tiktoken"]["1"]
        self.assertEqual(estimate["tokens"], 7)
        self.assertEqual(estimate["method"], "tiktoken:fake-model-encoding")

    def test_redaction_preserves_numeric_usage_metrics_and_masks_secrets(self) -> None:
        records = [
            MessageRecord(
                message={
                    "role": "assistant",
                    "content": "Use token=abc123 only in memory.",
                    "tool_calls": [
                        {
                            "tool_call_id": "call-secret",
                            "function_name": "exec_command",
                            "arguments": {
                                "api_key": "sk-secret",
                                "token_count": 7,
                                "token": "abc123",
                            },
                        }
                    ],
                    "timestamp_ms": 1000,
                },
                usage={"input_tokens": 11, "output_tokens": 13},
                accounting={"billable_input_tokens": 11, "billable_output_tokens": 13},
            )
        ]
        config = ToolConfig(adapter="psychevo", trajectory_id="trial:redaction")
        result = convert_records(records, config)
        step = result.trajectory["steps"][0]
        args = step["tool_calls"][0]["arguments"]
        self.assertEqual(step["message"], "Use token=<redacted> only in memory.")
        self.assertEqual(args["api_key"], "<redacted>")
        self.assertEqual(args["token"], "<redacted>")
        self.assertEqual(args["token_count"], 7)

        report = build_report(result, config, "inline")
        metrics = report["trajectory"][0]["final_metrics"]
        self.assertEqual(metrics["usage"]["input_tokens"], 11)
        self.assertEqual(metrics["usage"]["output_tokens"], 13)
        self.assertEqual(metrics["accounting"]["billable_input_tokens"], 11)
        self.assertEqual(metrics["accounting"]["billable_output_tokens"], 13)

    def test_cli_view_export_alias_smoke_and_legacy_commands_are_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            report_out = Path(tmp) / "report.json"
            export_out = Path(tmp) / "trajectory.json"
            command = shutil.which("peval-py") or "peval-py"
            result = subprocess.run(
                [
                    command,
                    "view",
                    "tr",
                    "-a",
                    "opencode",
                    "-p",
                    str(FIXTURES / "common_session.jsonl"),
                    "-o",
                    str(report_out),
                ],
                check=True,
                text=True,
                capture_output=True,
            )
            self.assertEqual(result.stderr, "")
            payload = json.loads(report_out.read_text(encoding="utf-8"))
            self.assertEqual(payload["trajectory"][0]["agent"]["name"], "opencode")
            subprocess.run(
                [sys.executable, "-m", "json.tool", str(report_out)],
                check=True,
                text=True,
                capture_output=True,
            )

            result = subprocess.run(
                [
                    command,
                    "export",
                    "tr",
                    "-a",
                    "opencode",
                    "-p",
                    str(FIXTURES / "common_session.jsonl"),
                    "-o",
                    str(export_out),
                ],
                check=True,
                text=True,
                capture_output=True,
            )
            self.assertEqual(result.stderr, "")
            payload = json.loads(export_out.read_text(encoding="utf-8"))
            self.assertEqual(payload["agent"]["name"], "opencode")
            subprocess.run(
                [sys.executable, "-m", "json.tool", str(export_out)],
                check=True,
                text=True,
                capture_output=True,
            )

            for legacy in ["report", "convert"]:
                with self.subTest(legacy=legacy):
                    result = subprocess.run(
                        [command, legacy, "--help"],
                        check=False,
                        text=True,
                        capture_output=True,
                    )
                    self.assertNotEqual(result.returncode, 0)

            for verb in ["view", "export"]:
                with self.subTest(verb=verb):
                    result = subprocess.run(
                        [command, verb, "trajectory", "--help"],
                        check=False,
                        text=True,
                        capture_output=True,
                    )
                    self.assertEqual(result.returncode, 0)

            result = subprocess.run(
                [command, "view", "tr", "--help"],
                check=True,
                text=True,
                capture_output=True,
            )
            self.assertIn("-p", result.stdout)
            self.assertIn("--path", result.stdout)
            self.assertIn("-n", result.stdout)
            self.assertIn("--note", result.stdout)

            default_report = Path(tmp) / "report-opencode-common_session.html"
            result = subprocess.run(
                [
                    command,
                    "view",
                    "tr",
                    "-a",
                    "opencode",
                    "-p",
                    str(FIXTURES / "common_session.jsonl"),
                    "-o",
                ],
                cwd=tmp,
                check=True,
                text=True,
                capture_output=True,
            )
            self.assertEqual(result.stderr, "")
            self.assertIn("<!doctype html>", default_report.read_text(encoding="utf-8"))

            zh_config = Path(tmp) / "zh.toml"
            zh_config.write_text(
                "[defaults]\nlocale = \"zh-CN\"\n",
                encoding="utf-8",
            )
            zh_report = Path(tmp) / "zh-report.html"
            result = subprocess.run(
                [
                    command,
                    "view",
                    "tr",
                    "-c",
                    str(zh_config),
                    "-a",
                    "opencode",
                    "-p",
                    str(FIXTURES / "common_session.jsonl"),
                    "-f",
                    "html",
                    "-o",
                    str(zh_report),
                ],
                check=True,
                text=True,
                capture_output=True,
            )
            self.assertEqual(result.stderr, "")
            html = zh_report.read_text(encoding="utf-8")
            self.assertIn('<html lang="zh-CN">', html)
            self.assertIn("<h1>Agent 轨迹报告</h1>", html)
            self.assertIn('"run": "Run"', html)
            self.assertIn('"session": "Session"', html)
            self.assertIn('"evidence": "Evidence"', html)
            self.assertIn('"turns": "Turns"', html)
            self.assertIn('"tool_calls": "Tool Calls"', html)
            self.assertIn('"tool_success_total": "tool success / total"', html)
            self.assertNotIn('"run": "运行"', html)
            self.assertNotIn('"turns": "轮次"', html)
            self.assertNotIn('"tool_calls": "工具调用"', html)
            self.assertIn("<h3>Steps (${count})</h3>", html)

            opencode_db = Path(tmp) / "opencode.db"
            create_opencode_db(opencode_db)
            opencode_db_report = Path(tmp) / "opencode-db-report.json"
            result = subprocess.run(
                [
                    command,
                    "view",
                    "tr",
                    "-a",
                    "opencode",
                    "-d",
                    str(opencode_db),
                    "-f",
                    "json",
                    "-o",
                    str(opencode_db_report),
                ],
                check=True,
                text=True,
                capture_output=True,
            )
            self.assertEqual(result.stderr, "")
            payload = json.loads(opencode_db_report.read_text(encoding="utf-8"))
            self.assertEqual(payload["trajectory"][0]["session_id"], "ses-latest")
            self.assertEqual(payload["trajectory"][0]["steps"][0]["message"], "latest prompt")

            hermes_db = Path(tmp) / "state.db"
            create_hermes_db(hermes_db)
            hermes_db_report = Path(tmp) / "hermes-db-report.json"
            result = subprocess.run(
                [
                    command,
                    "view",
                    "tr",
                    "-a",
                    "hermes",
                    "-d",
                    str(hermes_db),
                    "-f",
                    "json",
                    "-o",
                    str(hermes_db_report),
                ],
                check=True,
                text=True,
                capture_output=True,
            )
            self.assertEqual(result.stderr, "")
            payload = json.loads(hermes_db_report.read_text(encoding="utf-8"))
            self.assertEqual(payload["trajectory"][0]["session_id"], "hermes-latest")
            self.assertEqual(payload["trajectory"][0]["steps"][0]["source"], "system")
            self.assertEqual(
                payload["trajectory"][0]["steps"][0]["message"],
                "Hermes system prompt",
            )

            psychevo_db = Path(tmp) / "psychevo-state.db"
            create_messages_db(psychevo_db)
            psychevo_db_report = Path(tmp) / "psychevo-db-report.json"
            result = subprocess.run(
                [
                    command,
                    "view",
                    "tr",
                    "-a",
                    "psychevo",
                    "-d",
                    str(psychevo_db),
                    "-f",
                    "json",
                    "-o",
                    str(psychevo_db_report),
                ],
                check=True,
                text=True,
                capture_output=True,
            )
            self.assertEqual(result.stderr, "")
            payload = json.loads(psychevo_db_report.read_text(encoding="utf-8"))
            self.assertEqual(payload["trajectory"][0]["session_id"], "db-b")
            self.assertEqual(payload["trajectory"][0]["steps"][0]["message"], "hello b")

            default_report_json = Path(tmp) / "report-opencode-common_session.json"
            result = subprocess.run(
                [
                    command,
                    "view",
                    "tr",
                    "-a",
                    "opencode",
                    "-p",
                    str(FIXTURES / "common_session.jsonl"),
                    "-f",
                    "json",
                    "-o",
                ],
                cwd=tmp,
                check=True,
                text=True,
                capture_output=True,
            )
            self.assertEqual(result.stderr, "")
            subprocess.run(
                [sys.executable, "-m", "json.tool", str(default_report_json)],
                check=True,
                text=True,
                capture_output=True,
            )

            default_export = Path(tmp) / "trajectory-opencode-session.json"
            result = subprocess.run(
                [
                    command,
                    "export",
                    "tr",
                    "-a",
                    "opencode",
                    "-p",
                    str(FIXTURES / "common_session.jsonl"),
                    "-o",
                ],
                cwd=tmp,
                check=True,
                text=True,
                capture_output=True,
            )
            self.assertEqual(result.stderr, "")
            subprocess.run(
                [sys.executable, "-m", "json.tool", str(default_export)],
                check=True,
                text=True,
                capture_output=True,
            )


if __name__ == "__main__":
    unittest.main()
