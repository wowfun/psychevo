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
  tasks.jsonl
  tasks/
    rust-swe-add/
      workspace/
```

`benchmark.toml` owns benchmark identity, evaluator semantics, task sources, and
task sets:

```toml
schema_version = 4
id = "my-coding"
name = "My coding benchmark"

[evaluator]
kind = "local-coding"

[[task_sources]]
path = "tasks.jsonl"
format = "jsonl"

[[task_sets]]
id = "base"
name = "Base"
tasks = ["rust-swe-add"]
```

Benchmarks do not declare agents. Task sets select tasks only, so the same task
inventory can compare Psychevo, OpenCode, Hermes, fake agents, or later
adapters without duplicating data.

## Eval Config

An eval config selects a benchmark and declares the runnable matrix:

```toml
schema_version = 4
id = "my-coding-psychevo"
name = "My coding benchmark with Psychevo"

[benchmark]
path = "../my-benchmark/benchmark.toml"

[select]
agents = ["psychevo"]
task_sets = ["base"]
tasks = ["rust-swe-add"]

[[agents]]
id = "psychevo"
kind = "psychevo"
```

Run `peval check --config my-eval.toml` after each edit. The config must select
at least one agent and at least one task set or task.

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
id = "fake-pass"
kind = "fake"
fake = { behavior = "pass" }
```

Wrapper stdout may include JSONL events with `type`, `usage`, or `accounting`
fields. `peval` normalizes representative events into trajectory records and
derives duration, turns, tool calls, tool errors, token/cache usage, and cost
from stored metrics fields.

## Tasks

`tasks.jsonl` contains one task record per line. Task rows are benchmark data
records, not standalone TOML manifests:

```jsonl
{"schema_version":4,"task_id":"rust-swe-add","name":"Repair the add function","kind":"swe-style","dir":"tasks/rust-swe-add","problem_statement":"Fix the add function so the local tests pass.","workspace":{"source":"workspace"},"test_spec":{"checks":[{"kind":"cargo_test","timeout_seconds":30}]}}
```

When `dir` is present, workspace paths and task-local data paths resolve
relative to that task directory. Without `dir`, they resolve relative to the
task source file directory. Task rows do not declare arbitrary commands or
task-local executable scripts.

## Evaluators

`local-coding` interprets typed `test_spec.checks` and writes normalized
evaluator results into cell artifacts. Supported checks are:

- `cargo_test`
- `exact_file`
- `python_function_cases`

Example exact-file check:

```jsonl
{"schema_version":4,"task_id":"tool-state","kind":"stateful-tool-use","dir":"tasks/tool-state","problem_statement":"Update state.txt so it records prepare, edit, and verify in that order.","workspace":{"source":"workspace"},"test_spec":{"checks":[{"kind":"exact_file","path":"state.txt","expected":"prepare\nedit\nverify"}]}}
```

Declaration-only evaluators `tau2` and `swe-bench` can be recorded in
`benchmark.toml`; `peval check` accepts their structure with `run_supported =
false`, and `peval run` rejects them until those bridges are implemented.

## Matrix Selection

Use filters to keep local iteration small:

```bash
peval check --config my-eval.toml --task-set base
peval run --config my-eval.toml --task-set base --agent psychevo
peval view --config my-eval.toml --task-set base --agent psychevo -i summary,matrix,usage
```

Direct benchmark use is useful for one-off checks:

```bash
peval check \
  --benchmark my-coding \
  --agent psychevo \
  --task-set base \
  --task rust-swe-add
```

The benchmark id must exist in registry config unless `--benchmark` is a path to
`benchmark.toml`.

## Checklist

- Use `schema_version = 4` in `benchmark.toml`, eval configs, and task rows.
- Put task records in JSONL task sources, not per-task TOML files.
- Use `[evaluator]` plus typed `test_spec.checks`, not task-local executable
  scripts.
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
