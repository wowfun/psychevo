from __future__ import annotations

import json
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch

from scripts import high_risk_instrumentation as instrumentation


class HighRiskInstrumentationHarnessTests(unittest.TestCase):
    def test_reset_directory_removes_stale_evidence(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            output = Path(raw) / "deterministic-contracts"
            output.mkdir()
            (output / "stale-result").write_text("old", encoding="utf-8")

            instrumentation.reset_directory(output)

            self.assertTrue(output.is_dir())
            self.assertEqual(list(output.iterdir()), [])

    def test_run_reports_exact_timed_out_command(self) -> None:
        command = ["cargo", "test", "-p", "psychevo-ai"]
        timeout = subprocess.TimeoutExpired(command, 17)
        with patch.object(instrumentation.subprocess, "run", side_effect=timeout):
            with self.assertRaisesRegex(
                RuntimeError,
                r"instrumentation timeout after 17 seconds: cargo test -p psychevo-ai",
            ):
                instrumentation.run(command, timeout_seconds=17)

    def test_deterministic_contracts_clean_output_and_record_first_failure(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            artifact_root = Path(raw) / "instrumentation"
            stale = artifact_root / "deterministic-contracts" / "stale-result"
            stale.parent.mkdir(parents=True)
            stale.write_text("old", encoding="utf-8")
            calls: list[tuple[list[str], dict[str, str]]] = []

            def capture_contract(
                command: list[str], *, timeout_seconds: int, env=None
            ) -> str:
                del timeout_seconds
                self.assertFalse(stale.exists())
                calls.append((command, env or {}))
                if len(calls) == 2:
                    raise RuntimeError("boundary contract failed")
                return "test result: ok. 1 passed; 0 failed; 0 ignored"

            with (
                patch.object(instrumentation, "artifact_root", return_value=artifact_root),
                patch.object(
                    instrumentation,
                    "DETERMINISTIC_CONTRACTS",
                    (
                        ("protocol", ("cargo", "test", "protocol")),
                        ("stream", ("cargo", "test", "stream")),
                    ),
                ),
                patch.object(
                    instrumentation, "command_output", side_effect=capture_contract
                ),
            ):
                with self.assertRaisesRegex(
                    RuntimeError, r"boundary contract failed"
                ):
                    instrumentation.deterministic()

            self.assertEqual(
                calls[0][1]["CARGO_TARGET_DIR"],
                str(artifact_root / "deterministic-contracts" / "target"),
            )
            report = json.loads(
                (artifact_root / "deterministic-contracts" / "run.json").read_text(
                    encoding="utf-8"
                )
            )
            self.assertEqual(
                [contract["status"] for contract in report["contracts"]],
                ["passed", "failed"],
            )
            self.assertEqual(
                report["contracts"][1]["error"], "boundary contract failed"
            )

    def test_asan_constructs_one_exact_target_argument(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            artifact_root = Path(raw) / "instrumentation"
            calls: list[tuple[list[str], dict[str, str], int]] = []

            def record(
                command: list[str], *, timeout_seconds: int, env=None
            ) -> None:
                calls.append((command, env or {}, timeout_seconds))

            manifest = {
                "nightly": "nightly-2026-07-17",
                "target": "x86_64-unknown-linux-gnu",
            }
            with (
                patch.object(instrumentation, "artifact_root", return_value=artifact_root),
                patch.object(instrumentation, "load_manifest", return_value=manifest),
                patch.object(instrumentation, "run", side_effect=record),
            ):
                instrumentation.asan()

            self.assertEqual(len(calls), 1)
            command, environment, timeout = calls[0]
            self.assertEqual(
                command,
                [
                    "cargo",
                    "+nightly-2026-07-17",
                    "test",
                    "--locked",
                    "-Zbuild-std",
                    "--target",
                    "x86_64-unknown-linux-gnu",
                    "-p",
                    "psychevo-gateway",
                    "--lib",
                    instrumentation.ASAN_TEST,
                    "--",
                    "--nocapture",
                ],
            )
            self.assertEqual(command.count("x86_64-unknown-linux-gnu"), 1)
            self.assertEqual(
                environment["CARGO_TARGET_DIR"], str(artifact_root / "asan-target")
            )
            self.assertEqual(timeout, instrumentation.ASAN_TIMEOUT_SECONDS)

    def test_coverage_uses_the_pinned_nightly_for_every_llvm_cov_command(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            artifact_root = Path(raw) / "instrumentation"
            calls: list[list[str]] = []

            def record(command: list[str], *, timeout_seconds: int, env=None) -> None:
                del timeout_seconds, env
                calls.append(command)

            with (
                patch.object(instrumentation, "artifact_root", return_value=artifact_root),
                patch.object(
                    instrumentation,
                    "load_manifest",
                    return_value={"nightly": "nightly-2026-07-17"},
                ),
                patch.object(instrumentation, "run", side_effect=record),
                patch.object(instrumentation, "write_high_risk_coverage_summary"),
            ):
                instrumentation.coverage()

            self.assertEqual(len(calls), 4)
            for command in calls:
                self.assertEqual(
                    command[:3],
                    ["cargo", "+nightly-2026-07-17", "llvm-cov"],
                )

    def test_app_server_coverage_targets_the_public_event_projection(self) -> None:
        self.assertEqual(
            instrumentation.HIGH_RISK_COVERAGE_TARGETS[
                "appServerProtocolProjection"
            ],
            "crates/psychevo-gateway/src/app_server.rs",
        )


if __name__ == "__main__":
    unittest.main()
