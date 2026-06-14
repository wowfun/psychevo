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
Selecting `Config default` clears the per-workdir variant override and writes
the selected model's configured `reasoning_effort` into the scoped default model
object when that configured default is known. For fetched-only models with no
configured default, it writes only the provider/model id and lets the provider
default apply. Selecting an explicit variant writes that value as the scoped
default model object's `reasoning_effort`. `/model` opens the picker in
local-config mode, so final model selection writes the current workdir
`.psychevo/config.toml`. `/model -g` and `/model --global` open the same picker
in global-config mode and write `$PSYCHEVO_HOME/config.toml`.
`/variant <none|minimal|low|medium|high|xhigh|max>` continues to update only
the per-workdir variant override. Bare `/variant` is not a display command and
returns a bounded usage error. Obsolete `/variant set <value>` input is not a
compatibility command. These TUI state changes affect later prompts in the
current process and do not edit TOML provider configuration.

Model picker config writes clear the current workdir's TUI-local model and
variant overrides, update the recent-model list, and rebuild selected model
metadata from effective configuration. They do not write provider credentials,
provider catalog metadata, or provider `models.<id>` entries, and they do not
contact provider catalogs. If a global write is shadowed by local configuration,
TUI reports that the global default was saved while the current workdir remains
governed by local config.

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

`/mode` is scoped to the current runtime. With native Psychevo runtime, `/mode
<plan|default>` updates the per-workdir native mode and persists it. With an
ACP peer runtime, `/mode <value>` updates that peer runtime's session mode
option for the current session and does not rewrite native per-workdir mode.
Bare `/mode` opens the current runtime mode picker in fullscreen or prints the
current mode and selectable values in scripted mode. Obsolete `/mode set
<value>` input is not a compatibility command. Mode changes during a running
turn affect the next submitted prompt.

## Runtime Modes

Runtime mode is explicit and enforceable by the tool surface.

`default` is the default for `pevo run` and for `pevo tui` when TUI state has no
per-workdir mode. Default mode exposes the current full coding-core tools plus
the default-enabled read-only `web` toolset unless configuration disables it.

`plan` withholds file mutation tools and exposes only:

- `read`: read a file under the selected workdir
- `exec_command`: run bounded shell commands for read-only exploration
- `write_stdin`: poll or interact with a yielded `exec_command` session
- `web_fetch`: fetch known `http(s)` URLs when the `web` toolset is enabled

When interactive clarify support is enabled, fullscreen TUI may also expose
the read-only `clarify` tool in plan mode to ask bounded user questions. Plan
mode must not expose `write` or `edit`. Its shell read-only semantics are
governed by mode instructions and the normal permission/resource boundary, not
by a separate command allowlist.

Fullscreen `/tools` displays and toggles per-mode toolset configuration. TUI
toggles write only project-local `.psychevo/config.toml` and affect later
turns, not an already running provider request.

User shell escape is a user-supplied shell context action, not a model-visible
tool. It is available in both `plan` and `default` modes and must not add
additional agent-visible tools to the Plan surface. Successful, failed, non-zero,
timed-out, truncated, and interrupted user shell results are persisted
as user-role context records for subsequent provider requests when the current
provider/model configuration can be resolved.

The runtime sends an ephemeral mode instruction to the provider for the current
turn. The instruction is not persisted as a transcript message.

`pevo run` defaults to `default` and does not expose mode flags in this slice.

## Related Topics

- [210 pevo TUI](spec.md) is the parent topic.
- [210 pevo TUI Testing](testing.md) defines deterministic acceptance coverage.
