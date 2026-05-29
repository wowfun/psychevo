# Authoring Eval Configs And Benchmarks

A benchmark is stable task data rooted at `benchmark.toml`. An eval config is a
runnable plan rooted at any TOML file selected by `--config`. A peval workspace
is the operator area where runs, datasets, workspace registries, and helper
scripts live.

The checked-in seed benchmark is
`crates/psychevo-eval/benchmarks/pidx-coding/`. Test-only projects are generated
under temporary directories and are not a public fixture surface.

## Benchmark Layout

```text
my-benchmark/
  benchmark.toml
  tasks/
    rust-swe-add/
      task.toml
      instruction.md
      environment/
      tests/test.sh
```

`benchmark.toml` owns benchmark identity and typed sources:

```toml
schema_version = 5
id = "my-coding"
name = "My coding benchmark"

[[sources.peval_agent]]
id = "local"
path = "tasks"
verifier_timeout_seconds = 600

[[sources.peval_agent.sets]]
id = "smoke"
include = ["rust-swe-add"]
```

Benchmarks do not declare agents. Source sets select tasks only, so the same
task inventory can compare command agents, ACP agents, Psychevo presets, or
later adapters without duplicating data. Canonical task ids are
`source-id/native-task-id`; the full source set is `source-id`, and nested sets
are `source-id/set-id`.

## Eval Config

An eval config selects a benchmark and declares the runnable matrix:

```toml
schema_version = 5
id = "my-coding-psychevo"
name = "My coding benchmark with Psychevo"

[benchmark]
path = "../my-benchmark/benchmark.toml"

[select]
agents = ["psychevo"]
sets = ["local/smoke"]
tasks = ["local/rust-swe-add"]

[[agents]]
id = "psychevo"
kind = "psychevo"

[artifacts]
include = ["workspace"]
```

Run `peval check --config my-eval.toml` after each edit. The config must select
at least one agent and at least one set or task.

`[artifacts] include = [...]` sets debug artifact retention for runs from this
config. CLI `peval run --include ...` is additive. `workspace` retains the final
case workspace; `patch` and `raw-bodies` are accepted for bridge-specific
artifacts when those adapters produce them.

Reusable agents and benchmarks can also live in workspace `peval.toml` or user
`$PSYCHEVO_HOME/peval-config.toml`. Resolution priority is eval config,
workspace config, then user config.

## Agents

Psychevo agents call `pevo run` by default. Add adapter options only when the
eval needs an explicit command, model label, or argument template:

```toml
[[agents]]
id = "psychevo"
kind = "psychevo"

[agents.psychevo]
command = "sh"
args = ["scripts/psychevo-wrapper.sh", "{workspace}", "{prompt}"]
model = "default"
```

OpenCode and Hermes are wrapper kinds. Fake agents remain available for
deterministic tests:

```toml
[[agents]]
id = "local-solver"
kind = "command"

[agents.command]
command = "sh"
args = ["scripts/solve.sh", "{workspace}", "{prompt_file}"]
timeout_seconds = 600
```

Command and wrapper stdout may include JSONL events with `type`, `usage`, or `accounting`
fields. `peval` normalizes representative events into trajectory records and
derives duration, turns, tool calls, tool errors, token/cache usage, and cost
from stored metrics fields.

## Tasks

For `peval_agent`, each task is a directory. `task.toml` is required and must
parse as TOML, but the directory name is the native task id:

```toml
# tasks/rust-swe-add/task.toml
name = "Repair the add function"
kind = "swe-style"
```

`instruction.md` is the prompt shown to the agent. `environment/` is copied to
an isolated workspace. `tests/test.sh` is run from the workspace cwd after the
agent finishes.

## Evaluators

Verifier scripts write normalized pass/fail results from their exit status.
Optionally, a verifier can write `result.json` or `reward.txt` under
`$PEVAL_LOGS/verifier/` for a structured score/message import.

Official `harbor`, `swe_bench`, and `tau2` sources use their native harnesses.
They are opt-in source declarations; default checks stay local and deterministic
unless live validation is explicitly requested with `peval check --live`.

## Matrix Selection

Use filters to keep local iteration small:

```bash
peval check --config my-eval.toml --task-set local/smoke
peval run --config my-eval.toml --task-set local/smoke --agent psychevo
peval view --config my-eval.toml --task-set local/smoke --agent psychevo -i summary,matrix,usage
```

Direct benchmark use is useful for one-off checks:

```bash
peval check \
  --benchmark my-coding \
  --agent psychevo \
  --task-set local/smoke \
  --task local/rust-swe-add
```

The benchmark id must exist in registry config unless `--benchmark` is a path to
`benchmark.toml`.

## Checklist

- Use `schema_version = 5` in `benchmark.toml` and eval configs.
- Put local host-run tasks in `sources.peval_agent` task directories.
- Use `[select] sets = [...]`; `task_sets` is rejected.
- Initialize or select a peval workspace before store-backed commands.
- Run `peval check` before `peval run`.
- Use `--task-set` and `--agent` filters for real adapters.
- Keep benchmark projects under `crates/psychevo-eval/benchmarks/` when they are
  part of the repository.
- Review `peval view ... -i summary,matrix,usage` and relevant cell directories
  before adding more matrix entries.

For planned benchmark and adapter expansion, see the specs for
[benchmark integrations](../../specs/330-benchmark-integrations/spec.md),
[agent evaluation](../../specs/340-agent-evaluation/spec.md), and
[coding evaluation](../../specs/350-coding-evaluation/spec.md).
