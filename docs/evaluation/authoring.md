# Authoring Evaluation Projects

An evaluation project is a directory with `eval.toml` at its root. The first
implementation uses a small, Cargo-like layout:

```text
my-eval/
  eval.toml
  agents/
    fake-pass.toml
    psychevo-live.toml
  suites/
    rust-swe.toml
  tasks/
    rust-swe-add/
      task.toml
      workspace/
      scripts/
        score.sh
```

Run `peval check --config my-eval/eval.toml` after each edit.

## Project Manifest

`eval.toml` names the project and controls the live gate:

```toml
schema_version = 1
name = "my-eval"
output_root = "runs/my-eval"
allow_live = false
```

`output_root` is a namespace under the evaluation store, not an absolute
artifact path. Omit it to use `runs/<project-slug>`.

Set `allow_live = true` only for projects that may run real agents or provider
calls. `peval check` and `peval run` reject Psychevo live agents when the gate
is false.

## Agents

Fake agents are deterministic and safe for local validation:

```toml
schema_version = 1
id = "fake-pass"
name = "Passing fake agent"
kind = "fake"

[fake]
behavior = "pass"
```

Psychevo live agents call a command adapter:

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

The command path resolves from the task directory. The process runs in the
temporary workspace and receives `PEVAL_WORKSPACE` and `PEVAL_TASK_DIR`.
The adapter replaces `{workspace}` and `{prompt}` for each case.

## Suites

A suite selects tasks and agents:

```toml
schema_version = 1
id = "rust-swe"
name = "Local Rust SWE-style fixture"
description = "Tiny issue-to-patch workflow judged by local tests."
agents = ["fake-pass", "psychevo-live"]
tasks = ["../tasks/rust-swe-add/task.toml"]
```

Use CLI filters while developing:

```bash
peval check --config my-eval/eval.toml --suite rust-swe --agent fake-pass
peval run --config my-eval/eval.toml --suite rust-swe --agent fake-pass
```

## Tasks

A coding task defines the prompt, workspace source, scorer, and optional fake
behavior commands:

```toml
schema_version = 1
id = "rust-swe-add"
name = "Repair the add function"
kind = "swe-style"

[prompt]
text = "Fix the add function so the local tests pass."

[workspace]
source = "workspace"

[scorer]
command = ["sh", "scripts/score.sh"]
timeout_seconds = 30

[fake.pass]
command = ["sh", "scripts/fake-pass.sh"]
timeout_seconds = 5

[fake.fail]
command = ["sh", "scripts/fake-fail.sh"]
timeout_seconds = 5
```

Task paths resolve relative to the manifest that owns the field. Keep scorer
commands local and deterministic when you expect the task to run in default
validation.

## Scorers

Scorers print one JSON object to stdout:

```json
{
  "schema_version": 1,
  "passed": true,
  "score": 1.0,
  "message": "tests passed",
  "details": {
    "scorer": "cargo-test"
  }
}
```

Exit status and malformed output become scoring failures. Keep raw logs in
task-local files when they help diagnosis.

## Authoring Checklist

- Use `schema_version = 1` in every manifest.
- Run `peval check` before `peval run`.
- Start with fake agents and local scorers.
- Add `allow_live = true` only when you intend to run real Psychevo/provider
  work.
- Use `--suite` and `--agent` filters while building a new project.
- Review `summary.json` and `report.md` before adding more matrix entries.

For planned benchmark and adapter expansion, see the specs for
[benchmark integrations](../../specs/330-benchmark-integrations/spec.md),
[agent evaluation](../../specs/340-agent-evaluation/spec.md), and
[coding evaluation](../../specs/350-coding-evaluation/spec.md).
