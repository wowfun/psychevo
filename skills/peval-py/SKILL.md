---
name: peval-py
description: "Use when working with peval-py CLI operations or agent trajectory analysis: inspect retained sessions, export ATIF trajectories, list adapter DB sessions or saved workspace snapshots, import analysis reports into Trial cell artifacts, serve retained sessions locally, or evaluate trajectory quality, tool use, cost, grounding, and task success."
---

# peval-py

Use this skill for two jobs:

- operate the `peval-py` CLI correctly
- analyze one or more agent trajectories from the evidence available

## CLI Operations

Recognize only the input class needed for the command:

- **Workspace root**: contains `peval-py.toml`; pass it as `-r <workspace>` when workspace config, saved snapshots, or imported analysis files must be discovered.
- **Saved workspace snapshots**: `<workspace>/state.db` is only a saved snapshot input when used with explicit `-r <workspace>`. Use it for `view tr --list`, bounded inspection, or exporting a selected stored trajectory.
- **Adapter DB input**: use a real adapter-owned database path with `-d <adapter-db>`, or `-d @adapter` when the workspace config has a default adapter DB.
- **Trajectory path input**: use `-p <path-to-jsonl-or-atif-trajectory-or-cell-dir>` for JSONL, ATIF `trajectory.json`, or a Trial cell containing `agent/trajectory.json` and `agent/trajectory_meta.json`.
- **Trial cell or session artifact path**: under `runs/<eval>/<agent>/<session>/...`. If it names a cell, `view tr -p <cell-dir>` and `export tr -p <cell-dir>` are supported convenience inputs; `<cell-dir>/**`, `<cell-dir>/**/*`, and descendants inside the cell are accepted and override conflicting source flags. Raw reports preserve original source labels and add `artifact_ref` for the cell path. Read `agent/trajectory.json`, `agent/trajectory_meta.json`, and existing `analysis.json` / `analysis.md` directly when accuracy matters. If it names a session directory with exactly one cell, use that cell; if it contains multiple cells, ask which cell to target.
- **Report output**: static reports are outputs, not places to write analysis by hand. Use `references/view-tr.md` for raw JSON/HTML report flags after choosing full report output.

Choose the smallest CLI workflow:

- **Inspect first**: `peval-py view tr <source-flags>` for a compact trajectory digest. Use `--steps <step_ids>` or `--tool-call <tool_call_id>` only when the digest points to specific steps or a tool call that needs more evidence. Use `references/view-tr.md` for saved snapshots, adapter DB listing, targeted evidence, or raw reports.
- **List sessions or saved sources**: `peval-py view tr --list` with adapter DB flags or with `-r <workspace> -d <workspace>/state.db`.
- **Export one trajectory**: `peval-py export tr`; it is single-session output. Use `references/cli-workflows.md` for full export, `init`, and `serve` recipes.
- **Import analysis**: `peval-py import analysis -r <workspace> --run-path <cell-path> -p <analysis-report>` when an existing JSON or Markdown analysis report should be published into a Trial cell. Use `references/cli-workflows.md` for import examples.
- **Browse locally**: `peval-py serve -r <workspace> <source-flags>` only when the user asks for an interactive local view.

CLI guardrails:

- Do not mutate source session databases or original trajectory inputs.
- Treat `view tr` inspect output as an exploration aid, not an authoritative evaluation. Its summaries depend on the retained trajectory format and adapter mapping, so counts, timing, token/cost, and error statistics may be approximate or incomplete. When accuracy matters, narrow with `view tr` first, then read the targeted trajectory, metadata, JSONL, or report evidence directly.
- If `peval-py` cannot satisfy the user's request, explain whether the gap is in CLI/report behavior, skill guidance, or both. Ask whether to improve the relevant surface instead of inventing an unsupported workaround.
- Do not read large trajectory/report JSON or JSONL files all at once. Start with `view tr`, narrow with `--steps` or `--tool-call` when useful, then read only targeted evidence.
- Do not treat `<workspace>/state.db` as an adapter DB.
- Do not omit `-r <workspace>` when reading saved snapshots from `<workspace>/state.db`.
- Do not scan orphaned `runs/` directories to invent saved sources.
- Do not pass a session artifact directory to `view tr -p`; pass the Trial cell directory, a descendant inside it, or ask which cell to target.
- Use `scripts/report_tools.py subjects` only when the target cell path is missing; `references/analysis-guide.md` shows the report-based identity workflow.
- Do not assume every task needs `analysis.json`, `analysis.md`, a report, or `serve`.
- Preserve canonical session ids in reports; use aliases only as display aids.

## Agent Trajectory Analysis

Analyze trajectories by starting from the user's task and citing evidence, not by inventing a fixed score. Use step ids, tool calls, warnings, final metrics, timing metadata, token/cost fields, and final responses as references.

For a single trajectory, assess:

- **Task success**: whether the user goal was actually completed, partially completed, or abandoned.
- **Trajectory quality**: whether the plan and execution path were direct, coherent, and appropriately scoped.
- **Tool use**: whether the agent chose the right tools, supplied valid arguments, interpreted results correctly, and avoided redundant calls.
- **Failures and recovery**: tool errors, retries, dead ends, stuck loops, missed alternatives, and whether recovery was effective.
- **Grounding**: whether claims in the final response are supported by tool output or trajectory evidence.
- **Response quality**: completeness, clarity, instruction following, and useful next steps.
- **Performance and cost**: wall time, active duration, turns, tool calls, tokens, and cost when available.

For multiple trajectories, compare outcomes, failure modes, path length, tool behavior, latency/duration, token/cost footprint, and regressions. Do not judge a comparison from final answer text alone.

Write explanations and analysis in the user's language by default. Use `references/analysis-guide.md` when the task requires a written analysis report, deeper comparison method, Trial-cell import, or report-based identity extraction. When analysis output is requested, write one JSON or Markdown analysis report at the requested path; write both only when they are complementary. Import the report into a Trial cell only when the user asks or when it is useful to render the analysis inside peval-py.
