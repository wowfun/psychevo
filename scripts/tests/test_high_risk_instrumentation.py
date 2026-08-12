from __future__ import annotations

import json
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest
from unittest.mock import patch

from scripts import high_risk_instrumentation as instrumentation


class HighRiskInstrumentationHarnessTests(unittest.TestCase):
    def test_verify_does_not_require_an_artifact_root(self) -> None:
        with (
            patch.dict(instrumentation.os.environ, {}, clear=True),
            patch.object(instrumentation, "require_linux_x86_64"),
            patch.object(instrumentation, "load_manifest", return_value={}),
            patch.object(instrumentation, "verify") as verify,
            patch.object(
                instrumentation,
                "write_resource_policy",
                side_effect=AssertionError("verify must not write evidence"),
            ),
            patch.object(instrumentation.sys, "argv", ["runner", "verify"]),
        ):
            self.assertEqual(instrumentation.main(), 0)
        verify.assert_called_once_with()

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
            target_root = Path(raw) / "scratch" / "nightly-2026-08-01"
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
                    "instrumentation_target_root",
                    return_value=target_root,
                ),
                patch.object(
                    instrumentation,
                    "load_manifest",
                    return_value={
                        "nightly": "nightly-2026-08-01",
                        "resources": {"maximum-parallelism": 4},
                    },
                ),
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
                str(target_root / "deterministic"),
            )
            self.assertEqual(calls[0][1]["CARGO_BUILD_JOBS"], "4")
            self.assertEqual(calls[0][1]["RUST_TEST_THREADS"], "4")
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
            target_root = Path(raw) / "scratch" / "nightly-2026-07-17"
            calls: list[tuple[list[str], dict[str, str], int]] = []

            def record(
                command: list[str], *, timeout_seconds: int, env=None
            ) -> None:
                calls.append((command, env or {}, timeout_seconds))

            manifest = {
                "nightly": "nightly-2026-07-17",
                "target": "x86_64-unknown-linux-gnu",
                "resources": {"maximum-parallelism": 4},
            }
            with (
                patch.object(instrumentation, "artifact_root", return_value=artifact_root),
                patch.object(
                    instrumentation,
                    "instrumentation_target_root",
                    return_value=target_root,
                ),
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
                environment["CARGO_TARGET_DIR"], str(target_root / "asan")
            )
            self.assertEqual(environment["CARGO_BUILD_JOBS"], "4")
            self.assertEqual(environment["TOKIO_WORKER_THREADS"], "4")
            self.assertEqual(timeout, instrumentation.ASAN_TIMEOUT_SECONDS)

    def test_coverage_uses_the_pinned_nightly_for_every_llvm_cov_command(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            artifact_root = Path(raw) / "instrumentation"
            target_root = Path(raw) / "scratch" / "nightly-2026-07-17"
            calls: list[tuple[list[str], dict[str, str]]] = []

            def record(command: list[str], *, timeout_seconds: int, env=None) -> None:
                del timeout_seconds
                calls.append((command, env or {}))

            with (
                patch.object(instrumentation, "artifact_root", return_value=artifact_root),
                patch.object(
                    instrumentation,
                    "load_manifest",
                    return_value={
                        "nightly": "nightly-2026-07-17",
                        "resources": {"maximum-parallelism": 4},
                    },
                ),
                patch.object(
                    instrumentation,
                    "instrumentation_target_root",
                    return_value=target_root,
                ),
                patch.object(instrumentation, "run", side_effect=record),
                patch.object(instrumentation, "write_high_risk_coverage_summary"),
            ):
                instrumentation.coverage()

            self.assertEqual(len(calls), 4)
            for command, environment in calls:
                self.assertEqual(
                    command[:3],
                    ["cargo", "+nightly-2026-07-17", "llvm-cov"],
                )
                self.assertEqual(environment["CFLAGS"], "-O1")
                self.assertEqual(environment["PSYCHEVO_INSTRUMENTED_COVERAGE"], "1")
                self.assertEqual(environment["CARGO_BUILD_JOBS"], "4")
                self.assertEqual(
                    environment["CARGO_LLVM_COV_TARGET_DIR"],
                    str(target_root / "coverage"),
                )
            self.assertEqual(calls[0][0][-1], "--profraw-only")
            self.assertIn("--no-report", calls[1][0])
            self.assertNotIn("--no-clean", calls[1][0])

    def test_resource_policy_keeps_scratch_outside_evidence(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            evidence = Path(raw) / "evidence" / "instrumentation"
            target = Path(raw) / "scratch" / "nightly-2026-08-01"
            manifest = {
                "nightly": "nightly-2026-08-01",
                "resources": {
                    "maximum-parallelism": 4,
                    "minimum-available-memory-mib": 2048,
                    "minimum-free-scratch-mib": 8192,
                },
            }
            with (
                patch.object(instrumentation, "artifact_root", return_value=evidence),
                patch.object(
                    instrumentation,
                    "instrumentation_target_root",
                    return_value=target,
                ),
                patch.object(instrumentation, "available_memory_mib", return_value=16384),
                patch.object(
                    instrumentation.shutil,
                    "disk_usage",
                    return_value=shutil._ntuple_diskusage(100, 10, 90 * 1024 * 1024 * 1024),
                ),
            ):
                evidence.mkdir(parents=True)
                instrumentation.write_resource_policy(manifest)

            report = json.loads(
                (evidence / "resource-policy.json").read_text(encoding="utf-8")
            )
            self.assertEqual(report["maximumParallelism"], 4)
            self.assertEqual(report["observedAvailableMemoryMiB"], 16384)
            self.assertTrue(report["scratchOutsideEvidenceRoot"])
            self.assertEqual(report["scratchTargetRoot"], str(target))

    def test_repository_policy_runs_instrumentation_at_sixteen_way_parallelism(
        self,
    ) -> None:
        manifest = instrumentation.load_manifest()

        self.assertEqual(manifest["resources"]["maximum-parallelism"], 16)
        environment = instrumentation.bounded_environment(manifest)
        self.assertEqual(
            {
                name: environment[name]
                for name in (
                    "CARGO_BUILD_JOBS",
                    "RAYON_NUM_THREADS",
                    "RUST_TEST_THREADS",
                    "TOKIO_WORKER_THREADS",
                )
            },
            {
                "CARGO_BUILD_JOBS": "16",
                "RAYON_NUM_THREADS": "16",
                "RUST_TEST_THREADS": "16",
                "TOKIO_WORKER_THREADS": "16",
            },
        )

    def test_app_server_coverage_targets_the_public_event_projection(self) -> None:
        self.assertEqual(
            instrumentation.HIGH_RISK_COVERAGE_TARGETS[
                "appServerProtocolProjection"
            ]["path"],
            "crates/psychevo-gateway/src/app_server.rs",
        )

    def test_coverage_scope_uses_the_canonical_target_inventory(self) -> None:
        target = instrumentation.HIGH_RISK_COVERAGE_TARGETS["foreignLiveProjection"][
            "path"
        ]
        with patch.dict(
            instrumentation.os.environ,
            {"PSYCHEVO_CHANGED_FILES_JSON": json.dumps([target])},
            clear=True,
        ):
            with patch("builtins.print") as output:
                instrumentation.coverage_required()
        output.assert_called_once_with("coverage=true")

        with patch.dict(
            instrumentation.os.environ,
            {"PSYCHEVO_CHANGED_FILES_JSON": json.dumps(["README.md"])},
            clear=True,
        ):
            with patch("builtins.print") as output:
                instrumentation.coverage_required()
        output.assert_called_once_with("coverage=false")

    def test_resource_policy_fails_before_work_when_memory_reserve_is_unavailable(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            evidence = Path(raw) / "evidence" / "instrumentation"
            target = Path(raw) / "scratch"
            evidence.mkdir(parents=True)
            target.mkdir()
            manifest = {
                "resources": {
                    "maximum-parallelism": 4,
                    "minimum-available-memory-mib": 2048,
                    "minimum-free-scratch-mib": 8192,
                }
            }
            with (
                patch.object(instrumentation, "artifact_root", return_value=evidence),
                patch.object(
                    instrumentation, "instrumentation_target_root", return_value=target
                ),
                patch.object(instrumentation, "available_memory_mib", return_value=1024),
                patch.object(
                    instrumentation.shutil,
                    "disk_usage",
                    return_value=shutil._ntuple_diskusage(
                        100, 10, 90 * 1024 * 1024 * 1024
                    ),
                ),
            ):
                with self.assertRaisesRegex(RuntimeError, "available memory"):
                    instrumentation.write_resource_policy(manifest)

            report = json.loads(
                (evidence / "resource-policy.json").read_text(encoding="utf-8")
            )
            self.assertEqual(len(report["failures"]), 1)

    def test_high_risk_summary_enforces_named_per_metric_floor(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            output = Path(raw)
            target = instrumentation.HIGH_RISK_COVERAGE_TARGETS["frameworkLifecycle"]
            summary = {
                "data": [{
                    "files": [{
                        "filename": target["path"],
                        "summary": {
                            metric: {
                                "count": 100,
                                "covered": int(minimum) - 1,
                                "percent": minimum - 1,
                            }
                            for metric, minimum in target["minimum"].items()
                        },
                    }],
                }],
            }
            summary_path = output / "summary.json"
            summary_path.write_text(json.dumps(summary), encoding="utf-8")
            with (
                patch.object(
                    instrumentation,
                    "HIGH_RISK_COVERAGE_TARGETS",
                    {"frameworkLifecycle": target},
                ),
                self.assertRaisesRegex(RuntimeError, r"is below 80%"),
            ):
                instrumentation.write_high_risk_coverage_summary(summary_path, output)


if __name__ == "__main__":
    unittest.main()
