---
name: 230. pevo-acp
psychevo_self_edit: deny
---

# 230. pevo-acp

Define the concrete ACP server packaging for the `pevo` product.

`psychevo-acp` hosts the ACP protocol mapping defined by
[027 ACP](../027-acp/spec.md). This topic owns the crate, binary, `pevo acp`
command integration, process setup, stdio server packaging, and runtime call
construction for that product entrypoint.

## Scope

- `psychevo-acp` crate boundary and dependency direction
- `psychevo-acp` binary behavior
- `pevo acp` command behavior and process help positioning
- ACP JSON-RPC server over stdio for the first product slice
- product environment and path setup before runtime calls
- construction of Gateway calls from ACP inputs

Out of scope:

- ACP protocol semantics, request mapping, observation mapping, permission
  projection, command projection, auth projection, or MCP source semantics
- HTTP or WebSocket ACP transports
- editor-specific install instructions or client registry publishing
- agent execution, provider behavior, tool behavior, permission policy, or
  durable storage semantics

## Entry Points

`psychevo-acp` provides a library function that runs the ACP server over stdio.
The `psychevo-acp` binary and `pevo acp` command call that same function.

`pevo acp` is a product wrapper. It must not implement protocol behavior in
`psychevo-cli`, and it must not shell out to `pevo run` for prompting,
cancellation, permissions, MCP, command handling, model selection, config
updates, or session behavior.

Process help should describe `pevo acp` as the Agent Client Protocol stdio
server for ACP-speaking editors and clients.

`psychevo-acp --setup` and `pevo acp --setup` run the shared Psychevo provider
setup flow and exit instead of starting the stdio server. ACP terminal auth
advertises the same setup path with `args = ["--setup"]`, so clients can launch
the agent binary for out-of-band setup and then retry initialize/authenticate.

## Process Setup

The ACP server uses the same product path conventions as the `pevo` CLI:

- `PSYCHEVO_HOME` defaults to `~/.psychevo`
- `PSYCHEVO_DB` defaults to `$PSYCHEVO_HOME/state.db`
- `PSYCHEVO_CONFIG` may point at one TOML config file
- `PSYCHEVO_ACP_TERMINAL_OUTPUT=1` opts into ACP terminal-output display
  metadata when the client advertises support through `_meta.terminal_output`
- inherited environment variables are available to runtime provider and auth
  resolution

Relative paths resolve from the server process cwd. The server may create the
home directory before accepting ACP requests.

## Gateway Wiring

`psychevo-acp` depends on `psychevo-gateway` and constructs Gateway calls from
ACP session state and prompt inputs. It passes cwd, session id, mode, model,
image inputs, inherited environment, config path, database path, approval
handler, and ACP-provided MCP servers through normal Gateway/runtime inputs.

Runtime remains the owner of session coordination, model resolution, tool
surface assembly, capability source normalization, permission policy, command
metadata, persistence, and evidence.

The server exposes runtime model controls through standard ACP session config
options. On `session/new` and `session/load`, clients receive `mode`, `model`,
and `effort` options when values are available from local configuration.
`session/set_config_option` updates the in-memory ACP session and returns the
refreshed option set; the next prompt passes those values to Gateway/runtime.
`pevo acp` also continues to send ACP `usage_update` notifications from runtime
context snapshots so connected clients can show context-window usage. When a
turn has provider/runtime token accounting but no context snapshot, `pevo acp`
uses the resolved model context limit plus the best available total-token
accounting as a fallback usage update.

ACP sessions use the same runtime project context configuration as `pevo run`.
The server does not add ACP-specific project-context protocol fields. Workspace
`.psychevo/config.toml` may set `[project_context].instructions` to `git-root`,
`cwd`, or `off`; runtime applies that setting when assembling prompt prefixes
for ACP prompts. Runtime environment context still exposes the ACP session cwd
to the model.

`psychevo-acp` may keep transport-local state for active ACP actors, but active
turn queueing, steering, interrupt, permission, clarify, and source-to-thread
binding use Gateway semantics. Transport-local state is not durable session
evidence.

ACP request handling uses the SDK ACP v2 agent builder and v2 typed handlers.
Initialize responses return protocol version `V2` to v2 clients, while v1
clients are handled through the SDK compatibility layer when a v2 operation has
a lossless or documented compatibility projection. The server advertises only
implemented optional capabilities and avoids placeholder logout support. Runtime
usage, accounting, turns, warnings, and context-window updates are projected
according to [027 ACP](../027-acp/spec.md) without mutating the durable
transcript.

Mode is exposed through the ACP session config option surface. `pevo acp` does
not register the v1-only `session/set_mode` handler; clients set mode with
`session/set_config_option` using the `mode` config id. New and loaded sessions
return the current config options in their response instead of relying on the
deprecated v1 `modes` field.

`PSYCHEVO_ACP_TERMINAL_OUTPUT` affects only ACP presentation. It does not make
the editor execute Psychevo commands, and `pevo acp` must continue to route
`exec_command`, yielded command sessions, and `write_stdin` through runtime and
Gateway semantics. Under ACP v2 schema 0.13.6, terminal-output presentation is
encoded as text content plus `_meta` terminal output fields because the v2 tool
content model no longer has the v1 terminal content variant.

`psychevo-acp` sends ACP command availability after the client receives or can
apply the ACP session id. It also handles supported slash-command prompts
locally before invoking the model-backed runtime path.

ACP does not advertise the shared `Side chat` command capability.
`/btw` remains a TUI/Workbench `Side chat` affordance defined by
[213 Thread Navigation](../213-pevo-display-model/thread-navigation.md);
when an ACP prompt explicitly uses the known command, ACP returns bounded
unsupported-command guidance and must not create a child thread or pass the
command through to the model.

Only a prompt consisting of a single text block is eligible for ACP slash
command handling. Prompts with attachments or multiple blocks are passed through
to runtime as ordered user content.

Local observational commands such as `/diff` are resolved entirely inside the
ACP transport. `/diff` uses the shared runtime workspace diff collector and
emits a synthetic ACP tool-call update containing structured
`ToolCallContent::Diff` entries. It must not append assistant text chunks, and
it must not mutate runtime model-context messages, export content, statistics,
or durable session evidence.

ACP handles `/undo` and `/redo` locally through the shared runtime undo/redo
helpers when the ACP session is bound to a runtime session. They restore local
session/file snapshot state and send bounded text updates to the client. They
must not invoke providers, create durable command transcript messages, or pass
through to the model when no undo/redo target is available.

## Attachments

- [Testing](testing.md) defines acceptance scenarios and validation expectations.

## Setup

ACP setup is implemented by shared runtime setup services rather than by ACP
protocol code. The setup command supports explicit provider/model, custom
OpenAI-compatible base URLs, API key stdin, API key env references, explicit
no-auth providers, global/local scopes, optional catalog fetch, labels, and
JSON summary output. `--api-key-stdin` must reject TTY stdin. JSON summaries
must not contain secrets and may include a `warnings` array for non-fatal
conditions such as unconfirmed models or non-loopback no-auth URLs.

## Related Topics

- [001 Architecture](../001-architecture/spec.md) defines crate boundaries and
  dependency direction.
- [027 ACP](../027-acp/spec.md) defines ACP protocol mapping and runtime
  boundaries.
- [200 pevo CLI](../200-pevo-cli/spec.md) defines the concrete `pevo` command
  surface.
