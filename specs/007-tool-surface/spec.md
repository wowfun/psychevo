---
name: 007. Tool Surface
psychevo_self_edit: deny
---

Define the agent-invocation scoped tool surface contract owned by `psychevo-runtime`.

## Scope

- semantic tool surface exposed for one agent invocation
- generation-request tool declaration snapshots
- tool declarations made visible to the model
- execution bindings supplied by runtime
- agent-invocation scoped tool surface selection
- toolset grouping and expansion semantics
- boundary between tool requests, tool execution, and tool-result artifacts
- relationship between tool visibility and permission-gated execution
- tool surface facts that may contribute to durable evidence

Out of scope:
- concrete tool names, concrete tool behavior, or built-in tool lists
- tool schemas, payload schemas, Rust APIs, traits, structs, or function signatures
- provider-specific tool-call fields, wire formats, or schema translation rules
- concrete toolset names, tool discovery rules, configuration precedence, or plugin and extension APIs
- resource permission schemas, approval behavior, sandbox rules, path rules, or enforcement mechanics
- tool result formats, terminal rendering, UI rendering, or CLI behavior
- storage formats, session formats, replay formats, traces, migrations, or indexes

## Tool Surface

A tool surface is the runtime-selected set of tool candidates, tool declarations, and execution bindings available to one accepted agent invocation.

A model-visible tool declaration snapshot is the subset and provider-neutral shape of tool declarations supplied to one generation request. This snapshot may be refreshed between generation requests inside the same agent invocation when runtime observes changed registry state, availability, or toolset expansion facts.

A tool declaration is a model-visible semantic capability promise. It describes a tool the model may request. This spec does not define declaration fields, input schemas, provider-specific encoding, or prompt text.

Every execution binding has a canonical tool identity. The canonical identity
contains an optional namespace and a local tool name. Built-in tools may use an
empty namespace; external families such as MCP should use source-derived
namespaces so raw source identity, provider-visible fallback names, and dispatch
identity do not collapse into one string. Provider-specific flattened names are
lookup aliases for one generation request, not the owning runtime identity.

An execution binding is the runtime-supplied executable handler for a tool request. It connects a requested tool to behavior supplied by a built-in or external source. This spec does not define handler signatures, process boundaries, or implementation APIs.

Tool display metadata is UI-only metadata associated with an execution binding
or a concrete tool event. It may describe a display category, title preview
fields, summary fields, and detail/body preferences for local renderers. Tool
display metadata is not a model-visible tool declaration, is not prompt text,
and must not be inserted into tool-result content sent back to the model.

Runtime assembles the initial tool surface for an accepted agent invocation before model generation can request tools.

Runtime must expose a tool declaration in a generation-request snapshot only when the same agent invocation has a matching execution binding for that request. Runtime may omit unavailable tools from a snapshot or from the agent-invocation scoped tool surface.

This binding invariant applies to function declarations. A selected hosted tool
is a provider-executed generation declaration, not a runtime tool-surface entry,
and has no execution binding. When local and hosted variants share a provider-
visible name, runtime selects exactly one variant for each generation.

Runtime selects the agent-invocation scoped tool surface from available inputs. Capability extensions may declare tool candidates or toolset candidates, but [050 Capability Extensions](../050-capability-extensions/spec.md) owns source, declaration, activation, availability, conflict, and registry boundaries. This spec does not define source discovery, selection precedence, or plugin mechanics.

When an agent definition is selected, runtime applies that definition's tool
policy as an additional invocation-scoped constraint. Agent policy may narrow
tools, but it must not expose a tool declaration without a matching execution
binding or exceed the current runtime-mode hard ceiling. [051 Agents](../051-agents/spec.md)
defines agent tool-policy normalization.

Tool implementations may come from built-in or external sources, but source mechanics do not define the tool surface contract.

The AI generation request may include a refreshable tool snapshot from the agent-invocation scoped tool surface. The model-visible snapshot contains only declaration material required by the provider-facing tool API, such as the canonical tool identity, provider fallback name, description, and input parameters. UI-only display metadata must remain outside this generation-request tool snapshot. Tool refresh does not define a dynamic tool API, hook API, plugin API, or model-visible toolset concept. [003 AI Protocol](../003-ai-protocol/spec.md) owns provider-neutral generation request semantics and provider-facing normalization.
Prompt-prefix metadata may retain the effective canonical tool identities,
provider fallback names, declaration hash, and MCP runtime snapshot hash used
for request reconstruction. The full declaration payload is reconstructable
from the current registry by default; if the reconstructed payload or runtime
snapshot does not match the recorded hashes, the request reconstruction must be
labeled approximate.

Tool surface facts may contribute to durable evidence for agent-invocation inspection. These facts may include selected toolsets, expanded tool names, declaration hashes, refresh facts, omitted unavailable tools, execution bindings, tool requests, execution outcomes, and tool-result relationships. [005 Durable Evidence](../005-durable-evidence/spec.md) defines which relationships must be representable. Ordinary durable evidence does not require persisting a full capability snapshot or full model-visible tool declaration payload.

The runtime tool surface is assembled through a source-aware tool registry. Each
tool entry records source identity, source family, tool name, execution binding,
exposure state, optional owning toolsets, and conflict or omission reason. The
registry must preserve existing effective tool order for unchanged inputs while
making source-qualified acceptance facts available internally.

Built-in tools, clarify tools, skill tools, MCP tools, plugin worker tools, and
agent tools enter the same registry interface. A contributed tool name becomes
model-visible only when its execution binding is registered for the accepted
invocation and the current mode permits it. Duplicate model-visible names are
conflicts; later duplicates are omitted with source-qualified facts rather than
silently replacing earlier bindings.

## Toolsets

A toolset is a named grouping of tools or other toolsets used during runtime selection.

Toolset names are selection and configuration concepts. A toolset is not itself model-visible and is not a tool the model may request.

Toolset expansion resolves selected toolsets into a finite set of tool declarations and execution bindings during accepted agent-invocation assembly. Runtime may recompute expansion before a later generation request when registry, availability, or runtime-managed selection facts change. Only expanded tool declarations may enter a model-visible tool declaration snapshot.

A toolset may include other toolsets. Expansion must detect unknown includes, unavailable includes, and cycles. Those conditions must become observable as unavailable, degraded, omitted, or rejected selection facts; they must not be silently ignored.

Runtime-owned toolset selection may use per-mode `enabled_toolsets` and
`disabled_toolsets`. Disabled toolsets are applied as a subtraction step after
enabled toolset expansion. A runtime mode may additionally impose a hard safety
ceiling after expansion; for example Plan mode filters mutating tools even when
configuration enables a toolset that contains them.

Built-in and user-defined toolsets share the same expansion semantics. A
user-defined toolset may include built-in or other user-defined toolsets, but it
must not create a model-visible tool declaration unless a registered execution
binding with that name exists.

When expansion produces a tool declaration, runtime must still verify that a matching execution binding exists for the same agent invocation and for any generation-request snapshot that exposes that declaration.

Accepted toolset facts are the toolset names whose expansion produced at least
one accepted tool binding after disabled-toolset subtraction, mode filtering,
include resolution, and conflict handling. Skill prompt-visibility rules that
refer to toolsets must use these accepted toolset facts, not raw configuration
names.

Capability sources may declare toolsets. Runtime may also derive toolset facts
from accepted runtime sources when the source owns a stable tool family, such
as an MCP server. A contributed or derived toolset may include built-in,
configured, or other contributed toolsets, but it must not expose a tool
without a registered execution binding. Plugin and MCP toolset membership is
accepted only after the owning tool binding exists. Contributed and derived
toolsets are selection metadata only; they are never executable handlers and
never become model-visible declarations.

Profile-scoped GUI management may create, remove, enable, and disable toolsets
through tool-surface management helpers, but those writes only change runtime
selection configuration. They do not bypass expansion, mode filtering, source
acceptance, permission policy, or execution-binding checks.

This spec owns expansion semantics. [050 Capability Extensions](../050-capability-extensions/spec.md) owns source identity, activation, availability, degraded state, and conflicts for declared toolsets and tools.

## Request and Execution Boundary

A tool request is an assistant-requested tool call normalized by [003 AI Protocol](../003-ai-protocol/spec.md). Tool requests are model output, not direct execution authority.

A tool declaration says what the model may request. It does not authorize execution.

Tool exposure is invocation-scoped:

- `direct` declarations are included in the next generation-request tool
  snapshot.
- `deferred` declarations have accepted execution bindings but are omitted from
  the snapshot until the tool router activates them.
- `hidden` declarations are never model-visible and may only be used by
  host-owned runtime paths that explicitly target them.

When the accepted invocation surface contains deferred declarations and
`tool_search` is enabled, runtime exposes a single direct `tool_search`
declaration. `tool_search` searches deferred canonical identities,
provider-visible fallback names, descriptions, schemas, and source-qualified
search metadata. A successful search returns loadable declaration
specifications and activates the matching deferred tools in the same agent
invocation, so later generation-request snapshots can include those concrete
tool declarations. The activation state belongs to the agent loop's mutable
tool router, not to plugin manifests, MCP servers, or persistent configuration.
`tool_search` is enabled by default. Explicit configuration may disable it for
an invocation or bound the default and maximum number of returned loadable
declarations.

Exposure policy is source-family aware. Direct MCP tools and plugin worker
tools enter the router as deferred bindings when `tool_search` is enabled.
Host-owned runtime tools remain direct unless their own execution binding or
another owning policy marks them deferred, hidden, or omitted. Existing
`deferred` and `hidden` binding exposure must be preserved. When `tool_search`
is disabled, accepted direct MCP and plugin worker bindings are ordinary direct
tools subject to the same mode, conflict, permission, and agent policy checks as
other tools.

Agent execution observes assistant-requested tool calls and invokes runtime-supplied execution bindings through its tool execution flow. [002 Agent Execution](../002-agent-execution/spec.md) owns tool execution lifecycle events, ordering, scheduling latitude, causal linkage, and outcome semantics.

Resource gates and permission policy may affect execution after a tool request
and before or during the execution binding.
[009 Resource Surface](../009-resource-surface/spec.md) defines allow, deny, and
defer resource decision semantics. [041 Permissions](../041-permissions/spec.md)
defines the concrete runtime permission policy for local tool execution. This
spec does not define permission semantics, approval UX, sandbox behavior,
resource policy rules, path rules, or failure policy.

A tool result artifact is loop-visible result material produced after a tool execution. Tool-result message semantics are defined by [002 Agent Execution](../002-agent-execution/spec.md). This spec does not define tool result payloads, rendering, schema validation, or durable record shape.

## Attachments

- [Declaration Quality](declaration-quality.md) defines first-slice
  expectations for model-visible tool declaration descriptions.

## Related Topics

- [000 Foundation](../000-foundation/spec.md) defines the upstream project foundation and implementation-neutral principles.
- [001 Architecture](../001-architecture/spec.md) defines crate boundaries and dependency direction.
- [002 Agent Execution](../002-agent-execution/spec.md) defines tool execution lifecycle, ordering, and tool-result message semantics.
- [003 AI Protocol](../003-ai-protocol/spec.md) defines provider-neutral tool-call and generation request semantics.
- [004 Runtime Contract](../004-runtime-contract/spec.md) defines agent-invocation assembly and wiring responsibilities.
- [041 Permissions](../041-permissions/spec.md) defines permission policy that
  may gate execution after a model-visible tool request.
- [005 Durable Evidence](../005-durable-evidence/spec.md) defines durable linkage for tool requests, execution outcomes, and result artifacts.
- [006 Context Assembly](../006-context-assembly/spec.md) defines model context assembly, which stays separate from tool declarations and generation controls.
- [009 Resource Surface](../009-resource-surface/spec.md) defines resource gates that may affect tool execution.
- [030 State and Data Model](../030-state-and-data-model/spec.md) defines how tool facts relate to other state families.
- [050 Capability Extensions](../050-capability-extensions/spec.md) defines
  how extension declarations may provide tool candidates before invocation
  tool-surface selection.
- [051 Agents](../051-agents/spec.md) defines selected-agent tool policy.
- [051 Subagents](../051-agents/subagents.md) defines subagent control tools.
