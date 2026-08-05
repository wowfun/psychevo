from __future__ import annotations

import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

from scripts import package_artifact_smoke as smoke
from scripts import write_package_checksums as checksums


class PackageArtifactSmokeTests(unittest.TestCase):
    def test_retained_desktop_log_is_bounded_to_its_tail(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            log = Path(raw) / "desktop.log"
            prefix = b"discard-me"
            tail = b"z" * smoke.MAX_DESKTOP_LOG_BYTES
            log.write_bytes(prefix + tail)

            smoke.retain_bounded_log(log)

            self.assertEqual(log.read_bytes(), tail)

    def test_checksum_subjects_include_the_smoked_desktop_executable(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            artifact_root = Path(raw) / "artifacts"
            package = artifact_root / "package"
            suffix = ".exe" if checksums.os.name == "nt" else ""
            paths = [
                package / "cli-target" / "release" / f"pevo{suffix}",
                package / "desktop-target" / "release" / f"psychevo-desktop{suffix}",
                package
                / "desktop-target"
                / "release"
                / "bundle"
                / "appimage"
                / "Psychevo.AppImage",
                package / "python" / "wheels" / "psychevo.whl",
                package / "python" / "sdists" / "psychevo.tar.gz",
            ]
            for path in paths:
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_bytes(path.name.encode())

            with patch.dict(
                checksums.os.environ,
                {"PSYCHEVO_CI_ARTIFACT_ROOT": str(artifact_root)},
                clear=False,
            ):
                checksums.main()

            manifest = (package / "checksums.sha256").read_text(encoding="utf-8")
            self.assertIn(f"desktop-target/release/psychevo-desktop{suffix}", manifest)
            self.assertIn("desktop-target/release/bundle/appimage/Psychevo.AppImage", manifest)

    def test_failure_report_retains_completed_artifact_evidence(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            package_root = Path(raw)
            smoke.write_report(
                package_root,
                {
                    "schemaVersion": 1,
                    "platform": "linux",
                    "status": "running",
                    "artifacts": [{"path": "cli-target/release/pevo", "status": "passed"}],
                },
            )

            report = smoke.failure_report(
                package_root, "linux", RuntimeError("Desktop failed")
            )

            self.assertEqual(report["status"], "failed")
            self.assertEqual(report["error"], "Desktop failed")
            self.assertEqual(report["artifacts"][0]["status"], "passed")

    def test_startup_trace_requires_ordered_bridge_marks_from_one_process(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            trace = Path(raw) / "desktop-startup-rust.jsonl"
            records = [
                {
                    "schemaVersion": 1,
                    "id": milestone,
                    "sequence": sequence,
                    "sourceClock": "desktop-rust-monotonic",
                    "epochMs": 1_000 + sequence,
                    "monotonicOffsetMs": float(sequence),
                    "pid": 42,
                }
                for sequence, milestone in enumerate(smoke.STARTUP_TRACE_IDS, start=1)
            ]
            trace.write_text(
                "".join(f"{json.dumps(record)}\n" for record in records),
                encoding="utf-8",
            )

            evidence = smoke.read_complete_startup_trace(trace)

            self.assertEqual(evidence["pid"], 42)
            self.assertEqual(evidence["milestones"], list(smoke.STARTUP_TRACE_IDS))

            records[-1]["pid"] = 43
            trace.write_text(
                "".join(f"{json.dumps(record)}\n" for record in records),
                encoding="utf-8",
            )
            with self.assertRaisesRegex(RuntimeError, "spans multiple processes"):
                smoke.read_complete_startup_trace(trace)

    def test_linux_selects_appimage_and_leaves_installers_nonlaunchable(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            bundle = Path(raw) / "bundle"
            appimage = bundle / "appimage" / "Psychevo.AppImage"
            deb = bundle / "deb" / "psychevo.deb"
            appimage.parent.mkdir(parents=True)
            deb.parent.mkdir(parents=True)
            appimage.write_bytes(b"appimage")
            deb.write_bytes(b"deb")

            artifacts = smoke.bundle_artifacts(bundle)
            selected = smoke.launchable_bundle(artifacts, "linux")

            self.assertIsNotNone(selected)
            self.assertEqual(selected.path, appimage)
            self.assertEqual(selected.launch_path, appimage)
            deb_artifact = next(item for item in artifacts if item.path == deb)
            self.assertIsNone(deb_artifact.launch_path)

    def test_macos_selects_the_app_bundle_executable_not_the_dmg(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            bundle = Path(raw) / "bundle"
            app = bundle / "macos" / "Psychevo Desktop.app"
            executable = app / "Contents" / "MacOS" / "psychevo-desktop"
            dmg = bundle / "dmg" / "Psychevo.dmg"
            executable.parent.mkdir(parents=True)
            dmg.parent.mkdir(parents=True)
            executable.write_bytes(b"mach-o")
            dmg.write_bytes(b"dmg")

            artifacts = smoke.bundle_artifacts(bundle)
            selected = smoke.launchable_bundle(artifacts, "darwin")

            self.assertIsNotNone(selected)
            self.assertEqual(selected.path, app)
            self.assertEqual(selected.launch_path, executable)
            self.assertIsNone(next(item for item in artifacts if item.path == dmg).launch_path)

    def test_windows_installer_is_explicitly_not_launchable(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            bundle = Path(raw) / "bundle"
            msi = bundle / "msi" / "psychevo.msi"
            installer = bundle / "nsis" / "psychevo-setup.exe"
            msi.parent.mkdir(parents=True)
            installer.parent.mkdir(parents=True)
            msi.write_bytes(b"msi")
            installer.write_bytes(b"nsis")

            artifacts = smoke.bundle_artifacts(bundle)

            self.assertIsNone(smoke.launchable_bundle(artifacts, "win32"))
            self.assertTrue(all(item.launch_path is None for item in artifacts))
            self.assertEqual(
                smoke.desktop_executable(Path("target"), "win32"),
                Path("target/release/psychevo-desktop.exe"),
            )


if __name__ == "__main__":
    unittest.main()
