---
name: 200. pevo run Attachment
psychevo_self_edit: deny
---

Define the live coding-agent CLI entrypoint behavior for
`pevo run`.

This attachment is part of [200 pevo CLI](spec.md).

## Scope

- live coding-agent CLI invocation
- prompt input behavior
- text and NDJSON output modes
- model and reasoning override flags
- session selection flags
- exit behavior

Out of scope:

- interactive TUI, terminal rendering, slash commands, approvals, file
  attachments, fork/share/server attach, or background processes
- real-provider validation in the default test path
- provider login or credential management commands

## Command Contract

`pevo run` accepts:

- positional `[message..]`
- optional `--dir <path>`
- optional `-m, --model <provider/model>`
- optional `--variant <none|minimal|low|medium|high|xhigh|max>`
- optional `-s, --session <id>`
- optional `-c, --continue`
- optional `--format <default|json>`
- optional `--include-reasoning`
- optional `--no-skills`
- repeatable optional `--skill <name-or-path>`

The first-slice default format is `default`.

The removed first-slice flags are not accepted: `--prompt`, `--json`,
`--provider`, `--base-url`, `--api-key-env`, `--db`, `--workdir`,
`--max-context-messages`, `--verbose`, and `--config`.

## Prompt Input

Positional message arguments are joined with spaces. When stdin is not a TTY,
stdin text is appended after the positional message with one newline separator.
If the final prompt is empty after trimming whitespace, the command rejects
before session creation with `You must provide a message`.

## Workdir and State

The default workdir is process cwd. `--dir` overrides it. `~` expands, relative
paths resolve relative to process cwd, and the runtime receives a canonical
workdir.

The default SQLite path is `$PSYCHEVO_HOME/state.db`. `PSYCHEVO_DB` may override
the SQLite path and is an environment-only control.

`pevo run` normally requires an initialized `PSYCHEVO_HOME`. When both
`PSYCHEVO_CONFIG` and `PSYCHEVO_DB` are set, scripts and tests may bypass
global home initialization.

## Model and Variant

`-m`/`--model` must use `provider/model` form. Provider/model config and
resolution semantics belong to [120 Provider Registry](../120-provider-registry/spec.md).

`--variant` maps to the first-slice reasoning effort override. Valid values are
`none`, `minimal`, `low`, `medium`, `high`, `xhigh`, and `max`. `none`
explicitly suppresses the Chat request `reasoning_effort` field.

## Session Selection

`--session` resumes the specified session id.

`--continue` selects the latest `source = "run"` session for the canonical
workdir, ordered by update time then start time. If no matching session exists,
runtime creates a new session.

Supplying `--session` and `--continue` together is a usage error.

## Tool Surface

The first `pevo run` entrypoint enables the built-in `coding-core` tools by
default:

- `read`
- `write`
- `edit`
- `bash`

The same working-directory containment and tool JSON contracts used by
`pevo smoke` apply to `pevo run`.

When skills are enabled, `pevo run` may add skill adjunct tools and a compact
skill index as defined by [055 Skills](../055-skills/spec.md). `--no-skills`
disables default and configured skill discovery. Explicit `--skill` values
remain additive and may name a discovered skill or point at a skill path.

## Output

`--format default` writes only the final assistant text to stdout.

`--format json` writes newline-delimited JSON observation events to stdout.
Output is buffered in this slice. Each line is one JSON object. The first line
after a started run is `run_start` and identifies the session, selected
provider/model, database, workdir, and resolved core model metadata when known.
Subsequent lines project runtime
observation events: `agent_start`, `turn_start`, `message_start`,
`message_update`, `message_end`, `tool_execution_start`, `tool_execution_end`,
`turn_end`, and `agent_end`.

Reasoning/thinking content is folded out of JSON output by default. Supplying
`--include-reasoning` requires `--format json` and adds separate
`reasoning_delta` and `reasoning_end` events. The `message_*` and `agent_end`
events remain visible-transcript projections and must not carry reasoning
blocks or provider reasoning wire fields.

When `--format json` is selected and a runtime/configuration error happens
after argument parsing, stdout contains one JSON object:

```json
{"type":"error","message":"..."}
```

No `run_start` is emitted for errors before a session exists.

## Exit Behavior

`pevo run` exits with code 0 only for normal completion. Provider failures,
tool failures that produce a failed terminal outcome, invalid configuration,
session-start rejection, before-agent-start rejection, and usage errors exit
non-zero.

Live-provider calls are opt-in by command usage. The default validation path
must not require credentials, live network access, or user configuration.

## Cost and Metadata

`pevo run` enriches resolved provider/model metadata from local configuration,
cache-first `models.dev`, explicit catalog metadata when available, and
deterministic fallbacks. Runtime persists normalized usage metrics and local
estimated-cost accounting for completed assistant messages when usage is
reported. Cost is an estimate in local state, not a provider bill.

## Related Topics

- [200 pevo CLI](spec.md) defines the product CLI surface.
- [025 CLI](../025-cli/spec.md) defines command-line foundation semantics.
- [120 Provider Registry](../120-provider-registry/spec.md) defines provider
  and model resolution.
- [100 Runtime Assembly](../100-coding-agent/runtime-assembly.md) defines how
  runtime assembles the built-in coding agent.
- [055 Skills](../055-skills/spec.md) defines optional skill discovery and tools.
