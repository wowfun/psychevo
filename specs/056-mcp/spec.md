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
- MCP resources, prompts, elicitation, sampling, and roots as runtime-owned
  client surfaces
- minimal MCP server export for driving Psychevo from MCP clients
- permission and evidence requirements for MCP startup and MCP actions
- interface projection requirements for ACP or future interfaces

Out of scope:

- OAuth, registry discovery, marketplace install, or managed MCP server install
  lifecycle
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

MCP server declarations enter a source-aware catalog before runtime connects to
servers or lists capabilities. Catalog precedence is:

1. explicit session, ACP, or run-option input
2. profile configuration
3. selected capability roots
4. installed plugin packages

Higher-precedence sources replace lower-precedence sources that normalize to
the same runtime server name. Conflicts and replacements must remain observable
as compact source-qualified facts.

ACP-provided MCP servers are session-scoped sources. Profile-configured MCP
servers are profile-scoped sources. Selected capability root MCP servers are
selection-scoped sources. Plugin-provided MCP servers are package-scoped
sources. All sources enter the same runtime normalization path.

Plugin-provided MCP servers are package-scoped sources. Enabling a plugin only
makes its MCP server declarations candidates for acceptance. The MCP module
still owns normalization, startup approval, capability listing, model-visible
naming, action dispatch, and diagnostics.

An MCP server declaration may carry runtime policy fields for whether the
server is enabled, whether startup failure is required-fatal for the
invocation, which raw tool names are included or excluded, whether parallel
tool calls are allowed for that source, and bounded startup or tool-call
timeouts. These fields constrain acceptance and dispatch. They do not grant
permission to start a process, make a network call, or expose a model-visible
declaration.

## Runtime Snapshot

Runtime creates an MCP runtime snapshot at a generation-safe boundary. The
snapshot contains the resolved catalog, connected servers, discovered
capabilities, normalized tool declarations, utility-surface availability,
available environment identity, and compact hashes used for prompt-prefix
reconstruction.

Runtime resolves declarations through an MCP catalog and manages accepted
server connections through a connection-manager boundary. The catalog owns
source precedence, replacement, disabled sources, and compact diagnostics. The
connection manager owns startup, capability listing, call dispatch, connection
reuse, per-server policy, and dirty-state tracking.

Runtime may reuse server connections when the resolved catalog and available
environment identity are unchanged. `tools/list_changed` or equivalent runtime
signals may mark one or more servers dirty, but runtime must not silently
mutate model-visible MCP declarations in the middle of a generation request. A
dirty server may refresh only at a generation-safe boundary, such as before the
next generation request or after an explicit reload command. If a later request
reconstructs a prompt prefix with a different MCP snapshot hash or tool
declaration hash, the reconstruction must be labeled approximate.

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

MCP tools have separate raw, canonical, and provider-visible identities. Raw
identity is the server name and MCP object name used for protocol dispatch.
Canonical identity is runtime-owned and should use an MCP namespace derived from
the normalized server name. Provider-visible identity is the current request's
adapter-specific fallback name.

Chat-compatible providers that cannot encode namespaces use the fallback shape:

```text
mcp__<server>__<tool>
```

For example, server `repo tools` tool `read_file` becomes
`mcp__repo_tools__read_file`.

Interface presentation may use a shorter display title such as
`Tool: repo_tools/read_file`, but presentation names are not executable
identifiers. Conflicting provider-visible names must not silently override
existing capabilities; normalization must keep a raw dispatch identity for the
accepted binding.

## Tool Dispatch

MCP tool declarations enter the agent-invocation tool surface as runtime tool
bindings. Runtime preserves the server/tool source identity for dispatch and
evidence.

MCP tool execution dispatches to the selected server and raw MCP tool name. Tool
arguments must be JSON objects. Non-text MCP content may be preserved in
structured tool output when the current AI protocol cannot model it natively.

MCP tools should be treated as sequential unless a later capability contract
defines safe parallel dispatch for a specific server/tool source. A per-server
parallel-call policy may opt a source into parallel dispatch only for tools
accepted from that source.

## Resources And Prompts

MCP resources and prompts are exposed through host-owned global utility tools,
not through one generated tool per MCP object. Runtime may expose these utility
tools when at least one accepted server advertises the corresponding
capability:

- `list_mcp_resources`
- `list_mcp_resource_templates`
- `read_mcp_resource`
- `list_mcp_prompts`
- `get_mcp_prompt`

These utilities preserve raw server, URI, template, prompt name, and argument
identity for dispatch. They obey the same catalog scope, permission policy, and
compact evidence requirements as MCP tool calls.

## Elicitation

MCP elicitation is a runtime event/response flow. A server may request user
input through a bounded elicitation request. Runtime must surface the request to
the configured approval or review channel and wait for an explicit response
unless policy permits an automatic empty confirmation.

Form and URL elicitation requests are valid runtime request shapes. Missing
review handling, timeout, cancellation, invalid input, or denied permission must
fail closed and return an error to the MCP server. Runtime must not invent user
answers or prompt the model to answer on the user's behalf.

## Sampling

MCP sampling is enabled by default only behind runtime-owned bounds:

- timeout
- max tokens
- max tool rounds
- rate limit
- optional model override
- optional allowed model list

Sampling requests use the configured AI provider path but do not mutate the
main agent turn state. Sampling must not bypass permission policy, approval
policy, or provider configuration. Tests must use fake or deterministic
providers unless a caller explicitly opts into real provider validation.

## Roots

MCP roots are a client capability derived from runtime-owned cwd, workspace, and
readable sandbox roots. Roots are advertised to MCP servers as protocol
capability data. They are not model-visible tools and do not delegate interface
filesystem authority to an MCP server.

## Permissions

Runtime permission policy remains authoritative for MCP. MCP actions use the
existing `mcp` permission gate in the first implementation slice while
preserving action labels internally for startup, tool calls, resource reads,
prompt gets, elicitation, and sampling.

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

Default behavior may ask before starting MCP servers or executing MCP actions.
An interface may carry the approval prompt, but the final execution decision
belongs to runtime policy.

## Evidence

MCP evidence should be compact. Runtime should make these facts observable when
they affect an agent invocation:

- selected MCP server, source, and capability contribution
- omitted unavailable or unsupported MCP source
- degraded source or tool
- model-visible name conflict
- normalized raw/canonical/provider-visible identity mapping
- source identity and dispatch trace summary
- resource, prompt, elicitation, sampling, and roots action summaries

Runtime does not need to persist every discovered MCP candidate by default.

## MCP Server Export

Psychevo may expose a minimal MCP server so MCP clients can drive Psychevo as
an agent endpoint. This server is an interface adapter, not a
capability-extension source. It must stay separate from inbound MCP server
declarations and must not feed its own exported tools back into the runtime
tool surface.

The minimal server surface contains two tools:

- `psychevo`, which starts or continues a Psychevo turn from client-supplied
  prompt text and optional session identity.
- `psychevo-reply`, which replies to an existing Psychevo session.

`psychevo-reply` requires prompt text plus one session identity alias. The
exported input schema and handler must accept `sessionId`, `session_id`,
`threadId`, or `thread_id`, with all aliases resolving to the same Psychevo
session identity. The schema and implementation must stay aligned so strict
MCP clients can validate inputs before calling the tool.

Tool results should include the Psychevo session identity and final assistant
content in structured content when the MCP client supports it, with concise
text fallback for older clients. The exported MCP server must use normal
runtime entrypoints, provider configuration, permission policy, and session
storage. It must not give the MCP client ambient filesystem or terminal
authority beyond what the selected Psychevo invocation permits.

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
