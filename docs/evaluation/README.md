# Evaluation Guide

Use `peval` to check evaluation projects, run candidate agents, and review
local reports. It is the command-line surface for `psychevo-eval`.

The current implementation supports local fixture projects, fake candidates,
the Psychevo live adapter, store-backed run indexes, reports, comparisons,
replay, and local dataset records. Benchmark integrations and richer adapter
surfaces are specified under `specs/`, but this guide stays on behavior you can
run today.

## Install

From a checkout, install both product CLIs:

```bash
sh scripts/install.sh --with-peval
```

From the hosted install script:

```bash
curl -fsSL https://raw.githubusercontent.com/wowfun/psychevo/main/scripts/install.sh | sh -s -- --with-peval
```

The installer verifies `pevo --help` and `peval --help`. It runs `pevo init`
unless you pass `--no-init`; it does not run `peval init`.

From a source checkout without installing, use Cargo:

```bash
cargo run -p psychevo-eval --bin peval -- --help
```

In the examples below, replace `peval` with
`cargo run -p psychevo-eval --bin peval --` when you have not installed the
binary.

## Set Up The Store

Initialize the evaluation store:

```bash
peval init
```

By default this writes `$PSYCHEVO_HOME/peval.toml` and points the store at
`$HOME/.local/evals`. Store-backed commands also accept `--root <dir>` or the
`PEVAL_ROOT` environment variable:

```bash
PEVAL_ROOT="$PWD/.local/evals" peval list --kind runs --json
peval run --root "$PWD/.local/evals" --config crates/psychevo-eval/fixtures/local-coding/eval.toml
```

Use an explicit root when you want a workspace-local or CI-local store. Use
`peval init` when you want a normal user-level store.

## First Local Run

The repository includes a deterministic local fixture project:

```bash
peval check --config crates/psychevo-eval/fixtures/local-coding/eval.toml --json
```

Expected shape:

```json
{
  "cases": 6,
  "project": "local-coding",
  "status": "ok"
}
```

Run one passing local case:

```bash
peval run \
  --config crates/psychevo-eval/fixtures/local-coding/eval.toml \
  --suite rust-swe \
  --agent fake-pass
```

Human output includes the run id, status, artifact root, and case counts:

```text
run <run-id>: Passed
artifact root: <peval-root>/runs/local-coding/<run-id>
cases: 1 passed / 0 failed / 1 total
```

The full default matrix includes `fake-fail`, so it exits with status `1` by
design. Use that path when you want report and failure diagnostics.

## First Live Run

Live Psychevo evaluation uses the real `pevo run` path and may call a provider.
Confirm your normal Psychevo configuration first:

```bash
pevo model current
pevo run "hello"
```

Then run the live smoke helper:

```bash
scripts/eval/live-psychevo-smoke.sh
```

The helper uses `crates/psychevo-eval/fixtures/local-coding` by default,
selects the `rust-swe` suite and `psychevo-live` agent, creates an isolated
temporary `PSYCHEVO_HOME` and `PSYCHEVO_DB`, and writes evaluation artifacts
under `PEVAL_ROOT` or `<repo>/.local/evals`.

See [Live Psychevo Evaluation](live-psychevo.md) for the fixture smoke path and
a custom live evaluation tutorial.

## Review Artifacts

Each run writes a `summary.json`, `report.md`, and `report.html` under the
artifact root:

```bash
peval report --run-root latest --format markdown
peval report --run-root latest --format html --output /tmp/peval-report.html
peval replay --run-root latest --json
```

`latest` resolves through the persistent store. Add `--config`, `--suite`, or
`--agent` when you want a narrower selector.

## More Detail

- [peval CLI Workflows](peval-cli.md)
- [Authoring Evaluation Projects](authoring.md)
- [Live Psychevo Evaluation](live-psychevo.md)
- Specs: [peval CLI](../../specs/300-peval-cli/spec.md),
  [evaluation framework](../../specs/095-evaluation-framework/spec.md),
  [agent evaluation](../../specs/340-agent-evaluation/spec.md), and
  [coding evaluation](../../specs/350-coding-evaluation/spec.md)
