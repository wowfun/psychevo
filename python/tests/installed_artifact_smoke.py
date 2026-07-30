from __future__ import annotations

import argparse
import asyncio
import hashlib
import json
import os
import platform
import shutil
import subprocess
import sys
import tempfile
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

ROOT = Path(__file__).parents[2]
PYTHON_ROOT = ROOT / "python"
VERSION = "0.1.0"
FINAL_ANSWER = "Installed artifact response"


class FakeProvider(BaseHTTPRequestHandler):
    request_count = 0
    request_bodies: list[dict[str, object]] = []

    def log_message(self, _format: str, *_args: object) -> None:
        return

    def do_POST(self) -> None:
        length = int(self.headers.get("content-length", "0"))
        body = self.rfile.read(length)
        type(self).request_count += 1
        type(self).request_bodies.append(json.loads(body))
        if self.path.rstrip("/") != "/v1/chat/completions":
            self.send_error(404)
            return
        payload = {
            "id": "package-smoke",
            "model": "default",
            "choices": [
                {
                    "index": 0,
                    "delta": {"content": FINAL_ANSWER},
                    "finish_reason": "stop",
                }
            ],
        }
        response = (
            f"data: {json.dumps(payload, separators=(',', ':'))}\n\n"
            "data: [DONE]\n\n"
        ).encode()
        self.send_response(200)
        self.send_header("content-type", "text/event-stream")
        self.send_header("content-length", str(len(response)))
        self.end_headers()
        self.wfile.write(response)
        self.wfile.flush()


def run(
    command: list[str],
    *,
    env: dict[str, str] | None = None,
    capture_output: bool = False,
) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        command,
        cwd=ROOT,
        env={**os.environ, **(env or {})},
        check=True,
        text=True,
        capture_output=capture_output,
    )


def newest(directory: Path, pattern: str) -> Path:
    matches = sorted(directory.glob(pattern), key=lambda path: path.stat().st_mtime_ns)
    if not matches:
        raise RuntimeError(f"no artifact matching {pattern} in {directory}")
    return matches[-1]


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for block in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


async def smoke_installed_client(
    python: Path,
    home: Path,
    database: Path,
    config: Path,
    cwd: Path,
) -> dict[str, object]:
    probe = (
        "import asyncio,json,sys\n"
        "from psychevo import Client\n"
        "async def main():\n"
        "  async with Client(executable_args=["
        f"'--home',{str(home)!r},'--database',{str(database)!r},"
        f"'--config',{str(config)!r}]) as client:\n"
        f"    thread=await client.start_thread(cwd={str(cwd)!r},source='package-smoke')\n"
        "    turn=await thread.start_turn('Reply with the fixture response.',"
        "client_turn_id='installed-artifact-smoke',no_agents=True,no_skills=True)\n"
        "    result=await asyncio.wait_for(turn.wait(),30)\n"
        "    snapshot=await thread.snapshot()\n"
        "    summaries=(await client.list_threads(cwd="
        f"{str(cwd)!r})).threads\n"
        "    assert result.outcome == 'completed', result\n"
        f"    assert result.final_answer == {FINAL_ANSWER!r}, result\n"
        "    assert snapshot.id == thread.id\n"
        "    assert [item.id for item in summaries] == [thread.id]\n"
        "    print(json.dumps({'threadId':thread.id,'turnId':turn.receipt.turn_id,"
        "'outcome':result.outcome,'finalAnswer':result.final_answer,"
        "'snapshotItems':len(snapshot.items),'listedThreads':len(summaries)}))\n"
        "asyncio.run(main())\n"
    )
    try:
        completed = await asyncio.to_thread(
            run,
            [str(python), "-c", probe],
            capture_output=True,
        )
    except subprocess.CalledProcessError as error:
        raise RuntimeError(
            "installed Python client smoke failed"
            f"\nstdout:\n{error.stdout or '<empty>'}"
            f"\nstderr:\n{error.stderr or '<empty>'}"
        ) from error
    return json.loads(completed.stdout.strip().splitlines()[-1])


def build_and_smoke(artifact_root: Path) -> dict[str, object]:
    uv = shutil.which("uv")
    if uv is None:
        raise RuntimeError("uv is required for installed artifact validation")
    artifact_root.mkdir(parents=True, exist_ok=True)
    wheels = artifact_root / "wheels"
    sdists = artifact_root / "sdists"
    wheels.mkdir(parents=True, exist_ok=True)
    sdists.mkdir(parents=True, exist_ok=True)

    default_tree = set(
        run(
            [
                "cargo",
                "tree",
                "-p",
                "psychevo-gateway",
                "-e",
                "normal",
                "--prefix",
                "none",
            ],
            capture_output=True,
        ).stdout.splitlines()
    )
    app_server_tree = set(
        run(
            [
                "cargo",
                "tree",
                "-p",
                "psychevo-gateway",
                "-e",
                "normal",
                "--prefix",
                "none",
                "--no-default-features",
            ],
            capture_output=True,
        ).stdout.splitlines()
    )
    if any(
        dependency.startswith(("feishu-sdk ", "qrcode "))
        for dependency in app_server_tree
    ):
        raise RuntimeError("no-default App Server dependency tree contains native Channels")

    started = time.monotonic()
    run([uv, "build", "--wheel", "--out-dir", str(wheels), str(PYTHON_ROOT / "psychevo")])
    run([uv, "build", "--sdist", "--out-dir", str(sdists), str(PYTHON_ROOT / "psychevo")])
    run(
        [
            uv,
            "build",
            "--wheel",
            "--out-dir",
            str(wheels),
            str(PYTHON_ROOT / "app-server-bin"),
        ]
    )
    pevo = (
        artifact_root.parent
        / "cli-target"
        / "release"
        / ("pevo.exe" if os.name == "nt" else "pevo")
    )
    workbench = ROOT / "apps" / "workbench" / "dist"
    run(
        [
            uv,
            "build",
            "--wheel",
            "--out-dir",
            str(wheels),
            str(PYTHON_ROOT / "cli-bin"),
        ],
        env={
            "PSYCHEVO_CLI_BINARY": str(pevo),
            "PSYCHEVO_WORKBENCH_DIST": str(workbench),
        },
    )
    build_seconds = time.monotonic() - started

    sdk_sdist = newest(sdists, f"psychevo-{VERSION}.tar.gz")
    app_wheel = newest(wheels, f"psychevo_app_server_bin-{VERSION}-*.whl")
    cli_wheel = newest(wheels, f"psychevo_cli_bin-{VERSION}-*.whl")
    with tempfile.TemporaryDirectory(prefix="psychevo-installed-artifact-") as temp_name:
        temp = Path(temp_name)
        environment = temp / "venv"
        run([uv, "venv", "--python", sys.executable, str(environment)])
        python = environment / ("Scripts/python.exe" if os.name == "nt" else "bin/python")
        installed_pevo = environment / (
            "Scripts/pevo.exe" if os.name == "nt" else "bin/pevo"
        )
        run(
            [
                uv,
                "pip",
                "install",
                "--python",
                str(python),
                "--find-links",
                str(wheels),
                str(sdk_sdist),
                str(cli_wheel),
            ]
        )
        freeze = run(
            [uv, "pip", "freeze", "--python", str(python)],
            capture_output=True,
        ).stdout.splitlines()
        import_probe = run(
            [
                str(python),
                "-c",
                (
                    "import psychevo,psychevo_app_server_bin,psychevo_cli_bin;"
                    "assert psychevo.__version__==psychevo_app_server_bin.__version__"
                    "==psychevo_cli_bin.__version__=='0.1.0';"
                    "print(psychevo_app_server_bin.executable())"
                ),
            ],
            capture_output=True,
        )
        installed_binary = Path(import_probe.stdout.strip().splitlines()[-1])
        cli_version = run(
            [str(installed_pevo), "--version"],
            capture_output=True,
        ).stdout.strip()
        if VERSION not in cli_version:
            raise RuntimeError(f"installed pevo reported unexpected version: {cli_version}")
        home = temp / "home"
        cwd = temp / "workspace"
        home.mkdir()
        cwd.mkdir()
        database = temp / "state.sqlite3"

        FakeProvider.request_count = 0
        FakeProvider.request_bodies = []
        server = ThreadingHTTPServer(("127.0.0.1", 0), FakeProvider)
        server_thread = threading.Thread(target=server.serve_forever, daemon=True)
        server_thread.start()
        config = temp / "config.toml"
        config.write_text(
            "model = \"package-smoke/default\"\n\n"
            "[provider.package-smoke]\n"
            f"api = \"http://127.0.0.1:{server.server_address[1]}/v1\"\n"
            "no_auth = true\n\n"
            "[provider.package-smoke.models.default]\n",
            encoding="utf-8",
        )
        try:
            smoke = asyncio.run(
                smoke_installed_client(python, home, database, config, cwd)
            )
        finally:
            server.shutdown()
            server.server_close()
            server_thread.join(timeout=5)

        if FakeProvider.request_count < 1:
            raise RuntimeError("installed App Server made no fake-provider request")
        package_artifacts = [sdk_sdist, app_wheel, cli_wheel]
        return {
            "schemaVersion": 1,
            "platform": platform.platform(),
            "python": platform.python_version(),
            "buildSeconds": round(build_seconds, 3),
            "gatewayDependencyTree": {
                "defaultUniqueLines": len(default_tree),
                "appServerUniqueLines": len(app_server_tree),
                "removedByNoDefault": sorted(default_tree - app_server_tree),
            },
            "installedBinaryBytes": installed_binary.stat().st_size,
            "dependencies": freeze,
            "providerRequests": FakeProvider.request_count,
            "cliVersion": cli_version,
            "smoke": smoke,
            "artifacts": [
                {
                    "path": str(path.relative_to(artifact_root)),
                    "bytes": path.stat().st_size,
                    "sha256": sha256(path),
                }
                for path in package_artifacts
            ],
        }


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--artifact-root",
        type=Path,
        default=Path(
            os.environ.get(
                "PSYCHEVO_CI_ARTIFACT_ROOT",
                ROOT / ".local" / "ci-artifacts" / "installed-package-smoke",
            )
        )
        / "package"
        / "python",
    )
    args = parser.parse_args()
    report = build_and_smoke(args.artifact_root.resolve())
    report_path = args.artifact_root / "installed-artifact-smoke.json"
    report_path.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
    print(f"installed artifact smoke: ok ({report_path})")


if __name__ == "__main__":
    main()
