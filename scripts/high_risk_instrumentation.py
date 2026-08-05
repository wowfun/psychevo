#!/usr/bin/env python3
"""Run the bounded high-risk instrumentation profile."""

from __future__ import annotations

import argparse
import json
import os
from pathlib import Path
import platform
import re
import shlex
import shutil
import subprocess
import sys
import time
import tomllib
from typing import Any


ROOT = Path(__file__).resolve().parents[1]
MANIFEST_PATH = ROOT / "instrumentation-tools.toml"
PROBE_TIMEOUT_SECONDS = 120
COVERAGE_TIMEOUT_SECONDS = 3_600
DETERMINISTIC_COMMAND_TIMEOUT_SECONDS = 1_800
MIRI_TIMEOUT_SECONDS = 3_600
ASAN_TIMEOUT_SECONDS = 3_600
DETERMINISTIC_CONTRACTS = (
    (
        "gatewayPublicWireRoundTrips",
        (
            "cargo",
            "test",
            "--locked",
            "-p",
            "psychevo-gateway-protocol",
            "--lib",
            "app_server::app_server_contract_tests::public_python_wire_types_match_the_shared_cross_language_corpus",
            "--",
            "--exact",
        ),
    ),
    (
        "gatewaySafeIntegerBoundaries",
        (
            "cargo",
            "test",
            "--locked",
            "-p",
            "psychevo-gateway-protocol",
            "--lib",
            "safe_integer::tests::safe_integer_bounds_round_trip",
            "--",
            "--exact",
        ),
    ),
    (
        "providerStreamFragmentation",
        (
            "cargo",
            "test",
            "--locked",
            "-p",
            "psychevo-ai",
            "--lib",
            "--features",
            "openai",
            "tests::request_streaming::chat_stream_fragmentation_preserves_tool_arguments_and_terminal_event",
            "--",
            "--exact",
        ),
    ),
    (
        "toolArgumentAssembly",
        (
            "cargo",
            "test",
            "--locked",
            "-p",
            "psychevo-ai",
            "--test",
            "sdk",
            "tool_argument_fragmentation_boundary_matrix_is_lossless",
            "--",
            "--exact",
        ),
    ),
)
ASAN_TEST = "shell_scheduler_parks_without_tracked_activity_and_track_wakes_foreign_control"
HIGH_RISK_COVERAGE_TARGETS = {
    "frameworkLifecycle": "crates/psychevo/src/application/lifecycle.rs",
    "turnDeliveryPersistence": "crates/psychevo/src/store/turn_delivery.rs",
    "gatewayActivityPersistence": (
        "crates/psychevo-gateway/src/gateway/durable_activity.rs"
    ),
    "appServerProtocolProjection": "crates/psychevo-gateway/src/app_server.rs",
}


def load_manifest() -> dict[str, Any]:
    with MANIFEST_PATH.open("rb") as handle:
        manifest = tomllib.load(handle)
    if manifest.get("schema") != 1:
        raise RuntimeError(f"unsupported {MANIFEST_PATH.name} schema")
    nightly = manifest.get("nightly")
    target = manifest.get("target")
    if not isinstance(nightly, str) or not re.fullmatch(
        r"nightly-[0-9]{4}-[0-9]{2}-[0-9]{2}", nightly
    ):
        raise RuntimeError("instrumentation nightly must be date-pinned")
    if target != "x86_64-unknown-linux-gnu":
        raise RuntimeError(f"unsupported instrumentation target: {target!r}")
    for name in ("cargo-llvm-cov",):
        version = manifest.get(name, {}).get("version")
        if not isinstance(version, str) or not re.fullmatch(
            r"[0-9]+\.[0-9]+\.[0-9]+", version
        ):
            raise RuntimeError(f"{name} must have an exact semantic version")
    return manifest


def require_linux_x86_64() -> None:
    machine = platform.machine().lower()
    if sys.platform != "linux" or machine not in {"x86_64", "amd64"}:
        raise RuntimeError(
            f"instrumentation supports Linux x86_64, found {sys.platform}/{machine}"
        )


def artifact_root() -> Path:
    raw = os.environ.get("PSYCHEVO_CI_ARTIFACT_ROOT")
    if not raw:
        raise RuntimeError("PSYCHEVO_CI_ARTIFACT_ROOT is required")
    root = Path(raw).resolve() / "instrumentation"
    root.mkdir(parents=True, exist_ok=True)
    return root


def reset_directory(path: Path) -> None:
    if path.exists():
        shutil.rmtree(path)
    path.mkdir(parents=True)


def command_output(
    command: list[str],
    *,
    timeout_seconds: int = PROBE_TIMEOUT_SECONDS,
    env: dict[str, str] | None = None,
) -> str:
    try:
        result = subprocess.run(
            command,
            cwd=ROOT,
            env={**os.environ, **(env or {})},
            check=True,
            capture_output=True,
            text=True,
            timeout=timeout_seconds,
        )
    except FileNotFoundError as error:
        raise RuntimeError(f"required executable not found: {command[0]}") from error
    except subprocess.CalledProcessError as error:
        output = "\n".join(
            part.strip() for part in (error.stdout, error.stderr) if part.strip()
        )
        raise RuntimeError(f"{' '.join(command)} failed: {output}") from error
    except subprocess.TimeoutExpired as error:
        raise RuntimeError(
            f"instrumentation timeout after {timeout_seconds} seconds: "
            f"{shlex.join(command)}"
        ) from error
    return "\n".join(part.strip() for part in (result.stdout, result.stderr) if part.strip())


def run(
    command: list[str],
    *,
    timeout_seconds: int,
    env: dict[str, str] | None = None,
) -> None:
    try:
        status = subprocess.run(
            command,
            cwd=ROOT,
            env={**os.environ, **(env or {})},
            check=False,
            timeout=timeout_seconds,
        )
    except FileNotFoundError as error:
        raise RuntimeError(f"required executable not found: {command[0]}") from error
    except subprocess.TimeoutExpired as error:
        raise RuntimeError(
            f"instrumentation timeout after {timeout_seconds} seconds: "
            f"{shlex.join(command)}"
        ) from error
    if status.returncode != 0:
        raise RuntimeError(
            f"{' '.join(command)} exited with status {status.returncode}"
        )


def require_version(command: list[str], expected: str, name: str) -> None:
    output = command_output(command)
    versions = re.findall(r"(?<![0-9.])[0-9]+\.[0-9]+\.[0-9]+(?![0-9.])", output)
    if expected not in versions:
        raise RuntimeError(f"expected {name} {expected}, found: {output}")
    print(f"verified {name} {expected}")


def verify() -> None:
    manifest = load_manifest()
    require_linux_x86_64()
    require_version(
        ["cargo", "llvm-cov", "--version"],
        manifest["cargo-llvm-cov"]["version"],
        "cargo-llvm-cov",
    )
    nightly = manifest["nightly"]
    rustc = command_output(["rustc", f"+{nightly}", "--version", "--verbose"])
    if "release:" not in rustc or "commit-hash:" not in rustc:
        raise RuntimeError(f"pinned nightly did not return verbose identity: {rustc}")
    miri = command_output(["cargo", f"+{nightly}", "miri", "--version"])
    if not miri.startswith("miri "):
        raise RuntimeError(f"pinned Miri component is unavailable: {miri}")
    print(f"verified {nightly} with Miri")


def coverage() -> None:
    manifest = load_manifest()
    output = artifact_root() / "coverage"
    target = output / "target"
    reset_directory(output)
    llvm_cov = ["cargo", f"+{manifest['nightly']}", "llvm-cov"]
    environment = {
        "CARGO_INCREMENTAL": "0",
        "CARGO_LLVM_COV_TARGET_DIR": str(target),
    }
    run(
        [*llvm_cov, "clean", "--workspace"],
        env=environment,
        timeout_seconds=COVERAGE_TIMEOUT_SECONDS,
    )
    run(
        [
            *llvm_cov,
            "--locked",
            "--branch",
            "--no-report",
            "--package",
            "psychevo",
            "--package",
            "psychevo-gateway",
            "--package",
            "psychevo-gateway-protocol",
            "--lib",
        ],
        env=environment,
        timeout_seconds=COVERAGE_TIMEOUT_SECONDS,
    )
    run(
        [
            *llvm_cov,
            "report",
            "--branch",
            "--lcov",
            "--output-path",
            str(output / "lcov.info"),
        ],
        env=environment,
        timeout_seconds=COVERAGE_TIMEOUT_SECONDS,
    )
    run(
        [
            *llvm_cov,
            "report",
            "--branch",
            "--json",
            "--summary-only",
            "--output-path",
            str(output / "summary.json"),
        ],
        env=environment,
        timeout_seconds=COVERAGE_TIMEOUT_SECONDS,
    )
    write_high_risk_coverage_summary(output / "summary.json", output)


def write_high_risk_coverage_summary(summary_path: Path, output: Path) -> None:
    summary = json.loads(summary_path.read_text(encoding="utf-8"))
    reports = summary.get("data")
    if not isinstance(reports, list) or len(reports) != 1:
        raise RuntimeError("coverage summary must contain exactly one report")
    files = reports[0].get("files")
    if not isinstance(files, list):
        raise RuntimeError("coverage summary did not retain per-file counters")

    by_path: dict[str, dict[str, Any]] = {}
    for entry in files:
        if not isinstance(entry, dict) or not isinstance(entry.get("filename"), str):
            continue
        normalized = Path(entry["filename"]).as_posix()
        by_path[normalized] = entry

    targets: dict[str, dict[str, Any]] = {}
    for name, relative_path in HIGH_RISK_COVERAGE_TARGETS.items():
        matches = [
            entry
            for path, entry in by_path.items()
            if path == relative_path or path.endswith(f"/{relative_path}")
        ]
        if len(matches) != 1:
            raise RuntimeError(
                f"coverage target {relative_path} matched {len(matches)} report files"
            )
        counters = matches[0].get("summary")
        if not isinstance(counters, dict):
            raise RuntimeError(f"coverage target {relative_path} has no summary")
        retained: dict[str, Any] = {}
        for metric in ("lines", "functions", "branches"):
            values = counters.get(metric)
            if not isinstance(values, dict):
                raise RuntimeError(
                    f"coverage target {relative_path} has no {metric} counters"
                )
            count = values.get("count")
            covered = values.get("covered")
            percent = values.get("percent")
            if (
                type(count) is not int
                or type(covered) is not int
                or not isinstance(percent, int | float)
                or count <= 0
                or covered <= 0
            ):
                raise RuntimeError(
                    f"coverage target {relative_path} has no exercised {metric}"
                )
            retained[metric] = {
                "count": count,
                "covered": covered,
                "percent": percent,
            }
        targets[name] = {
            "path": relative_path,
            "summary": retained,
        }

    (output / "high-risk-targets.json").write_text(
        json.dumps(
            {
                "schemaVersion": 1,
                "branchInstrumentation": True,
                "targets": targets,
            },
            indent=2,
            sort_keys=True,
        )
        + "\n",
        encoding="utf-8",
    )


def deterministic() -> None:
    output = artifact_root() / "deterministic-contracts"
    reset_directory(output)
    target = output / "target"
    results: list[dict[str, Any]] = []
    for name, command_tuple in DETERMINISTIC_CONTRACTS:
        command = list(command_tuple)
        started = time.monotonic_ns()
        failure: RuntimeError | None = None
        try:
            contract_output = command_output(
                command,
                env={"CARGO_TARGET_DIR": str(target)},
                timeout_seconds=DETERMINISTIC_COMMAND_TIMEOUT_SECONDS,
            )
            print(contract_output)
            if not re.search(r"test result: ok\. 1 passed; 0 failed;", contract_output):
                raise RuntimeError(f"{name} did not execute exactly one passing test")
        except RuntimeError as error:
            failure = error
        result = {
            "contract": name,
            "status": "failed" if failure else "passed",
            "durationMs": (time.monotonic_ns() - started) // 1_000_000,
            "command": command,
            "executedTests": 0 if failure else 1,
        }
        if failure:
            result["error"] = str(failure)
        results.append(result)
        write_deterministic_report(output, results)
        if failure:
            raise failure


def write_deterministic_report(output: Path, results: list[dict[str, Any]]) -> None:
    report = {
        "schemaVersion": 1,
        "contracts": results,
    }
    (output / "run.json").write_text(
        json.dumps(report, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )


def miri() -> None:
    manifest = load_manifest()
    target = artifact_root() / "miri-target"
    reset_directory(target)
    run(
        [
            "cargo",
            f"+{manifest['nightly']}",
            "miri",
            "test",
            "--locked",
            "-p",
            "psychevo-gateway-protocol",
            "--lib",
        ],
        env={"CARGO_TARGET_DIR": str(target)},
        timeout_seconds=MIRI_TIMEOUT_SECONDS,
    )


def asan() -> None:
    manifest = load_manifest()
    target = artifact_root() / "asan-target"
    reset_directory(target)
    run(
        [
            "cargo",
            f"+{manifest['nightly']}",
            "test",
            "--locked",
            "-Zbuild-std",
            "--target",
            manifest["target"],
            "-p",
            "psychevo-gateway",
            "--lib",
            ASAN_TEST,
            "--",
            "--nocapture",
        ],
        env={
            "ASAN_OPTIONS": "detect_leaks=1:halt_on_error=1",
            "CARGO_INCREMENTAL": "0",
            "CARGO_TARGET_DIR": str(target),
            "RUSTDOCFLAGS": "-Zsanitizer=address",
            "RUSTFLAGS": "-Zsanitizer=address",
        },
        timeout_seconds=ASAN_TIMEOUT_SECONDS,
    )


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "command", choices=["verify", "coverage", "deterministic", "miri", "asan"]
    )
    args = parser.parse_args()
    try:
        require_linux_x86_64()
        {
            "verify": verify,
            "coverage": coverage,
            "deterministic": deterministic,
            "miri": miri,
            "asan": asan,
        }[args.command]()
    except (OSError, KeyError, TypeError, ValueError, RuntimeError) as error:
        print(f"instrumentation failed: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
