---
name: 247. Capability Management
psychevo_self_edit: deny
---

# 247. Capability Management

Define the Workbench product surface for managing agents, skills, plugins, MCP
servers, and toolsets.

## Scope

- top-level Workbench Capabilities navigation
- profile-scoped management defaults for GUI capability writes
- Project/Profile agent definition and backend management composition
- skill, plugin, MCP, and toolset management composition
- MCP OAuth, bearer-token environment references, and per-tool exposure policy
- deterministic frontend and Gateway validation expectations

Out of scope:

- a generic capability runtime owner or `capability/list` aggregation RPC
- hosted marketplaces, ratings, accounts, or remote registry trust
- live provider or live third-party OAuth validation in default tests
- changing toolset semantics into model-visible executable tools

## Workbench Surface

Workbench exposes `Capabilities` as a top-level view beside transcript,
settings, automations, and other app-level surfaces. The view has five tabs:
`Agents`, `Skills`, `Plugins`, `MCP`, and `Tools`. There is no `All` tab.

The frontend composes domain RPCs directly. It must not introduce a generic
capability object that hides the owning runtime module. Search filters the
active tab by name and description only. Lists use compact status chips,
enablement controls, source labels, and one details/configuration panel for the
selected row.

Workbench writes default to the active profile scope unless a domain explicitly
declares a project-local workflow. The Capabilities view must make effect
timing clear: saved configuration refreshes the management view immediately,
but running sessions are not implicitly restarted. Current-session runtime may
therefore differ from the next run until an explicit reload or new run occurs.
Create and install flows are opened from the active tab header instead of being
always-visible forms at the bottom of the page. `Create agent`, `Install
skill`, `Install plugin`, `Add MCP server`, and `Create toolset` open scoped
create panels that use shared Workbench action, field, and panel primitives.
Successful writes close the panel, refresh the active tab, and show the
existing next-run effect message; failed writes keep the panel open with the
user's draft. The active create panel must remain inside the visible
Capabilities page bounds at desktop and narrow widths, and long forms must be
reachable by scrolling the page or panel without horizontal overflow.

Settings must not duplicate or link to the Agents management surface. Users
reach agent definitions, teams, and ACP backend configuration from the
top-level `Capabilities > Agents` view only.

## Agents

The Agents tab lists and manages Project and Profile Markdown agent
definitions, plus configured ACP backends. It uses agent-owned and
backend-owned Gateway RPCs directly; it must not introduce a generic capability
object that hides the owning runtime module.

The tab has an internal segmented view with `Definitions`, `Teams`,
`Runtime Profiles`, and `ACP Backends`.
`Definitions` lists mutable Project/Profile Markdown agents across active,
shadowed, and disabled states. Rows show name, description, source label,
enablement, entrypoints, backend reference when present, and compact
diagnostics. Truncated row descriptions keep the full description in the native
hover title. Each mutable row owns one enablement switch; edit is exposed from
the Markdown preview action stack and delete lives in the detail header.
Built-in, generated, Claude-compatible, and plugin-provided agents are not
directly edited here; generated ACP agents are controlled by their backend.

Creating an agent defaults to the current Project target and allows switching
to Profile. Create and edit are both in the right-side detail panel rather than
an overlay create/edit panel. Editing an existing agent keeps name and target
read-only; users create a new agent and delete the old one to rename. The
editor defaults to a structured form with description, instructions,
enablement, entrypoints, optional backend ref, tools, and MCP servers. A
Markdown mode exposes the raw definition source for advanced edits. Structured
saves preserve frontmatter fields the form does not display. Raw saves are
validated before the file is written. Agent detail previews render the raw
Markdown definition, including YAML frontmatter, through the same shared
Markdown renderer used for `SKILL.md`.

`ACP Backends` carries the existing profile/project backend management
workflow: add/edit/delete, enablement, entrypoint controls, client capability
controls, MCP server list, and doctor checks. Backend command environment
values remain write-only; management responses may expose environment variable
names but never resolved secret values.

`Runtime Profiles` carries the user-facing runtime selector management surface
for native Psychevo, direct Codex, direct OpenCode, and future runtime
profiles. Rows show generated/configured state, runtime kind, enablement,
cached health, command diagnostics, Refresh/Doctor actions, and session
takeover affordances when supported. Editing or enabling a generated Codex or
OpenCode row writes a real Profile or Project `runtime_profiles.<id>` config
entry; ordinary list/refresh reads must not spawn real provider processes.

When the local Gateway detects known ACP executables in its launch environment,
it may auto-create Profile ACP backend rows so `Capabilities > Agents > ACP
Backends` opens with useful localhost options. The supported shortcuts are
`opencode` as command `opencode` with args `["acp"]`, and `hermes` as command
`hermes` with args `["acp"]`. Existing effective backend definitions take
precedence, so auto-creation is additive and never overwrites Profile or Project
configuration.

## Skills

The Skills tab lists, reads, installs, uninstalls, enables, and disables skills
through skill-owned Gateway RPCs. Project/Profile-managed skills can also
replace their `SKILL.md` source through `skill/write`; plugin, system,
configured external, explicit, and compatibility roots remain preview-only.
Installation accepts a local path or Git source. Scanner-blocked dangerous
results and overwrite operations require an explicit confirmation before the
frontend sends a force request.

The tab is skill-domain specific, not a lossy generic capability row. It shows
all valid discovered skills in the management catalog, including disabled,
unsupported, hidden, and collision-ambiguous rows. List rows show name,
description, the human-facing source label, non-default readiness/platform
state, and collision state as plain row metadata. The raw `source` value remains
available only for mutation target resolution and diagnostics. Prompt
visibility is not exposed as a row chip, detail row, or status filter. Skills
filtering is search-only in this UI pass. Each row owns an inline enablement
switch; the detail panel does not duplicate that switch.

Selecting a skill calls `skill/read` with the row path for a bounded `SKILL.md`
preview rendered with the shared Markdown renderer. The preview uses
`preview_content` when present and falls back to `content`; it must not recreate
Markdown, frontmatter parsing, or preview copy chrome inside the Capabilities
page. Its shared copy action copies the raw preview source through the host
clipboard boundary. Mutable Markdown previews show an edit action below the
copy action; disabled edit actions state why the source is read-only. Delete or
uninstall actions live in the detail header's top-right action area. The detail
card fills the available detail-column height, and the `SKILL.md` preview owns
internal scrolling inside that card. Details hide empty fields and show linked
files, paths, tags, missing environment variable names, missing credential file
paths, tools/toolsets hints, platform status, and compact diagnostics when
present. Redundant entry file paths are hidden when they are equivalent to
`skill_dir/SKILL.md`; non-standard entrypoints remain visible. Raw JSON is not
the primary detail view. Enablement is name-scoped and remains
available for disabled rows through the row switch. Uninstall is enabled only
for mutable profile/project-installed skills and otherwise shows the reason. The install
form supports a source, optional name, profile default or project target, and
explicit force confirmation, but it is opened through the `Install skill`
action instead of occupying persistent page space. It must not persist secret
values in frontend storage.

## Plugins

The Plugins tab uses plugin-owned Gateway RPCs for list, read, doctor, install,
uninstall, and enablement. Existing plugin read diagnostics remain the source
of truth for package metadata.

Plugin details show source identity, manifest path/kind, interface metadata,
declared skills, MCP servers, hook sources, agent roots, toolsets, provider
descriptors, worker state when available, and doctor diagnostics. Installing or
overwriting a plugin package requires explicit confirmation when the runtime
reports an existing package or force-worthy condition, and the install form is
opened through the `Install plugin` action.

The Plugins tab also supports compact catalog/import inspection. The scoped
install panel can inspect a local path, Git source, npm source, or catalog row
before installation. Inspection shows framework, canonical id, source kind,
scope, adapter mode, trust state, target lanes, projected contributions,
unsupported lanes, stage diagnostics, readiness, and whether changes affect the
current session or the next run.

Plugin row and detail states use the fixed status vocabulary `Available`,
`Installed`, `Disabled`, `Needs trust`, `Needs setup`, `Failed`, and
`Unsupported target`. Status text must describe the next useful action rather
than internal implementation state. Trust actions require explicit user intent
and record trust only for the current package fingerprint.

## MCP

The MCP tab manages profile-scoped stdio and streamable HTTP server
declarations. It supports add, edit, remove, enable, explicit test/probe, OAuth
login/logout, bearer-token environment references, and per-tool include/exclude
policy.

Streamable HTTP servers may declare:

- `bearer_token_env_var`
- `scopes`
- `oauth_resource`
- `[mcp_servers.<name>.oauth].client_id`

Inline bearer tokens are rejected. Stdio servers reject OAuth, OAuth resource,
bearer-token environment, and HTTP auth fields.

Runtime auth resolution for streamable HTTP is:

1. configured bearer-token environment variable, when present and non-empty
2. stored MCP OAuth token
3. unauthenticated connection

OAuth login is an asynchronous Gateway flow. Starting login returns a session
id and authorization URL. Workbench opens the URL through the host boundary and
polls status until success, failure, cancellation, or timeout. Production OAuth
tokens are stored in the system keyring under service
`psychevo-mcp-oauth`, with an account derived from active profile home, server
name, and server URL. Tests use an injected fake keyring store.

MCP status and probing must be explicit or scoped to the selected server so the
management screen does not unexpectedly start arbitrary local processes or make
network calls.
Adding or editing a server uses a scoped `Add MCP server` panel with the same
fields as the `mcp/upsert` RPC and no inline bearer-token value field.

## Tools

The Tools tab makes toolsets the primary management surface. It lists effective
toolsets, included tools, included toolsets, unknown tools, source facts, and
per-mode enabled state for `default` and `plan`.

Workbench can enable or disable a toolset per mode, create or overwrite custom
toolsets, and remove custom toolsets. Toolset writes default to the active
profile scope. Custom overwrite and removal require explicit confirmation.
The built-in `coding-core` toolset is view-only in Workbench and cannot be
enabled, disabled, created, overwritten, or removed through toolset management
writes. The built-in `web` toolset remains mode-configurable but not removable.
Toolset list/read rows expose management hints with `mode_mutable` and
`removable` booleans so clients can present only supported actions.
Creating or overwriting a custom toolset uses a scoped `Create toolset` panel
instead of a persistent form in the Tools tab.

Toolsets remain selection metadata only. They do not become model-visible
tools, and they do not own execution bindings.

## Gateway Interfaces

Gateway exposes domain RPCs instead of a capability aggregate:

- `agent/list`, `agent/read`, `agent/write`, `agent/setEnabled`,
  `agent/delete`, `backend/list`, `backend/doctor`, `backend/write`,
  `backend/delete`
- `runtime/profile/list`, `runtime/profile/read`, `runtime/profile/write`,
  `runtime/profile/setEnabled`, `runtime/profile/delete`, `runtime/snapshot`,
  `runtime/health/check`, `runtime/session/list`, `runtime/session/read`,
  `runtime/session/resume`, `runtime/session/archive`,
  `runtime/session/unarchive`, `runtime/session/delete`,
  `runtime/session/rename`, `runtime/session/rollback`
- `skill/list`, `skill/read`, `skill/install`, `skill/uninstall`,
  `skill/setEnabled`, `skill/write`
- `plugin/list`, `plugin/read`, `plugin/doctor`, `plugin/install`,
  `plugin/uninstall`, `plugin/setEnabled`, `plugin/import/inspect`,
  `plugin/setTrust`, `plugin/catalog/list`, `plugin/catalog/add`,
  `plugin/catalog/remove`
- `tool/list`, `tool/read`, `tool/setEnabled`, `tool/create`, `tool/remove`
- `mcp/list`, `mcp/read`, `mcp/upsert`, `mcp/remove`, `mcp/setEnabled`,
  `mcp/setToolPolicy`, `mcp/test`, `mcp/oauth/start`,
  `mcp/oauth/status`, `mcp/oauth/logout`

All responses are secret-free. RPCs that accept secret-bearing environment
variable names may echo the variable name but never the resolved secret value.

## Validation

Default validation uses deterministic local harnesses and fake providers.
Runtime tests cover MCP config parsing/writing, OAuth fields, bearer-token env
references, per-tool policy, inline token rejection, auth precedence, and fake
keyring save/load/delete behavior.

Gateway tests cover agent definition and backend writes, skill read/write,
plugin, toolset, and MCP write methods, force confirmation paths, secret-free
responses, and MCP OAuth start/status/logout against a fake local OAuth server
and fake keyring.

Workbench tests cover top-level navigation, tab search, detail panels, force
confirmations, agent detail-panel create/edit, Markdown preview copy/edit
actions, skill `SKILL.md` editing, delete/uninstall header placement, agent
enablement, backend management, plugin import inspection, trust flow,
unsupported target rendering, MCP per-tool policy, OAuth polling states, and
next-run effect messaging. Visual validation must include desktop and mobile
Capabilities screens without text overlap.

## Related Topics

- [051 Agents](../051-agents/spec.md) owns agent definition, discovery,
  enablement, and backend-reference semantics.
- [052 Agent Runtimes](../052-agent-runtimes/spec.md) owns Runtime Profiles,
  direct runtime snapshots, and native session takeover semantics.
- [055 Skills](../055-skills/spec.md) owns skill package and lifecycle
  semantics.
- [056 MCP](../056-mcp/spec.md) owns MCP runtime normalization, auth, and
  dispatch semantics.
- [150 Plugin Runtime](../150-plugin-runtime/spec.md) owns plugin package
  storage and diagnostics.
- [007 Tool Surface](../007-tool-surface/spec.md) owns toolset expansion and
  execution-binding semantics.
- [200 pevo CLI](../200-pevo-cli/spec.md) owns command spelling and scope
  behavior.
- [240 pevo Web](../240-pevo-web/spec.md) owns Workbench product layout.
