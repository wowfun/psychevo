---
name: 004. Runtime Contract
psychevo_self_edit: deny
---

Define the runtime contract owned by `psychevo-runtime`.

## Scope

- runtime run assembly
- model target and generation-control wiring
- provider configuration wiring
- model context assembly
- resource surface wiring
- run-scoped tool surface assembly
- stop, abort, and cancellation signal wiring
- durable evidence sink wiring
- transport-neutral runtime library boundary

Out of scope:
- agent execution semantics or event families
- AI provider protocol semantics
- model catalogs, fallback priority, or provider selection policy
- authentication storage, OAuth, environment lookup, headers, transport, retries, or billing
- concrete tool names, tool schemas, tool behavior, or permission rules
- resource permission schemas, policy rule languages, approval UX, sandbox behavior, path rules, concrete enforcement mechanics, or security policy
- durable record, trace, replay, session storage, or persistence formats
- CLI parsing, terminal rendering, process behavior, or exit codes
- memory, skills, evaluation, self-evolution, or workflow search

## Runtime Boundary

`psychevo-runtime` is the transport-neutral library boundary for starting and coordinating runs. CLI and future non-CLI entry points should use runtime libraries directly instead of routing through command-line transport.

Runtime owns run assembly and wiring. `psychevo-agent-core` keeps agent execution semantics, and `psychevo-ai` keeps provider-neutral AI protocol semantics.

Lower layers stay policy-free. Resource surface decisions, context policy, transport behavior, and product policy must not move into `psychevo-agent-core` or `psychevo-ai`.

CLI parsing, terminal rendering, stdin/stdout framing, and process exit behavior stay outside the runtime contract.

## Run Assembly

Runtime assembles a run from caller, configuration, and session continuity inputs. [008 Session Continuity](../008-session-continuity/spec.md) defines session continuity inputs. Run assembly connects:
- `psychevo-agent-core`
- `psychevo-ai`
- model target and generation controls
- provider configuration
- model context
- resource surface
- run-scoped tool surface
- stop, abort, and cancellation signals
- evidence sink

Runtime resolves the model target and generation controls for a run and passes them to the AI layer. This spec does not define model catalogs, provider selection, fallback priority, or model registry behavior.

Runtime wires provider configuration needed by a run. Authentication storage, OAuth, environment lookup, headers, transport, retry behavior, and billing policy belong outside this spec.

Runtime assembles model context for generation requests. [006 Context Assembly](../006-context-assembly/spec.md) defines context projection, visibility boundaries, and transformation boundaries. This spec does not define prompt templates, prompt section ordering, context schemas, memory behavior, or which runtime inputs become loop-visible messages.

Runtime wires the resource surface for non-model resources used by context assembly and tool execution. [009 Resource Surface](../009-resource-surface/spec.md) defines resource boundaries, access gates, and resource decisions. This spec does not define resource permission schemas, policy rule languages, approval UX, sandbox behavior, path rules, concrete enforcement mechanics, or security policy.

Runtime assembles the run-scoped tool surface. [007 Tool Surface](../007-tool-surface/spec.md) defines the declaration, binding, and selection contract. This spec does not define concrete tool names, tool schemas, tool result formats, or tool permission rules.

Runtime wires stop, abort, and cancellation signals into agent execution, AI generation, tool execution bindings, and the evidence sink. Outcome semantics remain owned by [002 Agent Execution](../002-agent-execution/spec.md) and [003 AI Protocol](../003-ai-protocol/spec.md).

Runtime connects run assembly facts, agent-core execution events, AI generation outcomes, tool outcomes, and terminal outcomes to an evidence sink. An evidence sink is the runtime-wired destination for durable run evidence. [005 Durable Evidence](../005-durable-evidence/spec.md) defines durable evidence semantics. This spec does not define record shape, storage format, trace format, replay semantics, or session storage format.

## Related Topics

- [000 Foundation](../000-foundation/spec.md) defines the upstream project foundation and implementation-neutral principles.
- [001 Architecture](../001-architecture/spec.md) defines Rust workspace layout, crate boundaries, runtime coordination, and dependency direction.
- [002 Agent Execution](../002-agent-execution/spec.md) defines agent-core execution semantics and core event families.
- [003 AI Protocol](../003-ai-protocol/spec.md) defines provider-neutral generation semantics consumed by agent execution.
- [005 Durable Evidence](../005-durable-evidence/spec.md) defines the durable evidence contract connected by runtime evidence sink wiring.
- [006 Context Assembly](../006-context-assembly/spec.md) defines model context assembly and transformation boundaries.
- [007 Tool Surface](../007-tool-surface/spec.md) defines run-scoped tool surface semantics.
- [008 Session Continuity](../008-session-continuity/spec.md) defines session continuity inputs for run assembly.
- [009 Resource Surface](../009-resource-surface/spec.md) defines runtime-owned resource surface and resource decision semantics.
- [010 Memory System](../010-memory-system/spec.md) defines optional memory boundaries outside required run assembly.
