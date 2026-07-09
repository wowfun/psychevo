---
name: 052. Agent Runtimes
psychevo_self_edit: deny
---

# 052. Agent Runtimes

Define Psychevo's Runtime Profile model for selecting native Psychevo,
Codex, OpenCode, and future agent runtimes from GUI, Channels, Teams, and
Gateway entrypoints.

## Scope

- Runtime Profile configuration, overlay, validation, and generated rows
- direct Codex `codex app-server --stdio` runtime boundary
- direct OpenCode `opencode serve` runtime boundary
- cached runtime snapshots, health checks, diagnostics, and session takeover
- `runtimeRef` meaning across Gateway, Composer, Channels, and source binding
- generated agent catalog entries from runtime snapshots

Out of scope:

- treating Codex or OpenCode as model providers
- writing raw runtime events into public transcript/history schemas
- real provider, account, auth, or remote service checks in default tests
- broad automatic discovery beyond local executable checks and cached snapshots

## Runtime Profiles

A Runtime Profile is the user-facing selector for an execution runtime. Its id
is the public `runtimeRef` used by Gateway requests, source bindings, Channels,
and Workbench. A profile may point to native Psychevo, direct Codex, direct
OpenCode, or a compatibility peer runtime.

Runtime Profiles are configured under `[runtime_profiles.<id>]` in profile and
project `.psychevo/config.toml` files. Project config overlays profile config
with the existing deep-merge behavior.

```toml
[runtime_profiles.opencode]
label = "OpenCode"
runtime = "opencode"
enabled = true
command = "opencode"
args = ["serve"]
default_mode = "build"
default_agent = "build"
approval_mode = "ask"
sandbox = "workspace-write"
workspace_roots = ["."]

[runtime_profiles.opencode.options]
auto_accept = false
```

Fields:

- `label`: optional display label, defaulting to the profile id
- `runtime`: `native`, `codex`, `opencode`, or `acp`
- `enabled`: defaults to `true`
- `command`: optional executable name or path
- `args`: structured process arguments
- `env`: string map of secret-free environment overlays
- `default_model`, `default_mode`, `default_agent`: adapter-owned defaults
- `approval_mode` and `sandbox`: concise safety defaults
- `workspace_roots`: runtime-visible workspace roots
- `options`: adapter-owned structured metadata

Gateway also returns generated, non-persisted built-in rows for `native`,
`codex`, and `opencode` when they are not already present in effective config.
Generated direct rows do not write config until the user edits or enables them.
Local executable discovery may mark generated Codex/OpenCode rows available or
missing, but it must not spawn provider processes during ordinary list reads.

## Runtime Identity

`runtimeRef` always names a Runtime Profile id. It no longer names a raw ACP
backend by implication. A Runtime Profile may internally reference an ACP peer
backend for compatibility, but product surfaces select the profile id.

Gateway keeps public Psychevo thread ids stable. Native runtime session ids
from Codex or OpenCode are stored as backend-native metadata and projected as
diagnostic aliases only. Source bindings record `runtimeRef` alongside backend
kind and native id so a Channel or GUI source can resume the same public thread
without guessing which runtime owns the native session.

## Gateway Interfaces

Gateway exposes Runtime Profile RPCs:

- `runtime/profile/list`
- `runtime/profile/read`
- `runtime/profile/write`
- `runtime/profile/delete`
- `runtime/profile/setEnabled`
- `runtime/snapshot`
- `runtime/health/check`
- `runtime/session/list`
- `runtime/session/read`
- `runtime/session/resume`
- `runtime/session/archive`
- `runtime/session/unarchive`
- `runtime/session/delete`
- `runtime/session/rename`
- `runtime/session/rollback`

Default list and snapshot calls are cached and local. They may resolve commands
on PATH and read config, but they must not contact real providers. Health
checks are explicit user actions. Default tests use fake runtime clients and
fake local executable discovery.

`runtime/options` accepts `runtimeRef` as a Runtime Profile id. Native returns
native Psychevo controls. Codex/OpenCode return adapter-declared mode/model/
feature controls from the latest snapshot or a bounded unavailable state.

## Direct Codex Adapter

The Codex direct adapter owns `codex app-server --stdio`. It translates public
Gateway actions into Codex app-server methods such as `thread/start`,
`thread/resume`, `thread/list`, `thread/read`, `thread/archive`,
`thread/unarchive`, `thread/delete`, `thread/rollback`, `thread/name/set`,
`thread/goal/*`, `turn/start`, `turn/steer`, `turn/interrupt`, `model/list`,
and `account/rateLimits/read`.

Raw app-server JSON-RPC traffic is adapter-private diagnostic data. Gateway
projects only typed transcript blocks, observations, permission/question
requests, file changes, tool calls, child/agent status, health, and
diagnostics.

Until the Codex worker is implemented, Gateway must reject `turn/start` for a
Codex Runtime Profile with an explicit unsupported-runtime error. It must not
silently fall back to the native runtime while preserving a Codex `runtimeRef`.

## Direct OpenCode Adapter

The OpenCode direct adapter owns `opencode serve`. It manages port selection,
auth/token state, process refcounts, startup diagnostics, and SSE subscription
to OpenCode global events. It maps `session.create`, `session.prompt`,
`session.history`, `session.switchAgent`, `session.switchModel`, `agent.list`,
permission APIs, and question APIs into Gateway operations.

OpenCode modes and agents are runtime-declared. Psychevo must not hardcode
`build`, `plan`, or future mode names into product controls. If the runtime
does not return dynamic metadata, the profile falls back to adapter defaults.

Until the OpenCode worker is implemented, Gateway must reject `turn/start` for
an OpenCode Runtime Profile with an explicit unsupported-runtime error. It must
not silently fall back to the native runtime while preserving an OpenCode
`runtimeRef`.

## Workbench UX

`Capabilities > Agents` has an internal `Runtime Profiles` segment beside
`Definitions`, `Teams`, and `ACP Backends`.

The segment shows compact rows for generated and configured profiles:

- label, runtime kind, source, generated/configured badge, and enabled state
- cached health: ready, missing, needs auth, unchecked, or error
- command/args diagnostics without resolved secrets
- Refresh and Doctor actions
- a details editor for configured Project/Profile profiles
- session browser/takeover actions for runtimes that support native sessions

The Composer shows a compact Runtime Profile selector and a small runtime label
on non-native threads. It exposes runtime controls only when the snapshot
declares them. Missing controls render read-only "runtime default" style
states instead of disabled controls that imply broken behavior.

## Channels

Channel config may store a `runtime_ref` default for new channel-created
threads. The field selects a Runtime Profile id and remains separate from
model, cwd, and permission defaults.

Channels expose selective non-destructive runtime commands:

- `/profile`
- `/profile list`
- `/profile use <id>`
- `/profile status`
- `/profile resume <native-session-id>`
- `/profile reset`

Destructive native session operations such as archive, rollback, rename, and
delete remain GUI-only because Channels cannot consistently provide the needed
confirmation UX.

## Agent Catalog And Teams

Runtime snapshots may generate agent catalog entries such as `opencode-build`
or `opencode-plan`. Generated entries preserve `runtimeRef` and native
agent/mode ids in metadata. Team templates and Mission orchestration reference
agent/profile ids, not raw runtime strings.

Runtime-specific options remain adapter-owned. Agent Markdown definitions do
not embed runtime commands, env, or provider secrets.

## Validation

Default validation uses deterministic fake runtimes and local harnesses:

- Runtime Profile config parsing, overlay, generated-row precedence, disabled
  overrides, and validation errors
- explicit `turn/start` rejection for direct Codex/OpenCode Runtime Profiles
  when the adapter worker is unavailable
- fake Codex app-server covering thread, turn, goal, interrupt, steer,
  archive, rollback, and event projection
- fake OpenCode serve covering session create/prompt/history, global SSE
  events, agent list, permissions, questions, and session takeover
- Gateway source binding tests proving Psychevo thread ids and native session
  ids remain distinct
- Channel `/profile` route tests for list/use/status/resume/reset
- Workbench tests for Runtime Profiles management, Composer selector, runtime
  status, and session browser actions

Real Codex/OpenCode binary and provider checks are opt-in live validation only.

## Related Topics

- [021 Gateway](../021-gateway/spec.md) defines threads, turns, source
  binding, and observation projection.
- [028 Channels](../028-channels/spec.md) defines Channel source and runtime
  policy boundaries.
- [051 Agents](../051-agents/spec.md) defines agent definitions and generated
  agent identities.
- [247 Capability Management](../247-capability-management/spec.md) defines
  Workbench Capabilities layout.
- [280 Channel UX](../280-channel-ux/spec.md) defines Channel setup and IM
  fallback behavior.
