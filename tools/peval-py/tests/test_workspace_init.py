from __future__ import annotations

import os

from peval_py_test_support import *

from peval_py.workspace import init_workspace


class PevalPyWorkspaceInitTests(unittest.TestCase):
    def test_init_creates_only_peval_py_state_and_preserves_config(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp) / "workspace"
            result = init_workspace(str(root))

            self.assertEqual(result.schema_version, 2)
            self.assertEqual(result.root, root.resolve())
            self.assertEqual(
                (root / "peval-py.toml").read_text(encoding="utf-8"),
                'state_db = "state.db"\n',
            )
            self.assertTrue((root / "state.db").is_file())
            for unwanted in [
                "peval.toml",
                "runs",
                "datasets",
                "scripts",
                "pidx-psychevo-acp.eval.toml",
                ".gitignore",
            ]:
                self.assertFalse((root / unwanted).exists(), unwanted)

            config = root / "peval-py.toml"
            config.write_text('state_db = "custom.db"\n', encoding="utf-8")
            second = init_workspace(str(root))

            self.assertEqual(second.state_db, root.resolve() / "custom.db")
            self.assertEqual(
                config.read_text(encoding="utf-8"),
                'state_db = "custom.db"\n',
            )
            self.assertTrue((root / "custom.db").is_file())

    def test_init_defaults_to_current_directory(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            old_cwd = Path.cwd()
            try:
                os.chdir(tmp)
                result = init_workspace()
            finally:
                os.chdir(old_cwd)
            self.assertEqual(result.root, Path(tmp).resolve())
            self.assertTrue((Path(tmp) / "peval-py.toml").is_file())
            self.assertTrue((Path(tmp) / "state.db").is_file())
            self.assertFalse((Path(tmp) / "peval.toml").exists())

    def test_init_rejects_invalid_peval_py_config(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            root.joinpath("peval-py.toml").write_text("state_db = [\n", encoding="utf-8")
            with self.assertRaisesRegex(ValueError, "failed to parse"):
                init_workspace(str(root))

    def test_cli_init_text_and_json_smoke(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp) / "workspace"
            text = run_cli(["init", "--root", str(root)])
            self.assertEqual(text.returncode, 0, text.stderr)
            self.assertIn(f"peval-py workspace: {root.resolve()}", text.stdout)
            self.assertIn("peval-py config:", text.stdout)
            self.assertIn("state db:", text.stdout)
            self.assertNotIn("default workspace:", text.stdout)
            self.assertNotIn("templates:", text.stdout)
            self.assertTrue((root / "state.db").is_file())
            self.assertFalse((root / "peval.toml").exists())

            json_root = Path(tmp) / "json-workspace"
            payload = run_cli(["init", "--root", str(json_root), "--json"])
            self.assertEqual(payload.returncode, 0, payload.stderr)
            data = json.loads(payload.stdout)
            self.assertEqual(
                sorted(data),
                ["peval_py_config", "root", "schema_version", "state_db"],
            )
            self.assertEqual(data["schema_version"], 2)
            self.assertEqual(data["root"], str(json_root.resolve()))
            self.assertEqual(data["state_db"], str(json_root.resolve() / "state.db"))

    def test_cli_init_rejects_removed_default_flags(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp) / "workspace"
            result = run_cli(["init", "--root", str(root), "--default"])
            self.assertEqual(result.returncode, 2)
            self.assertIn("unrecognized arguments: --default", result.stderr)


def run_cli(args: list[str], *, env: dict[str, str] | None = None) -> subprocess.CompletedProcess:
    return subprocess.run(
        [
            sys.executable,
            "-c",
            "from peval_py.cli import main; raise SystemExit(main())",
            *args,
        ],
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        env=env,
        check=False,
    )


if __name__ == "__main__":
    unittest.main()
