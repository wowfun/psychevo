# peval-py CLI Workflows

## Initialize A Workspace

```sh
peval-py init -r <workspace> --json
```

## Build A JSON Report

Use `view tr` when the user asks for a peval-py report, or when a JSON report is useful for deriving Trial identities and `run_path` before importing analysis. Pass `-r <workspace>` when workspace config or imported analysis files must be discovered from outside the workspace. If `-r` is omitted, run `view tr` from the workspace root or a descendant so current-directory discovery finds `peval-py.toml`.

Path input:

```sh
peval-py view tr \
  -r <workspace> \
  -a <adapter> \
  -p <path-to-session-or-atif> \
  --agent-name <agent-id> \
  -f json \
  -o <workspace>/report.json
```

DB input with an explicit session:

```sh
peval-py view tr \
  -r <workspace> \
  -a <adapter> \
  -d <path-to-state-db> \
  -s <session-id> \
  --agent-name <agent-id> \
  -f json \
  -o <workspace>/report.json
```

If `--agent-name` is omitted, peval-py uses the effective adapter id as the imported analysis `<agent-id>`.

## List DB Sessions

Use this before selecting a DB session when the session id is unknown:

```sh
peval-py view tr -r <workspace> -a <adapter> -d <path-to-state-db> --list
```

With one DB input, `-s #3` selects the third listed session, while bare numeric `-s 3` first tries session id `3` and then falls back to index 3. With multiple DB inputs, bind selections with `dN=`, such as `-s d1=#3`.

## Export One ATIF Trajectory

Export is single-session only:

```sh
peval-py export tr \
  -r <workspace> \
  -a <adapter> \
  -p <path-to-session> \
  --agent-name <agent-id> \
  -o <workspace>/trajectory.json
```

Use `view tr`, not `export tr`, for multi-session comparison reports.

## Import Analysis Reports

Use `import analysis` when an existing JSON or Markdown analysis report should be attached to a peval-py Trial cell. If the user did not provide the cell path, use `report_tools.py subjects` on a generated report to find the `run_path`.

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

For JSON field guidance, read `references/analysis-artifacts.md`.

## Render Or Inspect After Import

After importing analysis into a Trial cell, re-run the same report command with
`-r <workspace>` or from the workspace root/descendant when the user asks to see
the report output:

```sh
peval-py view tr -r <workspace> <same-input-flags> -f json -o <workspace>/report.json
```

## Render HTML Or Serve

Render static HTML when the user wants an HTML report. If the report should include imported analysis files, validate the JSON report first.

```sh
peval-py view tr -r <workspace> <same-input-flags> -f html -o <workspace>/report.html
```

Start local `serve` only for interactive browsing:

```sh
peval-py serve -r <workspace> <source-flags>
```

`serve` binds localhost only, defaults to ports `58010..58029`, and prints the selected URL. It overlays current workspace-side `analysis.json` and `analysis.md` when composing the active report.
