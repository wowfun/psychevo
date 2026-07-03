# Live Psychevo Evaluation

Live evaluation runs the real Psychevo runtime through `pevo run`. It may call
your configured provider. Use it when you want to evaluate current product
behavior, not only deterministic test harnesses.

## Prerequisites

Confirm `pevo` works before invoking live runs:

```bash
pevo model current
pevo run "hello"
```

Install `peval` if needed:

```bash
cargo install --locked --path crates/psychevo-eval --force
```

Create or choose a peval workspace:

```bash
mkdir -p ~/psychevo-evals/live
peval init --root ~/psychevo-evals/live --default
```

Use `PSYCHEVO_HOME`, `PSYCHEVO_DB`, or `PSYCHEVO_CONFIG` when a run needs
isolated state or credentials.

## Seed Benchmark Run

From a Psychevo checkout, copy or reference the seed live template:

```bash
mkdir -p ~/psychevo-evals/live/evals
cp crates/psychevo-eval/templates/pidx-psychevo-patch-add.eval.toml \
  ~/psychevo-evals/live/evals/pidx-live.eval.toml
```

If you move the copied file away from the repository, update its `[benchmark]`
path to the absolute path of
`crates/psychevo-eval/benchmarks/pidx-coding/benchmark.toml`.

Check the selected Psychevo matrix:

```bash
peval check \
  --root ~/psychevo-evals/live \
  --config ~/psychevo-evals/live/evals/pidx-live.eval.toml \
  --json
```

Run it:

```bash
peval run \
  --root ~/psychevo-evals/live \
  --config ~/psychevo-evals/live/evals/pidx-live.eval.toml \
  --json
```

The Psychevo adapter uses the current `pevo` executable and selected Psychevo
configuration.

## Custom Live Benchmark

Create this benchmark and eval layout:

```text
my-live/
  benchmark/
    benchmark.toml
    tasks/
      rust-swe-add/
        task.toml
        instruction.md
        environment/
          Cargo.toml
          src/lib.rs
        tests/test.sh
  my-live.eval.toml
```

`benchmark/benchmark.toml`:

```toml
schema_version = 5
id = "my-live-coding"
name = "My live coding benchmark"

[[sources.peval_agent]]
id = "local"
path = "tasks"
verifier_timeout_seconds = 600

[[sources.peval_agent.sets]]
id = "base"
name = "Base"
include = ["rust-swe-add"]
```

`my-live.eval.toml`:

```toml
schema_version = 5
id = "my-live-psychevo"
name = "My live Psychevo eval"

[benchmark]
path = "benchmark/benchmark.toml"

[select]
agents = ["psychevo"]
sets = ["local/base"]
tasks = ["local/rust-swe-add"]

[[agents]]
id = "psychevo"
name = "Psychevo"
kind = "psychevo"
```

`benchmark/tasks/rust-swe-add/task.toml`:

```toml
name = "Repair the add function"
kind = "swe-style"

[verifier]
timeout_seconds = 30
```

`benchmark/tasks/rust-swe-add/instruction.md`:

```markdown
The local Rust crate has a failing unit test because add subtracts instead of adding. Fix the implementation so the tests pass.
```

`benchmark/tasks/rust-swe-add/environment/Cargo.toml`:

```toml
[package]
name = "rust-swe-add"
version = "0.1.0"
edition = "2024"

[lib]
path = "src/lib.rs"
```

`benchmark/tasks/rust-swe-add/environment/src/lib.rs`:

```rust
pub fn add(left: i32, right: i32) -> i32 {
    left - right
}

#[cfg(test)]
mod tests {
    use super::add;

    #[test]
    fn adds_numbers() {
        assert_eq!(add(2, 3), 5);
    }
}
```

`benchmark/tasks/rust-swe-add/tests/test.sh`:

```bash
cargo test --quiet
```

Check and run:

```bash
peval check --root ~/psychevo-evals/live --config "$PWD/my-live/my-live.eval.toml"
peval run --root ~/psychevo-evals/live --config "$PWD/my-live/my-live.eval.toml" --json
```

Expected JSON shape:

```json
{
  "schema_version": 7,
  "selected_cells": 1,
  "executed_cells": 1,
  "reused_cells": 0,
  "status": "passed",
  "cells": [
    {
      "action": "executed"
    }
  ]
}
```

## Review A Live Run

Render views over stored cell facts:

```bash
peval view --root ~/psychevo-evals/live --config "$PWD/my-live/my-live.eval.toml" -i summary,matrix,usage --format markdown
peval view --root ~/psychevo-evals/live --config "$PWD/my-live/my-live.eval.toml" --output live-view.html
peval view --root ~/psychevo-evals/live --config "$PWD/my-live/my-live.eval.toml" -o
peval view --root ~/psychevo-evals/live --path runs/my-live-coding/psychevo --format json
```

A failed live run can still produce useful artifacts. Start with `peval view`,
then inspect the relevant cell directory's `run.json`, `evaluator.stdout`,
`evaluator.stderr`, and `trajectory.jsonl` when you need raw diagnostics.

## Safety Checks

- Use `peval check` before `peval run`.
- Use a dedicated workspace, `PEVAL_ROOT`, or `--root` for isolated evaluation
  artifacts.
- Set `PSYCHEVO_HOME` and `PSYCHEVO_DB` when a run should not touch normal user
  state.
- Keep provider credentials in Psychevo config or `.env` files; do not write keys
  into benchmark manifests or eval configs.
