---
name: 051. ACP Agents
psychevo_self_edit: deny
---

Define external ACP Agent backend registration, capability policy, and
execution through Gateway's first-class Agent Session seam.

## Scope

- configured ACP Agent backends
- generated and Markdown Agent Definitions referencing a backend
- top-level and managed subagent execution
- ACP client callback policy
- managed Codex ACP and local executable shortcuts
- commands, history, interactions, diagnostics, and session lifecycle

Out of scope:

- making ACP Agents model providers
- direct Codex app-server or OpenCode HTTP/SSE integration
- browser-owned Agent processes
- automatic network discovery
- treating ACP as Psychevo's internal application interface

## Backend Registration

External executable backends use `[agents.backends.<id>]` and `kind = "acp"`:

```toml
[agents.backends.cursor]
kind = "acp"
enabled = true
label = "Cursor"
command = "cursor-agent"
args = ["--acp"]
env = {}
entrypoints = ["peer", "subagent"]
client_capabilities = ["fs.read", "fs.write", "terminal"]
cwd = "invocation"
protocol = "stable_v1"
```

Defaults are:

- `enabled = true`
- `entrypoints = ["peer", "subagent"]`
- `client_capabilities = ["fs.read", "fs.write", "terminal"]`
- `cwd = "invocation"`
- `protocol = "stable_v1"`
- empty args and env
- label equal to backend id when omitted

`command` is one executable, never a shell command line. Arguments and
environment remain structured. Bare commands use native executable lookup,
including Windows `PATHEXT`; stored configuration stays platform-neutral.

Profile and Project configuration use the normal deep-merge rules. A disabled,
unresolvable, or protocol-incompatible backend stays catalog-visible with a
safe diagnostic and makes dependent targets non-runnable.

Executable configuration belongs only to the backend. Runtime Profiles refer
to it through `backend_ref` and must not duplicate command, args, or env.

## Agent Definitions And Targets

A Markdown Agent Definition may reference a backend:

```yaml
---
name: cursor-reviewer
description: Review code changes using Cursor's ACP agent.
backend:
  ref: cursor
entrypoints: [subagent]
tools: [read, write, edit]
mcpServers: [repo]
---
Review the requested changes and return concise findings.
```

Markdown must not declare executable details. Each enabled backend generates a
default Agent Definition using the backend id. A same-name Markdown definition
shadows it and supplies product identity and policy.

Execution is Agent-targeted through a Gateway-validated
`RunnableTarget { agentRef, runtimeProfileRef }`. Callers never execute a
backend id directly and never pair definitions and profiles themselves.

## Capability And Policy

`client_capabilities` is the backend hard ceiling:

- `fs.read`
- `fs.write`
- `terminal`

The captured Agent Definition narrows it: read enables filesystem read,
write/edit enables filesystem write, and exec/write-stdin enables terminal.
Gateway workspace, permission, source interaction, and secret policy narrow it
again. An unsupported callback fails closed.

When terminal is effective, the client implements the complete stable-v1
terminal lifecycle: create, output, wait-for-exit, kill, and release. Create
uses direct argv execution, a canonical cwd inside the captured workspace,
bounded environment entries, the captured process environment, and a Gateway
permission decision before spawn. Output is UTF-8-safe and bounded by the
requested byte limit. Terminal ids are scoped to the requesting ACP session;
kill, release, connection loss, and process shutdown terminate the owned child
tree and cannot address another session's terminal.

Agent-declared MCP servers are passed only when the same server name is present
in the captured backend allow-list and the captured Agent Definition. The
Adapter resolves those names from the effective profile, enabled plugin, and
selected capability-root MCP declarations; a missing, disabled, duplicated, or
backend-disallowed declaration fails before prompt delivery instead of being
silently omitted. Native Psychevo tools are not implicitly exposed to external
Agents.

For stable ACP v1 the Adapter sends the same resolved server declarations on
both `session/new` and `session/load`. Stdio declarations carry an Adapter-
resolved absolute executable path, args, and their explicit environment.
Streamable HTTP declarations lower to ACP HTTP
only when the Agent advertises `mcpCapabilities.http`; configured headers and a
bearer token resolved from the named environment variable or Psychevo's MCP
OAuth store remain in process memory and are never projected or logged. An
unsupported transport, an explicit stdio cwd different from the session cwd,
or a Native-only per-server tool/startup policy fails closed because stable ACP
has no field that can preserve that policy.

ACP permission and elicitation requests enter the shared Gateway interaction
broker. They retain option id, kind, scope, safe metadata, expiry, and source
policy. A missing interactive handler rejects the request safely.

The outbound client advertises ACP form elicitation only when it can route the
request through that broker. Session-scoped primitive form properties are
projected as typed clarify questions and accepted answers are validated back
against the requested schema before the ACP response is sent. URL mode,
request-scoped forms without a public Thread, unknown property types, and
unrenderable schemas are not advertised or are declined; they never become a
raw JSON prompt or an automatically accepted response.

An effective feature is the intersection of Agent negotiation, Adapter
implementation, Psychevo certification, and binding grant. Standard ACP is the
baseline. Reviewed Codex/OpenCode capability packs activate only for the exact
stable Agent versions whose local source was audited (`codex-acp 1.1.2` and
`OpenCode 1.17.18`). Future patch versions, prereleases, and build-qualified
versions remain standard ACP until separately reviewed. Raw or unknown `_meta`
never becomes a product action.

## Process And Session Lifecycle

Gateway owns every outbound ACP process. Workbench, Channels, and inbound
`psychevo-acp` never launch or supervise one.

A supervisor generation is keyed by captured backend/profile fingerprint,
canonical workspace, and auth scope. It owns a resident process and ACP
connection. Public threads own independent session actors and native session
ids. Sessions may share a connection only when the Agent supports routing them
safely.

Generation startup is atomic. Leases and reference counts prevent an old
generation from terminating active sessions in a replacement generation. Idle
eviction requires zero active turns and zero pending interactions.

The stable-v1 lifecycle is initialize, authenticate when needed,
new/load/resume, apply required config, prompt, stream updates, terminal, and
capability-gated list/fork/close/delete. The v1 `session/fork` extension is used
only when the initialize response advertises it; enabling the SDK's unstable
request type does not negotiate protocol v2. Every lifecycle request uses the
same ordered notification barrier as load/prompt and is serialized by the
owning public-thread session actor. Unsupported operations fail before a wire
request with `delivery=notDelivered`.

`session/list` preserves the exact absolute-cwd filter and opaque cursor.
`session/resume` and `session/fork` receive the exact resolved MCP declaration
set captured for the session; neither may silently substitute an empty set or
re-resolve mutable configuration. Resume creates a new session epoch without
claiming history replay. Fork creates a distinct public-thread/native-session
pair and reduces any response-preceding replay before publishing its snapshot.

Close and delete first cooperatively cancel active work, then issue their
advertised request under the same per-session lock. A successful close/delete
removes the resident session, callback context, and every owned terminal.
Delete may also target a listed non-resident native session. Failed lifecycle
requests do not fabricate successful cleanup. Generation shutdown closes
resident sessions when supported and always clears callback contexts and kills
owned terminals before the process is reaped.

An Agent `AuthRequired` error from a pre-delivery session lifecycle or config
request becomes product error `acp_auth_required`, remains
`delivery=notDelivered`, and points to `backend/doctor`. Other Agent request
errors retain only the numeric ACP code and a bounded single-line message.
Untrusted ACP error `data` is never copied to product errors, events, or
diagnostics.

Interrupt sends cooperative `session/cancel`; process termination is a timeout
fallback. A process exit wakes all waiters and never triggers an automatic
resend.

The binding and native session id are persisted before prompt delivery.
Top-level and subagent turns use the same Adapter Interface, with distinct
public thread identity and interaction policy.

## ACP Protocol And Projection

Gateway uses `agent-client-protocol = 1.2.0`. Outbound ACP wire v1 is the only
certified protocol and the initialize response version is validated.
`experimental_v2` is rejected with `unsupported_protocol`; no v2-first attempt
or fallback is allowed.

The SDK's `unstable` aggregate does not forward its schema crate's
`unstable_llm_providers` feature. Gateway therefore pins the SDK's exact
`agent-client-protocol-schema = 1.4.0` dependency with that feature so a
reviewed stable-v1 `agentCapabilities.providers` declaration remains a typed
negotiated fact. Provider capability-pack behavior still requires the exact
reviewed Agent identity, version, capability shape, and authentication method;
the schema feature alone never activates a product capability.

Prompt input preserves supported text, image, resource, resource-link, and
embedded-context blocks. Unsupported content is rejected before delivery.

Stable session config options drive typed model, effort, and mode controls.
Gateway applies required values before prompt and distinguishes a successful
set response from a later authoritative observation. Config failure blocks the
turn.

ACP updates map to bounded product facts:

- Agent message and thought chunks
- tool call and tool call update
- plan/status
- available commands
- mode and config options
- usage and session information
- permission, elicitation, auth, and terminal lifecycle

Unknown updates are tolerated and bounded in diagnostics. Raw ACP envelopes,
ids, secrets, or arbitrary metadata do not enter product transcript state.

Replay and live updates use the same reducer with explicit origin, process
generation, session epoch, and completion barrier. Time-based notification
draining is forbidden. An Agent that supports load/resume remains history
authoritative; Gateway stores product-safe projections and checkpoints. An
Agent without load/resume is explicitly non-resumable after process loss.

Available commands enter the shared command catalog through a namespaced typed
descriptor. Psychevo core commands cannot be overridden.

## Product Shortcuts

Gateway exposes a `codex` ACP backend backed by the managed
`@agentclientprotocol/codex-acp` version defined in [052 Agent
Runtimes](../052-agent-runtimes/spec.md). It remains visible when uninstalled and
provides an install recovery action.

If `opencode` resolves and no effective backend shadows it, Gateway writes a
Profile ACP backend with command `opencode` and args `["acp"]`. The existing
Hermes shortcut follows the same rule. Auto-materialization never overwrites
Profile or Project configuration and never downloads software.

## Management And Diagnostics

Workbench exposes Profile ACP backends under `Capabilities > Agents > ACP
Backends`. Backend administration edits configuration; it does not grant
execution permission.

Catalog and context reads are cache-only. `backend/doctor` performs explicit,
bounded checks for executable resolution, managed installation, initialize,
protocol/capability compatibility, auth, and a disposable session when needed.
Managed adapters additionally support `backend/install`, `backend/repair`, and
`backend/upgrade`.

CLI backend add/write commands keep structured command, args, and env fields.
Diagnostics are product-safe and redact environment values, credentials,
native ids, and raw protocol payloads.

## Acceptance Criteria

- Native and ACP top-level turns traverse the same Thread Application Interface.
- Two ACP threads can execute independently without per-turn process startup.
- Required v1 config is applied before prompt; failure prevents delivery.
- Images/resources are preserved or rejected, never textualized or omitted.
- Replay cannot leak into a new live turn or create duplicate transcript rows.
- Permission and terminal callbacks obey captured capability and workspace policy.
- Workbench and Channels do not branch on backend or Agent product names.
- Codex/OpenCode direct transports and fallback paths do not exist.
- Deterministic fake ACP Agents cover protocol, lifecycle, history, controls,
  interactions, process failure, and cleanup.
