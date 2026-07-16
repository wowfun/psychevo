---
name: 200. pevo CLI
psychevo_self_edit: deny
---

Define the concrete `pevo` command-line product surface.

This product surface builds on [025 CLI](../025-cli/spec.md) and
[026 Commands](../026-commands/spec.md), and routes agent work through
`psychevo-runtime`. It owns command spelling, user-facing process behavior, and
product-level environment variables.

## Scope

- `pevo` command families
- `pevo` command descriptions projected through process help
- global Psychevo home layout
- `pevo init`
- `pevo run`
- `pevo tui` product positioning
- `pevo acp` product positioning
- `pevo skill` product positioning
- `pevo plugin` product positioning
- `pevo profile` product positioning
- local session, model, config, and auth inspection/maintenance commands
- local session export/share artifacts

Out of scope:

- file attachments, fork/server attach, remote session publishing, provider
  login, or auth stores
- provider transport semantics beyond explicit `/models` fetches, OAuth, or
  credential pools
- SQLite schema details beyond product path selection
- SDK, HTTP, MCP, or HTTP/WebSocket ACP transports

## Psychevo Home

`PSYCHEVO_HOME` is the resolved active profile home for the `pevo` product CLI.
When no named profile is active, it defaults to `~/.psychevo`. Profile
resolution, named profile layout, sticky selection, and `-p/--profile` behavior
are defined by [057 Profiles](../057-profiles/spec.md). `~` expands to the
user's home directory, and relative `PSYCHEVO_HOME` values resolve relative to
the process cwd.

The initialized home tree contains:

- `config.toml`
- `.env`
- `state.db`
- `sessions/`
- `logs/`
- `cache/`
- `skills/`
- `plugins/`
- `agents/`

Named profiles additionally contain `profile.toml` when created through
`pevo profile create`.

When `config.toml` is absent, `pevo init` writes it from the compiled starter
config template. Existing `config.toml` files are not overwritten.

`state.db` is the only first-slice session/message store. The reserved
`sessions/` directory is not used for JSON or JSONL transcript sidecars in this
slice.

`pevo init --reset-state` resets this profile-local SQLite state and stops the
current profile's managed Gateway/Web server before backing up the old state
files, so a later `pevo web` launch cannot reuse a background process still
holding the previous database connection.

`PSYCHEVO_DB` may point at `:memory:` or a SQLite path. `~` expands, and
relative paths resolve relative to the process cwd. When unset, `pevo run` uses
`$PSYCHEVO_HOME/state.db`.

`PSYCHEVO_CONFIG` may point at one TOML config file. When set, it replaces
home and project config discovery for provider configuration.

`config.jsonc` is not part of the product home layout. If a file with that name
exists beside `config.toml`, runtime ignores it. There is no compatibility
loader or migration subcommand for it.

## Command Families

Implemented first-slice commands:

- `pevo init`
- `pevo run`
- `pevo tui`
- `pevo web`
- `pevo desktop`
- `pevo serve`
- `pevo gateway`
- `pevo doctor`
- `pevo setup`
- `pevo acp`
- `pevo mcp`
- `pevo profile`
- `pevo agent`
- `pevo skill`
- `pevo plugin`
- `pevo hooks`
- `pevo stats`
- `pevo context`
- `pevo session`
- `pevo model`
- `pevo tool`
- `pevo config`
- `pevo auth`

## Public Documentation

The root README is the concise entry point for developers who want to use a
local coding agent in an existing codebase. `README.md` is the English source,
and `README.zh-CN.md` is its complete Simplified Chinese counterpart. Both
files link to each other, keep the same product claims, command summary,
examples, and local documentation links, and preserve command spelling exactly.

Both README files lead with the user outcome and a concise capability summary,
then show source installation and a first task before detailed product-surface
or contributor information. The user entry path must explain that provider
configuration, permission policy, and session history stay local and can be
inspected.

The README command summary must describe only current public commands and
agree with process help without duplicating its flags or subcommand reference.
Source-install details remain in [Installation Guide](../../docs/install.md),
not in the README.

Process help is part of the product surface. Top-level commands, subcommands,
arguments, and flags should carry human-readable descriptions in `--help`
output, including stable value names for positional arguments and option
values. High-consequence commands should also use long help to make their
effects clear: whether they write local files or config, read secrets from
stdin, contact providers, emit machine output, include selected skills, or
expose sensitive reconstructed prompt material.

`pevo skill` is the only skill command family name. The obsolete plural
`pevo skills` is not accepted.

`pevo plugin` is the only plugin command family name. The obsolete plural
`pevo plugins` is not accepted.

`pevo hooks` owns local hook review and profile-owned hook state. It supports:

- `pevo hooks list`
- `pevo hooks trust <hook-key>`
- `pevo hooks enable <hook-key>`
- `pevo hooks disable <hook-key>`

Read commands accept `--json` and emit secret-free metadata including event,
matcher, handler type, source kind, source id, plugin id when relevant, enabled
state, current hash, trusted hash when present, trust status, skipped reason,
and source path when available. Mutating commands write only
`hooks.state.<hook_key>` in the active profile configuration. They must not edit
project hook files, plugin packages, session SQLite state, or hook declaration
content.

`pevo -p, --profile <name>` selects a named Psychevo profile for the entire
command invocation. It does not change the workspace/cwd protocol scope.
Profile selection is resolved before subcommand execution and before Gateway or
ACP child processes are launched.

`pevo` with no subcommand is the interactive default entrypoint. When stdin
and stdout are both terminals, it is equivalent to `pevo tui`. When either side
is not a terminal, it must not consume stdin; it exits with a concise error and
points users to explicit commands such as `pevo tui`, `pevo run`, `pevo web`,
and `pevo --help`.

`pevo agent` owns local agent definition inspection and first-class child-agent
control.
`pevo run` may accept `--agent <name-or-path>` to select the main-session
agent definition and `--no-agents` to disable agent discovery and agent
tools for that invocation. Agent definition behavior is defined by
[051 Agents](../051-agents/spec.md), and subagent behavior is defined by
[051 Subagents](../051-agents/subagents.md).

`pevo agent list` lists discoverable agent definitions. Runtime instances are
listed through `pevo agent status`, which defaults to the current/root session
tree and accepts `--all` for every durable or live child-agent session.
`pevo agent inspect`, `wait`, `send`, `close`, `resume`, `attach`, and `logs`
operate on agent ids or task names returned by `Agent` spawn and status output.
`inspect` is a local, read-only peek into a child-agent session: it resolves the
durable parent/child edge, reports the agent record plus parent and child
session summaries, and includes a bounded recent transcript projection. It must
not contact providers, refresh model catalogs, mutate session recency, or resume
stopped work. `send` may resume a closed or completed child agent in the
background as a continuation turn. `attach` enters the original child session
using its existing definition, mode, and tool policy; it does not promote the
child to a main session.
CLI output keeps the same split as the TUI: `inspect` and `logs` are
observational, while `attach`, `send`, `resume`, and `close` are explicit
control operations.

`pevo tui` owns interactive terminal projection. It accepts `--debug` for
TUI-local debug projections such as usage parts and allowlisted provider
metadata summaries. Debug projection does not change `pevo run --format json`,
does not expose folded reasoning in sanitized transcript messages, and does not
turn provider metadata into transcript content.

`pevo web` is the convenience entrypoint for the managed local Web UI. With no
subcommand it is equivalent to `pevo gateway open`, defaults to the current
working directory, keeps stdout as exactly one JSON object, and accepts the same
first-slice open flags: `--dir`, `--bind`, `--no-browser`, and `--print-url`.
`pevo web start [--bind <ADDR>]`, `pevo web stop`, and
`pevo web restart [--bind <ADDR>]` are Web-facing aliases for the matching
managed Gateway lifecycle commands. `pevo web restart` stops the current
profile's managed server when one is running, then starts it; if no server is
running, it starts one.
When `--bind` is omitted, the managed Web UI prefers `127.0.0.1:58080` and may
fall back through `127.0.0.1:58099` if earlier ports are already in use. The
JSON response always reports the actual bound URL in `baseUrl`. An explicit
`--bind` is strict and does not use managed port fallback.

`pevo desktop` is the source-checkout developer launcher for the native Desktop
shell. It discovers a Psychevo source checkout containing `apps/desktop/` and
runs the existing `@psychevo/desktop` Tauri development entrypoint. It accepts
`--dir <DIR>` to choose the Desktop fallback workspace cwd; otherwise it uses
the caller's cwd. The launcher resolves pnpm through the shared host executable
boundary, including Windows `PATH`/`PATHEXT` command shims, and defaults the
pnpm child to the installed usable package-manager version instead of requiring
Corepack to download the repository's recommended exact version. These
Corepack defaults are subprocess-scoped and preserve explicit caller settings.
On every Windows host, the launcher also defaults the child environment to
`CARGO_HTTP_CHECK_REVOKE=false` unless the caller explicitly set that variable,
so the Tauri-spawned Cargo process can fetch dependencies when Schannel cannot
complete certificate revocation checks. This default does not persist Cargo or
shell configuration, does not change other Cargo network settings, and preserves
explicit values such as `CARGO_HTTP_CHECK_REVOKE=true`.
The command preserves active profile selection for the Desktop child process
and passes the current `pevo` executable path as `PSYCHEVO_PEVO_BIN` so Desktop
managed Gateway startup uses the same CLI build that launched the native shell.
It is not a Desktop packaging, installer, update, or background lifecycle
command.

`pevo serve` starts the strict headless local Gateway API server. It binds
loopback by default, requires an explicit token from `PSYCHEVO_SERVE_TOKEN` or
`--token-file`, emits one ready JSON object to stdout, and does not mount the
Web Shell in the public command surface. Its concrete API behavior is owned by
[221 pevo Serve](../221-pevo-serve/spec.md).

`pevo gateway` owns managed local Web launch lifecycle. With no subcommand it is
equivalent to `pevo gateway open`. `open`, `start`, `status`, `stop`, and
`restart` emit one JSON object to stdout. Managed state lives under
`$PSYCHEVO_HOME/gateway/`, uses an owner-only generated token, and may start
`pevo serve` with internal flags to mount built Workbench assets. Managed
launch behavior is owned by [220 pevo Gateway](../220-pevo-gateway/spec.md);
the concrete Web Shell and Workbench behavior is owned by
[240 pevo Web](../240-pevo-web/spec.md).

`pevo doctor` owns local deterministic diagnostics. By default it checks local
paths, config readability, SQLite path selection, configured model/auth status,
Web UI asset resolution, managed Gateway status, and required local tool
availability without contacting providers. `--json` emits a structured,
secret-free report. `--live` is the explicit opt-in for provider/model network
checks.

`pevo setup` owns the full interactive first-run wizard. It is TTY-only and may
initialize or repair Psychevo home, guide provider/model selection, write scoped
provider config, read or reference an API key without printing it, check or
install Web UI assets from a source checkout, and finish with a doctor summary.
In non-terminal stdin/stdout it exits without prompting and points users to
`pevo init`, `pevo auth setup`, and `pevo doctor`.

The provider/model portion of `pevo setup` prompts in this order: provider,
base URL, API-key environment variable, API key, and model. The provider prompt
offers numbered built-in choices for DeepSeek, Z.AI, and Xiaomi Token Plan plus
a custom OpenAI-compatible provider. Z.AI defaults to the general OpenAI-
compatible endpoint while offering its Coding Plan endpoint as a base URL
shortcut. Xiaomi Token Plan prompts for the official CN, SGP, or AMS OpenAI-
compatible regional URL, defaulting to CN, and persists the canonical
`xiaomi-token-plan` provider id. Setup shows the recommended API-key environment
variable name and uses it by default; users edit the env var name only after
explicitly choosing to change it, and raw API keys are accepted only through the
following hidden API-key prompt. After credentials are captured or referenced,
setup attempts one provider `/models` fetch; on success users may select a
numbered fetched model or choose the custom model-id row, and on failure or an
empty catalog setup falls back to custom model-id entry.

`pevo acp` runs the ACP stdio server. It is equivalent to the
`psychevo-acp` binary and delegates behavior to the ACP crate instead of
implementing protocol handling in `psychevo-cli`.
`pevo acp --setup` runs provider setup and exits without starting the stdio
server. It accepts the same setup flags as `pevo auth setup`.

`pevo profile` owns local profile list/show/create/use/delete/rename/alias
commands. Profile behavior, metadata, clone rules, and alias wrappers are
defined by [057 Profiles](../057-profiles/spec.md).

`pevo skill` owns the singular skill hub/config/list/view router. With no
subcommand it shows help. `list` and `view` are read operations; `audit`
absorbs the old scan behavior; scoped `install`, `config`, and `bundle`
writes default to the current cwd local scope and use `-g`/`--global` for
global scope. Old verb-based
lifecycle subcommands (`create`, `patch`, `remove`, `enable`, `disable`,
`scan`) are not the CLI contract for this topic. Skill package, discovery,
scanner, hub, bundle, curator, and provenance semantics belong to
[055 Skills](../055-skills/spec.md).

`pevo plugin` owns local plugin package inspection, installation, policy, and
marketplace catalog management. With no subcommand it shows help. `list`,
`view`, and `doctor` are read or diagnostic operations and accept `--json`.
`install`, `uninstall`, `enable`, and `disable` write the active profile scope
by default. `--local` writes the current cwd `.psychevo` scope. `-g` or
`--global` writes the active profile scope and conflicts with `--local`.
`enable` enables the plugin package in the selected scope. Canonical installed
package selectors are `profile:name@source` and `project:name@source`; bare
`name` and unscoped `name@source` are accepted only when unique across both
installation scopes. `plugin marketplace list/add/remove` manages local
and Git source catalogs separately from plugin installation and enablement.
Plugin package, manifest, store, worker, hook, and declaration behavior
belongs to [054 Plugins](../054-plugins/spec.md),
[150 Plugin Runtime](../150-plugin-runtime/spec.md), and
[155 Plugin Manifest](../155-plugin-manifest/spec.md).

`pevo stats` owns local token and estimated-cost reporting from the SQLite
state database. It does not contact providers, refresh catalogs, or reconcile
provider invoices.

`pevo auth setup` owns interactive and non-interactive provider setup. It may
write provider/model config, selected model metadata, and scoped `.env`
credentials. It supports provider/model selection, custom OpenAI-compatible
base URLs, `--api-key-stdin`, `--api-key-env`, explicit `--no-auth`, scope
flags, catalog fetch controls, labels, and JSON summary output. Noninteractive
mode is selected by key setup flags or non-TTY stdio; missing required inputs
fail instead of prompting. `--api-key-stdin` must reject TTY stdin for all auth
commands so secrets are not visibly echoed. JSON output is a secret-free summary
with warnings for non-fatal conditions.

`pevo tool` owns local tool and toolset inspection and configuration. `list`
shows built-in and user-defined toolsets, effective mode enablement, and
expanded tools. `show <name>` displays one tool or toolset. `enable` and
`disable` update per-mode `tools.modes.<mode>.enabled_toolsets` or
`disabled_toolsets`; they default to the active profile
`$PSYCHEVO_HOME/config.toml` and accept `--local` for the current cwd
`.psychevo/config.toml`. `-g` and `--global` are not accepted for `pevo tool`
mutations. `create` and `remove` manage user-defined `[toolsets.<name>]`
entries with the same profile-default scope behavior.

`pevo context` owns local context-window usage inspection for one existing
session. It does not contact providers, refresh catalogs, or persist prompt
snapshots.

`pevo session` owns scriptable local session maintenance for the current
cwd: `list`, `show`, `rename`, `archive`, `restore`, `export`, and
`share`. `latest` resolves the latest active `run` or `tui` session for the
current canonical cwd. Exact session ids are matched exactly. The first CLI
batch intentionally does not expose session `delete`, `undo`, or `redo`.

`pevo session export <session|latest>` emits a local artifact from the SQLite
state database without contacting providers or external services. Markdown is
the default artifact format; JSON is available through `-f, --format json` for
structured automation. All CLI commands that expose a `--format` option also
support `-f`. When no output path is supplied, the artifact is written to
stdout. Default artifact filenames use a session-id prefix long enough to
distinguish sibling parent and child sessions created in the same time window.
When an output path is supplied through `-o, --output <path>`, parent
directories may be created, existing files may be overwritten, and the command
reports the written path.

Export content is selected with `-i, --include <comma-separated-list>`. The
export include vocabulary is `header` (`h`), `messages` (`m`), `reasoning`
(`r`), `provider-input-evidence` (`pie`), and `last-provider-request` (`lpr`).
It also supports `last-provider-response` for the latest persisted assistant
response projection.
If `--include` is
omitted, the effective include set is `messages`. The include set is exact:
`--include header` emits only header metadata, and
`--include last-provider-request` emits only the reconstructed provider request.
That request expands the persisted prompt prefix snapshot when the
corresponding user prompt's recorded prefix version and hash match a stored
snapshot, so hidden prefix slots such as agent catalog, skill index, selected
main agent, base mode, and project context appear in the reconstructed request
body. Assistant-turn prefix metadata may be used only as a fallback when prompt
metadata is unavailable. If the full snapshot is missing, stale, or cannot
verify the recorded tool declaration hash against the current registry
reconstruction, the request is marked approximate with warnings instead of
silently applying the latest prefix or current tool schema as exact history.
`reasoning` expands to include `messages` and retains assistant reasoning
blocks without provider evidence metadata inside exported messages. The old
`--with-reasoning`, `--full-inputs`, and `--last-request` flags are not
accepted.

JSON export uses top-level fields that correspond to selected sections.
`header`, when included, contains `{ "session": ..., "options": ...,
"prompt_prefix": ... }`. `prompt_prefix` is header-owned metadata: it exposes
slot names, roles, hashes, sources, provider/model, version, and invalidation
state, but not hidden full slot text. There is no top-level `prompt_prefix`.
`messages` contains the sanitized caller-facing transcript projection.
`provider_input_evidence` contains prompt-scoped provider-input evidence
retained in `context_evidence`, including mode/system instructions, project
instructions, selected skill context, source metadata, and content text. This
evidence bundle does not claim exact provider request replay.
`last_provider_request` contains the best-effort reconstructed provider request
body sent immediately before the latest persisted assistant generation in the
session. This body is regenerated from persisted transcript messages,
prompt-scoped context evidence, session metadata, and the current
OpenAI-compatible request adapter; it is labeled as reconstructed, excludes
HTTP headers and API keys, and may differ from the original network payload if
provider translation code, tool schemas, local image files, or unstored
pruning/runtime options changed. Last-request reconstruction is unredacted and
may expose hidden/system prompts, project instructions, skill context, tool
schemas, tool outputs, reasoning adapter fields, and image data URLs when those
inputs are reconstructable.
`last_provider_response` contains the latest persisted assistant response before
the current undo/revert boundary, derived from stored assistant message,
usage, and allowlisted provider metadata. It is labeled `raw: false` and
`reconstructed: true` because original provider SSE chunks and whole raw
response bodies are not persisted. Reasoning blocks inside its `message` follow
the same policy as transcript export: they are omitted unless `reasoning` is
also included, and provider evidence metadata is not exported.

`pevo session share <session|latest>` creates a local shareable Markdown
artifact for the session and reports its filesystem path. This command is an
explicit local packaging step, not remote publication: it must not upload to a
service, create gists, call provider APIs, or mark durable sharing state. When
no output path is supplied, the artifact is written as
`psychevo-share-<short-session-id>.md` in the current cwd, using the same
collision-resistant short session id as export filenames. `--json` changes the
command result reporting to JSON; it does not change the Markdown artifact
format. Share content is selected with `-i, --include`; the share include
vocabulary is restricted to `header` (`h`), `messages` (`m`), `reasoning`
(`r`), and `provider-input-evidence` (`pie`). If `--include` is omitted, the
effective include set is `messages`. `reasoning` expands to include `messages`.
Future remote
publishing may build on this artifact boundary with an opt-in command. `share`
does not support `last-provider-request`, `last-provider-response`, or legacy
raw provider request flags; provider request and response bodies are
intentionally excluded from share artifacts.

`pevo model` owns local model inspection, explicit default-model configuration,
and explicit provider catalog fetches: `list`, `current`, `set`, and `fetch`.
`list`, `current`, and `set` read only local configuration/cache and never
contact providers. `set <provider/model>` writes the top-level default model
in the current cwd `.psychevo/config.toml` by default, uses
`-g`/`--global` for `$PSYCHEVO_HOME/config.toml`, reports the written path, and
supports `--json`. `fetch` is the only model command that contacts provider
`/models` endpoints.

`pevo config` owns path/config/provider inspection plus scoped provider
creation. Scoped config writes default to the current cwd's `.psychevo`
scope; `-g`/`--global` writes `$PSYCHEVO_HOME`; `--local` explicitly selects
the default local scope. `--global` and `--local` are mutually exclusive.
`--project` is not accepted.

`pevo run` accepts `--permission-mode <default|acceptEdits|plan|dontAsk|bypassPermissions>`
to override the configured permission mode for that invocation. `plan` also
selects the read-only runtime mode. `dontAsk` is non-interactive and denies
any action that would otherwise prompt unless it already matches
`permissions.allow` or a safe default. `--dangerously-skip-permissions` is the
explicit bypass flag and selects `bypassPermissions`; hard denies still apply.
Permission semantics, rule precedence, approval modes, and hard-deny behavior
are defined by [041 Permissions](../041-permissions/spec.md); this topic owns
only the concrete `pevo` command spelling and output surface.

`pevo config permissions list/remove` manages the current cwd's
project-local `.psychevo/config.toml` permission rules. `allow always` approval
writes use the same project-local rule store.

`pevo auth` owns credential status and API-key writes for configured or
built-in providers. It supports `status` and `set`; destructive
unset/logout/remove behavior is not part of this batch. Raw API keys are never
accepted as argv values. `auth set` reads the key from stdin and writes to the
current cwd `.psychevo/.env` by default; `-g`/`--global` writes the global
`.env`.

New `session`, `model`, `config`, and `auth` commands emit human output by
default and support `--json`. JSON errors use:

```json
{"type":"error","message":"..."}
```

`scripts/install.sh` owns checkout-local source installation and source
reinstallation of the `pevo` binary. It verifies the installed binary, builds
and installs Workbench assets, and initializes the global Psychevo home.

## Attachments

- [pevo init](pevo-init.md) defines global home initialization.
- [pevo run](pevo-run.md) defines the live coding-agent command.
- [pevo stats](pevo-stats.md) defines local usage and estimated-cost
  reporting.
- [pevo context](pevo-context.md) defines local context-window usage
  inspection.
- [pevo install](install.md) defines the source install helper script.
- [Testing](testing.md) defines acceptance coverage.

## Related Topics

- [025 CLI](../025-cli/spec.md) defines command-line foundation semantics.
- [026 Commands](../026-commands/spec.md) defines shared command contract
  conventions.
- [210 pevo TUI](../210-pevo-tui/spec.md) defines the fullscreen interactive
  terminal command.
- [055 Skills](../055-skills/spec.md) defines the skill package and lifecycle
  semantics exposed by `pevo skill`.
- [051 Agents](../051-agents/spec.md) defines agent definition semantics.
- [051 Subagents](../051-agents/subagents.md) defines subagent command semantics.
- [280 Channel UX](../280-channel-ux/spec.md) defines `pevo gateway setup`
  channel behavior.
- [120 Provider Registry](../120-provider-registry/spec.md) defines
  provider/model configuration and resolution.
- [041 Permissions](../041-permissions/spec.md) defines permission rules,
  approval modes, and bypass semantics.
- [230 pevo-acp](../230-pevo-acp/spec.md) owns the ACP server packaging behind
  `pevo acp`.
