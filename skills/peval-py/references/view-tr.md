# view tr Usage

Use this guide when the user asks to inspect, list, render, or compare retained trajectories with `peval-py view tr`. Prefer the smallest command that produces the needed evidence.

## Choose Input

- Use `-p <path-to-jsonl-or-atif-trajectory-or-cell-dir>` for JSONL session files, ATIF `trajectory.json`, or a Trial cell containing `agent/trajectory.json` and `agent/trajectory_meta.json`.
- Trial cell paths are tolerant: `<cell-dir>/**`, `<cell-dir>/**/*`, and descendants inside the cell are canonicalized to `<cell-dir>` and override conflicting `-r/-a/-d/-s/-i` source flags.
- Use `-d <adapter-db>` for a real adapter-owned DB.
- Use `-d @adapter` when the workspace config has a default DB for that adapter.
- Use `-r <workspace> -d <workspace>/state.db` for saved workspace snapshots.
- Use `-i <manifest.csv|json|xlsx>` when multiple sources are easier to maintain as rows.
- Do not pass a session artifact directory to `view tr -p`; pass the Trial cell directory, a descendant inside it, or choose the target cell first.

Pass `-r <workspace>` whenever workspace config, saved snapshots, or imported analysis overlays must be discovered from outside the workspace.

## Inspect First

`view tr` defaults to bounded inspect mode. Use it before reading large trajectory/report JSON or JSONL files, and before rendering a full report when the user only needs evidence.

```sh
peval-py view tr \
  -r <workspace> \
  -a <adapter> \
  -p <path-to-jsonl-or-atif-trajectory-or-cell-dir>
```

Inspect output is a fixed digest. It includes session, agent/model, token totals, active duration in seconds, tool-call and turn totals, compact step head/tail previews, step/tool duration distributions, slowest steps/tools, token-heavy steps, and tool errors when available. `status` appears only when it is not `passed`; `score` appears only when populated.

## Select Evidence

Use selectors to keep output bounded:

- `--head N` and `--tail N` show first and last steps per source; both default to 2.
- `--top N` controls top duration/token lists; it defaults to 5.
- `--steps VALUE` adds `selected_steps` for matching `step_id` values, suppresses the default digest, and accepts repeated comma/range selectors such as `1,3:5,7:9`.
- `--tool-call ID` adds `selected_tool_calls` for matching `tool_call_id` values and the corresponding tool result when retained data provides one. It works without `--steps`.
- `--source N` restricts output to one one-based source.
- `--max-content-chars N` changes inspect preview length.

Example:

```sh
peval-py view tr \
  -r <workspace> \
  -a <adapter> \
  -p <path-to-jsonl-or-atif-trajectory> \
  --steps <step_id> \
  --tool-call <tool_call_id>
```

Bare `-o` writes a timestamped inspect JSON file and prints the saved path to stdout.

## List Adapter DB Sessions

Use this before selecting a DB session when the session id is unknown:

```sh
peval-py view tr -r <workspace> -a <adapter> -d <adapter-db> --list
```

`-d @adapter` is also valid for listing when the adapter default DB is configured. With one adapter DB input, `-s #3` selects the third listed session. With multiple DB inputs, bind selections with `dN=`, such as `-s d1=#3`.

## Saved Workspace Snapshots

Use `<workspace>/state.db` only with explicit `-r <workspace>`. In this mode it means saved workspace snapshots, not an adapter DB. Rendering should still work when the original source DB or file is no longer available. Do not refresh sources or scan orphaned `runs/` directories.

List saved sources:

```sh
peval-py view tr -r <workspace> -d <workspace>/state.db --list
```

Inspect all active saved sources:

```sh
peval-py view tr \
  -r <workspace> \
  -d <workspace>/state.db \
  -o
```

Inspect one selected saved source:

```sh
peval-py view tr \
  -r <workspace> \
  -d <workspace>/state.db \
  -s <source_key-or-#N-or-unique-session-or-trial> \
  -o
```

If `<workspace>/state.db` is passed without `-r <workspace>`, treat it as a misuse: add `-r <workspace>` for saved snapshots, or use `-d @adapter` / `-d <adapter-db>` for raw adapter DB access.

## Build A Full Report

Use `view tr -m raw` only when the user asks for a full peval-py JSON/HTML report, or when a report is needed to identify Trial subjects before importing analysis. Do not generate a report only to rediscover a Trial cell path the user already provided.

Raw report mode accepts report/conversion overrides such as `--agent-name`, `--agent-version`, `--model`, and `--no-redact`; default inspect mode rejects those flags.
`--trajectory-id` is not supported.

Path input:

```sh
peval-py view tr \
  -m raw \
  -r <workspace> \
  -a <adapter> \
  -p <path-to-jsonl-or-atif-trajectory-or-cell-dir> \
  --agent-name <agent-id> \
  -f json \
  -o <workspace>/report.json
```

Adapter DB input with an explicit session:

```sh
peval-py view tr \
  -m raw \
  -r <workspace> \
  -a <adapter> \
  -d <adapter-db> \
  -s <session-id> \
  --agent-name <agent-id> \
  -f json \
  -o <workspace>/report.json
```

Saved snapshot report:

```sh
peval-py view tr \
  -m raw \
  -r <workspace> \
  -d <workspace>/state.db \
  -s <source_key-or-#N-or-unique-session-or-trial> \
  -f json \
  -o <workspace>/report.json
```

Render static HTML only when the user wants an HTML report:

```sh
peval-py view tr -m raw -r <workspace> <same-input-flags> -f html -o <workspace>/report.html
```

## After Import

After importing analysis into a Trial cell, inspect with the original trajectory/source input flags, the exact cell directory, and `-r <workspace>`, or run from the workspace root or a descendant when current-directory discovery is enough.

```sh
peval-py view tr -r <workspace> <same-input-flags> -o
```

If the original input was a saved workspace snapshot, use `-r <workspace> -d <workspace>/state.db` plus any needed `-s <selector>`. Switch to `-m raw -f json|html` only when a full report artifact is required.
