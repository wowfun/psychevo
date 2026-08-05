#!/usr/bin/env python3
"""Measure Psychevo's checked-in non-functional budgets."""

from __future__ import annotations

import argparse
import json
import os
import platform
import subprocess
import sys
import tempfile
import time
import uuid
from pathlib import Path
from typing import Any


ROOT = Path(__file__).resolve().parents[1]
BUDGET_PATH = ROOT / "non-functional-budgets.json"


def require_linux_x86_64() -> None:
    machine = platform.machine().lower()
    if sys.platform != "linux" or machine not in {"x86_64", "amd64"}:
        raise RuntimeError(
            f"non-functional budgets support Linux x86_64, found {sys.platform}/{machine}"
        )


def load_budgets() -> dict[str, Any]:
    budgets = json.loads(BUDGET_PATH.read_text(encoding="utf-8"))
    if budgets.get("schemaVersion") != 1:
        raise RuntimeError(f"unsupported {BUDGET_PATH.name} schema")
    return budgets


def artifact_root() -> Path:
    raw = os.environ.get("PSYCHEVO_CI_ARTIFACT_ROOT")
    if not raw:
        raise RuntimeError("PSYCHEVO_CI_ARTIFACT_ROOT is required")
    root = Path(raw).resolve()
    root.mkdir(parents=True, exist_ok=True)
    return root


def run(
    command: list[str],
    *,
    env: dict[str, str] | None = None,
    unset_env: tuple[str, ...] = (),
) -> subprocess.CompletedProcess[str]:
    process_env = {**os.environ, **(env or {})}
    for key in unset_env:
        process_env.pop(key, None)
    try:
        return subprocess.run(
            command,
            cwd=ROOT,
            env=process_env,
            check=True,
            text=True,
            capture_output=True,
        )
    except FileNotFoundError as error:
        raise RuntimeError(f"required executable not found: {command[0]}") from error
    except subprocess.CalledProcessError as error:
        output = "\n".join(
            part.strip() for part in (error.stdout, error.stderr) if part.strip()
        )
        suffix = f":\n{output}" if output else ""
        raise RuntimeError(
            f"{' '.join(command)} exited with status {error.returncode}{suffix}"
        ) from error


def print_output(result: subprocess.CompletedProcess[str]) -> None:
    if result.stdout:
        print(result.stdout, end="")
    if result.stderr:
        print(result.stderr, end="", file=sys.stderr)


def write_report(name: str, report: dict[str, Any]) -> Path:
    output = artifact_root() / "non-functional"
    output.mkdir(parents=True, exist_ok=True)
    path = output / f"{name}.json"
    path.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    return path


def require_maximum(label: str, observed: int, maximum: int, failures: list[str]) -> None:
    if observed > maximum:
        failures.append(f"{label}: observed {observed}, maximum {maximum}")


def normal_dependency_counts() -> tuple[int, int]:
    command = [
        "cargo",
        "tree",
        "--locked",
        "-p",
        "psychevo",
        "--no-default-features",
        "--target",
        "x86_64-unknown-linux-gnu",
        "--edges",
        "normal",
        "--prefix",
        "none",
        "--no-dedupe",
        "--format",
        "{p}",
    ]
    tree = run(command)
    reachable_lines = {
        line.strip() for line in tree.stdout.splitlines() if line.strip()
    }
    if not reachable_lines:
        raise RuntimeError("cargo tree returned an empty Framework dependency graph")
    reachable = len(reachable_lines) - 1
    direct_tree = run([*command, "--depth", "1"])
    direct_lines = {
        line.strip() for line in direct_tree.stdout.splitlines() if line.strip()
    }
    if not direct_lines:
        raise RuntimeError("cargo tree returned an empty direct Framework dependency graph")
    direct = len(direct_lines) - 1
    return direct, reachable


def timed_framework_check() -> tuple[int, int, Path]:
    target = artifact_root() / "non-functional" / f"framework-check-{uuid.uuid4().hex}"
    target.mkdir(parents=True, exist_ok=False)
    environment = {
        "CARGO_TARGET_DIR": str(target),
        "CARGO_INCREMENTAL": "1",
    }
    command = [
        "cargo",
        "check",
        "--locked",
        "-p",
        "psychevo",
        "--no-default-features",
        "--lib",
        "--quiet",
    ]
    started = time.monotonic_ns()
    first = run(command, env=environment)
    clean_ms = (time.monotonic_ns() - started) // 1_000_000
    print_output(first)

    started = time.monotonic_ns()
    second = run(command, env=environment)
    noop_ms = (time.monotonic_ns() - started) // 1_000_000
    print_output(second)
    return clean_ms, noop_ms, target


def framework() -> None:
    budgets = load_budgets()["framework"]
    baseline = budgets["baseline"]
    maximum = budgets["maximum"]
    direct, reachable = normal_dependency_counts()
    clean_ms, noop_ms, target = timed_framework_check()
    ratio_millis = (noop_ms * 1_000) // max(clean_ms, 1)
    failures: list[str] = []
    require_maximum(
        "direct normal dependencies", direct, maximum["directDependencies"], failures
    )
    require_maximum(
        "reachable normal packages",
        reachable,
        maximum["reachablePackages"],
        failures,
    )
    require_maximum(
        "clean Framework check ms", clean_ms, maximum["cleanCheckMs"], failures
    )
    require_maximum(
        "no-op Framework check ms", noop_ms, maximum["noopCheckMs"], failures
    )
    require_maximum(
        "no-op/clean ratio per thousand",
        ratio_millis,
        maximum["noopRatioPerThousand"],
        failures,
    )
    report = {
        "schemaVersion": 1,
        "scope": "framework",
        "observed": {
            "directDependencies": direct,
            "reachablePackages": reachable,
            "cleanCheckMs": clean_ms,
            "noopCheckMs": noop_ms,
            "noopRatioPerThousand": ratio_millis,
        },
        "baseline": baseline,
        "maximum": maximum,
        "budgetSource": str(BUDGET_PATH),
        "cargoTarget": str(target),
        "failures": failures,
    }
    path = write_report("framework", report)
    print(f"Framework non-functional evidence: {path}")
    if failures:
        raise RuntimeError("; ".join(failures))


def wheel_artifact_size(report: dict[str, Any], distribution: str) -> int:
    prefix = f"{distribution}-"
    matches = [
        artifact
        for artifact in report["artifacts"]
        if Path(artifact["path"]).name.startswith(prefix)
        and Path(artifact["path"]).suffix == ".whl"
    ]
    if len(matches) != 1:
        raise RuntimeError(
            f"expected one {distribution!r} wheel artifact, found {len(matches)}"
        )
    return int(matches[0]["bytes"])


def cli_startup() -> None:
    root = artifact_root()
    cli_path = root / "package/cli-target/release/pevo"
    if not cli_path.is_file():
        raise RuntimeError(f"release CLI artifact is missing: {cli_path}")
    budgets = load_budgets()["cliStartup"]
    baseline = budgets["baseline"]
    maximum = budgets["maximum"]
    evidence_root = root / "non-functional"
    evidence_root.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="cli-startup-", dir=evidence_root) as raw_home:
        home = Path(raw_home)
        environment = {
            "HOME": str(home),
            "PSYCHEVO_HOME": str(home / "psychevo"),
            "XDG_CACHE_HOME": str(home / "cache"),
            "XDG_CONFIG_HOME": str(home / "config"),
            "XDG_DATA_HOME": str(home / "data"),
        }

        def sample() -> int:
            started = time.monotonic_ns()
            result = run(
                [str(cli_path), "--version"],
                env=environment,
                unset_env=(
                    "PSYCHEVO_CONFIG",
                    "PSYCHEVO_PROFILE",
                    "PSYCHEVO_PROFILE_HOME",
                ),
            )
            elapsed_ms = (time.monotonic_ns() - started + 999_999) // 1_000_000
            if not result.stdout.strip().startswith("pevo "):
                raise RuntimeError(
                    f"release CLI printed an invalid --version response: {result.stdout!r}"
                )
            return elapsed_ms

        first_ms = sample()
        repeated_ms = [sample() for _ in range(9)]

    sorted_repeated = sorted(repeated_ms)
    repeated_median_ms = sorted_repeated[len(sorted_repeated) // 2]
    observed = {
        "firstProcessMs": first_ms,
        "repeatedProcessMedianMs": repeated_median_ms,
    }
    failures: list[str] = []
    require_maximum(
        "release CLI first-process startup ms",
        first_ms,
        maximum["firstProcessMs"],
        failures,
    )
    require_maximum(
        "release CLI repeated-process median startup ms",
        repeated_median_ms,
        maximum["repeatedProcessMedianMs"],
        failures,
    )
    report = {
        "schemaVersion": 1,
        "scope": "release-cli-startup",
        "observed": observed,
        "samples": {
            "firstProcessMs": first_ms,
            "repeatedProcessMs": repeated_ms,
        },
        "baseline": baseline,
        "maximum": maximum,
        "budgetSource": str(BUDGET_PATH),
        "command": [str(cli_path), "--version"],
        "cacheSemantics": "first process followed by nine immediate restarts; OS page caches are not evicted",
        "failures": failures,
    }
    path = write_report("release-cli-startup", report)
    print(f"Release CLI startup budget evidence: {path}")
    if failures:
        raise RuntimeError("; ".join(failures))


def artifacts() -> None:
    root = artifact_root()
    budgets = load_budgets()["linuxArtifacts"]
    baseline = budgets["baseline"]
    maximum = budgets["maximum"]
    smoke_path = root / "package/python/installed-artifact-smoke.json"
    smoke = json.loads(smoke_path.read_text(encoding="utf-8"))
    cli_path = root / "package/cli-target/release/pevo"
    if not cli_path.is_file():
        raise RuntimeError(f"release CLI artifact is missing: {cli_path}")
    desktop_path = (
        root / "package/non-functional-desktop-target/release/psychevo-desktop"
    )
    if not desktop_path.is_file():
        raise RuntimeError(f"release Desktop artifact is missing: {desktop_path}")
    observed = {
        "releaseCliBytes": cli_path.stat().st_size,
        "releaseDesktopBytes": desktop_path.stat().st_size,
        "pythonSdkWheelBytes": wheel_artifact_size(smoke, "psychevo"),
        "appServerWheelBytes": wheel_artifact_size(smoke, "psychevo_app_server_bin"),
        "pythonCliWheelBytes": wheel_artifact_size(smoke, "psychevo_cli_bin"),
    }
    failures: list[str] = []
    for key, label in [
        ("releaseCliBytes", "release CLI bytes"),
        ("releaseDesktopBytes", "release Desktop bytes"),
        ("pythonSdkWheelBytes", "Python SDK wheel bytes"),
        ("appServerWheelBytes", "App Server wheel bytes"),
        ("pythonCliWheelBytes", "Python CLI wheel bytes"),
    ]:
        require_maximum(label, observed[key], maximum[key], failures)
    report = {
        "schemaVersion": 1,
        "scope": "linux-artifacts",
        "observed": observed,
        "baseline": baseline,
        "maximum": maximum,
        "budgetSource": str(BUDGET_PATH),
        "sources": {
            "releaseCli": str(cli_path),
            "releaseDesktop": str(desktop_path),
            "installedArtifactSmoke": str(smoke_path),
        },
        "failures": failures,
    }
    path = write_report("linux-artifacts", report)
    print(f"Linux artifact budget evidence: {path}")
    if failures:
        raise RuntimeError("; ".join(failures))


def workbench() -> None:
    root = artifact_root()
    output = root / "non-functional/workbench"
    output.mkdir(parents=True, exist_ok=True)
    result = run(
        [
            "pnpm",
            "exec",
            "playwright",
            "test",
            "apps/workbench/e2e/startup-performance.spec.ts",
            "--project=chromium-desktop",
        ],
        env={"PSYCHEVO_PLAYWRIGHT_SCREENSHOT_ROOT": str(output)},
    )
    print_output(result)
    proof_path = output / "startup-resources-chromium-desktop.json"
    proof = json.loads(proof_path.read_text(encoding="utf-8"))
    preview_root = ROOT / "apps/workbench/dist/file-viewer"
    if not preview_root.is_dir():
        raise RuntimeError(f"optional preview asset tree is missing: {preview_root}")
    preview_assets = sorted(path for path in preview_root.rglob("*") if path.is_file())
    if not preview_assets:
        raise RuntimeError(f"optional preview asset tree is empty: {preview_root}")
    if any(path.is_symlink() for path in preview_assets):
        raise RuntimeError(f"optional preview asset tree contains a symbolic link: {preview_root}")
    observed = {
        "initialJavascriptBytes": int(proof["initialEncodedBodySize"]),
        "optionalPreviewAssetBytes": sum(path.stat().st_size for path in preview_assets),
    }
    budgets = load_budgets()["workbench"]
    maximum = budgets["maximum"]
    failures: list[str] = []
    require_maximum(
        "initial Workbench JavaScript bytes",
        observed["initialJavascriptBytes"],
        maximum["initialJavascriptBytes"],
        failures,
    )
    require_maximum(
        "optional Workbench preview asset bytes",
        observed["optionalPreviewAssetBytes"],
        maximum["optionalPreviewAssetBytes"],
        failures,
    )
    report = {
        "schemaVersion": 1,
        "scope": "workbench-startup",
        "observed": observed,
        "baseline": budgets["baseline"],
        "maximum": maximum,
        "budgetSource": str(BUDGET_PATH),
        "proof": str(proof_path),
        "optionalPreviewAssets": {
            "fileCount": len(preview_assets),
            "root": str(preview_root),
        },
        "failures": failures,
    }
    path = write_report("workbench", report)
    print(f"Workbench startup budget evidence: {path}")
    if failures:
        raise RuntimeError("; ".join(failures))


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "scope", choices=["framework", "cli-startup", "artifacts", "workbench"]
    )
    args = parser.parse_args()
    try:
        require_linux_x86_64()
        if args.scope == "framework":
            framework()
        elif args.scope == "cli-startup":
            cli_startup()
        elif args.scope == "artifacts":
            artifacts()
        else:
            workbench()
    except (OSError, KeyError, TypeError, ValueError, RuntimeError) as error:
        raise SystemExit(f"non-functional budget failed: {error}") from error


if __name__ == "__main__":
    main()
