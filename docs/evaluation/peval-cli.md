# peval CLI Workflows

`peval` works with three local concepts:

- a workspace, initialized by `peval init`, which stores `runs/`, `views/`,
  `datasets/`, workspace registries, and helper scripts
- benchmarks rooted at `benchmark.toml`, which own typed sources and sets
- eval configs selected with `--config/-c`, which choose one benchmark, agents,
  and task filters for a runnable plan

Commands that read or write the workspace accept `--root/-r <dir>` or
`PEVAL_ROOT`. Without either, `peval` discovers a current-or-parent workspace,
then the default workspace recorded in `$PSYCHEVO_HOME/peval-config.toml`.

## Init

```bash
mkdir -p ~/psychevo-evals/local
cd ~/psychevo-evals/local
peval init --default
```

`peval init` creates missing workspace files and directories. It does not create
or modify `.gitignore`, `.cache`, or `dashboard.html`.

## Config Selection

Primary path:

```bash
peval check --config evals/pidx.eval.toml --json
```

One-off benchmark path:

```bash
peval check \
  --benchmark /path/to/psychevo/crates/psychevo-eval/benchmarks/pidx-coding/benchmark.toml \
  --agent psychevo \
  --task-set pidx \
  --task pidx/patch-add \
  --json
```

When `--benchmark` is an id, it is resolved from eval, workspace, then user
registry config. Direct `--benchmark` use always requires `--agent` and at
least one task selector. The named agent must already exist in workspace or user
registry config because direct benchmark selection has no inline eval layer.

## Doctor And List

```bash
peval doctor --config evals/pidx.eval.toml --json
peval list --config evals/pidx.eval.toml --kind task-sets
peval list --config evals/pidx.eval.toml --kind agents --json
peval list --kind benchmarks --root ~/psychevo-evals/local --json
peval list --kind datasets --json
```

Stored result inspection uses `peval view`, not `peval list --kind runs`.

## Check

```bash
peval check --config evals/pidx.eval.toml --json
peval check --config evals/pidx.eval.toml --task-set pidx --agent psychevo --json
peval check --config evals/pidx.eval.toml --live --json
```

`check` validates config loading, registry resolution, and matrix expansion
without running candidates or calling providers. `--live` is an explicit opt-in
for provider, official-tool, Docker, or network readiness probes; it still does
not execute benchmark cases.

## Run

```bash
peval run \
  --config evals/pidx.eval.toml \
  --task-set pidx \
  --agent psychevo \
  --include workspace \
  --json
```

Expected JSON shape:

```json
{
  "schema_version": 7,
  "benchmark": "pidx-coding",
  "selected_cells": 1,
  "executed_cells": 1,
  "reused_cells": 0,
  "overwritten_cells": 0,
  "retried_cells": 0,
  "passed_cells": 1,
  "failed_cells": 0,
  "status": "passed",
  "cells": [
    {
      "cell_key": "60ac314cc7fecb5c",
      "cell_root": "<workspace>/runs/pidx-coding/psychevo/pidx_patch-add/60ac314cc7fecb5c",
      "action": "executed"
    }
  ]
}
```

Repeated runs reuse completed cells when their semantic fingerprint still
matches. Missing, malformed, setup-failed, and runtime-failed cells are retried.
Use `--overwrite` to rerun selected cells and replace their cell directories.
`--run-id` is no longer supported.

`--include` accepts comma-separated debug artifact names and may be repeated.
`workspace` retains the final case workspace. `patch` and `raw-bodies` are
accepted for bridge-specific artifacts when selected adapters produce them.

Use `--output-root <dir>` for isolated one-off output outside workspace reuse:

```bash
peval run --config evals/pidx.eval.toml --output-root /tmp/peval-runs --json
```

## View

Render the selected benchmark:

```bash
peval view --config evals/pidx.eval.toml -i summary,matrix --format markdown
peval view --config evals/pidx.eval.toml -i summary,matrix,usage --format json
peval view --config evals/pidx.eval.toml -i all --format html -o
peval view --config evals/pidx.eval.toml --output view.html
peval view --config evals/pidx.eval.toml -o
```

Scope by path and then filter:

```bash
peval view --path runs/pidx-coding/psychevo --status passed
peval view --config evals/pidx.eval.toml --task-set pidx --agent psychevo --task pidx/patch-add
peval view --config evals/pidx.eval.toml --group-by agent,task-set,status --format markdown
```

`-i/--include` accepts comma-separated values and may be repeated. The default
is `summary,matrix`; add `usage` when you want token/cache/cost columns. Use
`-i all` for the full static diagnostic report, which expands to
`summary,matrix,usage,artifacts,timeline,atif,logs,analysis,diff`. If
`--format` is omitted, `--output .json`, `.md`, or `.html` selects the format;
without `--output`, Markdown is used. `-o/--output` may be used without a path;
that writes HTML by default to a `views/` path mirroring the selected `runs/`
scope, such as `views/pidx-coding/psychevo/index.html`.

Markdown output:

```markdown
# peval view

## Summary

- cells: 1
- passed: 1
- failed: 0
- status: Passed
```

JSON output has its own view DTO schema version. JSON may include artifact
paths and bounded diagnostic previews. Markdown and HTML full reports show
diagnostic sections and local paths, but still avoid unbounded raw trajectory or
log bodies.

## Dataset Import

```bash
peval dataset import ./data/tasks.jsonl \
  --id local-tasks \
  --name "Local tasks" \
  --kind jsonl \
  --split dev \
  --json
```

The first implementation records a dataset manifest in the workspace. It does
not download benchmark data.

## Removed Surfaces

`peval project`, `peval report`, `peval compare`, `peval replay`, `latest`,
`--run-id`, `--project`, `--suite`, `peval list --kind suites`, and
`peval list --kind runs` belonged to older layouts. Use `--config` or
`--benchmark`, `--task-set`, and `peval view` over cell facts instead. Eval
configs use `[select] sets = [...]` even though the CLI filter flag remains
`--task-set`.
