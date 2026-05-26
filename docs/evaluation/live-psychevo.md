# Live Psychevo Evaluation

Live evaluation runs the real Psychevo runtime through `pevo run`. It may call
your configured provider. Use it when you want to evaluate the current product
behavior, not only deterministic fake agents.

## Prerequisites

Confirm `pevo` works before invoking `peval` live runs:

```bash
pevo model current
pevo run "hello"
```

Install `peval` if needed:

```bash
sh scripts/install.sh --with-peval
```

Initialize an evaluation store or choose one explicitly:

```bash
peval init
export PEVAL_ROOT="$PWD/.local/evals"
```

`PEVAL_ROOT` overrides the user-level store for the current shell.

## Fixture Smoke

Run the repository live smoke:

```bash
scripts/eval/live-psychevo-smoke.sh
```

The script defaults to:

- project: `crates/psychevo-eval/fixtures/local-coding`
- suite: `rust-swe`
- agent: `psychevo-live`
- run id: `live-psychevo-smoke`

Override those values:

```bash
PEVAL_LIVE_SUITE=rust-swe \
PEVAL_LIVE_AGENT=psychevo-live \
PEVAL_LIVE_RUN_ID=my-live-smoke \
PEVAL_ROOT="$PWD/.local/evals" \
scripts/eval/live-psychevo-smoke.sh crates/psychevo-eval/fixtures/local-coding
```

The helper creates a temporary `PSYCHEVO_HOME` and `PSYCHEVO_DB`, but it can
reuse your real `PSYCHEVO_CONFIG` when set. If `PSYCHEVO_CONFIG` is empty and
`$PSYCHEVO_HOME/config.toml` exists before isolation, the helper points
`PSYCHEVO_CONFIG` at that config file.

## Custom Live Project

Create this project layout:

```text
my-live-eval/
  eval.toml
  agents/
    psychevo-live.toml
  suites/
    rust-swe.toml
  tasks/
    rust-swe-add/
      task.toml
      workspace/
        Cargo.toml
        src/lib.rs
      scripts/
        score.sh
        psychevo-live-wrapper.sh
```

`eval.toml`:

```toml
schema_version = 1
name = "my-live-eval"
output_root = "runs/my-live-eval"
allow_live = true
```

`agents/psychevo-live.toml`:

```toml
schema_version = 1
id = "psychevo-live"
name = "Psychevo live adapter"
kind = "psychevo"

[psychevo]
command = "sh"
args = [
  "scripts/psychevo-live-wrapper.sh",
  "{workspace}",
  "{prompt}",
]
```

`suites/rust-swe.toml`:

```toml
schema_version = 1
id = "rust-swe"
name = "Local Rust SWE-style fixture"
description = "Repair a tiny Rust crate with a live Psychevo run."
agents = ["psychevo-live"]
tasks = ["../tasks/rust-swe-add/task.toml"]
```

`tasks/rust-swe-add/task.toml`:

```toml
schema_version = 1
id = "rust-swe-add"
name = "Repair the add function"
kind = "swe-style"

[prompt]
text = "The local Rust crate has a failing unit test because add subtracts instead of adding. Fix the implementation so the tests pass."

[workspace]
source = "workspace"

[scorer]
command = ["sh", "scripts/score.sh"]
timeout_seconds = 30
```

`tasks/rust-swe-add/workspace/Cargo.toml`:

```toml
[package]
name = "rust-swe-add"
version = "0.1.0"
edition = "2024"
```

`tasks/rust-swe-add/workspace/src/lib.rs`:

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

`tasks/rust-swe-add/scripts/score.sh`:

```sh
#!/usr/bin/env sh
set -eu

if cargo test --quiet >peval-score.stdout 2>peval-score.stderr; then
    printf '%s\n' '{"schema_version":1,"passed":true,"score":1.0,"message":"cargo test passed","details":{"scorer":"cargo-test"}}'
else
    printf '%s\n' '{"schema_version":1,"passed":false,"score":0.0,"message":"cargo test failed","details":{"scorer":"cargo-test"}}'
    cat peval-score.stderr >&2 || true
fi
```

`tasks/rust-swe-add/scripts/psychevo-live-wrapper.sh`:

```sh
#!/usr/bin/env sh
set -eu

workspace="$1"
prompt="$2"
timeout_seconds="${PEVAL_LIVE_PEVO_TIMEOUT_SECONDS:-75}"

cd "$workspace"
timeout -k 5 "$timeout_seconds" pevo run \
    --dir "$workspace" \
    --format json \
    --variant none \
    --dangerously-skip-permissions \
    --no-skills \
    --no-agents \
    "$prompt"
```

Make scripts executable if your checkout preserves executable bits poorly:

```bash
chmod +x my-live-eval/tasks/rust-swe-add/scripts/*.sh
```

Check, then run:

```bash
peval check --config my-live-eval/eval.toml --suite rust-swe --agent psychevo-live
peval run --config my-live-eval/eval.toml --suite rust-swe --agent psychevo-live --json
```

Expected JSON shape:

```json
{
  "run_id": "<run-id>",
  "status": "passed",
  "artifact_root": "<peval-root>/runs/my-live-eval/<run-id>",
  "total_cases": 1
}
```

## Review A Live Run

Render the latest report:

```bash
peval report --config my-live-eval/eval.toml --run-root latest --format markdown
peval report --config my-live-eval/eval.toml --run-root latest --format html --output live-report.html
```

Replay stored trajectory events:

```bash
peval replay --config my-live-eval/eval.toml --run-root latest --json
```

A failed live run can still produce useful artifacts. Start with
`summary.json`, then read `report.md`, scorer logs, and trajectory links from
the artifact root.

## Safety Checks

- Keep `allow_live = false` until the project should run real agents.
- Use `peval check` before `peval run`.
- Use `PEVAL_ROOT` or `--root` when you want an isolated evaluation store.
- Use a temporary `PSYCHEVO_HOME` and `PSYCHEVO_DB` when a wrapper should not
  touch normal user state.
- Keep provider credentials in Psychevo config or `.env` files; do not write
  keys into evaluation manifests.
