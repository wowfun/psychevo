---
name: peval-py
description: Use when working with peval-py for retained agent sessions, trajectory JSONL or ATIF files, report JSON, input tables, or adapter-owned SQLite databases; rendering peval-py reports; exporting ATIF trajectories; listing DB sessions; writing analysis JSON or Markdown reports; importing analysis reports into Trial cell artifacts; or using peval-py serve for offline session diagnostics.
---

# peval-py

Use `peval-py` for offline inspection of retained agent sessions and trajectories. The common outputs are independent and composable:

- reports from `view tr`
- ATIF trajectory JSON from `export tr`
- analysis reports in JSON or Markdown, written wherever the user requests
- imported Trial cell analysis files created through
  `peval-py import analysis`
- local browsing with `serve`

## Choose The Workflow

- **Render or export only**: read `references/cli-workflows.md`, choose `view tr` for reports or `export tr` for a single ATIF trajectory, and stop when the requested output exists.
- **List or select DB sessions**: use `view tr --list` or `--list-interactive`; do not create analysis files unless requested.
- **Create analysis reports**: read `references/analysis-artifacts.md`. Analyze the requested data or cell path, then write one JSON or Markdown analysis report at the user's requested output path. Write both only when they are complementary.
- **Import analysis reports**: when requested, call `peval-py import analysis -r <workspace> --run-path <cell-path> -p <analysis-report>` using the known cell path.
- **Combine analysis with peval-py reports**: import the requested analysis report(s), then re-run `view tr -r <workspace>` or run from the workspace root/descendant when the user asks to render or inspect the report.
- **Browse locally**: use `serve` only when the user asks for interactive saved-source browsing.

## Guardrails

- Do not mutate source session databases or original trajectory inputs.
- Do not assume every task needs `analysis.json`, `analysis.md`, a report, or `serve`.
- Do not write analysis into static `report.json`; use separate analysis report files when analysis output is requested.
- Prefer `peval-py import analysis` only when Trial-cell import is useful or requested. Otherwise write the analysis report at the user's requested output location.
- If the user has not identified a Trial and a report contains multiple Trials, inspect the report subjects and ask for the intended Trial before importing analysis reports.
- Preserve canonical session ids in reports. Use source aliases only as display aids.

## References

- `references/cli-workflows.md`: command recipes for `view tr`, `export tr`, DB session listing, workspace discovery, validation, and `serve`.
- `references/analysis-artifacts.md`: analysis report formats, Trial cell import guidance, `analysis.md` template, and report identity extraction.
- `scripts/report_tools.py`: use `subjects` only when the target cell path is missing.
