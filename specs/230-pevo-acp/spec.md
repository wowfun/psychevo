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
- construction of runtime calls from ACP inputs

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

## Process Setup

The ACP server uses the same product path conventions as the `pevo` CLI:

- `PSYCHEVO_HOME` defaults to `~/.psychevo`
- `PSYCHEVO_DB` defaults to `$PSYCHEVO_HOME/state.db`
- `PSYCHEVO_CONFIG` may point at one TOML config file
- inherited environment variables are available to runtime provider and auth
  resolution

Relative paths resolve from the server process cwd. The server may create the
home directory before accepting ACP requests.

## Runtime Wiring

`psychevo-acp` depends on `psychevo-runtime` and constructs runtime calls
directly from ACP session state and prompt inputs. It passes cwd, session id,
mode, model, image inputs, inherited environment, config path, database path,
approval handler, and ACP-provided MCP servers through normal runtime inputs.

Runtime remains the owner of session coordination, model resolution, tool
surface assembly, capability source normalization, permission policy, command
metadata, persistence, and evidence.

`psychevo-acp` may keep transport-local state for active ACP sessions and
in-flight cancellation handles. That state is not durable session evidence.

`psychevo-acp` sends ACP command availability after the client receives or can
apply the ACP session id. It also handles supported slash-command prompts
locally before invoking the model-backed runtime path.

Local observational commands such as `/diff` are resolved entirely inside the
ACP transport. `/diff` uses the shared runtime workspace diff collector and
emits a synthetic ACP tool-call update containing structured
`ToolCallContent::Diff` entries. It must not append assistant text chunks, and
it must not mutate runtime model-context messages, export content, statistics,
or durable session evidence.

## Attachments

- [Testing](testing.md) defines acceptance scenarios and validation expectations.

## Related Topics

- [001 Architecture](../001-architecture/spec.md) defines crate boundaries and
  dependency direction.
- [027 ACP](../027-acp/spec.md) defines ACP protocol mapping and runtime
  boundaries.
- [200 pevo CLI](../200-pevo-cli/spec.md) defines the concrete `pevo` command
  surface.
