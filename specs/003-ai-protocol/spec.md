---
name: 003. AI Protocol
psychevo_self_edit: deny
---

Define the provider-neutral AI protocol and compatibility boundary owned by `psychevo-ai`.

## Scope

- provider-neutral AI protocol owned by `psychevo-ai`
- currently specified agent-facing generation protocol
- mainstream-compatible generation request and stream semantics
- semantic generation request concepts, including assembled model context
- normalized generation concepts consumed by agent execution without exposing raw provider wire formats
- metadata extension boundaries for provider-specific, replay, continuity, or diagnostic information
- terminal outcome semantics for generation
- boundaries between provider normalization, agent execution, and the runtime-supplied tool surface

Out of scope:
- model catalogs, model selection policy, or provider registry rules
- endpoint paths, authentication, network transport, retries, rate limits, or billing
- exact OpenAI, Anthropic, or other provider request, response, or stream fields
- concrete Rust traits, structs, functions, or module APIs
- payload schemas, metadata schemas, provider-specific metadata keys, or persistence formats
- concrete providers, concrete tools, or tool execution behavior
- test-provider scenarios or behavior
- CLI rendering, durable records, replay formats, traces, or sessions

## Capability Coverage

`003` owns the provider-neutral protocol space for AI capabilities exposed through `psychevo-ai`.

This document specifies generation because agent execution needs generation first. Embeddings, reranking, vision, and other AI capabilities remain within this protocol area, but this document does not define their detailed semantics.

Future specs may specialize non-generation capabilities. Those specs must preserve the provider-neutral boundary owned by `psychevo-ai`.

## Protocol Principles

The AI protocol is internal to Psychevo. It uses mainstream-compatible semantics as its baseline, but it is not an external provider wire protocol.

`psychevo-ai` should preserve mainstream-compatible semantics when practical. Provider adapters should prefer `OpenAI-compatible` or `Anthropic-compatible` generation families before introducing custom concepts.

`OpenAI-compatible` and `Anthropic-compatible` are named compatibility families for adapter design. They guide adapter choices and do not make external request fields, response fields, endpoints, authentication behavior, streaming frames, or structured payload details part of this spec.

Psychevo should not invent alternative semantics for roles, assistant output, tool calls, tool results, usage, or terminal outcomes when the mainstream families already provide usable semantics.

Metadata is the extension mechanism for provider-specific details needed for replay, continuity, diagnostics, or future compatibility. Metadata must not redefine core protocol semantics.

`psychevo-ai` normalizes provider differences before agent execution observes them. `psychevo-agent-core` should consume model output through provider-neutral generation semantics.

The protocol describes generation semantics, not implementation shape. Concrete API signatures, serialization, storage, network behavior, and provider adapters belong outside this spec.

Any provider implementation used by agent execution must conform to this protocol at the boundary with `psychevo-agent-core`.

## Generation Request

A generation request represents one model invocation made for agent execution.

At the semantic level, a generation request contains:
- a model target
- model context assembled by runtime
- the tool declaration snapshot available to the model for this request
- generation controls such as limits or stopping policy

These concepts should map cleanly to `OpenAI-compatible` or `Anthropic-compatible` generation families when practical. This spec does not adopt either family's concrete fields.

The model target identifies which model the AI layer should invoke. This spec does not define model catalog structure, provider selection, fallback, or routing policy.

Model context is the semantic input that runtime intends the model to consume. [006 Context Assembly](../006-context-assembly/spec.md) defines model context assembly and projection. Its loop-visible portion uses the message semantics from [002 Agent Execution](../002-agent-execution/spec.md).

The tool declaration snapshot describes what the model may request for this generation request. [007 Tool Surface](../007-tool-surface/spec.md) defines declaration snapshot and execution binding semantics. Tool declarations do not define concrete tool behavior, resource gate semantics, permission rules, or execution scheduling.

Generation controls constrain the model invocation. They do not define provider-specific options or transport-level behavior.

## Normalized Stream

The AI protocol may expose generation as a normalized stream.

The stream categories are:
- assistant text/content progress
- assistant reasoning/thinking progress when available
- assistant-requested tool-call progress
- optional usage metadata
- optional extension metadata
- terminal outcome

Normalized streams preserve the kind of model output being produced. Agent execution may project these categories onto the message lifecycle events defined by [002 Agent Execution](../002-agent-execution/spec.md), but this spec does not collapse all output progress into a single message-progress shape.

Assistant-requested tool-call progress identifies tool requests produced by the model. [002 Agent Execution](../002-agent-execution/spec.md), [004 Runtime Contract](../004-runtime-contract/spec.md), and [007 Tool Surface](../007-tool-surface/spec.md) define downstream execution and tool-result boundaries.

Usage metadata may describe consumption reported by a provider. Usage metadata is optional. Pricing, accounting, provider-specific token fields, and billing policy belong outside this spec.

Extension metadata may carry details that do not belong in core generation semantics. Metadata is optional unless a later spec promotes a field into core semantics.

The terminal outcome completes the generation stream. A generation stream must not leave agent execution without an observable terminal outcome.

## Metadata Extensions

Metadata may carry provider-specific identifiers, reasoning/thinking continuity data, tool-call continuity data, cache or usage adjuncts, and diagnostic context.

Metadata must remain optional for core agent execution unless a later spec promotes a field into core semantics.

Metadata shape, serialization, persistence, replay rules, and provider-specific keys belong outside this spec.

## Terminal Outcomes

Generation outcomes align with the outcome semantics defined by [002 Agent Execution](../002-agent-execution/spec.md):
- normal
- stopped
- failed
- aborted

A normal outcome means the model completed the generation without a stop-limit, failure, or abort condition.

A stopped outcome means generation ended because a configured generation limit or stopping policy was reached.

A failed outcome means the provider, model, or protocol normalization could not complete the generation.

An aborted outcome means the caller or runtime cancelled generation before normal completion.

Provider and model failures must surface as observable failed generation outcomes. This spec does not define whether an implementation reports that failure through return values, callbacks, streams, or errors.

## Boundaries

`psychevo-ai` owns mainstream-compatible adapter alignment, provider protocol normalization, metadata attachment, generation request translation, normalized stream production, and generation outcomes.

`psychevo-agent-core` owns agent execution, turn progression, core execution events, tool execution flow, and projection of normalized AI output categories into the agent loop.

`psychevo-runtime` owns the agent-invocation scoped tool surface, resource surface wiring, model context assembly, durable records, persistence, and replay wiring.

`psychevo-cli` owns process and terminal behavior. CLI rendering must not define AI protocol semantics.

## Related Topics

- [000 Foundation](../000-foundation/spec.md) defines the upstream project foundation and implementation-neutral principles.
- [001 Architecture](../001-architecture/spec.md) defines Rust workspace layout, crate boundaries, runtime coordination, and dependency direction.
- [002 Agent Execution](../002-agent-execution/spec.md) defines agent-core execution semantics and core event families.
- [004 Runtime Contract](../004-runtime-contract/spec.md) defines agent-invocation assembly and evidence sink wiring.
- [005 Durable Evidence](../005-durable-evidence/spec.md) defines durable evidence semantics for AI outcomes and optional metadata preservation.
- [006 Context Assembly](../006-context-assembly/spec.md) defines model context assembly consumed by generation requests.
- [007 Tool Surface](../007-tool-surface/spec.md) defines tool declarations and execution bindings available to generation requests.
- [009 Resource Surface](../009-resource-surface/spec.md) defines resource gate semantics outside the AI protocol.
- [030 State and Data Model](../030-state-and-data-model/spec.md) defines how AI generation facts relate to other state families.
