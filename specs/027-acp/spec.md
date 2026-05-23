---
name: 027. ACP
psychevo_self_edit: deny
---

# 027. ACP

Define Psychevo's Agent Client Protocol boundary.

ACP is a protocol interface over `psychevo-runtime`. It maps protocol requests
and notifications to runtime sessions, invocations, observations, permissions,
auth, commands, model and mode controls, config controls, and capability source
inputs. ACP does not own agent execution, provider behavior, tool semantics,
runtime permission policy, durable storage, or MCP semantics.

## Scope

- ACP session creation, loading, listing, closing, prompting, and cancellation
- ACP projection of runtime messages, reasoning, tools, permissions, models,
  modes, config options, and command metadata
- ACP-provided MCP servers as session-scoped runtime capability sources
- ACP authentication projection for configured model providers
- protocol-level rejection, observation, and completion mapping

Out of scope:

- product packaging, binary names, CLI wrappers, process help, or install
  behavior
- HTTP, WebSocket, or stdio server packaging choices
- ACP registry publishing and package distribution
- filesystem or terminal delegation to an ACP client
- replacing runtime permissions with client-side policy
- plugin marketplace, install, update, or trusted-script extension mechanics

## Protocol Boundary

An ACP implementation accepts protocol requests, translates them into runtime
inputs, projects runtime observations back to ACP updates, and keeps transport
state for active ACP sessions.

ACP implementations must call `psychevo-runtime` directly. They must not shell
out through a CLI command for normal prompting, cancellation, permission, MCP,
command, model, config, or session behavior.

Concrete product specs choose how Psychevo exposes an ACP server. This topic
owns the protocol mapping, not the product process that hosts it.

## Sessions

ACP session ids identify active ACP session actors. Each actor maps to a
Psychevo runtime session id once runtime creates or loads the backing session.
New ACP sessions use source `acp` for runtime persistence.

`session/new` creates a runtime session boundary for the selected cwd,
provider, model, mode, permissions, and ACP-supplied MCP sources. `session/load`
opens an existing Psychevo session for replay and future prompts. `session/list`
lists Psychevo sessions visible to the requested cwd. `session/close` closes
the ACP actor and aborts any active invocation for that actor.

Session history replay uses sanitized runtime messages and ACP session updates.
Replay is presentation, not new evidence.

## Prompting And Observation

ACP prompt content is converted into runtime prompt text plus supported image
inputs. Text content is preserved in order. Image content maps to runtime image
inputs when the source is usable. Embedded resources degrade to explicit text
context when runtime cannot preserve their original resource type.

Runtime observation maps to ACP session updates:

- assistant text progress becomes agent message chunks
- reasoning progress becomes agent thought chunks
- tool lifecycle events become tool call and tool call update records
- final outcomes become ACP stop reasons
- cancellation maps to runtime abort

ACP observation must not rewrite durable runtime transcript content.

## Commands

ACP slash command projection uses the runtime-owned shared command catalog
defined by [026 Commands](../026-commands/spec.md). TUI and ACP draw from the
same command metadata, parser, capability filtering, and UI-independent
execution effects so discovery, aliases, argument shape, active-turn
availability, and bounded unsupported behavior do not drift.

ACP sends available command updates only after the session exists from the
client's point of view. For `session/new`, the agent responds with the new
session id before sending `available_commands_update`. For `session/load`,
history replay may happen before the response as required by ACP, but command
availability must still be sent once the client can apply it to the session.

When an ACP prompt starts with `/`, the ACP layer resolves it through the shared
command parser before starting a model-backed runtime invocation. Known
commands return shared execution effects: local text updates, prompt
submission, steer, queue, pending cancel, session switch, state patch, artifact
result, unsupported guidance, or command-level approval. Unknown slash-looking
input is sent to the model as ordinary prompt text instead of being rejected by
ACP.

ACP exposes only capability-filtered commands through
`available_commands_update`. The core supported command set is advertised
first. Dynamic skill and bundle commands may be appended after core commands up
to the surface cap; omitted dynamic commands remain invokable when typed if they
resolve at execution time. Help output reports hidden dynamic counts when a cap
omits entries.

ACP does not advertise commands whose useful behavior depends on a TUI-only
panel, local clipboard, process exit, renderer toggle, side conversation, or
client-native image-attachment composer state. If those known commands are
typed, ACP returns bounded guidance such as using the ACP client's native image
attachment flow or using the TUI/CLI for local clipboard/display commands.

While an ACP session has an active runtime turn, ACP applies the shared
active-turn availability gate. Live-safe commands such as help, status,
context, usage, tools, agents, steer, queue, and pending may run. Disruptive
state changes such as new session, resume, mode/model changes, compaction, and
dynamic prompt commands are rejected with guidance to wait, cancel, or queue
ordinary prompt text.

ACP `/steer <text>` uses messaging-friendly semantics: if an agent turn is
running, the text is injected through the runtime control handle; if runtime is
still in pending setup, it is queued; if idle, the text is submitted as a
normal prompt. ACP `/queue <text>` appends to a session-local FIFO and does not
start a turn by itself when idle. Queued prompts drain after the current or
next normal prompt. `/pending cancel` cancels unsent steers and clears queued
prompts. ACP queue state is session-scoped and not durable.

ACP `/sessions` lists numbered sessions with title, id, and updated time.
`/resume` and `/continue` switch the current ACP actor to an existing runtime
session by `latest`, numbered row, full id, id prefix, or exact title. Ambiguous
title matches return candidate rows and do not switch.

ACP `/model` does not fetch provider catalogs. Without arguments it reports the
current session model plus locally configured candidates. With
`/model <id> [variant]`, it updates the ACP session's future runtime options.

Local artifact, config, permission-policy, and skill-state writes must not
bypass runtime permission or approval boundaries. If the write is not already
covered by runtime tool approval, ACP asks for command-level approval before
executing the shared command effect.

## Authentication

ACP `initialize` advertises provider authentication methods derived from
Psychevo's configured provider catalog. Environment-variable methods verify
that the required variable is present. Agent methods may use existing Psychevo
auth storage APIs to persist provider API keys.

Authentication is provider configuration. It must not bypass runtime model
selection, permission policy, or capability selection.

Logout may remove Psychevo-managed stored credentials. It must not mutate host
environment variables.

## Model, Mode, And Config Projection

ACP may expose model selection, mode selection, and session config options when
runtime can honor those inputs for future prompts in the same ACP session.

Mode and config updates change ACP session state first. The next runtime
invocation receives the resolved state through normal runtime inputs. Unsupported
model, mode, or config values must return bounded protocol errors instead of
falling back silently.

## MCP

ACP-provided MCP servers are session-scoped capability sources following
[056 MCP](../056-mcp/spec.md). ACP accepts supported MCP declarations from the
client and passes them to runtime. Runtime owns conversion into tool candidates,
availability, conflict handling, selection, permission wrapping, and evidence.

ACP tool-call presentation may use a shorter title such as
`Tool: repo_tools/read_file`, but the executable model-visible name follows the
runtime MCP naming contract.

MCP startup and tool execution remain local runtime actions. They do not imply
ACP client filesystem or terminal delegation.

## Permissions

Runtime permission policy remains authoritative. ACP can provide the user
interaction channel for permission asks through `session/request_permission`,
but registry, tool visibility, resource decisions, and final execution policy
stay in runtime.

MCP startup and MCP tool calls are permission-relevant actions. First-slice
permission projection should support allow once, allow for session, allow
always when runtime can persist a safe rule, and deny.

## Related Topics

- [001 Architecture](../001-architecture/spec.md) defines crate boundaries and
  dependency direction.
- [004 Runtime Contract](../004-runtime-contract/spec.md) defines runtime
  assembly and control wiring.
- [020 Interfaces](../020-interfaces/spec.md) defines caller-facing interface
  semantics.
- [026 Commands](../026-commands/spec.md) defines shared command metadata.
- [035 Permissions](../035-permissions/spec.md) defines runtime permission
  policy.
- [050 Capability Extensions](../050-capability-extensions/spec.md) defines
  capability contribution boundaries.
- [056 MCP](../056-mcp/spec.md) defines MCP source, naming, dispatch,
  permission, and evidence boundaries.
- [230 pevo-acp](../230-pevo-acp/spec.md) defines the concrete ACP server
  packaging for the `pevo` product.
