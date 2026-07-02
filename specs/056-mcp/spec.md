---
name: 056. MCP
psychevo_self_edit: deny
---

Define Psychevo's Model Context Protocol integration boundary.

MCP is a capability-extension source for tools and adjacent protocol objects. Psychevo
normalizes MCP servers into runtime-owned declarations before any
MCP tool becomes model-visible or executable. ACP may supply MCP servers for a
session, but MCP semantics are not owned by ACP.

## Scope

- MCP server source identity and session-scoped availability
- supported and degraded MCP transports at the runtime boundary
- MCP tool contribution, naming, conflict, and dispatch semantics
- permission and evidence requirements for MCP startup and tool execution
- interface projection requirements for ACP or future interfaces

Out of scope:

- implementing an MCP server inside Psychevo
- MCP resources, prompts, sampling, elicitation, or roots as first-class runtime
  surfaces
- OAuth, registry discovery, marketplace install, or managed MCP server config
- treating MCP as a trusted local-script extension mode
- delegating filesystem or terminal authority to an interface client

## Source Boundary

An MCP server is a capability-extension source. The source may be built-in, configured,
or provided by an interface for one session. Source presence does not imply
trust, activation, selection, permission approval, or persistence.

Runtime owns normalization from MCP server source to accepted declarations.
Interfaces may provide MCP server declarations, but they must not bypass
runtime tool assembly, permission wrapping, conflict checks, or evidence
capture.

ACP-provided MCP servers are session-scoped sources. Future configured MCP
servers may have broader scope, but they must still enter the same runtime
normalization path.

Plugin-provided MCP servers are package-scoped sources. Enabling a plugin only
makes its MCP server declarations candidates for acceptance. The MCP module
still owns normalization, startup approval, tool listing, model-visible naming,
tool execution, and diagnostics.

## Transports

The first supported client transports are:

- stdio child process
- streamable HTTP

Unsupported transports degrade the source. Degraded or unavailable servers must
be observable to the caller and must not produce model-visible tools.

Stdio MCP startup is a local runtime action. Streamable HTTP MCP calls are
runtime network actions. Neither transport delegates filesystem or terminal
authority to an ACP client or other interface.

Plugin MCP declarations may use a literal stdio command name or a package
relative command path beginning with `./`. Package-relative command paths and
`cwd` values must stay inside the plugin root. Relative `cwd` values are
resolved beneath the plugin root. Unsupported or malformed transport fields
must omit only the affected server when sibling server declarations are valid.

## Identity And Naming

MCP server identity uses a stable raw source name plus a normalized runtime name.
Whitespace in server names is normalized to underscores for runtime identity.
Other unsafe model-visible identifier characters are normalized before tool
exposure.

MCP tools exposed to the model must use conflict-safe names. The model-visible
shape is:

```text
mcp__<server>__<tool>
```

For example, server `repo tools` tool `read_file` becomes
`mcp__repo_tools__read_file`.

Interface presentation may use a shorter display title such as
`Tool: repo_tools/read_file`, but presentation names are not executable
identifiers. Conflicting model-visible names must not silently override existing
capabilities.

## Tool Dispatch

MCP tool declarations enter the agent-invocation tool surface as runtime tool
bindings. Runtime preserves the server/tool source identity for dispatch and
evidence.

MCP tool execution dispatches to the selected server and raw MCP tool name. Tool
arguments must be JSON objects. Non-text MCP content may be preserved in
structured tool output when the current AI protocol cannot model it natively.

MCP tools should be treated as sequential unless a later capability contract
defines safe parallel dispatch for a specific server/tool source.

## Permissions

Runtime permission policy remains authoritative for MCP. MCP tool calls use an
MCP permission action identified by `server/tool`.

MCP server startup is a distinct permission action because stdio startup can
launch a local process and streamable HTTP startup can establish a network
connection. Permission rules may address startup with:

```text
McpStartup(<server>)
```

Permission rules may address MCP actions with:

```text
Mcp(<server>/<tool>)
```

Default behavior may ask before starting MCP servers or executing MCP tools. An
interface may carry the approval prompt, but the final execution decision
belongs to runtime policy.

## Evidence

MCP evidence should be compact. Runtime should make these facts observable when
they affect an agent invocation:

- selected MCP server/tool contribution
- omitted unavailable or unsupported MCP source
- degraded source or tool
- model-visible name conflict
- source identity and dispatch trace summary

Runtime does not need to persist every discovered MCP candidate by default.

## Related Topics

- [004 Runtime Contract](../004-runtime-contract/spec.md) defines runtime
  assembly and control wiring.
- [007 Tool Surface](../007-tool-surface/spec.md) defines agent-invocation
  scoped tool surface semantics.
- [020 Interfaces](../020-interfaces/spec.md) defines caller-facing interface
  semantics.
- [041 Permissions](../041-permissions/spec.md) defines permission policy and
  approval semantics.
- [050 Capability Extensions](../050-capability-extensions/spec.md) defines
  capability-extension source, declaration, and registry boundaries.
- [027 ACP](../027-acp/spec.md) defines ACP protocol projection and
  ACP-provided MCP source input.
