from __future__ import annotations

import os
import shutil
import stat
import subprocess
import sys
import tarfile
import tempfile
import unittest
import zipfile
from pathlib import Path

ROOT = Path(__file__).parents[2]
PYTHON_ROOT = ROOT / "python"


class PackageTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name)
        self.wheels = self.root / "wheels"
        self.wheels.mkdir()
        self.uv = shutil.which("uv")
        if self.uv is None:
            self.fail("uv is required for deterministic PEP 517 build/install tests")
        suffix = ".exe" if os.name == "nt" else ""
        self.app_server = self.root / f"psychevo-app-server{suffix}"
        self.pevo = self.root / f"pevo{suffix}"
        self._write_executable(
            self.app_server,
            "#!/usr/bin/env python3\nprint('fake app server')\n",
        )
        self._write_executable(
            self.pevo,
            "#!/usr/bin/env python3\n"
            "import os\n"
            "print(os.environ.get('PSYCHEVO_WEB_DIST', ''))\n",
        )
        self.assets = self.root / "workbench"
        self.assets.mkdir()
        (self.assets / "index.html").write_text(
            "<!doctype html><title>Psychevo</title>", encoding="utf-8"
        )

    def tearDown(self) -> None:
        self.temp.cleanup()

    def _write_executable(self, path: Path, content: str) -> None:
        path.write_text(content, encoding="utf-8")
        path.chmod(path.stat().st_mode | stat.S_IXUSR)

    def _build_wheel(self, project: str, env: dict[str, str] | None = None) -> Path:
        before = set(self.wheels.glob("*.whl"))
        subprocess.run(
            [
                self.uv,
                "build",
                "--wheel",
                "--out-dir",
                str(self.wheels),
                str(PYTHON_ROOT / project),
            ],
            cwd=ROOT,
            env={**os.environ, **(env or {})},
            check=True,
            text=True,
            capture_output=True,
        )
        built = set(self.wheels.glob("*.whl")) - before
        self.assertEqual(len(built), 1, f"expected one new wheel for {project}: {built}")
        return built.pop()

    def _build_all(self) -> tuple[Path, Path, Path]:
        sdk = self._build_wheel("psychevo")
        app = self._build_wheel(
            "app-server-bin",
            {"PSYCHEVO_APP_SERVER_BINARY": str(self.app_server)},
        )
        cli = self._build_wheel(
            "cli-bin",
            {
                "PSYCHEVO_CLI_BINARY": str(self.pevo),
                "PSYCHEVO_WORKBENCH_DIST": str(self.assets),
            },
        )
        return sdk, app, cli

    def test_wheel_contract_and_exact_dependencies(self) -> None:
        sdk, app, cli = self._build_all()
        self.assertIn("py3-none-any", sdk.name)
        self.assertNotIn("none-any", app.name)
        self.assertNotIn("none-any", cli.name)
        with zipfile.ZipFile(sdk) as archive:
            metadata_name = next(
                name for name in archive.namelist() if name.endswith("/METADATA")
            )
            metadata = archive.read(metadata_name).decode()
            sdk_names = archive.namelist()
        self.assertIn("Requires-Dist: psychevo-app-server-bin==0.1.0", metadata)
        self.assertIn("Provides-Extra: cli", metadata)
        self.assertRegex(
            metadata,
            r"Requires-Dist: psychevo-cli-bin==0\.1\.0; extra == ['\"]cli['\"]",
        )
        self.assertNotIn("telemetry", metadata.lower())
        self.assertNotIn("Provides-Extra: all", metadata)
        self.assertIn("Description-Content-Type: text/markdown", metadata)
        self.assertIn("Project-URL: Documentation,", metadata)
        self.assertIn("# Psychevo Python SDK", metadata)
        self.assertIn("psychevo/py.typed", sdk_names)
        self.assertTrue(any(name.endswith("/licenses/LICENSE") for name in sdk_names))

        with zipfile.ZipFile(app) as archive:
            self.assertTrue(
                any(name.endswith("/licenses/LICENSE") for name in archive.namelist())
            )
            binary = next(
                info
                for info in archive.infolist()
                if "/bin/psychevo-app-server" in info.filename
            )
            self.assertEqual((binary.external_attr >> 16) & 0o111, 0o111)
        with zipfile.ZipFile(cli) as archive:
            self.assertTrue(
                any(name.endswith("/licenses/LICENSE") for name in archive.namelist())
            )
            self.assertIn(
                "psychevo_cli_bin/workbench/index.html", archive.namelist()
            )
            self.assertTrue(
                any(name.endswith("/entry_points.txt") for name in archive.namelist())
            )

    def test_sdk_sdist_rebuilds_and_binary_projects_reject_sdist(self) -> None:
        sdist_dir = self.root / "sdists"
        sdist_dir.mkdir()
        before = set(sdist_dir.glob("*.tar.gz"))
        subprocess.run(
            [
                self.uv,
                "build",
                "--sdist",
                "--out-dir",
                str(sdist_dir),
                str(PYTHON_ROOT / "psychevo"),
            ],
            cwd=ROOT,
            check=True,
            text=True,
            capture_output=True,
        )
        built = set(sdist_dir.glob("*.tar.gz")) - before
        self.assertEqual(len(built), 1)
        sdist = built.pop()
        with tarfile.open(sdist) as archive:
            names = archive.getnames()
        self.assertFalse(any(name.endswith("/build_backend.py") for name in names))
        self.assertTrue(any(name.endswith("/pyproject.toml") for name in names))
        self.assertTrue(any(name.endswith("/README.md") for name in names))
        self.assertTrue(any(name.endswith("/LICENSE") for name in names))
        self.assertTrue(any(name.endswith("/psychevo/py.typed") for name in names))

        for project in ("app-server-bin", "cli-bin"):
            rejected = subprocess.run(
                [
                    self.uv,
                    "build",
                    "--sdist",
                    "--out-dir",
                    str(sdist_dir),
                    str(PYTHON_ROOT / project),
                ],
                cwd=ROOT,
                text=True,
                capture_output=True,
            )
            self.assertNotEqual(rejected.returncode, 0)
            self.assertIn("wheel-only", rejected.stderr)

    def test_local_wheels_install_together_and_cli_uses_bundled_assets(self) -> None:
        self._build_all()
        environment = self.root / "venv"
        subprocess.run([self.uv, "venv", "--python", sys.executable, environment], check=True)
        python = environment / ("Scripts/python.exe" if os.name == "nt" else "bin/python")
        subprocess.run(
            [
                self.uv,
                "pip",
                "install",
                "--python",
                str(python),
                "--find-links",
                str(self.wheels),
                "psychevo[cli]==0.1.0",
            ],
            check=True,
            text=True,
            capture_output=True,
        )
        probe = subprocess.run(
            [
                str(python),
                "-c",
                (
                    "import psychevo,psychevo_app_server_bin,psychevo_cli_bin;"
                    "assert psychevo.__version__ == "
                    "psychevo_app_server_bin.__version__ == "
                    "psychevo_cli_bin.__version__ == '0.1.0';"
                    "print(psychevo_app_server_bin.executable());"
                    "print(psychevo_cli_bin.workbench_dist())"
                ),
            ],
            check=True,
            text=True,
            capture_output=True,
        )
        self.assertIn("psychevo-app-server", probe.stdout)
        self.assertIn("psychevo_cli_bin/workbench", probe.stdout.replace("\\", "/"))


if __name__ == "__main__":
    unittest.main()
