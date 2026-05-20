---
name: 006. Context Assembly
psychevo_self_edit: deny
---

Define the model context assembly contract owned by `psychevo-runtime`.

## Scope

- semantic model context assembled for one generation request
- per-agent-invocation context projection boundary
- generic source category visibility boundaries
- capability-supplied context candidates
- runtime-owned context projection
- context transformation boundaries
- prompt slot assembly and provider role fallback as specified by the
  [Prompt Assembly Attachment](prompt-assembly.md)
- durable evidence relationship for context assembly facts
- boundaries between context assembly, agent messages, AI generation requests, and tool inputs

Out of scope:
- exact prompt wording or byte-for-byte prompt templates
- exact provider request fields, response fields, stream fields, or wire formats
- exact source discovery rules, concrete file names, attachment syntax, or lookup syntax
- context-window reserve budgets, truncation limits, or compaction policy
- compaction algorithms, summarization algorithms, or summary schemas
- memory retention behavior, skill package formats or lifecycle management,
  session storage layouts, branches, replay formats, or workflow behavior
- tool declarations, tool schemas, tool execution behavior, or concrete tools
- resource permission schemas, policy rules, approval behavior, sandbox behavior, path rules, or enforcement mechanics
- Rust APIs, payload schemas, storage formats, persistence formats, or CLI rendering

## Model Context

Model context is the semantic model-facing input assembled for one generation request. It excludes the model target, generation controls, tool declarations, provider configuration, and transport details.

Instruction context is non-transcript instruction or runtime guidance intended
for the model. Provider role fallback, typed prompt slot ordering, cache tiers,
evidence, and accounting are defined by the
[Prompt Assembly Attachment](prompt-assembly.md). Exact prompt wording remains
outside this spec.

Loop-visible context is model-visible message material that uses the message semantics defined by [002 Agent Execution](../002-agent-execution/spec.md). Loop-visible context is the part of model context that belongs to the agent loop transcript.

Attached context is caller-supplied or runtime-supplied facts, source material, or artifacts made model-visible by context projection. This spec does not define attachment syntax, source discovery, source trust, or source storage.

Summary context is model-visible context produced by a context transformation. Summary context may stand in for earlier or larger source material, but this spec does not define summary structure, generation method, or persistence format.

These are semantic categories, not required prompt sections or provider fields.

## Context Projection

Context projection is the runtime-owned selection and transformation of available inputs into model context for one generation request. The projection boundary is per agent invocation and may differ for each generation request inside that invocation.

Runtime assembles model context before invoking generation.

Context projection may combine instruction context, loop-visible context, attached context, summary context, capability-supplied context candidates, session continuity inputs, memory recall candidates, and generic resource facts. These are source categories, not required sections or provider fields. [008 Session Continuity](../008-session-continuity/spec.md) defines session continuity inputs. [009 Resource Surface](../009-resource-surface/spec.md) defines resource facts and resource gates. [010 Memory System](../010-memory-system/spec.md) defines memory recall candidates. [050 Capability Extensions](../050-capability-extensions/spec.md) defines source and contribution boundaries for capability-supplied candidates.

Runtime must preserve visibility boundaries during projection. Inputs that remain runtime-only must not become model-visible. Inputs projected as loop-visible context must use agent execution message semantics.

Resource facts are candidates for context projection, not automatic model context. This spec owns model visibility for projected context.

Memory recall candidates are not automatically model-visible. This spec owns whether recalled memory becomes model context.

Capability selection may contribute instruction context, attached context candidates, or summary context candidates. Contribution does not make those candidates model-visible; runtime projection still owns visibility.

Skill discovery may contribute a compact skill index, explicit skill invocation
material, or skill-related context candidates as defined by
[055 Skills](../055-skills/spec.md). This spec owns whether that material is
projected into model context for a generation request.

Instruction context, attached context, and summary context may be model-visible without becoming finalized loop-visible message artifacts. A later spec may promote a source category into message semantics, but this spec does not do so.

The assembled model context is an input to the generation request defined by [003 AI Protocol](../003-ai-protocol/spec.md). Model target, tool declarations, and generation controls are adjacent generation request inputs and remain outside model context. [007 Tool Surface](../007-tool-surface/spec.md) defines tool declaration semantics.

Context assembly facts needed for agent-invocation inspection must be connectable to durable evidence. [005 Durable Evidence](../005-durable-evidence/spec.md) defines which durable evidence facts must be representable; this spec does not require full prompt snapshots.

Context projection is not required to write source material or projected
material into session messages. Runtime may persist model-visible runtime
injections as separate durable context evidence without making them
loop-visible transcript messages. If a projection becomes loop-visible message
material, [002 Agent Execution](../002-agent-execution/spec.md) owns the message
semantics.

## Context Usage Projection

Runtime may expose a context usage projection for the most recent provider
generation request or for a persisted session estimate. This projection is an
inspection aid; it must not redefine context assembly semantics, mutate session
state, or persist full prompt/request text.

The projection includes only source categories Psychevo can compute:

- base policy
- developer prompt
- project context
- history
- turn context
- current prompt
- system tools
- free space when a context limit is known

Human-readable context usage output may label model-facing transcript
categories as input categories to distinguish them from interface-visible
message counts. Structured snapshots retain the category keys defined by the
[Prompt Assembly Attachment](prompt-assembly.md).

Provider-reported input/context token usage, when available, is authoritative
for the headline total. Category totals are tokenizer estimates. If provider
usage is unavailable, the estimated category total may be used as the headline
and must be marked estimated.

Runtime-owned context usage data must retain counts, labels, category names,
tool counts, role counts, selected skill names, and per-skill index-entry
token counts only. Per-skill index-entry counts describe the compact skills
index entry, not loaded skill body text. Selected-agent text is developer
prompt material, not skill material. Runtime must not retain message bodies,
skill bodies, tool argument bodies, or provider request text after counting
completes.

This retention limit applies to the context usage projection. It does not
forbid the persistence layer from retaining model-visible runtime injections as
durable context evidence when those injections are intentionally attached to an
accepted user prompt for auditability.

For live agent invocations, the latest provider generation request wins. For
persisted session estimates, the projection uses current local runtime
assembly rules and persisted session messages; historical selected skill
bodies are not reconstructed unless explicitly available.

## Context Transformation

Context transformations may filter, truncate, summarize, or compact source material before a generation request.

A transformation changes the model-facing projection for a generation request. It must not redefine source semantics owned by agent execution, AI protocol, runtime contract, or durable evidence specs.

Summary context produced by a transformation is model-visible context. Summary content, summary schemas, summarizer models, thresholds, boundaries, and validation rules belong outside this spec.

Transformation policy may depend on caller inputs, runtime configuration, provider constraints, or model limits. This spec does not define those policies or their precedence.

## Related Topics

- [000 Foundation](../000-foundation/spec.md) defines the upstream project foundation and implementation-neutral principles.
- [001 Architecture](../001-architecture/spec.md) defines crate boundaries and dependency direction.
- [002 Agent Execution](../002-agent-execution/spec.md) defines loop-visible message semantics and execution events.
- [003 AI Protocol](../003-ai-protocol/spec.md) defines generation request semantics that consume assembled model context.
- [004 Runtime Contract](../004-runtime-contract/spec.md) defines agent-invocation assembly and wiring responsibilities.
- [005 Durable Evidence](../005-durable-evidence/spec.md) defines durable evidence semantics for inspectable agent-invocation facts.
- [007 Tool Surface](../007-tool-surface/spec.md) defines tool declarations that stay adjacent to model context.
- [008 Session Continuity](../008-session-continuity/spec.md) defines continuity inputs that may feed context projection.
- [009 Resource Surface](../009-resource-surface/spec.md) defines resource facts and gates that may constrain context projection.
- [010 Memory System](../010-memory-system/spec.md) defines memory recall candidates that may feed context projection.
- [030 State and Data Model](../030-state-and-data-model/spec.md) defines how context facts relate to other state families.
- [050 Capability Extensions](../050-capability-extensions/spec.md) defines capability contribution boundaries for context candidates.
- [055 Skills](../055-skills/spec.md) defines skill package discovery and model-visible skill index semantics.
- [130 Context Compaction](../130-context-compaction/spec.md) defines the
  implementation policy for compacted summary context.
- [Prompt Assembly Attachment](prompt-assembly.md) defines typed prompt slot
  ordering, prefix snapshots, provider-role fallback, and context usage
  categories.
