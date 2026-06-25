from __future__ import annotations

from peval_py_test_support import *


ATIF_TRAJECTORY_KEYS = {
    "schema_version",
    "session_id",
    "trajectory_id",
    "agent",
    "steps",
    "notes",
    "final_metrics",
    "continued_trajectory_ref",
    "extra",
    "subagent_trajectories",
}
ATIF_AGENT_KEYS = {"name", "version", "model_name", "tool_definitions", "extra"}
ATIF_STEP_KEYS = {
    "step_id",
    "timestamp",
    "source",
    "model_name",
    "reasoning_effort",
    "message",
    "reasoning_content",
    "tool_calls",
    "observation",
    "metrics",
    "is_copied_context",
    "llm_call_count",
    "extra",
}
ATIF_TOOL_CALL_KEYS = {"tool_call_id", "function_name", "arguments", "extra"}
ATIF_OBSERVATION_KEYS = {"results"}
ATIF_OBSERVATION_RESULT_KEYS = {
    "source_call_id",
    "content",
    "subagent_trajectory_ref",
    "extra",
}
ATIF_METRICS_KEYS = {
    "prompt_tokens",
    "completion_tokens",
    "cached_tokens",
    "cost_usd",
    "prompt_token_ids",
    "completion_token_ids",
    "logprobs",
    "extra",
}
ATIF_FINAL_METRICS_KEYS = {
    "total_prompt_tokens",
    "total_completion_tokens",
    "total_cached_tokens",
    "total_cost_usd",
    "total_steps",
    "extra",
}


def final_extra(trajectory):
    extra = trajectory["final_metrics"].get("extra")
    if not isinstance(extra, dict):
        raise AssertionError("final_metrics.extra missing")
    return extra


class PevalPySourceConversionTests(unittest.TestCase):
    def assertAtifCompatibleTrajectory(self, trajectory) -> None:
        self.assertTrue(set(trajectory).issubset(ATIF_TRAJECTORY_KEYS), trajectory)
        self.assertTrue(set(trajectory.get("agent", {})).issubset(ATIF_AGENT_KEYS))
        final_metrics = trajectory.get("final_metrics", {})
        self.assertTrue(set(final_metrics).issubset(ATIF_FINAL_METRICS_KEYS))
        for key in ("total_turns", "total_tool_calls", "total_tool_errors", "usage", "accounting"):
            self.assertNotIn(key, final_metrics)
        for step in trajectory.get("steps", []):
            self.assertTrue(set(step).issubset(ATIF_STEP_KEYS), step)
            metrics = step.get("metrics", {})
            self.assertTrue(set(metrics).issubset(ATIF_METRICS_KEYS), metrics)
            self.assertNotIn("usage", metrics)
            self.assertNotIn("accounting", metrics)
            for call in step.get("tool_calls", []) or []:
                self.assertTrue(set(call).issubset(ATIF_TOOL_CALL_KEYS), call)
            observation = step.get("observation")
            if observation is not None:
                self.assertTrue(set(observation).issubset(ATIF_OBSERVATION_KEYS), observation)
                for result in observation.get("results", []) or []:
                    self.assertTrue(set(result).issubset(ATIF_OBSERVATION_RESULT_KEYS), result)

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
            self.assertEqual(final_extra(latest.trajectory)["usage"]["input_tokens"], 5)
            self.assertEqual(final_extra(latest.trajectory)["usage"]["output_tokens"], 7)

            explicit = convert_db(str(db_path), "db-a", config)
            self.assertEqual(explicit.trajectory["session_id"], "db-a")
            self.assertEqual(explicit.trajectory["steps"][0]["message"], "hello a")


    def test_psychevo_db_adapter_prefers_trace_generation_timing(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            db_path = Path(tmp) / "state.db"
            create_messages_db(db_path)
            trace_dir = Path(tmp) / "sessions" / "db-b"
            trace_dir.mkdir(parents=True)
            (trace_dir / "events.jsonl").write_text(
                "\n".join(
                    [
                        json.dumps(
                            {
                                "schema_version": 2,
                                "seq": 1,
                                "session_id": "db-b",
                                "kind": "generation_end",
                                "timestamp_ms": 440,
                                "payload": {"elapsed_ms": 42},
                            }
                        )
                    ]
                )
                + "\n",
                encoding="utf-8",
            )

            result = convert_db(str(db_path), "db-b", ToolConfig(adapter="psychevo"))
            self.assertEqual(result.steps_meta[1].duration_ms, 42)
            self.assertEqual(result.warnings, [])


    def test_psychevo_db_adapter_prefers_trace_wall_timing_for_model_and_tools(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            db_path = Path(tmp) / "state.db"
            create_messages_db(db_path)
            conn = sqlite3.connect(db_path)
            conn.execute("DELETE FROM messages WHERE session_id = ?", ("db-b",))
            assistant = {
                "role": "assistant",
                "content": [
                    {
                        "type": "tool_call",
                        "id": "call-trace-db",
                        "name": "exec_command",
                        "arguments": {"cmd": "date"},
                    }
                ],
                "timestamp_ms": 1_200,
                "model": "db-model-b",
            }
            tool_result = {
                "role": "tool_result",
                "tool_call_id": "call-trace-db",
                "tool_name": "exec_command",
                "content": "ok",
                "timestamp_ms": 1_900,
            }
            rows = [
                ("db-b", 1, {"role": "user", "content": "run", "timestamp_ms": 1_000}),
                ("db-b", 2, assistant),
                ("db-b", 3, tool_result),
            ]
            for session_id, seq, message in rows:
                conn.execute(
                    """
                    INSERT INTO messages
                    (session_id, session_seq, message_json)
                    VALUES (?, ?, ?)
                    """,
                    (session_id, seq, json.dumps(message)),
                )
            conn.commit()
            conn.close()

            trace_dir = Path(tmp) / "sessions" / "db-b"
            trace_dir.mkdir(parents=True)
            events = [
                {
                    "schema_version": 2,
                    "seq": 1,
                    "session_id": "db-b",
                    "kind": "generation_start",
                    "timestamp_ms": 1_100,
                    "correlation": {"generation_id": "gen-db"},
                    "payload": {"started_at_ms": 1_100},
                },
                {
                    "schema_version": 2,
                    "seq": 2,
                    "session_id": "db-b",
                    "kind": "generation_end",
                    "timestamp_ms": 1_300,
                    "correlation": {"generation_id": "gen-db"},
                    "payload": {"elapsed_ms": 200},
                },
                {
                    "schema_version": 2,
                    "seq": 3,
                    "session_id": "db-b",
                    "kind": "tool_execution_start",
                    "timestamp_ms": 1_500,
                    "correlation": {
                        "tool_call_id": "call-trace-db",
                        "tool_name": "exec_command",
                    },
                    "payload": {"started_at_ms": 1_500},
                },
                {
                    "schema_version": 2,
                    "seq": 4,
                    "session_id": "db-b",
                    "kind": "tool_execution_end",
                    "timestamp_ms": 1_700,
                    "correlation": {
                        "tool_call_id": "call-trace-db",
                        "tool_name": "exec_command",
                    },
                    "payload": {"elapsed_ms": 200},
                },
            ]
            (trace_dir / "events.jsonl").write_text(
                "\n".join(json.dumps(event) for event in events) + "\n",
                encoding="utf-8",
            )

            config = ToolConfig(adapter="psychevo", trajectory_id="trial:trace-db")
            result = convert_db(str(db_path), "db-b", config)
            model_step = result.steps_meta[1]
            tool_meta = model_step.tool_calls[0]
            self.assertEqual(model_step.timestamp_ms, 1_100)
            self.assertEqual(model_step.duration_ms, 200)
            self.assertEqual(tool_meta.timestamp_ms, 1_500)
            self.assertEqual(tool_meta.execution_duration_ms, 200)
            self.assertEqual(tool_meta.execution_duration_source, "runtime_trace")

            report = build_report(result, config, "inline")
            report_step = report["trajectory_meta"][0]["steps"][1]
            self.assertEqual(report_step["timestamp_ms"], 1_100)
            self.assertEqual(report_step["duration_ms"], 200)
            self.assertEqual(report_step["tool_calls"][0]["timestamp_ms"], 1_500)
            self.assertEqual(report["trajectory_meta"][0]["duration_ms"], 400)
            auto_latency = report["annotations"]["analysis"][0]["analysis_metrics"][
                "auto"
            ]["latency"]
            self.assertEqual(auto_latency["model_duration_ms"]["max"], 200)
            self.assertEqual(auto_latency["tool_execution_duration_ms"]["max"], 200)
            self.assertEqual(result.warnings, [])


    def test_psychevo_trace_jsonl_conversion_uses_runtime_tool_timing(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            trace_path = Path(tmp) / "events.jsonl"
            events = [
                {
                    "schema_version": 1,
                    "seq": 1,
                    "session_id": "trace-session",
                    "kind": "generation_start",
                    "timestamp_ms": 90,
                    "correlation": {"generation_id": "gen-trace"},
                    "payload": {"started_at_ms": 90},
                },
                {
                    "schema_version": 1,
                    "seq": 2,
                    "session_id": "trace-session",
                    "kind": "generation_end",
                    "timestamp_ms": 110,
                    "correlation": {"generation_id": "gen-trace"},
                    "payload": {"elapsed_ms": 20},
                },
                {
                    "schema_version": 1,
                    "seq": 3,
                    "session_id": "trace-session",
                    "kind": "message_end",
                    "timestamp_ms": 110,
                    "payload": {
                        "message": {
                            "role": "assistant",
                            "message": "",
                            "timestamp_ms": 100,
                            "tool_calls": [
                                {
                                    "tool_call_id": "call-trace",
                                    "function_name": "read",
                                    "arguments": {"path": "README.md"},
                                }
                            ],
                        }
                    },
                },
                {
                    "schema_version": 1,
                    "seq": 4,
                    "session_id": "trace-session",
                    "kind": "tool_execution_start",
                    "timestamp_ms": 120,
                    "correlation": {
                        "tool_call_id": "call-trace",
                        "tool_name": "read",
                    },
                    "payload": {"started_at_ms": 120},
                },
                {
                    "schema_version": 1,
                    "seq": 5,
                    "session_id": "trace-session",
                    "kind": "tool_execution_end",
                    "timestamp_ms": 151,
                    "correlation": {
                        "tool_call_id": "call-trace",
                        "tool_name": "read",
                    },
                    "payload": {"elapsed_ms": 31},
                },
                {
                    "schema_version": 1,
                    "seq": 6,
                    "session_id": "trace-session",
                    "kind": "message_end",
                    "timestamp_ms": 151,
                    "payload": {
                        "message": {
                            "role": "tool_result",
                            "tool_call_id": "call-trace",
                            "tool_name": "read",
                            "content": "ok",
                            "timestamp_ms": 151,
                        }
                    },
                },
            ]
            trace_path.write_text(
                "\n".join(json.dumps(event) for event in events) + "\n",
                encoding="utf-8",
            )

            result = convert_path(str(trace_path), ToolConfig(adapter="psychevo"))
            self.assertEqual(result.trajectory["session_id"], "trace-session")
            self.assertEqual(result.steps_meta[0].timestamp_ms, 90)
            self.assertEqual(result.steps_meta[0].duration_ms, 20)
            tool_meta = result.steps_meta[0].tool_calls[0]
            self.assertEqual(tool_meta.timestamp_ms, 120)
            self.assertEqual(tool_meta.execution_duration_ms, 31)
            self.assertEqual(tool_meta.execution_duration_source, "runtime_trace")
            self.assertEqual(result.total_events, 6)


    def test_psychevo_compact_v2_trace_direct_conversion_warns(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            trace_path = Path(tmp) / "events.jsonl"
            events = [
                {
                    "schema_version": 2,
                    "seq": 1,
                    "session_id": "compact-session",
                    "kind": "generation_end",
                    "timestamp_ms": 110,
                    "payload": {"elapsed_ms": 10},
                },
                {
                    "schema_version": 2,
                    "seq": 2,
                    "session_id": "compact-session",
                    "kind": "message_end",
                    "timestamp_ms": 120,
                    "payload": {
                        "role": "assistant",
                        "summary": {"text_chars": 4},
                    },
                },
                {
                    "schema_version": 2,
                    "seq": 3,
                    "session_id": "compact-session",
                    "kind": "run_summary",
                    "timestamp_ms": 130,
                    "payload": {"event_counts": {"message_end": 1}},
                },
            ]
            trace_path.write_text(
                "\n".join(json.dumps(event) for event in events) + "\n",
                encoding="utf-8",
            )

            result = convert_path(str(trace_path), ToolConfig(adapter="psychevo"))

            self.assertEqual(result.trajectory["steps"], [])
            self.assertEqual(result.total_events, 3)
            self.assertTrue(
                any("compact trace v2" in warning for warning in result.warnings)
            )


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
        self.assertEqual(final_extra(trajectory)["total_tool_calls"], 1)
        self.assertEqual(final_extra(trajectory)["total_tool_errors"], 0)
        self.assertEqual(trajectory["final_metrics"]["total_steps"], 3)
        self.assertEqual(final_extra(trajectory)["usage"]["input_tokens"], 8)
        self.assertEqual(final_extra(trajectory)["usage"]["output_tokens"], 10)
        self.assertEqual(final_extra(trajectory)["usage"]["cache_read_tokens"], 1)
        self.assertAtifCompatibleTrajectory(trajectory)

        tool_meta = result.steps_meta[1].tool_calls[0]
        self.assertEqual(tool_meta.status, "completed")
        self.assertEqual(tool_meta.execution_duration_ms, 321)
        self.assertEqual(tool_meta.execution_duration_source, "message_metadata")


    def test_opencode_and_hermes_use_common_jsonl_adapter(self) -> None:
        records = read_jsonl(str(FIXTURES / "common_session.jsonl"))
        for adapter in ["opencode", "hermes"]:
            with self.subTest(adapter=adapter):
                result = convert_records(records, ToolConfig(adapter=adapter))
                self.assertAtifCompatibleTrajectory(result.trajectory)
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


    def test_path_input_rejects_non_atif_metric_fields(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            source = convert_records(
                read_jsonl(str(FIXTURES / "common_session.jsonl")),
                ToolConfig(adapter="opencode"),
            )
            source.trajectory["final_metrics"]["usage"] = {"total_tokens": 1}
            atif_path = Path(tmp) / "trajectory.json"
            atif_path.write_text(
                json.dumps(source.trajectory, ensure_ascii=False),
                encoding="utf-8",
            )

            with self.assertRaisesRegex(ValueError, "final_metrics.*usage"):
                convert_path(str(atif_path), ToolConfig(adapter="opencode"))


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
            tool_meta = latest.steps_meta[1].tool_calls[0]
            self.assertEqual(tool_meta.timestamp_ms, 2200)
            self.assertEqual(tool_meta.execution_duration_ms, 100)
            self.assertEqual(
                tool_meta.execution_duration_source,
                "opencode_part_timestamps",
            )
            report = build_report(latest, config, "inline")
            step_meta = report["trajectory_meta"][0]["steps"][1]
            self.assertIsNone(step_meta["duration_ms"])
            self.assertEqual(step_meta["tool_calls"][0]["timestamp_ms"], 2200)
            self.assertEqual(report["trajectory_meta"][0]["duration_ms"], 100)
            self.assertEqual(final_extra(latest.trajectory)["total_tool_calls"], 1)
            self.assertEqual(final_extra(latest.trajectory)["usage"]["input_tokens"], 2)

            old = convert_db(str(db_path), "ses-old", config)
            self.assertEqual(old.trajectory["session_id"], "ses-old")
            self.assertEqual(old.trajectory["steps"][0]["message"], "old prompt")


    def test_opencode_db_adapter_prefers_event_fused_timing(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            db_path = Path(tmp) / "opencode.db"
            create_opencode_event_timing_db(db_path)
            config = ToolConfig(adapter="opencode")

            latest = convert_db(str(db_path), None, config)

            self.assertEqual(latest.trajectory["session_id"], "ses-latest")
            agent_meta = latest.steps_meta[1]
            self.assertEqual(agent_meta.duration_ms, 100)
            self.assertEqual(
                agent_meta.duration_source,
                "opencode_model_boundary_estimate",
            )
            tool_meta = agent_meta.tool_calls[0]
            self.assertEqual(tool_meta.timestamp_ms, 2_500)
            self.assertEqual(tool_meta.execution_duration_ms, 48_000)
            self.assertEqual(
                tool_meta.execution_duration_source,
                "opencode_event_tool_timestamps",
            )
            report = build_report(latest, config, "inline")
            step_meta = report["trajectory_meta"][0]["steps"][1]
            self.assertEqual(step_meta["duration_ms"], 100)
            self.assertEqual(
                step_meta["duration_source"],
                "opencode_model_boundary_estimate",
            )
            self.assertEqual(
                step_meta["tool_calls"][0]["execution_duration_ms"],
                48_000,
            )
            self.assertEqual(
                step_meta["tool_calls"][0]["execution_duration_source"],
                "opencode_event_tool_timestamps",
            )
            self.assertEqual(report["trajectory_meta"][0]["duration_ms"], 48_100)
            auto_latency = report["annotations"]["analysis"][0]["analysis_metrics"][
                "auto"
            ]["latency"]
            self.assertNotIn("model_duration_ms", auto_latency)


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
            self.assertEqual(latest.timestamp_semantics, "order_only")
            report = build_report(latest, config, "inline")
            meta = report["trajectory_meta"][0]
            self.assertEqual(meta["timestamp_semantics"], "order_only")
            self.assertEqual(meta["wall_duration_ms"], 30_000)
            self.assertIsNone(meta["duration_ms"])
            step_meta = report["trajectory_meta"][0]["steps"][2]
            self.assertIsNone(step_meta["duration_ms"])
            self.assertEqual(step_meta["tool_calls"][0]["timestamp_ms"], 220_000)
            self.assertNotIn("execution_duration_ms", step_meta["tool_calls"][0])
            self.assertEqual(final_extra(latest.trajectory)["total_tool_calls"], 1)
            self.assertEqual(final_extra(latest.trajectory)["usage"]["input_tokens"], 11)
            self.assertEqual(final_extra(latest.trajectory)["usage"]["output_tokens"], 13)
            self.assertEqual(
                final_extra(latest.trajectory)["accounting"]["pricing_source"],
                "test-prices",
            )
            self.assertEqual(latest.warnings, [])

            old = convert_db(str(db_path), "hermes-old", config)
            self.assertEqual(old.trajectory["session_id"], "hermes-old")
            self.assertEqual(old.trajectory["steps"][0]["message"], "old prompt")
            self.assertEqual(len(old.trajectory["steps"]), 1)
            self.assertEqual(final_extra(old.trajectory)["usage"]["input_tokens"], 1)


    def test_hermes_db_adapter_fuses_current_agent_log_timing(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            db_path = create_hermes_log_timing_home(Path(tmp) / ".hermes")
            config = ToolConfig(adapter="hermes")

            result = convert_db(str(db_path), None, config)
            report = build_report(result, config, "inline")
            meta = report["trajectory_meta"][0]
            first_agent = result.steps_meta[1]
            second_agent = result.steps_meta[2]
            final_agent = result.steps_meta[-1]

            self.assertEqual(result.trajectory["session_id"], "hermes-log")
            self.assertEqual(first_agent.duration_ms, 5_700)
            self.assertEqual(first_agent.duration_source, "hermes_agent_log")
            self.assertEqual(first_agent.tool_calls[0].execution_duration_ms, 53_890)
            self.assertEqual(
                first_agent.tool_calls[0].execution_duration_source,
                "hermes_agent_log",
            )
            self.assertEqual(second_agent.duration_ms, 8_500)
            self.assertEqual(second_agent.tool_calls[1].execution_duration_ms, 80)
            self.assertTrue(second_agent.tool_error)
            self.assertEqual(final_agent.duration_ms, 6_500)
            self.assertEqual(meta["duration_ms"], 118_730)
            self.assertEqual(meta["wall_duration_ms"], 121_820)
            self.assertEqual(meta["steps"][1]["duration_ms"], 5_700)
            self.assertEqual(
                meta["steps"][1]["tool_calls"][0]["execution_duration_ms"],
                53_890,
            )
            self.assertEqual(
                meta["steps"][1]["tool_calls"][0]["execution_duration_source"],
                "hermes_agent_log",
            )
            self.assertEqual(result.warnings, [])


    def test_hermes_db_agent_log_mismatch_keeps_timing_unknown(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            hermes_home = Path(tmp) / ".hermes"
            db_path = create_hermes_log_timing_home(hermes_home)
            log_path = hermes_home / "logs" / "agent.log"
            log_path.write_text(
                hermes_agent_log_fixture("hermes-log")
                .replace("API call #7", "API call #ignored")
                .replace("tool write_file completed", "tool read_file completed"),
                encoding="utf-8",
            )
            config = ToolConfig(adapter="hermes")

            result = convert_db(str(db_path), None, config)
            report = build_report(result, config, "inline")
            meta = report["trajectory_meta"][0]

            self.assertIsNone(result.steps_meta[1].duration_ms)
            self.assertIsNone(result.steps_meta[1].tool_calls[0].execution_duration_ms)
            self.assertIsNone(meta["duration_ms"])
            self.assertEqual(result.warnings, [])


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

        metadata_timing = convert_records(
            [
                MessageRecord(
                    message={
                        "role": "assistant",
                        "tool_calls": [
                            {
                                "tool_call_id": "call-metadata",
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
                        "tool_call_id": "call-metadata",
                        "tool_name": "read",
                        "content": "ok",
                        "timestamp_ms": 1500,
                    },
                    metadata={"elapsed_ms": 125},
                ),
            ],
            ToolConfig(adapter="psychevo"),
        )
        metadata_tool = metadata_timing.steps_meta[0].tool_calls[0]
        self.assertEqual(metadata_tool.timestamp_ms, 1375)
        self.assertEqual(metadata_tool.execution_duration_ms, 125)
        self.assertEqual(metadata_tool.execution_duration_source, "message_metadata")
        metadata_report = build_report(
            metadata_timing,
            ToolConfig(adapter="psychevo", trajectory_id="trial:metadata-timing"),
            "inline",
        )
        metadata_step = metadata_report["trajectory_meta"][0]["steps"][0]
        self.assertIsNone(metadata_step["duration_ms"])
        self.assertEqual(metadata_step["tool_calls"][0]["timestamp_ms"], 1375)
        self.assertEqual(metadata_report["trajectory_meta"][0]["duration_ms"], 125)

        long_fallback = convert_records(
            [
                MessageRecord(
                    message={
                        "role": "assistant",
                        "tool_calls": [
                            {
                                "tool_call_id": "call-long",
                                "function_name": "exec_command",
                                "arguments": {"cmd": "sleep"},
                            }
                        ],
                        "timestamp_ms": 1_000,
                    }
                ),
                MessageRecord(
                    message={
                        "role": "tool_result",
                        "tool_call_id": "call-long",
                        "tool_name": "exec_command",
                                    "content": "failed",
                        "timestamp_ms": 602_000,
                    }
                ),
            ],
            ToolConfig(adapter="psychevo"),
        )
        self.assertIsNone(long_fallback.steps_meta[0].tool_calls[0].execution_duration_ms)

        unmatched = convert_records(
            [
                MessageRecord(
                    message={
                        "role": "tool_result",
                        "tool_call_id": "missing-call",
                        "tool_name": "read",
                        "content": "orphan",
                        "is_error": True,
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
        self.assertEqual(final_extra(unmatched.trajectory)["total_tool_calls"], 0)
        self.assertEqual(final_extra(unmatched.trajectory)["total_tool_errors"], 0)
        self.assertIn("unmatched tool result: missing-call", unmatched.warnings)


    def test_active_duration_excludes_long_idle_and_preserves_wall_duration(self) -> None:
        records = [
            MessageRecord(
                message={"role": "user", "content": "first", "timestamp_ms": 1_000}
            ),
            MessageRecord(
                message={"role": "assistant", "content": "first done", "timestamp_ms": 2_000},
                metadata={"elapsed_ms": 30_000},
            ),
            MessageRecord(
                message={"role": "user", "content": "second", "timestamp_ms": 65_000}
            ),
            MessageRecord(
                message={"role": "assistant", "content": "second done", "timestamp_ms": 66_000},
                metadata={"elapsed_ms": 5_000},
            ),
            MessageRecord(
                message={"role": "user", "content": "after idle", "timestamp_ms": 10_866_000}
            ),
            MessageRecord(
                message={"role": "assistant", "content": "idle done", "timestamp_ms": 10_867_000},
                metadata={"elapsed_ms": 7_000},
            ),
        ]
        config = ToolConfig(adapter="psychevo", trajectory_id="trial:active")
        result = convert_records(records, config)
        report = build_report(result, config, "inline")
        meta = report["trajectory_meta"][0]

        self.assertEqual(meta["started_at_ms"], 1_000)
        self.assertEqual(meta["finished_at_ms"], 10_874_000)
        self.assertEqual(meta["wall_duration_ms"], 10_873_000)
        self.assertEqual(meta["duration_ms"], 42_000)
        self.assertEqual(
            [step["duration_ms"] for step in meta["steps"]],
            [None, 30_000, None, 5_000, None, 7_000],
        )


    def test_active_duration_uses_bounded_timestamp_fallback(self) -> None:
        records = [
            MessageRecord(
                message={"role": "user", "content": "start", "timestamp_ms": 1}
            ),
            MessageRecord(
                message={"role": "assistant", "content": "short fallback", "timestamp_ms": 1_000}
            ),
            MessageRecord(
                message={"role": "user", "content": "within cap", "timestamp_ms": 101_000}
            ),
            MessageRecord(
                message={"role": "assistant", "content": "long fallback", "timestamp_ms": 200_000}
            ),
            MessageRecord(
                message={"role": "user", "content": "past cap", "timestamp_ms": 901_000}
            ),
        ]
        config = ToolConfig(adapter="psychevo", trajectory_id="trial:fallback")
        result = convert_records(records, config)
        report = build_report(result, config, "inline")
        meta = report["trajectory_meta"][0]

        self.assertEqual(meta["wall_duration_ms"], 900_999)
        self.assertEqual(meta["duration_ms"], 100_000)
        self.assertEqual(
            [step["duration_ms"] for step in meta["steps"]],
            [None, 100_000, None, None, None],
        )


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
                        },
                        {
                            "tool_call_id": "call-lint-1",
                            "function_name": "exec_command",
                            "arguments": {"cmd": "ruff check ."},
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
                    "role": "tool_result",
                    "tool_call_id": "call-lint-1",
                    "tool_name": "exec_command",
                    "content": {"exit_code": 1, "stderr": "F821 undefined name"},
                    "is_error": True,
                    "timestamp_ms": 1130,
                },
                metadata={"elapsed_ms": 100},
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
        self.assertEqual(final_extra(trajectory)["total_tool_calls"], 3)
        self.assertEqual(final_extra(trajectory)["total_tool_errors"], 2)
        self.assertEqual(result.steps_meta[1].tool_calls[0].status, "error")
        self.assertEqual(result.steps_meta[1].tool_calls[1].status, "error")
        self.assertTrue(result.steps_meta[1].tool_error)
        self.assertEqual(result.steps_meta[1].tool_calls[0].execution_duration_ms, 125)
        self.assertEqual(result.steps_meta[1].tool_calls[1].execution_duration_ms, 100)
        self.assertEqual(result.steps_meta[2].tool_calls[0].status, "completed")
        self.assertFalse(result.steps_meta[2].tool_error)
        self.assertEqual(result.warnings, [])

        config = ToolConfig(adapter="psychevo", trajectory_id="trial:failure")
        report = build_report(result, config, "inline")
        step_meta = report["trajectory_meta"][0]["steps"]
        self.assertIsNone(step_meta[1]["duration_ms"])
        self.assertIsNone(step_meta[2]["duration_ms"])
        self.assertEqual(step_meta[1]["tool_calls"][0]["execution_duration_ms"], 125)
        self.assertEqual(step_meta[1]["tool_calls"][1]["execution_duration_ms"], 100)
        self.assertEqual(step_meta[2]["tool_calls"][0]["execution_duration_ms"], 100)
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
        self.assertEqual(metrics["extra"]["usage"]["input_tokens"], 11)
        self.assertEqual(metrics["extra"]["usage"]["output_tokens"], 13)
        self.assertEqual(metrics["extra"]["accounting"]["billable_input_tokens"], 11)
        self.assertEqual(metrics["extra"]["accounting"]["billable_output_tokens"], 13)
