---
name: 006. Context Assembly
psychevo_self_edit: deny
---

Define the model context assembly contract owned by `psychevo-runtime`.

## Scope

- semantic model context assembled for one generation request
- per-agent-invocation context projection boundary
- generic source category visibility boundaries
- contributor-supplied context candidates
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

Runtime environment context is instruction context that tells the model the
canonical runtime cwd and the path-resolution boundary for local tools. It
is model-visible for every agent invocation and is distinct from permission or
sandbox enforcement.

Runtime time context is turn-scoped instruction context that supplies the
current local calendar date and UTC offset for relative-time planning. It is
refreshed for every main or child agent invocation, uses date-only precision to
avoid needless prompt volatility, and directs the model to interpret terms such
as today, latest, recent, and current against that date while verifying
time-sensitive facts with available tools. Runtime time context is durable
context evidence for the accepted prompt, but is not part of the
session-stable prefix or its hash.

Runtime-owned model prompt text should be maintained as embedded template
resources owned by the runtime implementation. Moving prompt text into template
resources must not change the semantic ordering of prompt slots, provider-role
fallback behavior, context category accounting, or durable evidence semantics;
content hashes continue to describe the final rendered model-visible text.

Loop-visible context is model-visible message material that uses the message semantics defined by [002 Agent Execution](../002-agent-execution/spec.md). Loop-visible context is the part of model context that belongs to the agent loop transcript.

Attached context is caller-supplied or runtime-supplied facts, source material, or artifacts made model-visible by context projection. This spec does not define attachment syntax, source discovery, source trust, or source storage.

Summary context is model-visible context produced by a context transformation. Summary context may stand in for earlier or larger source material, but this spec does not define summary structure, generation method, or persistence format.

These are semantic categories, not required prompt sections or provider fields.

## Context Projection

Context projection is the runtime-owned selection and transformation of available inputs into model context for one generation request. The projection boundary is per agent invocation and may differ for each generation request inside that invocation.

Runtime assembles model context before invoking generation.

Project instruction discovery is configurable separately from tool permissions.
The default policy follows the project root to cwd hierarchy. A cwd-only
policy limits discovery to the canonical cwd, and an off policy suppresses
project instruction injection. These policies change model-visible project
context only; they do not widen or narrow filesystem, shell, network, or
approval behavior.

Context projection may combine instruction context, loop-visible context, attached context, summary context, contributor-supplied context candidates, session continuity inputs, memory recall candidates, and generic resource facts. These are source categories, not required sections or provider fields. [008 Session Continuity](../008-session-continuity/spec.md) defines session continuity inputs. [009 Resource Surface](../009-resource-surface/spec.md) defines resource facts and resource gates. [010 Memory System](../010-memory-system/spec.md) defines memory recall candidates. [050 Capability Extensions](../050-capability-extensions/spec.md) defines source, declaration, and registry boundaries for contributor-supplied candidates.

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
Versioned prompt-prefix records may retain prompt slot metadata and rendered
slot text needed for request reconstruction. Capability assembly summaries are
not context facts unless runtime projects their material into model-visible
context.

Context projection is not required to write source material or projected
material into session messages. Runtime may persist model-visible runtime
injections as separate durable context evidence without making them
loop-visible transcript messages. If a projection becomes loop-visible message
material, [002 Agent Execution](../002-agent-execution/spec.md) owns the message
semantics.

## Context Usage Projection

Runtime may expose a context usage projection for the most recent completed
provider generation turn or for a persisted session estimate. This projection is an
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

The headline context total describes the latest completed provider turn and is
not cumulative. Provider-reported total tokens are authoritative when present;
otherwise a complete provider input/output pair is summed. Input already
includes cache token subcategories and output already includes reasoning token
subcategories, so those subcategories are not added again. Category totals
remain tokenizer estimates of the request/input side and do not redefine the
provider total.

Structured context projections identify whether the headline is provider
reported, derived from provider input/output, locally estimated, partial, or
unavailable. They also identify whether the basis is the latest provider turn,
an agent-reported context snapshot, or a persisted session projection, and
retain the visible session sequence to which the value applies when known. An
agent-reported current-window value such as ACP `usage_update.used` may be used
as a partial context fallback but must not be treated as provider usage or a
session token total.

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

For live agent invocations, the latest completed provider generation wins. For
persisted sessions, a visible assistant usage record later than the latest
valid compaction checkpoint wins. Otherwise runtime reconstructs from the
prompt-prefix version and durable context evidence that belonged to the
visible turn. Persisted tool-token counts are tied to the recorded tool
declaration hash; missing or mismatched historical tool facts make the
projection partial instead of silently substituting the current tool surface.

## Session Observability Projection

Runtime may expose a session observability projection for UI surfaces. It is a
display-only summary and does not become transcript content, prompt text,
message history, context evidence, or model-visible input.

The projection combines the current context usage projection with persisted
visible message/accounting facts from the selected session. It may include
session-level totals for context input, billable input, billable output,
reasoning, cache read, cache write, effective total tokens, the raw
provider-reported subtotal, estimated cost, unknown-pricing message count,
provider/model identity, accounted/unaccounted provider-call counts, and a
derived cache-read percentage. Per provider call, an explicit provider total
wins; otherwise a complete input/output pair is summed. Incomplete usage makes
the aggregate partial or unavailable and any known value is a lower bound, not
a fabricated exact zero. The cache-read percentage is computed from cached
input tokens divided by context input tokens when a non-zero context input
total is available. Protocols that report cumulative session counters, such as
ACP `PromptResponse.usage`, must retain the raw cumulative snapshot for audit
and persist only its visible non-negative turn delta in the additive provider
usage fields.

Global usage windows and activity buckets apply the same provider-call rule as
session observability when deriving token and pricing status. A complete
input/output pair participates in estimated/free/included/unknown pricing
counters even when the provider omitted an explicit total. UI surfaces render
an `unavailable` total as an unavailable label or neutral placeholder, never as
an exact numeric zero.

Session observability must respect the current session boundary and any
history/revert visibility boundary used by transcript reload. It must not sum
messages from other sessions, hidden reverted ranges, or other cwds. Missing
accounting facts are treated as unknown/zero for display rather than
reconstructing provider requests.

UI surfaces may render compact always-visible metrics such as context percent,
cache percent, total session tokens, and estimated cost, with richer detail in
status or usage panels. These displays may be refreshed on resume using the
persisted session summary plus a fresh context-window estimate.

## Context Transformation

Context transformations may filter, truncate, summarize, or compact source material before a generation request.

A transformation changes the model-facing projection for a generation request. It must not redefine source semantics owned by agent execution, AI protocol, runtime contract, or durable evidence specs.

Summary context produced by a transformation is model-visible context. Summary content, summary schemas, summarizer models, thresholds, boundaries, and validation rules belong outside this spec.

Transformation policy may depend on caller inputs, runtime configuration, provider constraints, or model limits. This spec does not define those policies or their precedence.

## Attachments

- [Prompt Assembly Attachment](prompt-assembly.md) defines typed prompt slot
  ordering, prefix snapshots, provider-role fallback, and context usage
  categories.

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
- [050 Capability Extensions](../050-capability-extensions/spec.md) defines
  source, declaration, and registry boundaries for context candidates.
- [055 Skills](../055-skills/spec.md) defines skill package discovery and model-visible skill index semantics.
- [130 Context Compaction](../130-context-compaction/spec.md) defines the
  implementation policy for compacted summary context.
