from __future__ import annotations

import base64
import csv
import hashlib
import io
import tarfile
import tomllib
import zipfile
from pathlib import Path

_ROOT = Path(__file__).parent
_REPOSITORY = _ROOT.parents[1]


def _project() -> dict[str, object]:
    return tomllib.loads((_ROOT / "pyproject.toml").read_text(encoding="utf-8"))[
        "project"
    ]


def _dist_name(name: str) -> str:
    return name.replace("-", "_")


def _metadata() -> str:
    project = _project()
    lines = [
        "Metadata-Version: 2.3",
        f"Name: {project['name']}",
        f"Version: {project['version']}",
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
    for dependency in project.get("dependencies", []):
        lines.append(f"Requires-Dist: {dependency}")
    for extra, dependencies in project.get("optional-dependencies", {}).items():
        lines.append(f"Provides-Extra: {extra}")
        for dependency in dependencies:
            lines.append(f'Requires-Dist: {dependency}; extra == "{extra}"')
    readme = (_ROOT / str(project["readme"])).read_text(encoding="utf-8")
    return "\n".join(lines) + "\n\n" + readme.rstrip() + "\n"


def _dist_info() -> str:
    project = _project()
    return f"{_dist_name(str(project['name']))}-{project['version']}.dist-info"


def _wheel_name() -> str:
    project = _project()
    return (
        f"{_dist_name(str(project['name']))}-{project['version']}"
        "-py3-none-any.whl"
    )


def get_requires_for_build_wheel(config_settings=None) -> list[str]:
    return []


def get_requires_for_build_sdist(config_settings=None) -> list[str]:
    return []


def prepare_metadata_for_build_wheel(
    metadata_directory: str, config_settings=None
) -> str:
    dist_info = _dist_info()
    target = Path(metadata_directory) / dist_info
    target.mkdir(parents=True, exist_ok=True)
    (target / "METADATA").write_text(_metadata(), encoding="utf-8")
    (target / "WHEEL").write_text(
        "Wheel-Version: 1.0\n"
        "Generator: psychevo-build-backend 0.1\n"
        "Root-Is-Purelib: true\n"
        "Tag: py3-none-any\n",
        encoding="utf-8",
    )
    return dist_info


def build_wheel(
    wheel_directory: str, config_settings=None, metadata_directory=None
) -> str:
    filename = _wheel_name()
    target = Path(wheel_directory) / filename
    dist_info = _dist_info()
    entries: dict[str, bytes] = {}
    for source in sorted((_ROOT / "src" / "psychevo").rglob("*")):
        if source.is_file() and "__pycache__" not in source.parts:
            relative = source.relative_to(_ROOT / "src").as_posix()
            entries[relative] = source.read_bytes()
    entries[f"{dist_info}/METADATA"] = _metadata().encode()
    entries[f"{dist_info}/licenses/LICENSE"] = (
        _REPOSITORY / "LICENSE"
    ).read_bytes()
    entries[f"{dist_info}/WHEEL"] = (
        "Wheel-Version: 1.0\n"
        "Generator: psychevo-build-backend 0.1\n"
        "Root-Is-Purelib: true\n"
        "Tag: py3-none-any\n"
    ).encode()
    _write_wheel(target, entries, dist_info)
    return filename


def _write_wheel(target: Path, entries: dict[str, bytes], dist_info: str) -> None:
    records: list[tuple[str, str, str]] = []
    with zipfile.ZipFile(target, "w", compression=zipfile.ZIP_DEFLATED) as wheel:
        for path, content in sorted(entries.items()):
            wheel.writestr(path, content)
            digest = base64.urlsafe_b64encode(hashlib.sha256(content).digest()).rstrip(
                b"="
            )
            records.append((path, f"sha256={digest.decode()}", str(len(content))))
        record_path = f"{dist_info}/RECORD"
        records.append((record_path, "", ""))
        buffer = io.StringIO()
        csv.writer(buffer, lineterminator="\n").writerows(records)
        wheel.writestr(record_path, buffer.getvalue().encode())


def build_sdist(sdist_directory: str, config_settings=None) -> str:
    project = _project()
    root_name = f"{_dist_name(str(project['name']))}-{project['version']}"
    filename = f"{root_name}.tar.gz"
    target = Path(sdist_directory) / filename
    sources = [
        (_ROOT / "pyproject.toml", "pyproject.toml"),
        (_ROOT / "README.md", "README.md"),
        (_REPOSITORY / "LICENSE", "LICENSE"),
        (_ROOT / "build_backend.py", "build_backend.py"),
        *[
            (source, source.relative_to(_ROOT).as_posix())
            for source in sorted((_ROOT / "src").rglob("*"))
            if source.is_file() and "__pycache__" not in source.parts
        ],
    ]
    with tarfile.open(target, "w:gz", format=tarfile.PAX_FORMAT) as archive:
        for source, relative in sources:
            info = archive.gettarinfo(
                str(source), arcname=f"{root_name}/{relative}"
            )
            info.uid = info.gid = 0
            info.uname = info.gname = ""
            info.mtime = 0
            with source.open("rb") as stream:
                archive.addfile(info, stream)
    return filename
