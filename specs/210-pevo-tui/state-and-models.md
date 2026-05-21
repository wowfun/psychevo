---
name: 210. pevo TUI
psychevo_self_edit: deny
---

# 210. pevo TUI State and Models

Define persisted TUI-local state, model selection, catalog fetching, and runtime mode behavior.

## TUI State

`$PSYCHEVO_HOME/tui-state.json` is a TUI-local state file. It must not store raw
API keys, provider credentials, full prompts, transcripts, reasoning text, tool
results, or provider payloads.

The state file stores:

- a version number
- global `thinking_visible`
- global `raw_visible`
- global `sidebar_visible`
- current model and optional variant override per canonical workdir
- current `mode` per canonical workdir
- a bounded global recent-model list

`thinking_visible` defaults to `true`. `raw_visible` defaults to `false`.
Per-workdir `mode` defaults to `default`. `sidebar_visible` defaults to
`false`, preserving the hidden sidebar startup behavior unless the user has
explicitly toggled it in fullscreen TUI.

Startup model and variant precedence is:

1. `pevo tui` CLI flags for the current process
2. per-workdir TUI state
3. existing provider config and environment resolution

Fullscreen `/model` opens an interactive model picker. It starts from local
configured models and never fetches remote model catalogs until the user
selects an explicit fetch row. Selecting a model then opens a variant picker.
Selecting `Config default` clears the per-workdir variant override so runtime
uses the selected model's configured `reasoning_effort` when it has one, or the
provider default for fetched-only models; selecting an explicit variant persists
that override.
`/variant <none|minimal|low|medium|high|xhigh|max>` continues to update only
the per-workdir variant override. Bare `/variant` is not a display command and
returns a bounded usage error. Obsolete `/variant set <value>` input is not a
compatibility command. These TUI state changes affect later prompts in the
current process and do not edit JSONC provider configuration.

`/show-thinking` toggles global thinking visibility and persists it. It is a
visibility-only control: it does not enable or disable provider reasoning, does
not change `--variant`, and does not edit provider configuration. Fullscreen
TUI must refresh the current transcript projection immediately and must not
append a status row for thinking visibility changes. `/show-thinking on` and
`/show-thinking off` set the value explicitly. `/thinking` is obsolete and is
not a compatibility command.

`/show-raw` toggles global raw transcript visibility and persists it. It is a
display-only control: it keeps the pevo ledger structure but renders assistant
answer bodies, and visible Thinking bodies, from raw Markdown source instead of
rich Markdown projection. It does not alter persisted messages, provider
payloads, non-terminal output, or `/copy` behavior. Fullscreen TUI must refresh
the current transcript projection immediately and must not append a transcript
status row for raw visibility changes. `/show-raw on` and `/show-raw off` set
the value explicitly. `/raw` is not a compatibility command.

`/mode <plan|default>` updates the per-workdir mode and persists it. Bare
`/mode` is not a display command and returns a bounded usage error. Obsolete
`/mode set <value>` input is not a compatibility command. Mode changes during a
running turn affect the next submitted prompt.

## Runtime Modes

Runtime mode is explicit and enforceable by the tool surface.

`default` is the default for `pevo run` and for `pevo tui` when TUI state has no
per-workdir mode. Default mode exposes the current full coding-core tools.

`plan` is hard read-only. It exposes only:

- `read`: read a file under the selected workdir
- `list`: list files or directories under the selected workdir with limits and
  truncation metadata
- `search`: literal text search under the selected workdir with limits and
  truncation metadata

When interactive clarify support is enabled, fullscreen TUI may also expose
the read-only `clarify` tool in plan mode to ask bounded user questions. Plan
mode must not expose `exec_command`, `write_stdin`, `write`, or `edit`. Its read-only semantics must
not depend only on provider instructions.

User shell escape is a user-supplied shell context action, not a model-visible
tool. It is available in both `plan` and `default` modes and must not add
`exec_command` or `write_stdin` to the agent-visible tool surface for `plan` mode. Successful, failed,
non-zero, timed-out, truncated, and interrupted user shell results are persisted
as user-role context records for subsequent provider requests when the current
provider/model configuration can be resolved.

The runtime sends an ephemeral mode instruction to the provider for the current
turn. The instruction is not persisted as a transcript message.

`pevo run` defaults to `default` and does not expose mode flags in this slice.

## Related Topics

- [210 pevo TUI](spec.md) is the parent topic.
- [210 pevo TUI Testing](testing.md) defines deterministic acceptance coverage.
