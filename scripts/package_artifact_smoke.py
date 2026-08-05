#!/usr/bin/env python3
"""Exercise the exact native release artifacts produced by the package profile."""

from __future__ import annotations

from dataclasses import dataclass
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import time
from typing import Any
from urllib.request import urlopen


ROOT = Path(__file__).resolve().parents[1]
CLI_TIMEOUT_SECONDS = 90
DESKTOP_TIMEOUT_SECONDS = 90
PROCESS_STOP_TIMEOUT_SECONDS = 10
MAX_DESKTOP_LOG_BYTES = 1024 * 1024
KNOWN_BUNDLE_SUFFIXES = {".appimage", ".deb", ".dmg", ".msi", ".rpm", ".exe"}
STARTUP_TRACE_IDS = (
    "process_start",
    "window_ready",
    "managed_gateway_ready",
    "bridge_connected",
)


@dataclass(frozen=True)
class BundleArtifact:
    path: Path
    launch_path: Path | None


def relative_path(path: Path, package_root: Path) -> str:
    return path.resolve().relative_to(package_root.resolve()).as_posix()


def desktop_executable(desktop_target: Path, platform_name: str) -> Path:
    suffix = ".exe" if platform_name == "win32" else ""
    return desktop_target / "release" / f"psychevo-desktop{suffix}"


def bundle_artifacts(bundle_root: Path) -> list[BundleArtifact]:
    artifacts: list[BundleArtifact] = []
    app_roots = sorted(path for path in bundle_root.rglob("*.app") if path.is_dir())
    for app_root in app_roots:
        executables = sorted(
            path
            for path in (app_root / "Contents" / "MacOS").iterdir()
            if path.is_file()
        ) if (app_root / "Contents" / "MacOS").is_dir() else []
        if len(executables) != 1:
            raise RuntimeError(
                f"Desktop app bundle must contain one executable, found {len(executables)}: "
                f"{app_root}"
            )
        artifacts.append(BundleArtifact(app_root, executables[0]))

    for path in sorted(bundle_root.rglob("*")):
        if not path.is_file():
            continue
        if any(app_root in path.parents for app_root in app_roots):
            continue
        if path.suffix.lower() in KNOWN_BUNDLE_SUFFIXES:
            launch_path = path if path.suffix.lower() == ".appimage" else None
            artifacts.append(BundleArtifact(path, launch_path))
    return artifacts


def launchable_bundle(
    artifacts: list[BundleArtifact], platform_name: str
) -> BundleArtifact | None:
    if platform_name == "linux":
        candidates = [
            artifact
            for artifact in artifacts
            if artifact.path.suffix.lower() == ".appimage"
        ]
    elif platform_name == "darwin":
        candidates = [artifact for artifact in artifacts if artifact.path.suffix == ".app"]
    else:
        return None
    if len(candidates) != 1:
        raise RuntimeError(
            f"expected one launchable {platform_name} Desktop bundle, found "
            f"{len(candidates)}"
        )
    return candidates[0]


def run_json(
    command: list[str], environment: dict[str, str], *, timeout_seconds: int
) -> dict[str, Any]:
    try:
        completed = subprocess.run(
            command,
            cwd=ROOT,
            env=environment,
            capture_output=True,
            text=True,
            timeout=timeout_seconds,
            check=False,
        )
    except FileNotFoundError as error:
        raise RuntimeError(f"required executable not found: {command[0]}") from error
    except subprocess.TimeoutExpired as error:
        raise RuntimeError(
            f"native artifact command timed out after {timeout_seconds} seconds: "
            f"{' '.join(command)}"
        ) from error
    if completed.returncode != 0:
        details = "\n".join(
            part.strip() for part in (completed.stdout, completed.stderr) if part.strip()
        )
        raise RuntimeError(
            f"{' '.join(command)} exited with status {completed.returncode}: {details}"
        )
    for line in reversed(completed.stdout.splitlines()):
        try:
            value = json.loads(line)
        except json.JSONDecodeError:
            continue
        if isinstance(value, dict):
            return value
    raise RuntimeError(f"{' '.join(command)} did not emit a JSON object")


def isolated_environment(
    run_root: Path, cli: Path, *, trace_root: Path | None = None
) -> dict[str, str]:
    home = run_root / "home"
    home.mkdir(parents=True, exist_ok=True)
    config = run_root / "config.toml"
    config.write_text('model = "lmstudio/noop"\n', encoding="utf-8")
    workbench_dist = ROOT / "apps" / "workbench" / "dist"
    if not (workbench_dist / "index.html").is_file():
        raise RuntimeError(f"built Workbench is missing: {workbench_dist}")
    environment = dict(os.environ)
    environment.update(
        {
            "PSYCHEVO_CHANNEL_RUNTIME": "off",
            "PSYCHEVO_CONFIG": str(config),
            "PSYCHEVO_DB": str(run_root / "state.db"),
            "PSYCHEVO_DESKTOP_STARTUP_TRACE_ROOT": str(
                trace_root if trace_root is not None else run_root / "startup-trace"
            ),
            "PSYCHEVO_HOME": str(home),
            "PSYCHEVO_PEVO_BIN": str(cli.resolve()),
            "PSYCHEVO_WEB_DIST": str(workbench_dist.resolve()),
        }
    )
    return environment


def ready_handshake(value: dict[str, Any], cli: Path) -> dict[str, Any]:
    if value.get("ok") is not True or value.get("running") is not True:
        raise RuntimeError(f"managed Gateway did not report running: {value}")
    readyz_url = value.get("readyzUrl")
    if not isinstance(readyz_url, str) or not readyz_url.startswith("http://127.0.0.1:"):
        raise RuntimeError(f"managed Gateway returned an invalid readyz URL: {readyz_url!r}")
    executable_path = value.get("executablePath")
    if not isinstance(executable_path, str):
        raise RuntimeError("managed Gateway status omitted executablePath")
    if Path(executable_path).resolve() != cli.resolve():
        raise RuntimeError(
            f"managed Gateway used {executable_path}, expected release CLI {cli.resolve()}"
        )
    with urlopen(readyz_url, timeout=5) as response:  # noqa: S310 - loopback only
        if response.status != 200:
            raise RuntimeError(f"managed Gateway readyz returned HTTP {response.status}")
        response.read(64 * 1024)
    return {
        "baseUrl": value.get("baseUrl"),
        "pid": value.get("pid"),
        "readyzUrl": readyz_url,
    }


def stop_gateway(cli: Path, environment: dict[str, str]) -> None:
    command = [str(cli), "gateway", "stop"]
    try:
        completed = subprocess.run(
            command,
            cwd=ROOT,
            env=environment,
            capture_output=True,
            text=True,
            timeout=PROCESS_STOP_TIMEOUT_SECONDS,
            check=False,
        )
    except subprocess.TimeoutExpired as error:
        raise RuntimeError(
            f"native artifact cleanup timed out after {PROCESS_STOP_TIMEOUT_SECONDS} "
            f"seconds: {' '.join(command)}"
        ) from error
    if completed.returncode != 0:
        details = "\n".join(
            part.strip() for part in (completed.stdout, completed.stderr) if part.strip()
        )
        raise RuntimeError(
            f"native artifact cleanup exited with status {completed.returncode}: {details}"
        )


def smoke_cli(cli: Path) -> dict[str, Any]:
    with tempfile.TemporaryDirectory(prefix="psychevo-package-cli-") as raw:
        environment = isolated_environment(Path(raw), cli)
        try:
            version = subprocess.run(
                [str(cli), "--version"],
                cwd=ROOT,
                env=environment,
                capture_output=True,
                text=True,
                timeout=PROCESS_STOP_TIMEOUT_SECONDS,
                check=False,
            )
            if version.returncode != 0 or not version.stdout.strip():
                raise RuntimeError(
                    f"release CLI --version failed with status {version.returncode}: "
                    f"{version.stderr.strip()}"
                )
            started = run_json(
                [str(cli), "gateway", "start", "--bind", "127.0.0.1:0"],
                environment,
                timeout_seconds=CLI_TIMEOUT_SECONDS,
            )
            if started.get("ok") is not True or started.get("running") is not True:
                raise RuntimeError(f"release CLI failed to start managed Gateway: {started}")
            status = run_json(
                [str(cli), "gateway", "status"],
                environment,
                timeout_seconds=PROCESS_STOP_TIMEOUT_SECONDS,
            )
            return {
                "version": version.stdout.strip(),
                "handshake": ready_handshake(status, cli),
            }
        finally:
            stop_gateway(cli, environment)


def desktop_command(executable: Path, platform_name: str) -> list[str]:
    if platform_name == "linux" and not os.environ.get("DISPLAY"):
        xvfb = shutil.which("xvfb-run")
        if not xvfb:
            raise RuntimeError(
                "Linux Desktop artifact smoke requires DISPLAY or xvfb-run"
            )
        return [xvfb, "-a", str(executable)]
    return [str(executable)]


def read_complete_startup_trace(path: Path) -> dict[str, Any]:
    if not path.is_file():
        raise RuntimeError(f"Desktop startup trace is not available: {path}")
    records: list[dict[str, Any]] = []
    for line in path.read_text(encoding="utf-8").splitlines():
        if not line.strip():
            continue
        try:
            record = json.loads(line)
        except json.JSONDecodeError as error:
            raise RuntimeError(f"Desktop startup trace contains invalid JSON: {path}") from error
        if not isinstance(record, dict):
            raise RuntimeError(f"Desktop startup trace contains a non-object record: {path}")
        records.append(record)
    expected_fields = {
        "epochMs",
        "id",
        "monotonicOffsetMs",
        "pid",
        "schemaVersion",
        "sequence",
        "sourceClock",
    }
    for record in records:
        if set(record) != expected_fields:
            raise RuntimeError(
                f"Desktop startup trace contains unsupported fields: {sorted(record)}"
            )
        if record.get("id") not in STARTUP_TRACE_IDS:
            raise RuntimeError(
                f"Desktop startup trace contains unsupported milestone: {record.get('id')!r}"
            )
    by_id = {record["id"]: record for record in records}
    if len(by_id) != len(records):
        raise RuntimeError("Desktop startup trace contains duplicate milestone ids")
    missing = [milestone for milestone in STARTUP_TRACE_IDS if milestone not in by_id]
    if missing:
        raise RuntimeError(f"Desktop startup trace is missing: {', '.join(missing)}")
    ordered = [by_id[milestone] for milestone in STARTUP_TRACE_IDS]
    for record in ordered:
        if (
            record.get("schemaVersion") != 1
            or record.get("sourceClock") != "desktop-rust-monotonic"
            or type(record.get("sequence")) is not int
            or type(record.get("pid")) is not int
            or type(record.get("epochMs")) is not int
            or not isinstance(record.get("monotonicOffsetMs"), int | float)
            or record["sequence"] <= 0
            or record["pid"] <= 0
            or record["epochMs"] <= 0
            or record["monotonicOffsetMs"] < 0
        ):
            raise RuntimeError(
                f"Desktop startup trace milestone {record.get('id')!r} is incomplete"
            )
    sequences = [record["sequence"] for record in ordered]
    offsets = [record["monotonicOffsetMs"] for record in ordered]
    pids = {record["pid"] for record in ordered}
    if sequences != list(range(1, len(STARTUP_TRACE_IDS) + 1)):
        raise RuntimeError(f"Desktop startup trace is out of order: {sequences}")
    if offsets != sorted(offsets):
        raise RuntimeError(f"Desktop startup trace clock regressed: {offsets}")
    if len(pids) != 1:
        raise RuntimeError(f"Desktop startup trace spans multiple processes: {sorted(pids)}")
    return {
        "milestones": list(STARTUP_TRACE_IDS),
        "pid": next(iter(pids)),
        "trace": str(path),
    }


def wait_for_desktop_handshake(
    process: subprocess.Popen[Any],
    cli: Path,
    environment: dict[str, str],
    log_path: Path,
) -> dict[str, Any]:
    deadline = time.monotonic() + DESKTOP_TIMEOUT_SECONDS
    last_error = "managed Gateway state was not available"
    ready: dict[str, Any] | None = None
    trace_path = environment_path(
        environment, "PSYCHEVO_DESKTOP_STARTUP_TRACE_ROOT"
    ) / "desktop-startup-rust.jsonl"
    while time.monotonic() < deadline:
        return_code = process.poll()
        if return_code is not None:
            log_tail = log_path.read_text(encoding="utf-8", errors="replace")[-4_000:]
            raise RuntimeError(
                f"Desktop artifact exited before its Gateway handshake with status "
                f"{return_code}: {log_tail}"
            )
        try:
            if ready is None:
                status = run_json(
                    [str(cli), "gateway", "status"],
                    environment,
                    timeout_seconds=PROCESS_STOP_TIMEOUT_SECONDS,
                )
                if status.get("running") is not True:
                    raise RuntimeError(f"Gateway status was not running: {status}")
                ready = ready_handshake(status, cli)
            trace = read_complete_startup_trace(trace_path)
            if process.poll() is not None:
                raise RuntimeError("Desktop artifact exited during its Gateway handshake")
            return {"gateway": ready, "startupTrace": trace}
        except RuntimeError as error:
            last_error = str(error)
        time.sleep(0.5)
    raise RuntimeError(
        f"Desktop artifact did not complete its managed-Gateway handshake within "
        f"{DESKTOP_TIMEOUT_SECONDS} seconds: {last_error}"
    )


def stop_process(process: subprocess.Popen[Any]) -> None:
    if process.poll() is not None:
        return
    process.terminate()
    try:
        process.wait(timeout=PROCESS_STOP_TIMEOUT_SECONDS)
    except subprocess.TimeoutExpired:
        process.kill()
        process.wait(timeout=PROCESS_STOP_TIMEOUT_SECONDS)


def environment_path(environment: dict[str, str], name: str) -> Path:
    value = environment.get(name)
    if not value:
        raise RuntimeError(f"native artifact environment omitted {name}")
    return Path(value)


def smoke_desktop(
    executable: Path,
    cli: Path,
    run_root: Path,
    platform_name: str,
) -> dict[str, Any]:
    with tempfile.TemporaryDirectory(prefix="psychevo-package-desktop-") as raw:
        environment = isolated_environment(
            Path(raw), cli, trace_root=run_root / "startup-trace"
        )
        return smoke_desktop_with_environment(
            executable, cli, run_root, platform_name, environment
        )


def smoke_desktop_with_environment(
    executable: Path,
    cli: Path,
    run_root: Path,
    platform_name: str,
    environment: dict[str, str],
) -> dict[str, Any]:
    if platform_name == "linux" and executable.suffix.lower() == ".appimage":
        environment["APPIMAGE_EXTRACT_AND_RUN"] = "1"
    log_path = run_root / "desktop.log"
    log_path.parent.mkdir(parents=True, exist_ok=True)
    command = desktop_command(executable, platform_name)
    try:
        with log_path.open("w", encoding="utf-8") as log:
            try:
                process = subprocess.Popen(
                    command,
                    cwd=ROOT,
                    env=environment,
                    stdout=log,
                    stderr=subprocess.STDOUT,
                )
            except FileNotFoundError as error:
                raise RuntimeError(f"required executable not found: {command[0]}") from error
            try:
                handshake = wait_for_desktop_handshake(
                    process, cli, environment, log_path
                )
            finally:
                stop_process(process)
                stop_gateway(cli, environment)
    finally:
        retain_bounded_log(log_path)
    return {
        "command": command,
        "handshake": handshake,
        "log": str(log_path),
    }


def retain_bounded_log(path: Path) -> None:
    if not path.is_file():
        return
    content = path.read_bytes()
    if len(content) > MAX_DESKTOP_LOG_BYTES:
        path.write_bytes(content[-MAX_DESKTOP_LOG_BYTES:])


def evidence_entry(
    artifact: Path,
    package_root: Path,
    *,
    status: str,
    validation: str,
    detail: dict[str, Any] | None = None,
    reason: str | None = None,
) -> dict[str, Any]:
    entry: dict[str, Any] = {
        "path": relative_path(artifact, package_root),
        "status": status,
        "validation": validation,
    }
    if detail is not None:
        entry["detail"] = detail
    if reason is not None:
        entry["reason"] = reason
    return entry


def execute(package_root: Path, platform_name: str) -> dict[str, Any]:
    cli = package_root / "cli-target" / "release" / (
        "pevo.exe" if platform_name == "win32" else "pevo"
    )
    desktop_target = package_root / "desktop-target"
    raw_desktop = desktop_executable(desktop_target, platform_name)
    bundle_root = desktop_target / "release" / "bundle"
    for required in (cli, raw_desktop, bundle_root):
        if not required.exists():
            raise RuntimeError(f"native package artifact is missing: {required}")

    run_root = package_root / "native-artifact-smoke"
    if run_root.exists():
        shutil.rmtree(run_root)
    run_root.mkdir(parents=True)
    report: dict[str, Any] = {
        "schemaVersion": 1,
        "platform": platform_name,
        "status": "running",
        "artifacts": [],
    }
    write_report(package_root, report)

    def record(entry: dict[str, Any]) -> None:
        report["artifacts"].append(entry)
        write_report(package_root, report)

    record(
        evidence_entry(
            cli,
            package_root,
            status="passed",
            validation="version-and-managed-gateway-ready-handshake",
            detail=smoke_cli(cli),
        )
    )

    if platform_name in {"linux", "win32"}:
        record(
            evidence_entry(
                raw_desktop,
                package_root,
                status="passed",
                validation="same-process-ordered-startup-and-workbench-bridge-handshake",
                detail=smoke_desktop(
                    raw_desktop,
                    cli,
                    run_root / "desktop-executable",
                    platform_name,
                ),
            )
        )
    else:
        record(
            evidence_entry(
                raw_desktop,
                package_root,
                status="build-only",
                validation="none",
                reason="macOS native runtime is launched from its signed-layout app bundle",
            )
        )

    bundles = bundle_artifacts(bundle_root)
    if not bundles:
        raise RuntimeError(f"no Desktop bundle artifacts found under {bundle_root}")
    selected_bundle = launchable_bundle(bundles, platform_name)
    for bundle in bundles:
        if selected_bundle is not None and bundle.path == selected_bundle.path:
            if bundle.launch_path is None:
                raise RuntimeError(f"launchable bundle has no executable: {bundle.path}")
            record(
                evidence_entry(
                    bundle.path,
                    package_root,
                    status="passed",
                    validation="same-process-ordered-startup-and-workbench-bridge-handshake",
                    detail=smoke_desktop(
                        bundle.launch_path,
                        cli,
                        run_root / "desktop-bundle",
                        platform_name,
                    ),
                )
            )
        else:
            record(
                evidence_entry(
                    bundle.path,
                    package_root,
                    status="build-only",
                    validation="none",
                    reason=(
                        "installer/container installation requires host mutation; "
                        "this unprivileged job verifies its build and checksum only"
                    ),
                )
            )
    report["status"] = "passed"
    return report


def write_report(package_root: Path, report: dict[str, Any]) -> Path:
    output = package_root / "native-artifact-smoke.json"
    output.write_text(
        json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8"
    )
    return output


def failure_report(package_root: Path, platform_name: str, error: Exception) -> dict[str, Any]:
    output = package_root / "native-artifact-smoke.json"
    report: dict[str, Any] = {}
    if output.is_file():
        try:
            value = json.loads(output.read_text(encoding="utf-8"))
            if isinstance(value, dict):
                report = value
        except (OSError, json.JSONDecodeError):
            pass
    report.update(
        {
            "schemaVersion": 1,
            "platform": platform_name,
            "status": "failed",
            "error": str(error),
        }
    )
    report.setdefault("artifacts", [])
    return report


def main() -> int:
    raw_root = os.environ.get("PSYCHEVO_CI_ARTIFACT_ROOT")
    if not raw_root:
        print(
            "native artifact smoke failed: PSYCHEVO_CI_ARTIFACT_ROOT is required",
            file=sys.stderr,
        )
        return 1
    package_root = Path(raw_root).resolve() / "package"
    package_root.mkdir(parents=True, exist_ok=True)
    try:
        report = execute(package_root, sys.platform)
    except (OSError, RuntimeError, ValueError, subprocess.SubprocessError) as error:
        report = failure_report(package_root, sys.platform, error)
        write_report(package_root, report)
        print(f"native artifact smoke failed: {error}", file=sys.stderr)
        return 1
    output = write_report(package_root, report)
    print(f"native artifact smoke passed: {output}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
