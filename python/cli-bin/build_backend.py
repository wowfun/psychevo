from __future__ import annotations

import base64
import csv
import hashlib
import io
import os
import subprocess
import sysconfig
import tomllib
import zipfile
from pathlib import Path

_ROOT = Path(__file__).parent
_REPOSITORY = _ROOT.parents[1]
_MODULE = "psychevo_cli_bin"


def _project() -> dict[str, object]:
    return tomllib.loads((_ROOT / "pyproject.toml").read_text(encoding="utf-8"))[
        "project"
    ]


_NAME = str(_project()["name"])
_VERSION = str(_project()["version"])


def _platform_tag() -> str:
    return sysconfig.get_platform().replace("-", "_").replace(".", "_")


def _dist_info() -> str:
    return f"{_NAME.replace('-', '_')}-{_VERSION}.dist-info"


def _metadata() -> str:
    project = _project()
    lines = [
        "Metadata-Version: 2.3",
        f"Name: {_NAME}",
        f"Version: {_VERSION}",
        f"Summary: {project['description']}",
        f"Requires-Python: {project['requires-python']}",
        "License-Expression: MIT",
        "License-File: LICENSE",
        "Description-Content-Type: text/markdown",
    ]
    for classifier in project.get("classifiers", []):
        lines.append(f"Classifier: {classifier}")
    for label, url in project.get("urls", {}).items():
        lines.append(f"Project-URL: {label}, {url}")
    readme = (_ROOT / str(project["readme"])).read_text(encoding="utf-8")
    return "\n".join(lines) + "\n\n" + readme.rstrip() + "\n"


def _inputs() -> tuple[Path, Path]:
    binary_override = os.environ.get("PSYCHEVO_CLI_BINARY")
    assets_override = os.environ.get("PSYCHEVO_WORKBENCH_DIST")
    suffix = ".exe" if os.name == "nt" else ""
    binary = (
        Path(binary_override)
        if binary_override
        else _REPOSITORY / "target" / "release" / f"pevo{suffix}"
    )
    assets = (
        Path(assets_override)
        if assets_override
        else _REPOSITORY / "apps" / "workbench" / "dist"
    )
    if not binary_override:
        subprocess.run(
            [
                "cargo",
                "build",
                "--locked",
                "--release",
                "-p",
                "psychevo-cli",
                "--bin",
                "pevo",
            ],
            cwd=_REPOSITORY,
            check=True,
        )
    if not assets_override:
        subprocess.run(
            ["pnpm", "--filter", "@psychevo/workbench", "build"],
            cwd=_REPOSITORY,
            check=True,
        )
    if not binary.is_file():
        raise RuntimeError(f"pevo binary is missing: {binary}")
    if not (assets / "index.html").is_file():
        raise RuntimeError(f"Workbench distribution is missing: {assets}")
    return binary, assets


def get_requires_for_build_wheel(config_settings=None) -> list[str]:
    return []


def get_requires_for_build_sdist(config_settings=None) -> list[str]:
    raise RuntimeError(f"{_NAME} is wheel-only")


def prepare_metadata_for_build_wheel(
    metadata_directory: str, config_settings=None
) -> str:
    dist_info = _dist_info()
    target = Path(metadata_directory) / dist_info
    target.mkdir(parents=True, exist_ok=True)
    (target / "METADATA").write_text(_metadata(), encoding="utf-8")
    (target / "WHEEL").write_text(_wheel_metadata(), encoding="utf-8")
    (target / "entry_points.txt").write_text(
        "[console_scripts]\npevo=psychevo_cli_bin:main\n",
        encoding="utf-8",
    )
    return dist_info


def build_wheel(
    wheel_directory: str, config_settings=None, metadata_directory=None
) -> str:
    tag = _platform_tag()
    filename = f"{_NAME.replace('-', '_')}-{_VERSION}-py3-none-{tag}.whl"
    target = Path(wheel_directory) / filename
    dist_info = _dist_info()
    source_binary, assets = _inputs()
    executable_name = "pevo.exe" if os.name == "nt" else "pevo"
    entries = {
        f"{_MODULE}/__init__.py": (
            _ROOT / "src" / _MODULE / "__init__.py"
        ).read_bytes(),
        f"{_MODULE}/bin/{executable_name}": source_binary.read_bytes(),
        f"{dist_info}/METADATA": _metadata().encode(),
        f"{dist_info}/licenses/LICENSE": (_REPOSITORY / "LICENSE").read_bytes(),
        f"{dist_info}/WHEEL": _wheel_metadata().encode(),
        f"{dist_info}/entry_points.txt": (
            "[console_scripts]\npevo=psychevo_cli_bin:main\n"
        ).encode(),
    }
    for source in sorted(assets.rglob("*")):
        if source.is_file():
            entries[
                f"{_MODULE}/workbench/{source.relative_to(assets).as_posix()}"
            ] = source.read_bytes()
    _write_wheel(
        target,
        entries,
        dist_info,
        executable_paths={f"{_MODULE}/bin/{executable_name}"},
    )
    return filename


def build_sdist(sdist_directory: str, config_settings=None) -> str:
    raise RuntimeError(f"{_NAME} is wheel-only")


def _wheel_metadata() -> str:
    return (
        "Wheel-Version: 1.0\n"
        "Generator: psychevo-build-backend 0.1\n"
        "Root-Is-Purelib: false\n"
        f"Tag: py3-none-{_platform_tag()}\n"
    )


def _write_wheel(
    target: Path,
    entries: dict[str, bytes],
    dist_info: str,
    *,
    executable_paths: set[str],
) -> None:
    records: list[tuple[str, str, str]] = []
    with zipfile.ZipFile(target, "w", compression=zipfile.ZIP_DEFLATED) as wheel:
        for path, content in sorted(entries.items()):
            info = zipfile.ZipInfo(path)
            info.compress_type = zipfile.ZIP_DEFLATED
            info.external_attr = (
                (0o755 if path in executable_paths else 0o644) & 0xFFFF
            ) << 16
            wheel.writestr(info, content)
            digest = base64.urlsafe_b64encode(hashlib.sha256(content).digest()).rstrip(
                b"="
            )
            records.append((path, f"sha256={digest.decode()}", str(len(content))))
        record_path = f"{dist_info}/RECORD"
        records.append((record_path, "", ""))
        buffer = io.StringIO()
        csv.writer(buffer, lineterminator="\n").writerows(records)
        wheel.writestr(record_path, buffer.getvalue().encode())
