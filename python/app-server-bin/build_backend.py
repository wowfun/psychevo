from __future__ import annotations

import os
import subprocess
import sysconfig
import tomllib
from pathlib import Path
from zipfile import ZipInfo

from wheel.wheelfile import WheelFile

_ROOT = Path(__file__).parent
_REPOSITORY = _ROOT.parents[1]
_MODULE = "psychevo_app_server_bin"


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


def _binary() -> Path:
    override = os.environ.get("PSYCHEVO_APP_SERVER_BINARY")
    suffix = ".exe" if os.name == "nt" else ""
    binary = (
        Path(override)
        if override
        else _REPOSITORY / "target" / "release" / f"psychevo-app-server{suffix}"
    )
    if not override:
        subprocess.run(
            [
                "cargo",
                "build",
                "--locked",
                "--release",
                "-p",
                "psychevo-gateway",
                "--bin",
                "psychevo-app-server",
                "--no-default-features",
            ],
            cwd=_REPOSITORY,
            check=True,
        )
    if not binary.is_file():
        raise RuntimeError(f"Psychevo App Server binary is missing: {binary}")
    return binary


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
    return dist_info


def build_wheel(
    wheel_directory: str, config_settings=None, metadata_directory=None
) -> str:
    tag = _platform_tag()
    filename = f"{_NAME.replace('-', '_')}-{_VERSION}-py3-none-{tag}.whl"
    target = Path(wheel_directory) / filename
    dist_info = _dist_info()
    source_binary = _binary()
    executable_name = "psychevo-app-server.exe" if os.name == "nt" else "psychevo-app-server"
    entries = {
        f"{_MODULE}/__init__.py": (
            _ROOT / "src" / _MODULE / "__init__.py"
        ).read_bytes(),
        f"{_MODULE}/bin/{executable_name}": source_binary.read_bytes(),
        f"{dist_info}/METADATA": _metadata().encode(),
        f"{dist_info}/licenses/LICENSE": (_REPOSITORY / "LICENSE").read_bytes(),
        f"{dist_info}/WHEEL": _wheel_metadata().encode(),
    }
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
    with WheelFile(target, "w") as wheel:
        for path, content in sorted(entries.items()):
            info = ZipInfo(path)
            info.external_attr = (
                (0o755 if path in executable_paths else 0o644) & 0xFFFF
            ) << 16
            wheel.writestr(info, content)
