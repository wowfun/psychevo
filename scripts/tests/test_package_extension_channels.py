from __future__ import annotations

import hashlib
import importlib.util
import json
import tarfile
import tempfile
import unittest
from pathlib import Path


SCRIPT = Path(__file__).parents[1] / "package_extension_channels.py"
SPEC = importlib.util.spec_from_file_location("package_extension_channels", SCRIPT)
assert SPEC is not None and SPEC.loader is not None
package_extension_channels = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(package_extension_channels)


class PackageExtensionChannelsTests(unittest.TestCase):
    def test_deterministic_archive_has_bounded_manifest_and_executable(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            root = Path(raw)
            executable = root / "sidecar"
            executable.write_bytes(b"binary fixture")
            manifest = b'{"schemaVersion":1}\n'
            first = root / "first.tar.gz"
            second = root / "second.tar.gz"

            package_extension_channels.write_deterministic_tar(
                first, manifest, executable
            )
            package_extension_channels.write_deterministic_tar(
                second, manifest, executable
            )

            self.assertEqual(first.read_bytes(), second.read_bytes())
            with tarfile.open(first, "r:gz") as archive:
                self.assertEqual(
                    archive.getnames(), ["psychevo.extension.json", "sidecar"]
                )
                self.assertEqual(
                    archive.extractfile("psychevo.extension.json").read(), manifest
                )
                self.assertEqual(archive.getmember("sidecar").mode, 0o755)
                self.assertEqual(archive.getmember("sidecar").mtime, 0)

    def test_merge_requires_and_verifies_one_native_artifact_per_family(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            root = Path(raw)
            source = root / "source"
            output = root / "output"
            self._write_release_matrix(source)

            package_extension_channels.merge_fragments(source, output)

            descriptors = sorted(output.glob("*.release.json"))
            archives = sorted(output.glob("*.tar.gz"))
            self.assertEqual(len(descriptors), 3)
            self.assertEqual(len(archives), 9)
            for path in descriptors:
                descriptor = json.loads(path.read_text(encoding="utf-8"))
                self.assertEqual(len(descriptor["artifacts"]), 3)
                self.assertEqual(
                    {
                        package_extension_channels.target_family(target)
                        for target in descriptor["artifacts"]
                    },
                    {"linux", "macos", "windows"},
                )
            checksum_lines = (output / "checksums.sha256").read_text(
                encoding="utf-8"
            ).splitlines()
            self.assertEqual(len(checksum_lines), 12)

    def test_merge_rejects_an_archive_changed_after_fragment_creation(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            root = Path(raw)
            source = root / "source"
            output = root / "output"
            archives = self._write_release_matrix(source)
            original = archives[0].read_bytes()
            archives[0].write_bytes(bytes([original[0] ^ 0x01]) + original[1:])

            with self.assertRaisesRegex(RuntimeError, "digest mismatch"):
                package_extension_channels.merge_fragments(source, output)
            self.assertEqual(list(output.iterdir()), [])

    def _write_release_matrix(self, source: Path) -> list[Path]:
        targets = (
            "x86_64-unknown-linux-gnu",
            "aarch64-apple-darwin",
            "x86_64-pc-windows-msvc",
        )
        archives = []
        for target in targets:
            host = source / target / "extensions"
            fragments = host / "fragments"
            fragments.mkdir(parents=True)
            for extension_id, _, binary in package_extension_channels.CHANNELS:
                archive_name = f"{extension_id}-0.1.0-{target}.tar.gz"
                archive = host / archive_name
                archive.write_bytes(f"{extension_id}|{target}".encode())
                archives.append(archive)
                digest = hashlib.sha256(archive.read_bytes()).hexdigest()
                executable = f"./{binary}{'.exe' if 'windows' in target else ''}"
                descriptor = {
                    "schemaVersion": 1,
                    "id": extension_id,
                    "version": "0.1.0",
                    "artifacts": {
                        target: {
                            "url": "https://github.com/wowfun/psychevo/releases/"
                            f"download/v0.1.0/{archive_name}",
                            "sha256": digest,
                            "format": "tar.gz",
                            "executable": executable,
                            "size": archive.stat().st_size,
                        }
                    },
                }
                (fragments / f"{extension_id}.{target}.json").write_text(
                    json.dumps(descriptor), encoding="utf-8"
                )
        return archives


if __name__ == "__main__":
    unittest.main()
