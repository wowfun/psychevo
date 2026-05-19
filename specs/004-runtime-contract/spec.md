---
name: 004. Runtime Contract
psychevo_self_edit: deny
---

Define the runtime contract owned by `psychevo-runtime`.

## Scope

- session boundary coordination
- agent-invocation assembly
- model target and generation-control wiring
- provider configuration wiring
- model context assembly
- resource surface wiring
- agent-invocation scoped tool surface assembly
- permission policy, permission mode, approval handler, and session grant wiring
- capability extension resolution
- stop, abort, and cancellation signal wiring
- durable evidence sink wiring
- transport-neutral runtime library boundary

Out of scope:
- agent execution semantics or event families
- AI provider protocol semantics
- model catalogs, fallback priority, or provider selection policy
- authentication storage, OAuth, environment lookup, headers, transport, retries, or billing
- concrete tool names, tool schemas, tool behavior, or permission rules
- permission rule languages, approval UX, sandbox behavior, path rules, concrete enforcement mechanics, or security policy
- plugin manifests, extension APIs, package formats, discovery paths, hot reload, startup protocols, shutdown protocols, or healthcheck protocols
- durable record, trace, replay, session storage, or persistence formats
- CLI parsing, terminal rendering, process behavior, or exit codes
- memory, skills, evaluation, self-evolution, or workflow search

## Runtime Boundary

`psychevo-runtime` is the transport-neutral library boundary for session coordination and agent-invocation assembly. CLI and future non-CLI entry points should use runtime libraries directly instead of routing through command-line transport.

Runtime owns session boundary resolution and execution wiring. `psychevo-agent-core` keeps agent execution semantics, and `psychevo-ai` keeps provider-neutral AI protocol semantics.

Lower layers stay policy-free. Resource surface decisions, context policy, transport behavior, and product policy must not move into `psychevo-agent-core` or `psychevo-ai`.

CLI parsing, terminal rendering, stdin/stdout framing, and process exit behavior stay outside the runtime contract.

## Session Boundary

Runtime resolves the session boundary before assembling an agent invocation.

A session boundary may be created, opened, reopened, resumed as the same session, or provided as an ephemeral in-memory session. [008 Session Continuity](../008-session-continuity/spec.md) defines session continuity and lifecycle semantics.

If runtime cannot create, open, or provide the required session boundary, the request becomes a session-start rejection. A session-start rejection does not emit `agent_start` and does not imply agent execution lifecycle semantics.

## Agent-Invocation Assembly

After a session boundary exists, runtime assembles an agent invocation from caller inputs, configuration, session continuity inputs, and available capability contributions. Agent-invocation assembly connects:
- `psychevo-agent-core`
- `psychevo-ai`
- optional capability target and toolset hints
- model target and generation controls
- provider configuration
- model context
- resource surface
- agent-invocation scoped tool surface
- permission policy inputs, permission mode, approval handler, and session grants
- generation-request tool declaration snapshots
- capability extension selections
- optional selected agent definition and child-agent control scope
- stop, abort, and cancellation signals
- evidence sink

Runtime resolves the model target and generation controls for an agent invocation and passes them to the AI layer. This spec does not define model catalogs, provider selection, fallback priority, or model registry behavior.

Runtime wires provider configuration needed by an agent invocation. Authentication storage, OAuth, environment lookup, headers, transport, retry behavior, and billing policy belong outside this spec.

Runtime assembles model context for generation requests. [006 Context Assembly](../006-context-assembly/spec.md) defines context projection, visibility boundaries, and transformation boundaries. This spec does not define prompt templates, prompt section ordering, context schemas, memory behavior, or which runtime inputs become loop-visible messages.

Runtime wires the resource surface for non-model resources used by context
assembly and tool execution. [009 Resource Surface](../009-resource-surface/spec.md)
defines resource boundaries, access gates, and resource decisions.
[035 Permissions](../035-permissions/spec.md) defines the concrete runtime
permission policy that may specialize those gates. This spec does not define
permission rule languages, approval UX, sandbox behavior, path rules, concrete
enforcement mechanics, or security policy.

Runtime assembles the agent-invocation scoped tool surface and supplies tool declaration snapshots for generation requests. Runtime may refresh those snapshots between generation requests when registry, availability, or toolset expansion facts change. [007 Tool Surface](../007-tool-surface/spec.md) defines the declaration snapshot, binding, and selection contract. This spec does not define concrete tool names, tool schemas, tool result formats, or tool permission rules.

Runtime assembles permission policy inputs for an accepted invocation, including
resolved permission configuration, runtime mode, permission mode, approval mode,
approval handler availability, and session-scoped grants. Permission assembly
is invocation state: it constrains tool execution and resource operations but
does not change which tool declarations runtime may expose.
[035 Permissions](../035-permissions/spec.md) owns permission semantics, rule
precedence, approval behavior, and fallback policy.

Runtime resolves optional capability targets and toolset hints from built-in, runtime-provided, or external contributions. If required capability material, working context, toolset, model, resource boundary, or evidence wiring cannot be assembled, runtime rejects the request before `agent_start`. A before-agent-start rejection is an invocation rejection, not a failed agent invocation. [050 Capability Extensions](../050-capability-extensions/spec.md) defines capability source, contribution, activation, availability, and conflict boundaries. This spec does not define plugin manifests, extension APIs, package formats, discovery paths, hot reload, startup protocols, shutdown protocols, or healthcheck protocols.

Runtime may resolve a selected agent definition for an invocation. Agent
definitions contribute instructions, model preferences, tool policy, hooks,
skills, and MCP scope through runtime assembly. Child subagent runs remain
runtime-owned agent invocations with parent session relationships and control
signals; they do not redefine core execution semantics. [051 Agents](../051-agents/spec.md)
defines selected-agent semantics, and [051 Subagents](../051-agents/subagents.md)
defines child control semantics.

Runtime wires stop, abort, and cancellation signals into agent execution, AI generation, tool execution bindings, and the evidence sink. Outcome semantics remain owned by [002 Agent Execution](../002-agent-execution/spec.md) and [003 AI Protocol](../003-ai-protocol/spec.md).

Runtime connects agent-invocation assembly facts, tool declaration snapshot facts, `agent_start` and `agent_end` events, AI generation outcomes, tool outcomes, messages, resource decisions, and terminal outcomes to an evidence sink. An evidence sink is the runtime-wired destination for durable session and agent-invocation evidence. [005 Durable Evidence](../005-durable-evidence/spec.md) defines durable evidence semantics. [040 Storage and Persistence](../040-storage-and-persistence/spec.md) defines the persistence substrate boundary. This spec does not define record shape, storage format, trace format, replay semantics, or session storage format.

When the AI layer reports normalized usage or allowlisted provider metadata,
runtime may associate those facts with the nearest completed assistant message
and persist them through the evidence sink. These facts are metrics and
diagnostic evidence, not transcript content blocks. Runtime summaries may expose
sanitized messages together with per-message metrics so interfaces do not need
direct storage coupling.

Runtime projections must keep metric/debug facts separate from sanitized
transcript messages. A caller-facing interface may choose to display token
totals, context percentages, usage parts, or provider metadata summaries, but
that projection must not redefine message content or provider replay semantics.

## Related Topics

- [000 Foundation](../000-foundation/spec.md) defines the upstream project foundation and implementation-neutral principles.
- [001 Architecture](../001-architecture/spec.md) defines Rust workspace layout, crate boundaries, runtime coordination, and dependency direction.
- [002 Agent Execution](../002-agent-execution/spec.md) defines agent-core execution semantics and core event families.
- [003 AI Protocol](../003-ai-protocol/spec.md) defines provider-neutral generation semantics consumed by agent execution.
- [035 Permissions](../035-permissions/spec.md) defines runtime permission
  policy, approval semantics, and permission modes wired by runtime.
- [005 Durable Evidence](../005-durable-evidence/spec.md) defines the durable evidence contract connected by runtime evidence sink wiring.
- [006 Context Assembly](../006-context-assembly/spec.md) defines model context assembly and transformation boundaries.
- [007 Tool Surface](../007-tool-surface/spec.md) defines agent-invocation scoped tool surface semantics.
- [008 Session Continuity](../008-session-continuity/spec.md) defines the session boundary and continuity inputs for agent-invocation assembly.
- [009 Resource Surface](../009-resource-surface/spec.md) defines runtime-owned resource surface and resource decision semantics.
- [010 Memory System](../010-memory-system/spec.md) defines optional memory boundaries outside required agent-invocation assembly.
- [020 Interfaces](../020-interfaces/spec.md) defines caller-facing invocation, observation, completion, and control-signal semantics.
- [030 State and Data Model](../030-state-and-data-model/spec.md) defines runtime assembly state relationships.
- [040 Storage and Persistence](../040-storage-and-persistence/spec.md) defines persistence substrate boundaries for runtime-wired durable facts.
- [050 Capability Extensions](../050-capability-extensions/spec.md) defines capability contributions resolved by runtime into agent-invocation scoped selections.
- [051 Agents](../051-agents/spec.md) defines selected agent definitions.
- [051 Subagents](../051-agents/subagents.md) defines child-agent control behavior.
- [100 Coding Agent](../100-coding-agent/spec.md) defines a built-in capability target assembled by runtime.
