---
name: 007. Tool Surface
psychevo_self_edit: deny
---

Define the run-scoped tool surface contract owned by `psychevo-runtime`.

## Scope

- semantic tool surface exposed for one run
- tool declarations made visible to the model
- execution bindings supplied by runtime
- run-scoped tool surface selection
- boundary between tool requests, tool execution, and tool-result artifacts
- tool surface facts that may contribute to durable evidence

Out of scope:
- concrete tool names, concrete tool behavior, or built-in tool lists
- tool schemas, payload schemas, Rust APIs, traits, structs, or function signatures
- provider-specific tool-call fields, wire formats, or schema translation rules
- toolset names, tool discovery rules, configuration precedence, or plugin and extension APIs
- resource permission schemas, approval behavior, sandbox rules, path rules, or enforcement mechanics
- tool result formats, terminal rendering, UI rendering, or CLI behavior
- storage formats, session formats, replay formats, traces, migrations, or indexes

## Tool Surface

A tool surface is the run-scoped set of tool declarations paired with execution bindings.

A tool declaration is a model-visible semantic capability promise. It describes a tool the model may request. This spec does not define declaration fields, input schemas, provider-specific encoding, or prompt text.

An execution binding is the runtime-supplied executable handler for a tool request. It connects a requested tool to behavior supplied by a built-in or external source. This spec does not define handler signatures, process boundaries, or implementation APIs.

Runtime assembles the tool surface for a run before model generation can request tools.

Runtime must expose a tool declaration to the model only when the same run has a matching execution binding. Runtime may omit unavailable tools from the run-scoped tool surface.

Runtime selects the run-scoped tool surface from available inputs. This spec does not define source discovery, selection precedence, toolset composition, or plugin mechanics.

Tool implementations may come from built-in or external sources, but source mechanics do not define the tool surface contract.

The AI generation request may include tool declarations from the run-scoped tool surface. [003 AI Protocol](../003-ai-protocol/spec.md) owns provider-neutral generation request semantics and provider-facing normalization.

Tool surface facts may contribute to durable evidence for run inspection. [005 Durable Evidence](../005-durable-evidence/spec.md) defines which tool request, execution outcome, and tool-result relationships must be representable.

## Request and Execution Boundary

A tool request is an assistant-requested tool call normalized by [003 AI Protocol](../003-ai-protocol/spec.md). Tool requests are model output, not direct execution authority.

A tool declaration says what the model may request. It does not authorize execution.

Agent execution observes assistant-requested tool calls and invokes runtime-supplied execution bindings through its tool execution flow. [002 Agent Execution](../002-agent-execution/spec.md) owns tool execution lifecycle events, ordering, scheduling latitude, and outcome semantics.

Resource gates may affect execution after a tool request and before or during the execution binding. [009 Resource Surface](../009-resource-surface/spec.md) defines allow, deny, and defer resource decision semantics. This spec does not define permission semantics, approval UX, sandbox behavior, resource policy rules, path rules, or failure policy.

A tool result artifact is loop-visible result material produced after a tool execution. Tool-result message semantics are defined by [002 Agent Execution](../002-agent-execution/spec.md). This spec does not define tool result payloads, rendering, schema validation, or durable record shape.

## Related Topics

- [000 Foundation](../000-foundation/spec.md) defines the upstream project foundation and implementation-neutral principles.
- [001 Architecture](../001-architecture/spec.md) defines crate boundaries and dependency direction.
- [002 Agent Execution](../002-agent-execution/spec.md) defines tool execution lifecycle, ordering, and tool-result message semantics.
- [003 AI Protocol](../003-ai-protocol/spec.md) defines provider-neutral tool-call and generation request semantics.
- [004 Runtime Contract](../004-runtime-contract/spec.md) defines runtime run assembly and wiring responsibilities.
- [005 Durable Evidence](../005-durable-evidence/spec.md) defines durable linkage for tool requests, execution outcomes, and result artifacts.
- [006 Context Assembly](../006-context-assembly/spec.md) defines model context assembly, which stays separate from tool declarations and generation controls.
- [009 Resource Surface](../009-resource-surface/spec.md) defines resource gates that may affect tool execution.
