from __future__ import annotations

import argparse
import gzip
import hashlib
import json
import os
import shutil
import subprocess
import tarfile
from pathlib import Path
from urllib.parse import urlsplit

ROOT = Path(__file__).parents[1]
CHANNELS = (
    ("psychevo.channel.wechat", "psychevo-extension-channel-wechat", "psychevo-channel-wechat"),
    ("psychevo.channel.telegram", "psychevo-extension-channel-telegram", "psychevo-channel-telegram"),
    ("psychevo.channel.feishu-lark", "psychevo-extension-channel-feishu-lark", "psychevo-channel-feishu-lark"),
)
EXPECTED_TARGET_FAMILIES = {"linux", "macos", "windows"}


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for block in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def rust_host() -> str:
    output = subprocess.run(
        ["rustc", "-vV"], cwd=ROOT, check=True, capture_output=True, text=True
    ).stdout
    for line in output.splitlines():
        if line.startswith("host: "):
            return line.removeprefix("host: ").strip()
    raise RuntimeError("rustc -vV did not report a host target")


def write_deterministic_tar(archive: Path, manifest: bytes, executable: Path) -> None:
    archive.parent.mkdir(parents=True, exist_ok=True)
    with archive.open("wb") as raw:
        with gzip.GzipFile(filename="", mode="wb", fileobj=raw, mtime=0) as compressed:
            with tarfile.open(fileobj=compressed, mode="w") as tar:
                manifest_info = tarfile.TarInfo("psychevo.extension.json")
                manifest_info.size = len(manifest)
                manifest_info.mode = 0o644
                manifest_info.mtime = 0
                tar.addfile(manifest_info, fileobj=BytesReader(manifest))
                executable_info = tar.gettarinfo(str(executable), arcname=executable.name)
                executable_info.mode = 0o755
                executable_info.mtime = 0
                executable_info.uid = executable_info.gid = 0
                executable_info.uname = executable_info.gname = ""
                with executable.open("rb") as source:
                    tar.addfile(executable_info, source)


class BytesReader:
    def __init__(self, value: bytes) -> None:
        self.value = value
        self.offset = 0

    def read(self, size: int = -1) -> bytes:
        if size < 0:
            size = len(self.value) - self.offset
        result = self.value[self.offset : self.offset + size]
        self.offset += len(result)
        return result


def package_current_target(output: Path) -> None:
    target = rust_host()
    suffix = ".exe" if target.endswith("-windows-msvc") else ""
    artifact_root = Path(
        os.environ.get("PSYCHEVO_CI_ARTIFACT_ROOT", ROOT / ".local" / "ci-artifacts" / "package")
    )
    binary_root = artifact_root / "package" / "extension-target" / "release"
    fragments = output / "fragments"
    fragments.mkdir(parents=True, exist_ok=True)
    for extension_id, package, binary in CHANNELS:
        executable = binary_root / f"{binary}{suffix}"
        if not executable.is_file():
            raise RuntimeError(f"missing built Extension executable: {executable}")
        manifest_path = ROOT / "crates" / package / "psychevo.extension.json"
        manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
        version = str(manifest["version"])
        manifest["runtime"]["executable"] = f"./{executable.name}"
        manifest_bytes = (json.dumps(manifest, indent=2, sort_keys=True) + "\n").encode()
        archive_name = f"{extension_id}-{version}-{target}.tar.gz"
        archive = output / archive_name
        write_deterministic_tar(archive, manifest_bytes, executable)
        descriptor = {
            "schemaVersion": 1,
            "id": extension_id,
            "version": version,
            "artifacts": {
                target: {
                    "url": f"https://github.com/wowfun/psychevo/releases/download/v{version}/{archive_name}",
                    "sha256": sha256(archive),
                    "format": "tar.gz",
                    "executable": f"./{executable.name}",
                    "size": archive.stat().st_size,
                }
            },
        }
        (fragments / f"{extension_id}.{target}.json").write_text(
            json.dumps(descriptor, indent=2, sort_keys=True) + "\n", encoding="utf-8"
        )
    print(f"packaged {len(CHANNELS)} Channel Extensions for {target} in {output}")


def merge_fragments(source: Path, output: Path) -> None:
    output.mkdir(parents=True, exist_ok=True)
    if any(output.iterdir()):
        raise RuntimeError(f"Extension release output must be empty: {output}")
    grouped: dict[str, list[dict[str, object]]] = {}
    for path in sorted(source.rglob("fragments/*.json")):
        value = json.loads(path.read_text(encoding="utf-8"))
        grouped.setdefault(str(value["id"]), []).append(value)
    expected = {item[0] for item in CHANNELS}
    if set(grouped) != expected:
        raise RuntimeError(f"Extension fragment ids differ: expected {sorted(expected)}, got {sorted(grouped)}")
    merged_descriptors: dict[str, dict[str, object]] = {}
    referenced_archives: dict[str, Path] = {}
    for extension_id, descriptors in grouped.items():
        version = descriptors[0]["version"]
        artifacts: dict[str, object] = {}
        for descriptor in descriptors:
            if descriptor.get("schemaVersion") != 1 or descriptor.get("id") != extension_id:
                raise RuntimeError(f"invalid release descriptor fragment for {extension_id}")
            if descriptor["version"] != version:
                raise RuntimeError(f"mixed versions for {extension_id}")
            for target, artifact in dict(descriptor["artifacts"]).items():
                if target in artifacts and artifacts[target] != artifact:
                    raise RuntimeError(f"conflicting {extension_id} artifact for {target}")
                artifacts[target] = artifact
        families = {target_family(target) for target in artifacts}
        if len(artifacts) != 3 or families != EXPECTED_TARGET_FAMILIES:
            raise RuntimeError(
                f"{extension_id} target families differ: expected {sorted(EXPECTED_TARGET_FAMILIES)}, "
                f"got {sorted(families)} from {sorted(artifacts)}"
            )
        for target, raw_artifact in artifacts.items():
            artifact = dict(raw_artifact)
            url = urlsplit(str(artifact.get("url", "")))
            archive_name = Path(url.path).name
            if url.scheme != "https" or not archive_name.endswith(".tar.gz"):
                raise RuntimeError(f"invalid {extension_id} artifact URL for {target}")
            candidates = list(source.rglob(archive_name))
            if len(candidates) != 1:
                raise RuntimeError(
                    f"expected one archive named {archive_name}, found {len(candidates)}"
                )
            archive = candidates[0]
            if artifact.get("size") != archive.stat().st_size:
                raise RuntimeError(f"{extension_id} archive size mismatch for {target}")
            if artifact.get("sha256") != sha256(archive):
                raise RuntimeError(f"{extension_id} archive digest mismatch for {target}")
            if archive_name in referenced_archives:
                raise RuntimeError(f"archive name is referenced more than once: {archive_name}")
            referenced_archives[archive_name] = archive
        merged_descriptors[extension_id] = {
            "schemaVersion": 1,
            "id": extension_id,
            "version": version,
            "artifacts": dict(sorted(artifacts.items())),
        }
    archives = sorted(referenced_archives.values())
    if len(archives) != len(CHANNELS) * 3:
        raise RuntimeError(f"expected {len(CHANNELS) * 3} referenced archives, found {len(archives)}")
    unreferenced = {
        path.resolve() for path in source.rglob("psychevo.channel.*.tar.gz")
    } - {path.resolve() for path in archives}
    if unreferenced:
        raise RuntimeError(
            "unreferenced Extension archives: " + ", ".join(sorted(path.name for path in unreferenced))
        )
    for extension_id, descriptor in merged_descriptors.items():
        (output / f"{extension_id}.release.json").write_text(
            json.dumps(descriptor, indent=2, sort_keys=True) + "\n", encoding="utf-8"
        )
    for archive in archives:
        destination = output / archive.name
        shutil.copy2(archive, destination)
    checksums = [
        f"{sha256(path)}  {path.name}"
        for path in sorted(output.iterdir())
        if path.is_file() and path.name != "checksums.sha256"
    ]
    (output / "checksums.sha256").write_text("\n".join(checksums) + "\n", encoding="utf-8")
    print(f"merged {len(grouped)} descriptors and {len(archives)} archives in {output}")


def target_family(target: str) -> str:
    if "-linux-" in target:
        return "linux"
    if target.endswith("-apple-darwin"):
        return "macos"
    if "-windows-" in target:
        return "windows"
    return f"unknown:{target}"


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--merge-input", type=Path)
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    if args.merge_input:
        output = args.output or ROOT / ".local" / "extension-release"
        merge_fragments(args.merge_input.resolve(), output.resolve())
        return
    artifact_root = Path(
        os.environ.get("PSYCHEVO_CI_ARTIFACT_ROOT", ROOT / ".local" / "ci-artifacts" / "package")
    )
    output = args.output or artifact_root / "package" / "extensions"
    package_current_target(output.resolve())


if __name__ == "__main__":
    main()
