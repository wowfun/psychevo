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
- permission and runtime mode override flags
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
- optional `-f, --format <default|json>`
- optional `--include-reasoning`
- optional `--permission-mode <default|acceptEdits|plan|dontAsk|bypassPermissions>`
- optional `--dangerously-skip-permissions`
- optional `--project-context <git-root|cwd|off>`
- optional `--isolated`
- optional `--runtime <id>`
- repeatable optional `--runtime-option <key=value>`
- optional `--no-skills`
- repeatable optional `--skill <name-or-path>`

The first-slice default format is `default`.

The removed first-slice flags are not accepted: `--prompt`, `--json`,
`--provider`, `--base-url`, `--api-key-env`, `--db`, `--cwd`,
`--max-context-messages`, `--verbose`, and `--config`.

## Prompt Input

Positional message arguments are joined with spaces. When stdin is not a TTY,
stdin text is appended after the positional message with one newline separator.
If the final prompt is empty after trimming whitespace, the command rejects
before session creation with `You must provide a message`.

## Cwd and State

The default cwd is process cwd. `--dir` overrides it. `~` expands, relative
paths resolve relative to process cwd, and the runtime receives a canonical
cwd.

The default SQLite path is `$PSYCHEVO_HOME/state.db`. `PSYCHEVO_DB` may override
the SQLite path and is an environment-only control.

`pevo run` normally requires an initialized `PSYCHEVO_HOME`. When both
`PSYCHEVO_CONFIG` and `PSYCHEVO_DB` are set, scripts and tests may bypass
global home initialization.

## Project Context

`pevo run` always makes the canonical runtime cwd visible to the model as
runtime environment context. This context explains that relative paths resolve
from that cwd and that absolute paths remain subject to the normal
permission gates.

Project instruction discovery defaults to the configured
`[project_context].instructions` value, or `git-root` when unset.
`git-root` preserves the existing behavior of loading AGENTS/project
instructions from the discovered Git root through the cwd. `cwd` loads only
instruction files in the canonical cwd. `off` suppresses project
instruction injection.

`--project-context <git-root|cwd|off>` overrides configuration for the
invocation. `--isolated` is an alias for `--project-context cwd`. Supplying
both is a usage error. These options change model-visible project context only;
they do not change permission profiles, sandbox policy, or approval behavior.

## Model and Variant

`-m`/`--model` must use `provider/model` form. Provider/model config and
resolution semantics belong to [120 Provider Registry](../120-provider-registry/spec.md).

`--variant` maps to the first-slice reasoning effort override. Valid values are
`none`, `minimal`, `low`, `medium`, `high`, `xhigh`, and `max`. `none`
explicitly suppresses the Chat request `reasoning_effort` field.

`--permission-mode` overrides the configured permission mode for this
invocation. `plan` selects the read-only runtime mode; `dontAsk` denies actions
that would otherwise prompt unless already allowed by policy or safe defaults.
`--dangerously-skip-permissions` selects `bypassPermissions`; hard/protected
denies still apply. Permission policy semantics belong to
[041 Permissions](../041-permissions/spec.md).

`--runtime <id>` selects a configured runtime backend for this invocation. The
default is native Psychevo runtime behavior. `--runtime-option <key=value>`
supplies current-runtime session options for backends that expose them. The
first supported key is `mode`, used by ACP peer runtimes such as OpenCode to
select the peer ACP `mode` config option. The flag is namespaced as a runtime
option rather than `--mode` because native Psychevo work mode remains owned by
runtime defaults and interactive `/mode`. Unknown keys are preserved for
Gateway/backend validation and must fail with bounded errors when the selected
runtime cannot honor them.

## Session Selection

`--session` resumes the specified session id without changing session recency.
The resumed session becomes recently updated only when new transcript material
is persisted.

`--continue` selects the latest `source = "run"` session for the canonical
cwd, ordered by latest persisted activity then start time. Viewing or
opening a session does not affect this ordering. If no matching session exists,
runtime creates a new session.

Routing `pevo run` through Gateway does not change that selection contract:
Gateway resolves `--continue` against the canonical cwd and allowed sources
before it materializes a new public thread. An invocation-scoped CLI source key
must not force a second session when a matching run session already exists.

Supplying `--session` and `--continue` together is a usage error.

## Tool Surface

The first `pevo run` entrypoint enables the built-in `coding-core` tools by
default:

- `read`
- `write`
- `edit`
- `exec_command`
- `write_stdin`

The same working-directory containment and tool JSON contracts used by the
runtime tool layer apply to `pevo run`.

When skills are enabled, `pevo run` may add skill adjunct tools and a compact
skill index as defined by [055 Skills](../055-skills/spec.md). `--no-skills`
disables default and configured skill discovery. Explicit `--skill` values
remain additive and may name a discovered skill or point at a skill path.

## Output

`--format default` writes only the final assistant text to stdout.

`--format json` writes newline-delimited typed transcript events to stdout.
Output is buffered in this slice. Each line is one JSON object. The event shape
is Psychevo-owned and uses dotted event names:

- `thread.started`
- `turn.started`
- `item.started`
- `item.updated`
- `item.completed`
- `turn.completed`
- `turn.failed`
- `error`

`item.*` events carry typed transcript entries rather than raw runtime event
payloads. Tool and artifact entries include bounded preview/detail references
when output is large. `turn.completed` includes usage when known and the
terminal outcome. `turn.failed` and `error` contain bounded human-readable
diagnostics without provider secrets.

Reasoning/thinking content is folded out of JSON output by default. Supplying
`--include-reasoning` requires `--format json` and allows typed reasoning
transcript entries or updates. Assistant message entries remain
visible-transcript projections and must not carry provider reasoning wire
fields.

When a started run ends because the agent loop reached its model-turn budget,
the terminal `turn.completed` or `turn.failed` JSON event includes
`terminalReason: {"type":"max_turns_exceeded","max_turns":N}` and a
human-readable `terminalMessage`. This is a terminal outcome projection, not a
runtime error event.

When `--format json` is selected and a runtime/configuration error happens
after argument parsing, stdout contains one JSON object:

```json
{"type":"error","message":"..."}
```

No `thread.started` is emitted for errors before a session exists.

## Exit Behavior

`pevo run` exits with code 0 only for normal completion. Provider failures,
tool failures that produce a failed terminal outcome, invalid configuration,
session-start rejection, before-agent-start rejection, and usage errors exit
non-zero. In default output mode, terminal outcomes with a diagnostic terminal
reason write the diagnostic message to stderr while stdout remains reserved for
final assistant text.

Live-provider calls are opt-in by command usage. The default validation path
must not require credentials, live network access, or user configuration.

## Cost and Metadata

`pevo run` resolves provider/model metadata from local configuration, the
existing cache-first `models.dev` cache, explicit catalog metadata when
available, and deterministic fallbacks. It never refreshes `models.dev` on the
run hot path. Runtime persists normalized usage metrics and local estimated-cost
accounting for completed assistant messages when usage is reported. Cost is an
estimate in local state, not a provider bill.

## Related Topics

- [200 pevo CLI](spec.md) defines the product CLI surface.
- [025 CLI](../025-cli/spec.md) defines command-line foundation semantics.
- [120 Provider Registry](../120-provider-registry/spec.md) defines provider
  and model resolution.
- [041 Permissions](../041-permissions/spec.md) defines permission mode and
  approval semantics.
- [100 Runtime Assembly](../100-coding-agent/runtime-assembly.md) defines how
  runtime assembles the built-in coding agent.
- [055 Skills](../055-skills/spec.md) defines optional skill discovery and tools.
