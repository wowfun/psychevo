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
                rows[0]["cell_dir"],
                str(workspace / "runs" / "custom_eval" / "Agent_A" / "trial_one" / "trial_one"),
            )
            self.assertEqual(
                rows[0]["notes_path"],
                str(
                    workspace
                    / "runs"
                    / "custom_eval"
                    / "Agent_A"
                    / "trial_one"
                    / "trial_one"
                    / "notes.md"
                ),
            )
            self.assertEqual(rows[1]["agent_segment"], "Agent_B")
            self.assertEqual(rows[1]["session_segment"], "common_session")
            self.assertEqual(rows[1]["cell_segment"], "trial_two")

    def test_report_tools_check_can_target_trial_annotations(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            report_json = Path(tmp) / "report.json"
            write_report(report_json)

            by_index = run_report_tools(
                "check",
                str(report_json),
                "--index",
                "2",
                "--require-analysis",
                "--require-notes",
                "--require-findings",
            )
            self.assertEqual(by_index.returncode, 0, by_index.stderr)
            self.assertIn("requested annotation fields recognized", by_index.stdout)

            by_trial = run_report_tools(
                "check",
                str(report_json),
                "--trial-key",
                "trial/two",
                "--require-summary",
                "--require-notes",
            )
            self.assertEqual(by_trial.returncode, 0, by_trial.stderr)

            missing = run_report_tools(
                "check",
                str(report_json),
                "--index",
                "1",
                "--require-summary",
            )
            self.assertNotEqual(missing.returncode, 0)
            self.assertIn(
                "missing annotations.analysis.summary for trial_key=trial:one",
                missing.stderr,
            )


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
                    "notes": [
                        {
                            "trial_key": "trial/two",
                            "source": "cell",
                            "label": "notes.md",
                            "markdown": "Second Trial notes.",
                        }
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
