---
name: 027. ACP
psychevo_self_edit: deny
---

# 027. ACP

Define Psychevo's Agent Client Protocol boundary.

ACP is a protocol interface over `psychevo-gateway` and `psychevo-runtime`. It maps protocol requests
and notifications to gateway threads, runtime sessions, invocations, observations, permissions,
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

An ACP implementation accepts protocol requests, translates them into gateway
inputs, projects gateway/runtime observations back to ACP updates, and keeps transport
state for active ACP sessions.

ACP implementations must call `psychevo-gateway` for normal prompting,
cancellation, permission, clarify, queue, steer, reset, and source-to-thread
behavior. They may call `psychevo-runtime` directly for non-interactive
administrative projections that are not gateway semantics. ACP must not shell
out through a CLI command for normal prompting, cancellation, permission, MCP,
command, model, config, or session behavior.

Concrete product specs choose how Psychevo exposes an ACP server. This topic
owns the protocol mapping, not the product process that hosts it.

## Sessions

ACP session ids identify active ACP session actors. Each actor maps to a
Psychevo runtime session id once runtime creates or loads the backing session.
New ACP sessions use gateway source kind `acp` and runtime source `acp` for persistence. A newly created ACP
session may remain transport-local until the first model-backed prompt creates
the durable runtime session; the ACP id must remain stable for the client and
must be linked to the runtime id once that id exists.

ACP uses a `Persistent` Gateway source lifetime. Source-to-thread binding is
therefore durable across reconnects, while active turns and queued turns remain
process-local to the Gateway instance that owns the running ACP agent.

`session/new` creates a runtime session boundary for the selected cwd,
provider, model, mode, permissions, and ACP-supplied MCP sources. `session/load`
opens an existing Psychevo session for replay and future prompts. `session/list`
lists Psychevo sessions visible to the requested cwd. `session/close` closes
the ACP actor and aborts any active invocation for that actor.

Session history replay uses sanitized runtime messages and ACP session updates.
Replay is presentation, not new evidence. `session/load` must replay history
before returning the load response. Replay is best effort: corrupt, older, or
unsupported message shapes produce visible placeholder updates and a structured
`_meta.psychevo.replay_warnings` summary instead of failing the load when the
runtime session itself exists.

## Prompting And Observation

ACP prompt content is converted into ordered runtime user content blocks. Text
content is preserved in order. A single text block that starts with `/` may be
handled as an ACP slash command; prompts with multiple blocks, images, or
resources are model prompts even when a text block starts with `/`.

Image content maps to runtime image blocks when the source is usable. Image
data becomes a data URL; non-empty image URIs pass through as image URLs.
Image file resources use the runtime image pipeline. Audio and unsupported
resources degrade to explicit visible text.

Text resource links use the ACP client filesystem read capability first when
the client advertises it, then local file reading as a fallback. Client reads
use a fixed line limit, and all text resources are capped at 512 KiB after
decoding. Remote HTTP(S) resource links are not fetched proactively and degrade
to a visible resource-link note. Resource handling records prompt-scoped
summaries in runtime context evidence; text that is actually inlined for the
model is persisted in the user message for new runs.

Gateway transcript observation maps to ACP session updates:

- assistant text progress becomes agent message chunks
- reasoning progress becomes agent thought chunks; ACP only exposes reasoning
  already projected by runtime or Gateway and must not mine provider-private
  raw reasoning fields
- pending tool-call argument progress becomes pending tool call update records
  when the runtime exposes it, so clients can distinguish model generation of a
  tool request from local tool execution
- typed tool transcript lifecycle events become tool call and tool call
  update records
- final outcomes become ACP stop reasons
- cancellation maps to runtime abort

ACP observation must not rewrite durable runtime transcript content. ACP must
not consume or expose raw runtime fallback events as ordinary client updates;
bounded debug records are diagnostics only.

Tool call projection must preserve structured `rawInput` and `rawOutput` while
also sending a human-readable title and display content when the runtime
transcript has them. Command tools such as `exec_command` use the visible title
for a short command summary and content for the full command/output text, so ACP
clients are not required to inspect raw JSON to show useful progress.

ACP may provide terminal-style output presentation for command tools only as a
display enhancement. Runtime remains the executor and permission authority. The
ACP layer must not delegate command execution to client `terminal/create`, and
must not use client terminal presentation to bypass runtime `exec_command`,
`write_stdin`, yield-session, persistence, permission, or accounting semantics.

Runtime usage and accounting are projected at the ACP prompt boundary. When the
ACP SDK exposes unstable usage fields, Psychevo sends `PromptResponse.usage`
for provider token usage and puts Psychevo-specific accounting, turns, and
warnings under `_meta.psychevo`. ACP `UsageUpdate` is reserved for context
window snapshots and cumulative session cost; its tokens are not added to
provider usage. Provider-reported total tokens are authoritative when present;
otherwise totals are `input + output` and do not double-count reasoning or
cache subcategories. If multiple runtime model turns drain under one ACP
prompt response, numeric accounting fields are summed and inconsistent pricing
source or tier strings become `mixed`.

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
context, usage, tools, agents, diff, steer, queue, and pending may run.
Disruptive state changes such as new session, resume, mode/model changes,
compaction, and dynamic prompt commands are rejected with guidance to wait,
cancel, or queue ordinary prompt text.

ACP `/diff` projects an observational structured diff update. It sends a
synthetic tool-call update whose content uses ACP `ToolCallContent::Diff` and
must not send a plain assistant text fallback. The update is display-only and
must not append runtime messages, affect model context, session export content,
or usage/accounting statistics.

ACP `/steer <text>` uses Gateway active-turn semantics: if an agent turn is
running, the text is injected through Gateway into the active runtime control
handle; if runtime is still in pending setup, it is queued; if idle, the text
is submitted as a normal prompt. ACP `/queue <text>` appends to a session-local
FIFO and does not start a turn by itself when idle. Queued prompts drain after
the current or next normal prompt. `/pending cancel` cancels unsent steers and
clears queued prompts. ACP queue state follows Gateway first-slice semantics
and is session-scoped but not durable.

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

ACP `initialize` advertises provider authentication methods derived from the
current effective model provider. A ready provider is advertised as an agent
auth method. Terminal setup auth is advertised only when the client declares
terminal auth support. ACP does not advertise environment-variable auth
methods. If no provider is ready and no supported terminal setup path exists,
`session/new` fails with `auth_required`.

Authentication is provider configuration. It must not bypass runtime model
selection, permission policy, or capability selection.

Logout is advertised only when the ACP server implements the logout request. It
must not be advertised as a placeholder.

## Model, Mode, And Config Projection

ACP may expose model selection, mode selection, and session config options when
runtime can honor those inputs for future prompts in the same ACP session.

ACP initializes and loads sessions with a model state derived from local config
and cache-first model metadata. It must not fetch provider catalogs during ACP
initialize, new-session, or load-session. The current selected model is always
included in the available model list, synthesized when it is absent from local
catalogs. ACP model ids use `provider/model`; bare model ids are accepted for
model switching only when they unambiguously resolve to one configured
provider.

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

ACP advertises only MCP transports that are supported by the runtime bridge.
The first slice advertises HTTP MCP support and accepts stdio declarations
without advertising SSE or ACP-over-ACP. Unsupported MCP transports or startup
failures produce structured `_meta.psychevo.mcp_warnings` and visible guidance,
but they do not fail session creation or prompting by themselves.

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
- [021 Gateway](../021-gateway/spec.md) defines source mapping and thread/turn orchestration.
- [035 Permissions](../035-permissions/spec.md) defines runtime permission
  policy.
- [050 Capability Extensions](../050-capability-extensions/spec.md) defines
  capability contribution boundaries.
- [056 MCP](../056-mcp/spec.md) defines MCP source, naming, dispatch,
  permission, and evidence boundaries.
- [230 pevo-acp](../230-pevo-acp/spec.md) defines the concrete ACP server
  packaging for the `pevo` product.
