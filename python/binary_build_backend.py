from __future__ import annotations

import base64
import csv
import hashlib
import io
import json
import os
import stat
import subprocess
import sysconfig
import tomllib
from pathlib import Path
from typing import NamedTuple, cast
from zipfile import ZIP_DEFLATED, ZipFile, ZipInfo

from wheel.wheelfile import WheelFile


_RELEASE_ASSET_MANIFEST = "psychevo-release-assets.json"
_WHEEL_TIMESTAMP = (1980, 1, 1, 0, 0, 0)
_COPY_BUFFER_BYTES = 1024 * 1024
_DUPLICATE_PAYLOAD_MIN_BYTES = 64 * 1024


class _DeterministicWheelFile(WheelFile):
    def __init__(self, target: Path) -> None:
        super().__init__(target, "w", compression=ZIP_DEFLATED)
        self.compresslevel = 9

    @staticmethod
    def _info(path: str, mode: int) -> ZipInfo:
        info = ZipInfo(path, date_time=_WHEEL_TIMESTAMP)
        info.create_system = 3
        info.compress_type = ZIP_DEFLATED
        info.external_attr = (stat.S_IFREG | mode) << 16
        return info

    def _record(self, path: str, digest: bytes, size: int) -> None:
        self._file_hashes[path] = (
            "sha256",
            base64.urlsafe_b64encode(digest).decode("ascii"),
        )
        self._file_sizes[path] = size

    def add_bytes(self, path: str, content: bytes, mode: int = 0o644) -> None:
        info = self._info(path, mode)
        ZipFile.writestr(self, info, content, compress_type=ZIP_DEFLATED)
        self._record(path, hashlib.sha256(content).digest(), len(content))

    def add_file(self, path: str, source: Path, mode: int = 0o644) -> None:
        info = self._info(path, mode)
        digest = hashlib.sha256()
        size = 0
        with source.open("rb") as reader, ZipFile.open(
            self, info, "w", force_zip64=True
        ) as writer:
            while chunk := reader.read(_COPY_BUFFER_BYTES):
                writer.write(chunk)
                digest.update(chunk)
                size += len(chunk)
        self._record(path, digest.digest(), size)

    def close(self) -> None:
        if self.fp is not None and self.mode == "w" and self._file_hashes:
            record = io.StringIO()
            writer = csv.writer(record, lineterminator="\n")
            for path in sorted(self._file_hashes):
                algorithm, digest = self._file_hashes[path]
                writer.writerow(
                    (path, f"{algorithm}={digest}", self._file_sizes[path])
                )
            writer.writerow((self.record_path, "", ""))
            info = self._info(self.record_path, 0o644)
            ZipFile.writestr(
                self,
                info,
                record.getvalue().encode("utf-8"),
                compress_type=ZIP_DEFLATED,
            )
        ZipFile.close(self)


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
        byte_entries = {
            f"{self.binary_project.module}/__init__.py": (
                self.root / "src" / self.binary_project.module / "__init__.py"
            ).read_bytes(),
            f"{dist_info}/METADATA": self._metadata().encode(),
            f"{dist_info}/licenses/LICENSE": (
                self.repository / "LICENSE"
            ).read_bytes(),
            f"{dist_info}/WHEEL": self._wheel_metadata().encode(),
        }
        if self.binary_project.console_script is not None:
            byte_entries[f"{dist_info}/entry_points.txt"] = self._entry_points().encode()
        file_entries = {executable_path: source_binary}
        assets = self._assets()
        if assets is not None:
            for relative_path, source in self._release_assets(assets):
                file_entries[
                    f"{self.binary_project.module}/workbench/{relative_path}"
                ] = source
        self._write_wheel(
            target,
            byte_entries,
            file_entries,
            executable_paths={executable_path},
        )
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

    def _release_assets(self, assets: Path) -> list[tuple[str, Path]]:
        manifest_path = assets / _RELEASE_ASSET_MANIFEST
        if not manifest_path.is_file():
            raise RuntimeError(f"Workbench release manifest is missing: {manifest_path}")
        try:
            manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
        except (json.JSONDecodeError, OSError) as error:
            raise RuntimeError(f"Workbench release manifest is invalid: {error}") from error
        if manifest.get("schemaVersion") != 1 or not isinstance(manifest.get("files"), list):
            raise RuntimeError("Workbench release manifest must use schema version 1")

        selected: list[tuple[str, Path]] = []
        seen: set[str] = set()
        digests: dict[tuple[str, int], str] = {}
        for entry in manifest["files"]:
            if not isinstance(entry, dict):
                raise RuntimeError("Workbench release manifest file entries must be objects")
            relative_path = entry.get("path")
            expected_digest = entry.get("sha256")
            expected_size = entry.get("size")
            if (
                not isinstance(relative_path, str)
                or not isinstance(expected_digest, str)
                or not isinstance(expected_size, int)
            ):
                raise RuntimeError("Workbench release manifest file entry is malformed")
            parts = Path(relative_path).parts
            if (
                not relative_path
                or relative_path.startswith(("/", "\\"))
                or ".." in parts
                or "\\" in relative_path
                or relative_path in seen
                or relative_path.endswith(".map")
            ):
                raise RuntimeError(f"Unsafe Workbench release asset path: {relative_path}")
            source = assets.joinpath(*relative_path.split("/"))
            if not source.is_file() or source.is_symlink():
                raise RuntimeError(f"Workbench release asset is missing: {relative_path}")
            actual_size, actual_digest = self._hash_file(source)
            if actual_size != expected_size or actual_digest != expected_digest:
                raise RuntimeError(
                    f"Workbench release asset does not match its manifest: {relative_path}"
                )
            duplicate_key = (actual_digest, actual_size)
            if actual_size >= _DUPLICATE_PAYLOAD_MIN_BYTES:
                if original := digests.get(duplicate_key):
                    raise RuntimeError(
                        "Workbench release contains duplicate payloads: "
                        f"{original} and {relative_path}"
                    )
                digests[duplicate_key] = relative_path
            seen.add(relative_path)
            selected.append((relative_path, source))

        actual = {
            source.relative_to(assets).as_posix()
            for source in assets.rglob("*")
            if source.is_file()
            and source.name != _RELEASE_ASSET_MANIFEST
            and not source.name.endswith(".map")
        }
        if actual != seen:
            missing = sorted(seen - actual)
            extra = sorted(actual - seen)
            raise RuntimeError(
                f"Workbench release manifest mismatch; missing={missing}, unmanifested={extra}"
            )
        return sorted(selected)

    @staticmethod
    def _hash_file(source: Path) -> tuple[int, str]:
        digest = hashlib.sha256()
        size = 0
        with source.open("rb") as reader:
            while chunk := reader.read(_COPY_BUFFER_BYTES):
                digest.update(chunk)
                size += len(chunk)
        return size, digest.hexdigest()

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
        byte_entries: dict[str, bytes],
        file_entries: dict[str, Path],
        *,
        executable_paths: set[str],
    ) -> None:
        collisions = byte_entries.keys() & file_entries.keys()
        if collisions:
            raise RuntimeError(f"duplicate wheel entries: {sorted(collisions)}")
        with _DeterministicWheelFile(target) as wheel:
            for path in sorted((*byte_entries.keys(), *file_entries.keys())):
                mode = 0o755 if path in executable_paths else 0o644
                if content := byte_entries.get(path):
                    wheel.add_bytes(path, content, mode)
                elif path in byte_entries:
                    wheel.add_bytes(path, b"", mode)
                else:
                    wheel.add_file(path, file_entries[path], mode)


def backend_for(root: Path) -> BinaryWheelBackend:
    return BinaryWheelBackend(root)
