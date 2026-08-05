#!/usr/bin/env python3
"""Install and verify the repository-pinned Linux supply-chain scanners."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import re
import shutil
import subprocess
import sys
import tarfile
import tempfile
import tomllib
from urllib.request import Request, urlopen


ROOT = Path(__file__).resolve().parents[1]
MANIFEST = ROOT / "supply-chain-tools.toml"
TOOL_COMMANDS = {
    "cargo-deny": ["cargo-deny", "--version"],
    "cargo-machete": ["cargo-machete", "--version"],
    "gitleaks": ["gitleaks", "version"],
}


def load_manifest() -> dict[str, object]:
    with MANIFEST.open("rb") as handle:
        manifest = tomllib.load(handle)
    if manifest.get("schema") != 1:
        raise RuntimeError(f"unsupported {MANIFEST.name} schema")
    return manifest


def require_linux_x86_64() -> None:
    machine = platform.machine().lower()
    if sys.platform != "linux" or machine not in {"x86_64", "amd64"}:
        raise RuntimeError(
            f"supply-chain scanners support Linux x86_64, found {sys.platform}/{machine}"
        )


def tool_config(manifest: dict[str, object], name: str) -> tuple[str, str, str]:
    raw = manifest.get(name)
    if not isinstance(raw, dict):
        raise RuntimeError(f"missing [{name}] in {MANIFEST.name}")
    values = tuple(raw.get(key) for key in (
        "version",
        "linux-x86_64-url",
        "linux-x86_64-sha256",
    ))
    if not all(isinstance(value, str) and value for value in values):
        raise RuntimeError(f"incomplete [{name}] pin in {MANIFEST.name}")
    version, url, digest = values
    if not re.fullmatch(r"[0-9]+\.[0-9]+\.[0-9]+", version):
        raise RuntimeError(f"invalid [{name}] version: {version}")
    if not re.fullmatch(r"[0-9a-f]{64}", digest):
        raise RuntimeError(f"invalid [{name}] SHA-256: {digest}")
    return version, url, digest


def install_tool(name: str, bin_dir: Path, manifest: dict[str, object]) -> None:
    version, url, expected_digest = tool_config(manifest, name)
    request = Request(url, headers={"User-Agent": "psychevo-supply-chain-installer"})
    with tempfile.TemporaryFile() as archive:
        digest = hashlib.sha256()
        with urlopen(request, timeout=60) as response:
            while chunk := response.read(1024 * 1024):
                digest.update(chunk)
                archive.write(chunk)
        actual_digest = digest.hexdigest()
        if actual_digest != expected_digest:
            raise RuntimeError(
                f"{name} {version} SHA-256 mismatch: {actual_digest}"
            )

        archive.seek(0)
        with tarfile.open(fileobj=archive, mode="r:gz") as package:
            members = [
                member
                for member in package.getmembers()
                if member.isfile() and Path(member.name).name == name
            ]
            if len(members) != 1:
                raise RuntimeError(f"{name} archive contains {len(members)} executables")
            source = package.extractfile(members[0])
            if source is None:
                raise RuntimeError(f"cannot read {name} from release archive")
            destination = bin_dir / name
            temporary = bin_dir / f".{name}.tmp"
            with temporary.open("wb") as output:
                shutil.copyfileobj(source, output)
            temporary.chmod(0o755)
            os.replace(temporary, destination)
    print(f"installed {name} {version} ({expected_digest})")


def command_output(command: list[str]) -> str:
    try:
        result = subprocess.run(
            command,
            cwd=ROOT,
            check=True,
            capture_output=True,
            text=True,
        )
    except FileNotFoundError as error:
        raise RuntimeError(f"required executable not found: {command[0]}") from error
    except subprocess.CalledProcessError as error:
        output = "\n".join(part.strip() for part in (error.stdout, error.stderr) if part)
        raise RuntimeError(f"{' '.join(command)} failed: {output}") from error
    return "\n".join(part.strip() for part in (result.stdout, result.stderr) if part)


def verify_tool(name: str, manifest: dict[str, object]) -> None:
    expected, _, _ = tool_config(manifest, name)
    output = command_output(TOOL_COMMANDS[name])
    versions = re.findall(r"(?<![0-9.])[0-9]+\.[0-9]+\.[0-9]+(?![0-9.])", output)
    if expected not in versions:
        raise RuntimeError(f"expected {name} {expected}, found: {output}")
    print(f"verified {name} {expected}")


def verify_pnpm() -> None:
    package = json.loads((ROOT / "package.json").read_text(encoding="utf-8"))
    package_manager = package.get("packageManager", "")
    match = re.fullmatch(r"pnpm@([0-9]+\.[0-9]+\.[0-9]+)", package_manager)
    if match is None:
        raise RuntimeError("package.json must pin packageManager to an exact pnpm version")
    expected = match.group(1)
    actual = command_output(["pnpm", "--version"]).strip()
    if actual != expected:
        raise RuntimeError(f"expected pnpm {expected}, found: {actual}")
    print(f"verified pnpm {expected}")


def main() -> int:
    parser = argparse.ArgumentParser()
    subparsers = parser.add_subparsers(dest="command", required=True)
    install = subparsers.add_parser("install")
    install.add_argument("--bin-dir", type=Path, required=True)
    subparsers.add_parser("verify")
    args = parser.parse_args()

    try:
        require_linux_x86_64()
        manifest = load_manifest()
        if args.command == "install":
            args.bin_dir.mkdir(parents=True, exist_ok=True)
            for name in TOOL_COMMANDS:
                install_tool(name, args.bin_dir, manifest)
        else:
            for name in TOOL_COMMANDS:
                verify_tool(name, manifest)
            verify_pnpm()
    except (OSError, RuntimeError, tarfile.TarError) as error:
        print(f"error: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
