---
name: peval-py
description: Use when working with peval-py for retained agent sessions, trajectory JSONL or ATIF files, report JSON, input tables, or adapter-owned SQLite databases; rendering peval-py reports; exporting ATIF trajectories; listing DB sessions; creating analysis.json or analysis.md artifacts that peval-py can recognize; validating annotations.analysis; or using peval-py serve for offline session diagnostics.
---

# peval-py

Use `peval-py` for offline inspection of retained agent sessions and trajectories. The common outputs are independent and composable:

- reports from `view tr`
- ATIF trajectory JSON from `export tr`
- workspace-recognized analysis artifacts: `analysis.json`, `analysis.md`, or both when complementary
- local browsing with `serve`

Analysis artifact construction is a core use of this skill, but do not assume every request needs both artifacts and a regenerated report.

When the user wants peval-py reports or `serve` to recognize analysis artifacts, prefer this workspace path:

```text
<workspace>/runs/<analysis_eval_slug>/<agent-id>/<session-id>/<cell_key>/analysis.json
<workspace>/runs/<analysis_eval_slug>/<agent-id>/<session-id>/<cell_key>/analysis.md
```

`analysis.json` is the fixed-format, machine-readable analysis artifact. Current peval-py report generation recognizes its top-level `summary` field.

`analysis.md` is a free-form analysis artifact readable by humans and agents. Its structure and content are not fixed by the skill; current peval-py report generation renders it as the selected Trial Analysis body.

## Choose The Workflow

- **Render or export only**: read `references/cli-workflows.md`, choose `view tr` for reports or `export tr` for a single ATIF trajectory, and stop when the requested output exists.
- **List or select DB sessions**: use `view tr --list` or `--list-interactive`; do not create analysis files unless requested.
- **Create analysis artifacts**: read `references/analysis-artifacts.md`. Generate or inspect a JSON report only as needed to derive the displayed `session_id`, Trial key, adapter, and agent identity. Write one artifact by default, or write both only when they are complementary. Use the user-requested path, or the preferred workspace path when peval-py recognition is desired.
- **Combine artifacts with reports**: create or update the requested analysis artifact(s), then re-run `view tr -r <workspace>` or run from the workspace root/descendant and validate the relevant `annotations.analysis[]` fields.
- **Browse locally**: use `serve` only when the user asks for interactive saved-source browsing.

## Guardrails

- Do not mutate source session databases or original trajectory inputs.
- Do not assume every task needs `analysis.json`, `analysis.md`, a report, or `serve`.
- Do not write analysis into static `report.json`; use separate analysis artifacts when analysis output is requested.
- Prefer the workspace `runs/...` path only when peval-py recognition is useful or requested. Otherwise respect the user's requested output location.
- If more than one analysis cell exists for the same session, stop and ask for the intended cell. peval-py intentionally omits ambiguous workspace analysis.
- Preserve canonical session ids in reports. Use source aliases only as display aids.

## References

- `references/cli-workflows.md`: command recipes for `view tr`, `export tr`, DB session listing, workspace discovery, validation, and `serve`.
- `references/analysis-artifacts.md`: analysis artifact placement, `analysis.json` schema guidance, `analysis.md` template, and report identity extraction.
- `scripts/report_tools.py`: use `subjects` to extract session, agent, adapter, and Trial identities; use `check` to validate report JSON and optional `annotations.analysis` recognition.
