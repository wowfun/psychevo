from __future__ import annotations

import os

from peval_py_test_support import *
from peval_py.config import write_workspace_adapter_default_db


class PevalPyConfigAdapterTests(unittest.TestCase):
    def test_config_uses_adapter_default_and_accepts_legacy_agent_key(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            adapter_config = Path(tmp) / "adapter.toml"
            adapter_config.write_text(
                "[defaults]\nadapter = \"opencode\"\n",
                encoding="utf-8",
            )
            self.assertEqual(load_config(str(adapter_config)).adapter, "opencode")

            legacy_config = Path(tmp) / "legacy.toml"
            legacy_config.write_text(
                "[defaults]\nagent = \"hermes\"\n",
                encoding="utf-8",
            )
            self.assertEqual(load_config(str(legacy_config)).adapter, "hermes")


    def test_config_locale_defaults_aliases_and_invalid_values(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            old_cwd = Path.cwd()
            try:
                os.chdir(tmp)
                default_config = load_config(None)
                self.assertEqual(default_config.locale, "en")
                self.assertEqual(default_config.analysis_eval_slug, "default")
            finally:
                os.chdir(old_cwd)
            for value, expected in [
                ("en", "en"),
                ("en-US", "en"),
                ("zh-CN", "zh-CN"),
                ("zh", "zh-CN"),
            ]:
                with self.subTest(value=value):
                    config_path = Path(tmp) / f"{value}.toml"
                    config_path.write_text(
                        f"[defaults]\nlocale = \"{value}\"\n",
                        encoding="utf-8",
                    )
                    self.assertEqual(load_config(str(config_path)).locale, expected)

            invalid_config = Path(tmp) / "invalid.toml"
            invalid_config.write_text(
                "[defaults]\nlocale = \"fr-FR\"\n",
                encoding="utf-8",
            )
            with self.assertRaisesRegex(
                ValueError,
                "unsupported locale: fr-FR; supported locales: en, zh-CN",
            ):
                load_config(str(invalid_config))

    def test_peval_py_toml_locale_discovery_and_config_overlay(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            child = root / "nested" / "child"
            child.mkdir(parents=True)
            root.joinpath("peval-py.toml").write_text(
                'state_db = "state.db"\nlocale = "zh-CN"\nanalysis_eval_slug = "custom-eval"\n',
                encoding="utf-8",
            )
            explicit = root / "explicit.toml"
            explicit.write_text("[defaults]\nadapter = \"opencode\"\n", encoding="utf-8")
            explicit_locale = root / "explicit-locale.toml"
            explicit_locale.write_text("[defaults]\nlocale = \"en\"\n", encoding="utf-8")
            explicit_analysis = root / "explicit-analysis.toml"
            explicit_analysis.write_text('analysis_eval_slug = "override-eval"\n', encoding="utf-8")
            old_cwd = Path.cwd()
            try:
                os.chdir(child)
                discovered = load_config(None)
                self.assertEqual(discovered.locale, "zh-CN")
                self.assertEqual(discovered.analysis_eval_slug, "custom-eval")
                self.assertEqual(discovered.workspace_root, str(root.resolve()))
                overlaid = load_config(str(explicit))
                self.assertEqual(overlaid.adapter, "opencode")
                self.assertEqual(overlaid.locale, "zh-CN")
                self.assertEqual(overlaid.analysis_eval_slug, "custom-eval")
                self.assertEqual(load_config(str(explicit_locale)).locale, "en")
                analysis_overlaid = load_config(str(explicit_analysis))
                self.assertEqual(analysis_overlaid.locale, "zh-CN")
                self.assertEqual(analysis_overlaid.analysis_eval_slug, "override-eval")
            finally:
                os.chdir(old_cwd)

            root.joinpath("peval-py.toml").write_text(
                'state_db = "state.db"\nlocale = "fr-FR"\n',
                encoding="utf-8",
            )
            old_cwd = Path.cwd()
            try:
                os.chdir(child)
                with self.assertRaisesRegex(ValueError, "unsupported locale"):
                    load_config(None)
            finally:
                os.chdir(old_cwd)


    def test_config_passes_selected_adapter_options(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            config_path = Path(tmp) / "adapters.toml"
            config_path.write_text(
                """
[defaults]
adapter = "opencode"

[adapters.custom]
label_prefix = "configured"
enabled = true
""",
                encoding="utf-8",
            )
            config = load_config(str(config_path))
            self.assertEqual(config.adapter, "opencode")
            self.assertEqual(config.adapter_options, {})

            overridden = apply_overrides(
                config,
                SimpleNamespace(adapter="custom", no_redact=False),
            )
            self.assertEqual(overridden.adapter, "custom")
            self.assertEqual(
                overridden.adapter_options,
                {"label_prefix": "configured", "enabled": True},
            )

            list_override = apply_overrides(
                config,
                SimpleNamespace(adapter=["custom", "p1=opencode"], no_redact=False),
            )
            self.assertEqual(list_override.adapter, "custom")
            self.assertEqual(
                list_override.adapter_options,
                {"label_prefix": "configured", "enabled": True},
            )

            selected = config_for_adapter(config, "custom")
            self.assertEqual(
                selected.adapter_options,
                {"label_prefix": "configured", "enabled": True},
            )

    def test_adapter_default_db_path_resolves_relative_to_defining_config(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            config_dir = Path(tmp) / "configs"
            config_dir.mkdir()
            config_path = config_dir / "adapters.toml"
            config_path.write_text(
                """
[adapters.psychevo]
default_db_path = "state.db"
label_prefix = "configured"

[adapters.hermes]
default_db_path = "../hermes/state.db"
""",
                encoding="utf-8",
            )

            config = load_config(str(config_path))
            self.assertEqual(
                config.adapter_default_db_paths,
                {
                    "psychevo": str((config_dir / "state.db").resolve()),
                    "hermes": str((config_dir / "../hermes/state.db").resolve()),
                },
            )
            self.assertEqual(
                config.adapter_options_by_id["psychevo"],
                {"label_prefix": "configured"},
            )
            self.assertEqual(config.adapter_options_by_id["hermes"], {})


    def test_adapter_default_db_path_expands_home_and_absolute_like_paths(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            home = root / "home"
            home.mkdir()
            config_path = root / "peval-py.toml"
            windows_path = r"C:\Users\me\AppData\Local\opencode\opencode.db"
            unc_path = r"\\server\share\hermes\state.db"
            config_path.write_text(
                f"""
[adapters.psychevo]
default_db_path = "~/.psychevo/state.db"

[adapters.opencode]
default_db_path = '{windows_path}'

[adapters.hermes]
default_db_path = '{unc_path}'
""",
                encoding="utf-8",
            )

            with patch.dict(os.environ, {"HOME": str(home)}):
                config = load_config(str(config_path))

            self.assertEqual(
                config.adapter_default_db_paths["psychevo"],
                str((home / ".psychevo/state.db").resolve()),
            )
            self.assertEqual(config.adapter_default_db_paths["opencode"], windows_path)
            self.assertEqual(config.adapter_default_db_paths["hermes"], unc_path)


    def test_write_workspace_adapter_default_db_uses_tilde_for_home_paths(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            home = root / "home"
            home.mkdir()
            config_path = root / "peval-py.toml"
            config_path.write_text('state_db = "state.db"\n', encoding="utf-8")
            home_db = home / ".psychevo" / "state.db"

            with patch.dict(os.environ, {"HOME": str(home)}):
                resolved = write_workspace_adapter_default_db(
                    config_path,
                    "psychevo",
                    str(home_db),
                )
                config = load_config(str(config_path))

            self.assertEqual(resolved, str(home_db.resolve()))
            self.assertEqual(
                config.adapter_default_db_paths["psychevo"],
                str(home_db.resolve()),
            )
            self.assertIn(
                'default_db_path = "~/.psychevo/state.db"\n',
                config_path.read_text(encoding="utf-8"),
            )


    def test_write_workspace_adapter_default_db_preserves_adapter_options(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            config_path = Path(tmp) / "peval-py.toml"
            config_path.write_text(
                """
locale = "en"

[adapters.opencode]
label_prefix = "configured"
default_db_path = "old.db"
enabled = true

[adapters.hermes]
default_db_path = "hermes.db"
""",
                encoding="utf-8",
            )

            resolved = write_workspace_adapter_default_db(
                config_path,
                "opencode",
                "db/new.db",
            )

            self.assertEqual(resolved, str((Path(tmp) / "db/new.db").resolve()))
            config = load_config(str(config_path))
            self.assertEqual(
                config.adapter_default_db_paths["opencode"],
                str((Path(tmp) / "db/new.db").resolve()),
            )
            self.assertEqual(
                config.adapter_default_db_paths["hermes"],
                str((Path(tmp) / "hermes.db").resolve()),
            )
            self.assertEqual(
                config.adapter_options_by_id["opencode"],
                {"label_prefix": "configured", "enabled": True},
            )
            text = config_path.read_text(encoding="utf-8")
            self.assertIn('default_db_path = "db/new.db"\n', text)
            self.assertIn('label_prefix = "configured"\n', text)
            self.assertIn('enabled = true\n', text)

            cleared = write_workspace_adapter_default_db(
                config_path,
                "opencode",
                "",
            )

            self.assertIsNone(cleared)
            config = load_config(str(config_path))
            self.assertNotIn("opencode", config.adapter_default_db_paths)
            self.assertEqual(
                config.adapter_default_db_paths["hermes"],
                str((Path(tmp) / "hermes.db").resolve()),
            )
            self.assertEqual(
                config.adapter_options_by_id["opencode"],
                {"label_prefix": "configured", "enabled": True},
            )
            text = config_path.read_text(encoding="utf-8")
            opencode_section = text.split("[adapters.opencode]", 1)[1].split(
                "[adapters.hermes]",
                1,
            )[0]
            self.assertNotIn("default_db_path", opencode_section)
            self.assertIn('default_db_path = "hermes.db"\n', text)


    def test_adapter_registry_discovers_builtins_and_entry_points_lazily(self) -> None:
        custom_entry = FakeEntryPoint("custom", CustomPathAdapter)
        unused_entry = BrokenEntryPoint("unused", object())
        with patch(
            "peval_py.adapters.entry_points",
            return_value=FakeEntryPoints([custom_entry, unused_entry]),
        ):
            self.assertEqual(adapter_for("psychevo").agent_id, "psychevo")
            self.assertIn("custom", available_adapter_ids())
            self.assertEqual(custom_entry.load_count, 0)
            self.assertEqual(unused_entry.load_count, 0)

            adapter = adapter_for("custom")
            self.assertEqual(adapter.agent_id, "custom")
            self.assertEqual(custom_entry.load_count, 1)
            self.assertEqual(unused_entry.load_count, 0)


    def test_adapter_registry_accepts_class_factory_and_instance_entry_points(self) -> None:
        values = [CustomPathAdapter, lambda: CustomPathAdapter(), CustomPathAdapter()]
        for value in values:
            with self.subTest(value=type(value).__name__):
                with patch(
                    "peval_py.adapters.entry_points",
                    return_value=FakeEntryPoints([FakeEntryPoint("custom", value)]),
                ):
                    adapter = adapter_for("custom")
                    self.assertTrue(callable(getattr(adapter, "convert_path", None)))


    def test_adapter_registry_reports_duplicate_and_unknown_ids(self) -> None:
        duplicate = FakeEntryPoint("opencode", CustomPathAdapter)
        with patch(
            "peval_py.adapters.entry_points",
            return_value=FakeEntryPoints([duplicate]),
        ):
            with self.assertRaisesRegex(ValueError, "duplicate adapter id: opencode"):
                available_adapter_ids()
            self.assertEqual(duplicate.load_count, 0)

        custom = FakeEntryPoint("custom", CustomPathAdapter)
        with patch(
            "peval_py.adapters.entry_points",
            return_value=FakeEntryPoints([custom]),
        ):
            with self.assertRaisesRegex(ValueError, "unsupported adapter: missing"):
                adapter_for("missing")
            self.assertEqual(custom.load_count, 0)
