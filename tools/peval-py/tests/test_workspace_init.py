from __future__ import annotations

import os

from peval_py_test_support import *

from peval_py.workspace import init_workspace


class PevalPyWorkspaceInitTests(unittest.TestCase):
    def test_init_creates_only_peval_py_state_and_preserves_config(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp) / "workspace"
            result = init_workspace(str(root))

            self.assertEqual(result.schema_version, 1)
            self.assertEqual(result.root, root.resolve())
            config_text = (root / "peval-py.toml").read_text(encoding="utf-8")
            self.assertIn("[adapters.psychevo]\n", config_text)
            self.assertIn('default_db_path = "~/.psychevo/state.db"\n', config_text)
            self.assertIn("[adapters.opencode]\n", config_text)
            self.assertIn(
                'default_db_path = "~/.local/share/opencode/opencode.db"\n',
                config_text,
            )
            self.assertIn("[adapters.hermes]\n", config_text)
            self.assertIn('default_db_path = "~/.hermes/state.db"\n', config_text)
            self.assertEqual(result.log_path, root.resolve() / "logs" / "peval-py-serve.jsonl")
            self.assertTrue((root / "logs").is_dir())
            self.assertFalse((root / "state.db").exists())
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
            config.write_text('[adapters.psychevo]\ndefault_db_path = "custom.db"\n', encoding="utf-8")
            second = init_workspace(str(root))

            self.assertEqual(second.log_path, root.resolve() / "logs" / "peval-py-serve.jsonl")
            self.assertEqual(
                config.read_text(encoding="utf-8"),
                '[adapters.psychevo]\ndefault_db_path = "custom.db"\n',
            )

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
            self.assertTrue((Path(tmp) / "logs").is_dir())
            self.assertFalse((Path(tmp) / "state.db").exists())
            self.assertFalse((Path(tmp) / "peval.toml").exists())

    def test_init_rejects_invalid_peval_py_config(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            root.joinpath("peval-py.toml").write_text("locale = [\n", encoding="utf-8")
            with self.assertRaisesRegex(ValueError, "failed to parse"):
                init_workspace(str(root))

    def test_cli_init_text_and_json_smoke(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp) / "workspace"
            text = run_cli(["init", "--root", str(root)])
            self.assertEqual(text.returncode, 0, text.stderr)
            self.assertIn(f"peval-py workspace: {root.resolve()}", text.stdout)
            self.assertIn("peval-py config:", text.stdout)
            self.assertIn("serve log:", text.stdout)
            self.assertNotIn("default workspace:", text.stdout)
            self.assertNotIn("templates:", text.stdout)
            self.assertTrue((root / "logs").is_dir())
            self.assertFalse((root / "state.db").exists())
            self.assertFalse((root / "peval.toml").exists())

            json_root = Path(tmp) / "json-workspace"
            payload = run_cli(["init", "--root", str(json_root), "--json"])
            self.assertEqual(payload.returncode, 0, payload.stderr)
            data = json.loads(payload.stdout)
            self.assertEqual(
                sorted(data),
                ["log_path", "peval_py_config", "root", "schema_version"],
            )
            self.assertEqual(data["schema_version"], 1)
            self.assertEqual(data["root"], str(json_root.resolve()))
            self.assertEqual(
                data["log_path"],
                str(json_root.resolve() / "logs" / "peval-py-serve.jsonl"),
            )

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
