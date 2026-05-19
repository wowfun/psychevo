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

An execution binding is the runtime-supplied executable handler for a tool request. It connects a requested tool to behavior supplied by a built-in or external source. This spec does not define handler signatures, process boundaries, or implementation APIs.

Runtime assembles the initial tool surface for an accepted agent invocation before model generation can request tools.

Runtime must expose a tool declaration in a generation-request snapshot only when the same agent invocation has a matching execution binding for that request. Runtime may omit unavailable tools from a snapshot or from the agent-invocation scoped tool surface.

Runtime selects the agent-invocation scoped tool surface from available inputs. Capability extensions may contribute tool candidates or toolset candidates, but [050 Capability Extensions](../050-capability-extensions/spec.md) owns source, contribution, activation, availability, and conflict boundaries. This spec does not define source discovery, selection precedence, or plugin mechanics.

When an agent definition is selected, runtime applies that definition's tool
policy as an additional invocation-scoped constraint. Agent policy may narrow
tools, but it must not expose a tool declaration without a matching execution
binding or exceed the current runtime-mode hard ceiling. [051 Agents](../051-agents/spec.md)
defines agent tool-policy normalization.

Tool implementations may come from built-in or external sources, but source mechanics do not define the tool surface contract.

The AI generation request may include a refreshable tool snapshot from the agent-invocation scoped tool surface. Tool refresh does not define a dynamic tool API, hook API, plugin API, or model-visible toolset concept. [003 AI Protocol](../003-ai-protocol/spec.md) owns provider-neutral generation request semantics and provider-facing normalization.

Tool surface facts may contribute to durable evidence for agent-invocation inspection. These facts may include selected toolsets, expanded tools, declaration snapshots, refresh facts, omitted unavailable tools, execution bindings, tool requests, execution outcomes, and tool-result relationships. [005 Durable Evidence](../005-durable-evidence/spec.md) defines which relationships must be representable.

## Toolsets

A toolset is a named grouping of tools or other toolsets used during runtime selection.

Toolset names are selection and configuration concepts. A toolset is not itself model-visible and is not a tool the model may request.

Toolset expansion resolves selected toolsets into a finite set of tool declarations and execution bindings during accepted agent-invocation assembly. Runtime may recompute expansion before a later generation request when registry, availability, or runtime-managed selection facts change. Only expanded tool declarations may enter a model-visible tool declaration snapshot.

A toolset may include other toolsets. Expansion must detect unknown includes, unavailable includes, and cycles. Those conditions must become observable as unavailable, degraded, omitted, or rejected selection facts; they must not be silently ignored.

When expansion produces a tool declaration, runtime must still verify that a matching execution binding exists for the same agent invocation and for any generation-request snapshot that exposes that declaration.

This spec owns expansion semantics. [050 Capability Extensions](../050-capability-extensions/spec.md) owns source identity, activation, availability, degraded state, and conflicts for contributed toolsets and tools.

## Request and Execution Boundary

A tool request is an assistant-requested tool call normalized by [003 AI Protocol](../003-ai-protocol/spec.md). Tool requests are model output, not direct execution authority.

A tool declaration says what the model may request. It does not authorize execution.

Agent execution observes assistant-requested tool calls and invokes runtime-supplied execution bindings through its tool execution flow. [002 Agent Execution](../002-agent-execution/spec.md) owns tool execution lifecycle events, ordering, scheduling latitude, causal linkage, and outcome semantics.

Resource gates and permission policy may affect execution after a tool request
and before or during the execution binding.
[009 Resource Surface](../009-resource-surface/spec.md) defines allow, deny, and
defer resource decision semantics. [035 Permissions](../035-permissions/spec.md)
defines the concrete runtime permission policy for local tool execution. This
spec does not define permission semantics, approval UX, sandbox behavior,
resource policy rules, path rules, or failure policy.

A tool result artifact is loop-visible result material produced after a tool execution. Tool-result message semantics are defined by [002 Agent Execution](../002-agent-execution/spec.md). This spec does not define tool result payloads, rendering, schema validation, or durable record shape.

## Related Topics

- [000 Foundation](../000-foundation/spec.md) defines the upstream project foundation and implementation-neutral principles.
- [001 Architecture](../001-architecture/spec.md) defines crate boundaries and dependency direction.
- [002 Agent Execution](../002-agent-execution/spec.md) defines tool execution lifecycle, ordering, and tool-result message semantics.
- [003 AI Protocol](../003-ai-protocol/spec.md) defines provider-neutral tool-call and generation request semantics.
- [004 Runtime Contract](../004-runtime-contract/spec.md) defines agent-invocation assembly and wiring responsibilities.
- [035 Permissions](../035-permissions/spec.md) defines permission policy that
  may gate execution after a model-visible tool request.
- [005 Durable Evidence](../005-durable-evidence/spec.md) defines durable linkage for tool requests, execution outcomes, and result artifacts.
- [006 Context Assembly](../006-context-assembly/spec.md) defines model context assembly, which stays separate from tool declarations and generation controls.
- [009 Resource Surface](../009-resource-surface/spec.md) defines resource gates that may affect tool execution.
- [030 State and Data Model](../030-state-and-data-model/spec.md) defines how tool facts relate to other state families.
- [050 Capability Extensions](../050-capability-extensions/spec.md) defines how capability contributions may provide tool candidates before agent-invocation scoped selection.
- [051 Agents](../051-agents/spec.md) defines selected-agent tool policy.
- [051 Subagents](../051-agents/subagents.md) defines subagent control tools.
