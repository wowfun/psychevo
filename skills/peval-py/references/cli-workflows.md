# peval-py CLI Workflows

Use these recipes when the user asks to initialize a workspace, export a trajectory, import analysis, or serve retained trajectories.

## Initialize A Workspace

```sh
peval-py init -r <workspace> --json
```

## Export One ATIF Trajectory

Export is single-session only:

```sh
peval-py export tr \
  -r <workspace> \
  -a <adapter> \
  -p <path-to-jsonl-or-atif-trajectory-or-cell-dir> \
  --agent-name <agent-id> \
  -o <workspace>/trajectory.json
```

For an exact Trial cell directory containing `agent/trajectory.json` and `agent/trajectory_meta.json`, `peval-py export tr -p <cell-dir>` can export the retained trajectory without reading the original source DB.

Use `view tr -m raw`, not `export tr`, for multi-session comparison reports.

## Import Analysis Reports

Use `import analysis` when an existing JSON or Markdown analysis report should be attached to a peval-py Trial cell. If the user already provided the cell path, use it directly. If the cell path is missing, use `report_tools.py subjects` on a generated report to find the `run_path`.

JSON report:

```sh
peval-py import analysis \
  -r <workspace> \
  --run-path <cell-path> \
  -p <analysis-report.json>
```

Markdown report:

```sh
peval-py import analysis \
  -r <workspace> \
  --run-path <cell-path> \
  -p <analysis-report.md>
```

Complementary JSON and Markdown reports:

```sh
peval-py import analysis \
  -r <workspace> \
  --run-path <cell-path> \
  -p <analysis-report.json> \
  -p <analysis-report.md>
```

For new report content, use the analysis method and templates in [analysis-guide.md](analysis-guide.md).

## Render Or Inspect After Import

After importing analysis into a Trial cell, inspect with the same source flags through `view tr` unless a full raw report is required.

## Serve

Start local `serve` only for interactive browsing:

```sh
peval-py serve -r <workspace> <source-flags>
```

`serve` binds localhost only, defaults to ports `58010..58029`, and prints the selected URL.
