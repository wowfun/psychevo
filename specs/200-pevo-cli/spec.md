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
- `pevo smoke` product positioning
- `pevo tui` product positioning
- `pevo skill` product positioning
- local session, model, config, and auth inspection/maintenance commands
- local session export/share artifacts

Out of scope:

- file attachments, fork/server attach, remote session publishing, provider
  login, or auth stores
- provider transport semantics beyond explicit `/models` fetches, OAuth, or
  credential pools
- SQLite schema details beyond product path selection
- SDK, HTTP, or MCP transports

## Psychevo Home

`PSYCHEVO_HOME` is the single global directory concept for the `pevo` product
CLI. When unset, it defaults to `~/.psychevo`. `~` expands to the user's home
directory, and relative `PSYCHEVO_HOME` values resolve relative to the process
cwd.

The initialized home tree contains:

- `config.jsonc`
- `.env`
- `state.db`
- `sessions/`
- `logs/`
- `cache/`

`state.db` is the only first-slice session/message store. The reserved
`sessions/` directory is not used for JSON or JSONL transcript sidecars in this
slice.

`PSYCHEVO_DB` may point at `:memory:` or a SQLite path. `~` expands, and
relative paths resolve relative to the process cwd. When unset, `pevo run` uses
`$PSYCHEVO_HOME/state.db`.

`PSYCHEVO_CONFIG` may point at one JSONC config file. When set, it replaces
home and project config discovery for provider configuration.

## Command Families

Implemented first-slice commands:

- `pevo init`
- `pevo run`
- `pevo smoke`
- `pevo tui`
- `pevo skill`
- `pevo stats`
- `pevo context`
- `pevo session`
- `pevo model`
- `pevo config`
- `pevo auth`

Process help is part of the product surface. Top-level commands, subcommands,
arguments, and flags should carry human-readable descriptions in `--help`
output, including stable value names for positional arguments and option
values. High-consequence commands should also use long help to make their
effects clear: whether they write local files or config, read secrets from
stdin, contact providers, emit machine output, include selected skills, or
expose sensitive reconstructed prompt material.

`pevo skill` is the only skill command family name. The obsolete plural
`pevo skills` is not accepted.

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

`pevo smoke` is a deterministic development and validation harness. It keeps
its explicit fake-provider flags and is not redesigned as a live-provider
product entrypoint in this topic.

`pevo tui` owns interactive terminal projection. It accepts `--debug` for
TUI-local debug projections such as usage parts and allowlisted provider
metadata summaries. Debug projection does not change `pevo run --format json`,
does not expose folded reasoning in sanitized transcript messages, and does not
turn provider metadata into transcript content.

`pevo skill` owns local skill lifecycle operations: list, view, create, patch,
remove, enable, disable, install, and scan. Skill package, discovery, scanner,
and provenance semantics belong to [055 Skills](../055-skills/spec.md).

`pevo stats` owns local token and estimated-cost reporting from the SQLite
state database. It does not contact providers, refresh catalogs, or reconcile
provider invoices.

`pevo context` owns local context-window usage inspection for one existing
session. It does not contact providers, refresh catalogs, or persist prompt
snapshots.

`pevo session` owns scriptable local session maintenance for the current
workdir: `list`, `show`, `rename`, `archive`, `restore`, `export`, and
`share`. `latest` resolves the latest active `run` or `tui` session for the
current canonical workdir. Exact session ids are matched exactly. The first CLI
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
If `--include` is
omitted, the effective include set is `messages`. The include set is exact:
`--include header` emits only header metadata, and
`--include last-provider-request` emits only the reconstructed provider request.
That request expands the persisted prompt prefix snapshot when the
corresponding user prompt's recorded prefix hash matches the latest stored
snapshot, so hidden prefix slots such as agent catalog, skill index, selected
main agent, base mode, and project context appear in the reconstructed request
body. Assistant-turn prefix metadata may be used only as a fallback for older
records. If the full snapshot is missing or stale, the request is marked
approximate with warnings instead of silently applying the latest prefix.
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

`pevo session share <session|latest>` creates a local shareable Markdown
artifact for the session and reports its filesystem path. This command is an
explicit local packaging step, not remote publication: it must not upload to a
service, create gists, call provider APIs, or mark durable sharing state. When
no output path is supplied, the artifact is written as
`psychevo-share-<short-session-id>.md` in the current workdir, using the same
collision-resistant short session id as export filenames. `--json` changes the
command result reporting to JSON; it does not change the Markdown artifact
format. Share content is selected with `-i, --include`; the share include
vocabulary is restricted to `header` (`h`), `messages` (`m`), `reasoning`
(`r`), and `provider-input-evidence` (`pie`). If `--include` is omitted, the
effective include set is `messages`. `reasoning` expands to include `messages`.
Future remote
publishing may build on this artifact boundary with an opt-in command. `share`
does not support `last-provider-request` or legacy raw provider request flags;
provider request bodies are intentionally excluded from share artifacts.

`pevo model` owns local model inspection and explicit provider catalog fetches:
`list`, `current`, and `fetch`. `list` and `current` read only local
configuration/cache. `fetch` is the only model command that contacts provider
`/models` endpoints.

`pevo config` owns path/config/provider inspection plus scoped provider
creation. Config writes default to the global `$PSYCHEVO_HOME` scope; `--local`
writes the current workdir's `.psychevo` scope; `--global` explicitly selects
the default scope. `--global` and `--local` are mutually exclusive.

`pevo run` accepts `--permission-mode <default|acceptEdits|plan|dontAsk|bypassPermissions>`
to override the configured permission mode for that invocation. `plan` also
selects the read-only runtime mode. `dontAsk` is non-interactive and denies
any action that would otherwise prompt unless it already matches
`permissions.allow` or a safe default. `--dangerously-skip-permissions` is the
explicit bypass flag and selects `bypassPermissions`; hard denies still apply.
Permission semantics, rule precedence, approval modes, and hard-deny behavior
are defined by [035 Permissions](../035-permissions/spec.md); this topic owns
only the concrete `pevo` command spelling and output surface.

`pevo config permissions list/remove` manages the current workdir's
project-local `.psychevo/config.jsonc` permission rules. `allow always` approval
writes use the same project-local rule store.

`pevo auth` owns credential status and API-key writes for configured or
built-in providers. It supports `status` and `set`; destructive
unset/logout/remove behavior is not part of this batch. Raw API keys are never
accepted as argv values. `auth set` reads the key from stdin and writes only to
the selected scope's `.env`.

New `session`, `model`, `config`, and `auth` commands emit human output by
default and support `--json`. JSON errors use:

```json
{"type":"error","message":"..."}
```

`scripts/install.sh` owns source-based installation of the `pevo` binary. It
supports installing from a local checkout or a cloned Git repository, verifies
the installed binary, and optionally initializes the global Psychevo home.

## Related Topics

- [025 CLI](../025-cli/spec.md) defines command-line foundation semantics.
- [026 Commands](../026-commands/spec.md) defines shared command contract
  conventions.
- [200 pevo init](pevo-init.md) defines global home initialization.
- [200 pevo run](pevo-run.md) defines the live coding-agent command.
- [200 pevo stats](pevo-stats.md) defines local usage and estimated-cost
  reporting.
- [200 pevo context](pevo-context.md) defines local context-window usage
  inspection.
- [200 pevo install](install.md) defines the source install helper script.
- [210 pevo TUI](../210-pevo-tui/spec.md) defines the fullscreen interactive
  terminal command.
- [055 Skills](../055-skills/spec.md) defines the skill package and lifecycle
  semantics exposed by `pevo skill`.
- [051 Agents](../051-agents/spec.md) defines agent definition semantics.
- [051 Subagents](../051-agents/subagents.md) defines subagent command semantics.
- [200 Testing](testing.md) defines acceptance coverage.
- [120 Provider Registry](../120-provider-registry/spec.md) defines
  provider/model configuration and resolution.
- [035 Permissions](../035-permissions/spec.md) defines permission rules,
  approval modes, and bypass semantics.
