# peval-py CLI Workflows

## Initialize A Workspace

```sh
peval-py init -r <workspace> --json
```

## Build A JSON Report

Use `view tr` when the user asks for a report, or when a JSON report is useful as an intermediate artifact for analysis placement. Pass `-r <workspace>` when workspace config or report-recognized analysis artifacts must be discovered from outside the workspace. If `-r` is omitted, run `view tr` from the workspace root or a descendant so current-directory discovery finds `peval-py.toml`.

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

If `--agent-name` is omitted, peval-py uses the effective adapter id as the report-recognized analysis `<agent-id>`.

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

## Validate A Report

Validate report JSON:

```sh
python <skill-dir>/scripts/report_tools.py check <workspace>/report.json
```

When Trial cell artifacts are intended to be recognized by peval-py reports, re-run the same report command with `-r <workspace>` or from the workspace root/descendant, then validate the fields matching the artifact(s) you wrote. Pass `--trial-key <trial-key>` or `--index <n>` when the report contains more than one Trial:

```sh
# If notes.md was written:
python <skill-dir>/scripts/report_tools.py check <workspace>/report.json --trial-key <trial-key> --require-notes

# If analysis.json was written:
python <skill-dir>/scripts/report_tools.py check <workspace>/report.json --trial-key <trial-key> --require-summary

# If analysis findings were written:
python <skill-dir>/scripts/report_tools.py check <workspace>/report.json --trial-key <trial-key> --require-findings

# If analysis.md was written:
python <skill-dir>/scripts/report_tools.py check <workspace>/report.json --trial-key <trial-key> --require-md-report
```

## Render HTML Or Serve

Render static HTML when the user wants an HTML report. If the report should include recognized analysis artifacts, validate the JSON report first.

```sh
peval-py view tr -r <workspace> <same-input-flags> -f html -o <workspace>/report.html
```

Start local `serve` only for interactive browsing:

```sh
peval-py serve -r <workspace> <source-flags>
```

`serve` binds localhost only, defaults to ports `58010..58029`, and prints the selected URL. It overlays current workspace-side `analysis.json`, `analysis.md`, and `notes.md` when composing the active report.
