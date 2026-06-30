---
name: 020. Interfaces
psychevo_self_edit: deny
---

Define Psychevo's caller-facing interface layer.

## Scope

- caller-facing interface semantics
- entrypoint categories at the semantic level
- invocation baseline
- live agent-invocation observation baseline
- final completion baseline
- session-start and before-agent-start rejection
- caller-facing stop, abort, and live user-input control signals

Out of scope:
- Rust traits, structs, functions, modules, or concrete APIs
- CLI commands, flags, rendering, process behavior, or exit codes
- HTTP, JSON, SDK, stream protocol, wire format, or payload schemas
- error envelopes, stable error codes, provider payloads, storage formats, replay formats, trace formats, or session file formats
- concrete model, tool, resource, context, session, memory, or evidence storage behavior

## Interface Layer

A caller is any product surface, library consumer, SDK, transport adapter, test harness, or automation layer that asks Psychevo to perform work.

An entrypoint is a caller-facing way to invoke Psychevo. Gateway libraries are the stable interactive substrate entrypoint, while runtime libraries remain the execution substrate beneath Gateway. CLI is a product entrypoint category. SDK, HTTP, and other transports may exist as future entrypoint categories.

Interactive entrypoints should route work through `psychevo-gateway` instead of reaching into lower layers for thread/turn orchestration. Gateway delegates execution to `psychevo-runtime`. ACP is a concrete transport entrypoint category for editor and agent-client integrations. `psychevo-agent-core` owns execution semantics, and `psychevo-ai` owns provider-neutral AI protocol semantics. Interface behavior must not redefine those lower-layer contracts.

Interactive entrypoints provide a source identity with an explicit lifetime to
Gateway. Invocation-only callers may avoid automatic source continuity, process
surfaces may keep continuity inside one Gateway instance, and reconnectable
transports may request persistent source binding. Interface specs choose the
caller semantics; Gateway owns normalization, queueing, and source-to-thread
resolution.

This spec defines interface semantics, not implementation shape. Narrower interface specs may specialize product entrypoints or transport behavior while preserving this caller-facing boundary.

## Invocation

An invocation is a caller request for runtime to resolve a session boundary and assemble one agent invocation.

At the semantic level, an invocation may include:
- caller input or intent
- optional session selector
- optional capability target
- optional model and generation preferences
- optional context hints
- optional tool surface hints or toolset selectors
- optional resource surface hints
- optional memory hints

Runtime first resolves the session boundary, then assembles the agent invocation. These invocation inputs are hints to runtime assembly. Runtime resolves them through the source-of-truth specs for session continuity, runtime assembly, context assembly, tool surface, resource surface, memory, and capability-extension declarations.

A session-start rejection happens when an entrypoint or runtime cannot create, open, reopen, or provide the required session boundary.

A before-agent-start rejection happens after the session boundary is considered but before `agent_start`, when runtime cannot assemble required capability, context, working context, resource, model, or toolset material. A rejected invocation is not a failed agent invocation and does not imply agent execution lifecycle semantics.

Once runtime emits `agent_start`, terminal outcome semantics belong to agent execution and AI protocol specs.

## Observation

An observation is caller-visible progress from an active agent invocation.

Live observation and final completion are separate interface concepts. A caller may observe an agent invocation while it is active, then receive final completion after it settles.

Observations project the execution event families owned by agent execution: `agent_start`, `agent_end`, `turn_start`, `turn_end`, `message_start`, `message_update`, `message_end`, `tool_execution_start`, `tool_execution_update`, and `tool_execution_end`. AI output progress may preserve the output categories owned by the AI protocol, including assistant text or content progress, reasoning or thinking progress, and tool-call progress.

An observed `agent_end` may be a semantic projection derived from lower-level execution events, provider results, runtime completion facts, and evidence-backed final material. Interface observation must not require the core loop event payload to carry every caller-facing completion field.

Session lifecycle observations, when an entrypoint exposes them, are separate interface observations and are not core agent execution events.

Observation streams are not durable evidence. Durable evidence records final facts for inspection and future replay work; observation is the caller-facing progress surface during execution.

This spec does not define event payload schemas, transport framing, buffering, replay, ordering beyond agent execution semantics, or terminal rendering.

## Completion

Completion is the caller-facing settlement of a started agent invocation.

At minimum, completion carries terminal outcome semantics and a way for the caller to reach final facts or evidence-backed result material through retained session, evidence, message, and material relationships. This spec does not define fields, identifiers, transcript formats, storage layout, or result rendering.

Normal, stopped, failed, and aborted outcomes are owned by agent execution and AI protocol specs. Interface entrypoints may present those outcomes differently, but they must not change their meaning.

Final loop-visible artifacts, tool outcomes, AI generation outcomes, resource decisions, memory-related facts, and causal relationships remain governed by their source-of-truth specs. The interface layer exposes or reaches those facts; it does not become a second execution record.

## Control Signals

A control signal is a caller-facing request that affects an active or pending agent invocation.

Interfaces may support graceful stop and abort or cancellation. Graceful stop asks runtime to end at a supported execution boundary. Abort or cancellation asks runtime to interrupt work that can still be interrupted.

Interfaces may support live user-input steering for an active agent
invocation. A steer request asks runtime and agent execution to add a user
message to the current invocation at the next supported generation boundary.
Until the lower layer commits that pending input into a provider request, the
caller-facing interface may update or cancel it. Once committed, the input is
ordinary loop-visible user message evidence and cannot be retracted by pending
input controls.

Next-turn queueing is caller-owned scheduling, not core execution semantics. A
queued input waits for a later invocation and only becomes execution input when
the entrypoint drains it into runtime invocation or steering APIs. If a running
invocation cannot accept steering, an entrypoint may degrade live input into a
next-turn queue while preserving that distinction for the user.

Runtime wires control signals into lower layers. Outcome semantics remain owned by agent execution and AI protocol specs.

Pause, resume, retry, undo, branch navigation, and checkpoint restore are out of scope for this interface baseline.

ACP interfaces specialize this baseline by mapping protocol requests to gateway
threads, observations, control signals, permissions, auth, commands, model and
mode controls, config controls, and capability-extension source inputs. That protocol
mapping is owned by [027 ACP](../027-acp/spec.md).

## Related Topics

- [000 Foundation](../000-foundation/spec.md) defines the upstream project foundation and implementation-neutral principles.
- [001 Architecture](../001-architecture/spec.md) defines crate boundaries, dependency direction, and transport separation.
- [002 Agent Execution](../002-agent-execution/spec.md) defines execution concepts, event families, message semantics, and outcome semantics.
- [003 AI Protocol](../003-ai-protocol/spec.md) defines provider-neutral AI output categories and generation outcomes.
- [004 Runtime Contract](../004-runtime-contract/spec.md) defines session coordination, agent-invocation assembly, and control-signal wiring.
- [005 Durable Evidence](../005-durable-evidence/spec.md) defines durable evidence semantics for final agent-invocation facts.
- [006 Context Assembly](../006-context-assembly/spec.md) defines model context assembly from invocation and runtime inputs.
- [007 Tool Surface](../007-tool-surface/spec.md) defines agent-invocation scoped tool surface semantics.
- [008 Session Continuity](../008-session-continuity/spec.md) defines the session boundary and continuity inputs for invocation.
- [009 Resource Surface](../009-resource-surface/spec.md) defines resource surface and resource decision semantics.
- [010 Memory System](../010-memory-system/spec.md) defines optional memory boundaries that may provide invocation hints.
- [025 CLI](../025-cli/spec.md) defines command-line interface foundation semantics.
- [027 ACP](../027-acp/spec.md) defines the Agent Client Protocol boundary.
- [021 Gateway](../021-gateway/spec.md) defines transport-neutral thread and turn orchestration.
- [200 pevo CLI](../200-pevo-cli/spec.md) defines the concrete `pevo` product CLI.
- [230 pevo-acp](../230-pevo-acp/spec.md) defines the concrete ACP server
  packaging for the `pevo` product.
- [030 State and Data Model](../030-state-and-data-model/spec.md) defines semantic state relationships projected through interfaces.
- [031 Storage and Persistence](../031-storage-and-persistence/spec.md) defines persistence boundaries for evidence-backed result material.
- [070 Experience](../070-experience/spec.md) defines cross-cutting UX and DX defaults for caller-facing and developer-facing behavior.
