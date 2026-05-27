# Evaluation Guide

Use `peval` to check eval configs, run or reuse agent/task cells, and review
local artifacts. The checked-in seed benchmark is
`crates/psychevo-eval/benchmarks/pidx-coding/`.

`peval` is for agent-behavior evaluation. It does not replace Psychevo's normal
wide or narrow validation paths.

## Install

From a checkout, install both product CLIs:

```bash
sh scripts/install.sh --with-peval
```

From source without installing, use Cargo:

```bash
cargo run -p psychevo-eval --bin peval -- --help
```

## Create A Workspace

A peval root is an evaluation workspace. It stores reusable cell runs, datasets,
workspace registry entries, and local helper scripts:

```bash
mkdir -p ~/psychevo-evals/local
cd ~/psychevo-evals/local
peval init --default
```

This creates or repairs:

```text
peval.toml
scripts/
runs/
datasets/
```

`peval.toml` is a schema v2 workspace registry config. User-wide defaults and
registries live in `$PSYCHEVO_HOME/peval-config.toml`. Older workspaces may
still contain `index.json`, `latest.json`, `.cache/`, `dashboard.html`, or v2
`summary.json` files; current `peval run` and `peval view` ignore them.

## First Eval Config

Benchmarks define stable task data in `benchmark.toml`. Eval configs define a
runnable plan: which benchmark, which agents, and which tasks to select.

Use the seed template directly from the repository:

```bash
config=/path/to/psychevo/crates/psychevo-eval/templates/pidx-psychevo-patch-add.eval.toml
peval check \
  --config "$config" \
  --json
```

For repeated local work, copy an eval config into your workspace, commonly under
`evals/`, and edit it there. You can also use `--benchmark <id-or-path>` for a
one-off run, but then you must pass `--agent` plus `--task-set` or `--task`.

## First Check

`check` validates config loading, registry resolution, and matrix expansion. It
does not call providers or run agent commands:

```bash
peval check --config "$config" --task-set base --agent psychevo --json
```

Expected shape:

```json
{
  "cases": 1,
  "benchmark": "pidx-coding",
  "status": "ok"
}
```

## First Run

Running executes or reuses selected semantic cells. Each cell is stored under
`runs/<benchmark>/<agent-id>/<task-id>/<cell-key>/`:

```bash
peval run --config "$config" --task-set base --agent psychevo --json
```

Repeated runs skip completed matching cells. Use `--overwrite` to rerun selected
cells and replace their cell directories.

## Review Results

`peval view` is the built-in reporting, comparison, and inspection surface:

```bash
peval view --config "$config" -i summary,matrix,usage --format markdown
peval view --config "$config" --group-by agent,task --format json
peval view --config "$config" --output /tmp/peval-view.html
```

Scope by path or filters:

```bash
peval view --root ~/psychevo-evals/local --path runs/pidx-coding/psychevo
peval view --config "$config" --task-set base --agent psychevo --status passed
```

Markdown and HTML views do not inline raw trajectories or evaluator logs. JSON
may include artifact paths for explicit follow-up inspection.

## More Detail

- [peval CLI Workflows](peval-cli.md)
- [Authoring Eval Configs And Benchmarks](authoring.md)
- [Live Psychevo Evaluation](live-psychevo.md)
- Specs: [peval CLI](../../specs/300-peval-cli/spec.md),
  [evaluation framework](../../specs/095-evaluation-framework/spec.md),
  [agent evaluation](../../specs/340-agent-evaluation/spec.md), and
  [coding evaluation](../../specs/350-coding-evaluation/spec.md)
