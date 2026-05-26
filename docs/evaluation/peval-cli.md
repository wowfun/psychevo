# peval CLI Workflows

`peval` commands work with evaluation projects rooted at `eval.toml`. Commands
that inspect or run a project accept `--config/-c <path>`. Commands that read
or write the persistent store accept `--root <dir>` or `PEVAL_ROOT`.

## Init

Create a user-level store configuration:

```bash
peval init
```

Use a custom store root:

```bash
peval init --root "$HOME/evals"
```

Changing an existing `$PSYCHEVO_HOME/peval.toml` root requires `--force`.

## Doctor

Inspect the selected project and store:

```bash
peval doctor --config crates/psychevo-eval/fixtures/local-coding/eval.toml --json
```

Expected shape:

```json
{
  "project": "local-coding",
  "allow_live": true,
  "fake_adapter": "available",
  "psychevo_adapter": "manifest-gated",
  "suites": 3
}
```

`doctor` does not execute benchmark tasks.

## List

List project inventory:

```bash
peval list --config crates/psychevo-eval/fixtures/local-coding/eval.toml --kind suites
peval list --config crates/psychevo-eval/fixtures/local-coding/eval.toml --kind agents --json
```

List store inventory:

```bash
peval list --kind runs
peval list --kind datasets --json
```

Store-only listing needs an initialized store, `--root`, or `PEVAL_ROOT`.

## Check

Validate manifests and matrix expansion without running candidates:

```bash
peval check --config crates/psychevo-eval/fixtures/local-coding/eval.toml --json
```

Filter the matrix:

```bash
peval check \
  --config crates/psychevo-eval/fixtures/local-coding/eval.toml \
  --suite rust-swe \
  --agent fake-pass
```

Use `check` before a live run. It verifies structure and live gates without
calling a provider.

## Run

Run an evaluation matrix:

```bash
peval run \
  --config crates/psychevo-eval/fixtures/local-coding/eval.toml \
  --suite rust-swe \
  --agent fake-pass
```

Use `--json` when another process will parse the result:

```bash
peval run \
  --config crates/psychevo-eval/fixtures/local-coding/eval.toml \
  --suite rust-swe \
  --agent fake-pass \
  --json
```

Expected shape:

```json
{
  "run_id": "<run-id>",
  "status": "passed",
  "artifact_root": "<peval-root>/runs/local-coding/<run-id>",
  "passed_cases": 1,
  "failed_cases": 0,
  "total_cases": 1
}
```

`peval run` exits with status `0` only when every selected case passes. It
continues after per-case failures and writes artifacts before exiting.

Use `--output-root <dir>` for one-off output outside the persistent store:

```bash
peval run --config eval.toml --output-root /tmp/peval-runs
```

That run writes `/tmp/peval-runs/<run-id>` and does not update the store index.

## Report

Render a stored run:

```bash
peval report --run-root latest --format markdown
peval report --run-root latest --format html --output report.html
peval report --run-root latest --format json
```

Scope `latest` through a config, suite, agent, or status:

```bash
peval report \
  --config crates/psychevo-eval/fixtures/local-coding/eval.toml \
  --run-root latest \
  --suite rust-swe \
  --agent fake-pass
```

`report` reads artifacts. It does not rerun candidates or scorers.

## Compare

Compare two stored runs or artifact roots:

```bash
peval compare latest /path/to/older/run --json
```

Comparison uses structured summaries and case records. It does not parse
terminal logs.

## Replay

Replay stored trajectory events:

```bash
peval replay --run-root latest
peval replay --run-root latest --case <case-id> --json
```

Replay is for diagnosis. It reads artifacts and does not execute the agent.

## Dataset Import

Register a local dataset payload:

```bash
peval dataset import ./data/tasks.jsonl \
  --id local-tasks \
  --name "Local tasks" \
  --kind jsonl \
  --split dev \
  --json
```

The first implementation records a dataset manifest in the store. It does not
download benchmark data.
