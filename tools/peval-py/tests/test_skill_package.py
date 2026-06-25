from __future__ import annotations

import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[3]
REPORT_TOOLS = REPO_ROOT / "skills" / "peval-py" / "scripts" / "report_tools.py"


class PevalPySkillPackageTests(unittest.TestCase):
    def test_report_tools_subjects_emit_cell_segments_and_paths(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            workspace = root / "workspace"
            report_json = root / "report.json"
            write_report(report_json)

            result = run_report_tools(
                "subjects",
                str(report_json),
                "--workspace",
                str(workspace),
                "--eval-slug",
                "custom eval",
            )

            self.assertEqual(result.returncode, 0, result.stderr)
            rows = [json.loads(line) for line in result.stdout.splitlines()]
            self.assertEqual(len(rows), 2)
            self.assertEqual(rows[0]["agent_segment"], "Agent_A")
            self.assertEqual(rows[0]["session_segment"], "trial_one")
            self.assertEqual(rows[0]["cell_segment"], "trial_one")
            self.assertEqual(
                rows[0]["run_path"],
                "runs/custom_eval/Agent_A/trial_one/trial_one",
            )
            self.assertEqual(
                rows[0]["cell_dir"],
                str(workspace / "runs" / "custom_eval" / "Agent_A" / "trial_one" / "trial_one"),
            )
            self.assertNotIn("notes_path", rows[0])
            self.assertEqual(rows[1]["agent_segment"], "Agent_B")
            self.assertEqual(rows[1]["session_segment"], "common_session")
            self.assertEqual(rows[1]["cell_segment"], "trial_two")
            self.assertEqual(
                rows[1]["run_path"],
                "runs/custom_eval/Agent_B/common_session/trial_two",
            )

    def test_report_tools_check_subcommand_is_removed(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            report_json = Path(tmp) / "report.json"
            write_report(report_json)

            removed_check = run_report_tools("check", str(report_json))

            self.assertNotEqual(removed_check.returncode, 0)
            self.assertIn("invalid choice", removed_check.stderr)

    def test_skill_docs_separate_analysis_reports_from_import(self) -> None:
        skill = (REPO_ROOT / "skills" / "peval-py" / "SKILL.md").read_text(
            encoding="utf-8"
        )
        artifacts = (
            REPO_ROOT
            / "skills"
            / "peval-py"
            / "references"
            / "analysis-artifacts.md"
        ).read_text(encoding="utf-8")
        workflows = (
            REPO_ROOT
            / "skills"
            / "peval-py"
            / "references"
            / "cli-workflows.md"
        ).read_text(encoding="utf-8")

        for text in (skill, artifacts, workflows):
            self.assertIn("peval-py import analysis", text)
            self.assertIn("--run-path", text)
            self.assertIn("analysis report", text)
            self.assertNotIn("notes.md", text)
            self.assertNotIn("annotations.notes", text)
            self.assertNotIn("report_tools.py check", text)
            self.assertNotIn("--require-", text)
            lowered = text.lower()
            self.assertNotIn("analysis draft", lowered)
            self.assertNotIn("json draft", lowered)
            self.assertNotIn("markdown draft", lowered)
        self.assertNotIn("## Contents", artifacts)
        self.assertIn("Create analysis reports", skill)
        self.assertIn("Import analysis reports", skill)
        self.assertIn("--run-path <cell-path>", skill)
        self.assertIn("Trial cell or session artifact path", skill)
        self.assertIn("agent/trajectory.json", skill)
        self.assertIn("agent/trajectory_meta.json", skill)
        self.assertIn("Do not pass a Trial cell or session artifact directory to `view tr -p`", skill)
        self.assertIn("only when the target cell path is missing", skill)
        self.assertNotIn("runs/<analysis_eval_slug>", skill)
        self.assertNotIn("Standard JSON report fields", skill)
        self.assertNotIn("compiled `analysis.json.extra`", skill)
        self.assertIn("--run-path <cell-path>", artifacts)
        self.assertIn("Analyze Trial Cell Artifacts", artifacts)
        self.assertIn("not as a `view tr -p` source", artifacts)
        self.assertIn("agent/trajectory.json", artifacts)
        self.assertIn("agent/trajectory_meta.json", artifacts)
        self.assertIn("contains exactly one cell", artifacts)
        self.assertIn("--run-path <cell-path>", workflows)
        self.assertIn("<path-to-jsonl-or-atif-trajectory>", workflows)
        self.assertIn("Do not generate a report only to rediscover", workflows)
        self.assertIn("If the user already provided the cell path, use it directly", workflows)
        self.assertNotIn("<path-to-session-or-atif>", workflows)
        self.assertNotIn("runs/<analysis_eval_slug>", artifacts)
        self.assertNotIn("runs/<analysis_eval_slug>", workflows)
        self.assertNotIn("Standard JSON report fields", workflows)
        self.assertNotIn("extra", workflows)
        self.assertIn("extra", artifacts)
        self.assertIn("summary", artifacts)
        self.assertIn("recommendations", artifacts)
        self.assertIn("subject", artifacts)
        self.assertIn("do not override", artifacts)
        self.assertNotIn('"commands": []', artifacts)
        self.assertNotIn('"metrics": {', artifacts)


def write_report(path: Path) -> None:
    path.write_text(
        json.dumps(
            {
                "trajectory": [
                    {
                        "session_id": None,
                        "agent": {"name": "Agent A"},
                    },
                    {
                        "session_id": "common/session",
                        "agent": {"name": "Agent/B"},
                    },
                ],
                "trajectory_meta": [
                    {
                        "adapter": "open code",
                        "trial_key": "trial:one",
                    },
                    {
                        "adapter": "opencode",
                        "trial_key": "trial/two",
                    },
                ],
                "annotations": {
                    "analysis": [
                        {
                            "trial_key": "trial:one",
                            "md_report": "First Trial has markdown only.",
                        },
                        {
                            "trial_key": "trial/two",
                            "summary": "Second Trial summary.",
                            "md_report": "Second Trial markdown.",
                            "findings": [{"title": "Second Trial finding."}],
                        },
                    ],
                },
            }
        ),
        encoding="utf-8",
    )


def run_report_tools(*args: str) -> subprocess.CompletedProcess:
    return subprocess.run(
        [sys.executable, str(REPORT_TOOLS), *args],
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=False,
    )


if __name__ == "__main__":
    unittest.main()
