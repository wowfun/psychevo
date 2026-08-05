from __future__ import annotations

import os
import subprocess
import sysconfig
import tomllib
from pathlib import Path
from typing import NamedTuple, cast
from zipfile import ZipInfo

from wheel.wheelfile import WheelFile


class _BinaryProject(NamedTuple):
    module: str
    binary_environment: str
    executable: str
    build_command: tuple[str, ...]
    assets_environment: str | None = None
    assets_path: tuple[str, ...] = ()
    assets_build_command: tuple[str, ...] = ()
    console_script: str | None = None


_PROJECTS = {
    "app-server": _BinaryProject(
        module="psychevo_app_server_bin",
        binary_environment="PSYCHEVO_APP_SERVER_BINARY",
        executable="psychevo-app-server",
        build_command=(
            "cargo",
            "build",
            "--locked",
            "--release",
            "-p",
            "psychevo-gateway",
            "--bin",
            "psychevo-app-server",
            "--no-default-features",
        ),
    ),
    "cli": _BinaryProject(
        module="psychevo_cli_bin",
        binary_environment="PSYCHEVO_CLI_BINARY",
        executable="pevo",
        build_command=(
            "cargo",
            "build",
            "--locked",
            "--release",
            "-p",
            "psychevo-cli",
            "--bin",
            "pevo",
        ),
        assets_environment="PSYCHEVO_WORKBENCH_DIST",
        assets_path=("apps", "workbench", "dist"),
        assets_build_command=("pnpm", "--filter", "@psychevo/workbench", "build"),
        console_script="pevo=psychevo_cli_bin:main",
    ),
}


class BinaryWheelBackend:
    def __init__(self, root: Path) -> None:
        self.root = root
        self.repository = root.parents[1]
        document = tomllib.loads((root / "pyproject.toml").read_text(encoding="utf-8"))
        self.project = cast(dict[str, object], document["project"])
        tool = cast(dict[str, object], document["tool"])
        settings = cast(dict[str, object], tool["psychevo-build"])
        kind = str(settings["kind"])
        try:
            self.binary_project = _PROJECTS[kind]
        except KeyError as error:
            raise RuntimeError(f"unsupported Psychevo binary project kind: {kind}") from error
        self.name = str(self.project["name"])
        self.version = str(self.project["version"])

    def get_requires_for_build_wheel(self, config_settings=None) -> list[str]:
        return []

    def get_requires_for_build_sdist(self, config_settings=None) -> list[str]:
        raise RuntimeError(f"{self.name} is wheel-only")

    def prepare_metadata_for_build_wheel(
        self, metadata_directory: str, config_settings=None
    ) -> str:
        dist_info = self._dist_info()
        target = Path(metadata_directory) / dist_info
        target.mkdir(parents=True, exist_ok=True)
        (target / "METADATA").write_text(self._metadata(), encoding="utf-8")
        (target / "WHEEL").write_text(self._wheel_metadata(), encoding="utf-8")
        if self.binary_project.console_script is not None:
            (target / "entry_points.txt").write_text(
                self._entry_points(), encoding="utf-8"
            )
        return dist_info

    def build_wheel(
        self, wheel_directory: str, config_settings=None, metadata_directory=None
    ) -> str:
        tag = self._platform_tag()
        filename = f"{self.name.replace('-', '_')}-{self.version}-py3-none-{tag}.whl"
        target = Path(wheel_directory) / filename
        dist_info = self._dist_info()
        source_binary = self._binary()
        executable_name = self.binary_project.executable + (
            ".exe" if os.name == "nt" else ""
        )
        executable_path = f"{self.binary_project.module}/bin/{executable_name}"
        entries = {
            f"{self.binary_project.module}/__init__.py": (
                self.root / "src" / self.binary_project.module / "__init__.py"
            ).read_bytes(),
            executable_path: source_binary.read_bytes(),
            f"{dist_info}/METADATA": self._metadata().encode(),
            f"{dist_info}/licenses/LICENSE": (
                self.repository / "LICENSE"
            ).read_bytes(),
            f"{dist_info}/WHEEL": self._wheel_metadata().encode(),
        }
        if self.binary_project.console_script is not None:
            entries[f"{dist_info}/entry_points.txt"] = self._entry_points().encode()
        assets = self._assets()
        if assets is not None:
            for source in sorted(assets.rglob("*")):
                if source.is_file():
                    entries[
                        f"{self.binary_project.module}/workbench/"
                        f"{source.relative_to(assets).as_posix()}"
                    ] = source.read_bytes()
        self._write_wheel(target, entries, executable_paths={executable_path})
        return filename

    def build_sdist(self, sdist_directory: str, config_settings=None) -> str:
        raise RuntimeError(f"{self.name} is wheel-only")

    def _platform_tag(self) -> str:
        return sysconfig.get_platform().replace("-", "_").replace(".", "_")

    def _dist_info(self) -> str:
        return f"{self.name.replace('-', '_')}-{self.version}.dist-info"

    def _metadata(self) -> str:
        lines = [
            "Metadata-Version: 2.3",
            f"Name: {self.name}",
            f"Version: {self.version}",
            f"Summary: {self.project['description']}",
            f"Requires-Python: {self.project['requires-python']}",
            "License-Expression: MIT",
            "License-File: LICENSE",
            "Description-Content-Type: text/markdown",
        ]
        for classifier in cast(list[object], self.project.get("classifiers", [])):
            lines.append(f"Classifier: {classifier}")
        for label, url in cast(dict[str, object], self.project.get("urls", {})).items():
            lines.append(f"Project-URL: {label}, {url}")
        readme = (self.root / str(self.project["readme"])).read_text(encoding="utf-8")
        return "\n".join(lines) + "\n\n" + readme.rstrip() + "\n"

    def _binary(self) -> Path:
        override = os.environ.get(self.binary_project.binary_environment)
        suffix = ".exe" if os.name == "nt" else ""
        binary = (
            Path(override)
            if override
            else self.repository
            / "target"
            / "release"
            / f"{self.binary_project.executable}{suffix}"
        )
        if not override:
            subprocess.run(
                self.binary_project.build_command,
                cwd=self.repository,
                check=True,
            )
        if not binary.is_file():
            raise RuntimeError(f"Psychevo binary is missing: {binary}")
        return binary

    def _assets(self) -> Path | None:
        environment = self.binary_project.assets_environment
        if environment is None:
            return None
        override = os.environ.get(environment)
        assets = (
            Path(override)
            if override
            else self.repository.joinpath(*self.binary_project.assets_path)
        )
        if not override:
            subprocess.run(
                self.binary_project.assets_build_command,
                cwd=self.repository,
                check=True,
            )
        if not (assets / "index.html").is_file():
            raise RuntimeError(f"Workbench distribution is missing: {assets}")
        return assets

    def _entry_points(self) -> str:
        return f"[console_scripts]\n{self.binary_project.console_script}\n"

    def _wheel_metadata(self) -> str:
        return (
            "Wheel-Version: 1.0\n"
            "Generator: psychevo-build-backend 0.1\n"
            "Root-Is-Purelib: false\n"
            f"Tag: py3-none-{self._platform_tag()}\n"
        )

    def _write_wheel(
        self,
        target: Path,
        entries: dict[str, bytes],
        *,
        executable_paths: set[str],
    ) -> None:
        with WheelFile(target, "w") as wheel:
            for path, content in sorted(entries.items()):
                info = ZipInfo(path)
                info.external_attr = (
                    (0o755 if path in executable_paths else 0o644) & 0xFFFF
                ) << 16
                wheel.writestr(info, content)


def backend_for(root: Path) -> BinaryWheelBackend:
    return BinaryWheelBackend(root)
