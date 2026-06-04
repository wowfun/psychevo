---
name: 051. Peer Agents
psychevo_self_edit: deny
---

Define external peer-agent backend registration and ACP-compatible agent
execution for Psychevo.

## Scope

- configured external agent backends
- generated agent identities from backend registrations
- Markdown `backend.ref` integration with existing agent definitions
- top-level peer-thread and subagent execution semantics
- ACP client capabilities, command projection, session import, and diagnostics

Out of scope:

- network relay, LAN exposure, cloud agent registries, and automatic discovery
- durable cron or scheduled peer-agent execution
- built-in templates for specific ACP agents beyond a generic ACP template
- making external agents model providers

## Backend Registration

External executable backends are configured under `[agents.backends.<id>]`.
The first supported backend kind is `acp`.

```toml
[agents.backends.cursor]
kind = "acp"
enabled = true
label = "Cursor"
description = "Cursor ACP coding agent."
command = "cursor-agent"
args = ["--acp"]
env = {}
entrypoints = ["peer", "subagent"]
client_capabilities = ["fs.read", "fs.write", "terminal"]
cwd = "invocation"
```

Defaults are:

- `enabled = true`
- `entrypoints = ["peer", "subagent"]`
- `client_capabilities = ["fs.read", "fs.write", "terminal"]`
- `cwd = "invocation"`
- `args = []`
- `env = {}`
- `label = <backend id>`

`description` is required for generated agent catalog visibility. Backends
without a description may still appear in diagnostics but must not generate a
model-visible agent.

Global and project config use the normal deep-merge behavior; project config may
define or override command-bearing backends. `enabled = false` disables the
generated agent and makes Markdown definitions that reference the backend
non-runnable with a diagnostic.

## Agent Definitions

Markdown agent definitions reference an external backend with `backend.ref`:

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
Review the requested changes and return a concise finding list.
```

Markdown files must not declare `command`, `args`, or executable backend
details. A command-bearing Markdown definition is invalid and should surface a
diagnostic. Markdown definitions may define identity, body instructions,
entrypoints, skills, MCP scope, model preference, effort, background behavior,
and tool policy.

Each enabled backend with a description generates a default agent definition
using the backend id as the agent name. Generated agents default to the
backend's `entrypoints`, have no instruction body, and send no extra prompt
wrapper. A same-name Markdown definition shadows the generated definition and
becomes the editable source of identity and policy.

All peer/subagent execution is agent-targeted. Public APIs target `agentName`;
direct task execution by backend id is not supported.

## Capability And Policy

`client_capabilities` declares the backend hard ceiling for ACP client
callbacks:

- `fs.read`
- `fs.write`
- `terminal`

When omitted, the provider-passthrough default enables all three. The selected
agent's `tools` policy then narrows the effective callbacks:

- `read` maps to `fs.read`
- `write` and `edit` map to `fs.write`
- `exec_command` and `write_stdin` map to `terminal`

For peer agents, `mcpServers` are passed to ACP `session/new` only when they are
explicitly declared by the backend or selected Markdown agent. Psychevo runtime
tools are not automatically exposed to external ACP agents.

External ACP `session/request_permission` requests are projected as Gateway
permission requests. If no interactive approval handler is available, requests
fail closed and the peer timeline records a diagnostic.

## Execution Semantics

Top-level peer threads use a Psychevo-local thread id as the durable public
identifier. ACP native session ids are stored as backend metadata. Surfaces may
display an alias of the form `acp:<backend-id>:<native-session-id>` for search,
debugging, and imported sessions.

ACP process lifecycle is per session:

- a top-level peer thread owns one ACP process and native session until closed
- each subagent run starts a fresh ACP process and native session
- different peer threads and subagent runs may run concurrently

Gateway queueing is per thread. Peer turns support queue and interrupt. Live
steering is unsupported for ACP peers in the first version. Cancel first sends
the cooperative ACP close/cancel operation when available, then kills the peer
process after a timeout and marks the turn interrupted.

Subagent results use the existing compact subagent result contract. The parent
agent receives a compact summary; the full peer transcript remains in the child
thread timeline.

## ACP Projection

Psychevo acts as an ACP client for peer agents. It starts stdio processes,
initializes ACP, creates or loads sessions, sends prompts, maps session updates
to Gateway events, and persists normalized semantic timeline rows. Live ACP
updates provide immediacy; Psychevo's normalized timeline is authoritative for
reloads across TUI, Web, Desktop, and future surfaces.

Markdown body instructions are delivered by first trying a supported ACP
config/system-like option. If unavailable, the body is prepended to the first
prompt in a new ACP session only. Generated agents have no body and therefore
send no instruction prefix.

Existing ACP native sessions may be listed through a resume/import picker when
the peer supports `session/list`. Import creates a Psychevo thread bound to the
ACP native session id. Native sessions are not auto-imported.

ACP `available_commands_update` entries are projected into the shared command
catalog as namespaced peer commands. Users type `/agent:command`; Gateway
removes the namespace before sending the slash command to that peer. Psychevo
core commands keep their names and are never overridden by peer commands.

Initial Gateway implementation slice:

- `turn/start` accepts optional `agentName`; when it resolves to an ACP backend
  agent with the `peer` entrypoint, Gateway routes the turn to the ACP peer.
- The first execution slice starts a stdio ACP process per turn and reloads the
  stored ACP native session id when present. A later slice may keep the process
  resident for the lifetime of the Psychevo thread.
- Gateway persists prompt and assistant timeline rows plus session metadata with
  the selected agent, backend id, and ACP native session id.
- Client callbacks initially advertise and implement `fs.read` and `fs.write`
  when allowed by backend capabilities and the selected agent tool policy.
  `terminal` is not advertised until terminal lifecycle projection lands.
- ACP permission requests and write-file callbacks use the existing Gateway
  approval handler when available; otherwise they fail closed.

## Diagnostics And CLI

Backend probing is lazy. Normal startup must not spawn every configured peer.
`pevo agents backend doctor` may run explicit local diagnostics with short
timeouts: command resolution, process spawn, ACP initialize, session/new,
reported models/modes, commands, and capability status.

`pevo agents backend add` supports a generic ACP backend template in the first
version and writes global config by default.
