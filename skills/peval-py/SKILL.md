---
name: peval-py
description: Use when working with peval-py for retained agent sessions, trajectory JSONL or ATIF files, report JSON, input tables, or adapter-owned SQLite databases; rendering peval-py reports; exporting ATIF trajectories; listing DB sessions; creating notes.md, analysis.json, or analysis.md Trial cell artifacts that peval-py can recognize; validating annotations.notes or annotations.analysis; or using peval-py serve for offline session diagnostics.
---

# peval-py

Use `peval-py` for offline inspection of retained agent sessions and trajectories. The common outputs are independent and composable:

- reports from `view tr`
- ATIF trajectory JSON from `export tr`
- workspace-recognized Trial cell artifacts: `notes.md`, `analysis.json`, `analysis.md`, or complementary analysis files
- local browsing with `serve`

Trial notes and analysis artifact construction are core uses of this skill, but do not assume every request needs all artifacts and a regenerated report.

When the user wants peval-py reports or `serve` to recognize Trial cell artifacts, prefer this workspace path:

```text
<workspace>/runs/<analysis_eval_slug>/<agent-id>/<session-id>/<cell_key>/notes.md
<workspace>/runs/<analysis_eval_slug>/<agent-id>/<session-id>/<cell_key>/analysis.json
<workspace>/runs/<analysis_eval_slug>/<agent-id>/<session-id>/<cell_key>/analysis.md
```

Use the rendered Trial key from `trajectory_meta[].trial_key` for `<cell_key>`,
normalized as a path segment. Use `trajectory.session_id` for `<session-id>` when present; otherwise use the rendered Trial key normalized as the session segment.

`notes.md` is a cell-local manual Trial note. Current peval-py report generation recognizes it as `annotations.notes[]` with `source = "cell"` and `label = "notes.md"`.

`analysis.json` is the fixed-format, machine-readable analysis artifact. Current peval-py report generation recognizes `summary` plus a typed whitelist of incremental fields such as findings, recommendations, limitations, commands, metrics, confidence, subject, and analysis status.

`analysis.md` is a free-form analysis artifact readable by humans and agents. Its structure and content are not fixed by the skill; current peval-py report generation renders it as the selected Trial Analysis body.

## Choose The Workflow

- **Render or export only**: read `references/cli-workflows.md`, choose `view tr` for reports or `export tr` for a single ATIF trajectory, and stop when the requested output exists.
- **List or select DB sessions**: use `view tr --list` or `--list-interactive`; do not create analysis files unless requested.
- **Create Trial notes or analysis artifacts**: read `references/analysis-artifacts.md`. Generate or inspect a JSON report only as needed to derive the displayed session id, Trial key, adapter, and agent identity. For notes, write `notes.md`. For analysis, write one artifact by default, or write both only when they are complementary. Use the user-requested path, or the preferred workspace cell path when peval-py recognition is desired.
- **Combine artifacts with reports**: create or update the requested cell artifact(s), then re-run `view tr -r <workspace>` or run from the workspace root/descendant and validate the relevant `annotations.notes[]` or `annotations.analysis[]` fields.
- **Browse locally**: use `serve` only when the user asks for interactive saved-source browsing.

## Guardrails

- Do not mutate source session databases or original trajectory inputs.
- Do not assume every task needs `analysis.json`, `analysis.md`, a report, or `serve`.
- Do not write analysis into static `report.json`; use separate analysis artifacts when analysis output is requested.
- Prefer the workspace `runs/...` path only when peval-py recognition is useful or requested. Otherwise respect the user's requested output location.
- If the user has not identified a Trial and a report contains multiple Trials, inspect the report subjects and ask for the intended Trial before writing a recognized cell artifact.
- Preserve canonical session ids in reports. Use source aliases only as display aids.

## References

- `references/cli-workflows.md`: command recipes for `view tr`, `export tr`, DB session listing, workspace discovery, validation, and `serve`.
- `references/analysis-artifacts.md`: Trial cell artifact placement, `notes.md` guidance, `analysis.json` schema guidance, `analysis.md` template, and report identity extraction.
- `scripts/report_tools.py`: use `subjects` to extract raw and path-normalized session, agent, adapter, and Trial identities; use `check` to validate report JSON and optional `annotations.notes` or `annotations.analysis` recognition.
