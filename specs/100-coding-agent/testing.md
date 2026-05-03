---
name: 100. Coding Agent Testing
psychevo_self_edit: deny
---

Define acceptance expectations and validation scenarios for the built-in `coding-agent` capability.

## Long-Term Acceptance Contract

- A `coding-agent` invocation without a resolvable session boundary is rejected before `agent_start`.
- A `coding-agent` invocation without a resolvable working context is rejected before `agent_start`.
- A `coding-agent` invocation whose required `coding-core` toolset cannot be assembled is rejected before `agent_start`.
- A normal coding-agent invocation accepts coding intent, uses runtime-resolved session and working context, can execute the `coding-core` tools, and completes with caller-facing answer material.
- Working context may be resolved from runtime-owned default sources. Tests should verify successful default resolution and rejection when resolution remains missing, ambiguous, unavailable, or outside the accepted runtime boundary.
- Refreshing a model-visible tool declaration snapshot must preserve the accepted minimum `coding-core` capability unless the invocation explicitly uses a runtime-accepted specialized mode.
- Completion exposes final material that can be retrieved or inspected through session, evidence, message, and material relationships.
- Live observation projects `agent_start`, `agent_end`, turn, message, and tool execution event families without requiring event payload schemas in this spec.
- Resource denials, deferrals, timeouts, bounds, and aborts become observable as before-agent-start rejection or tool-result errors.

## Current Implementation Slice

There is currently no required validation command for this capability because this repository slice is specification-only. When code exists, the default validation path must use deterministic local harnesses and fake or test providers.

The default validation path must not require real API keys, live providers, live services, or user-specific host configuration.

Manual real-provider smoke validation is allowed only as an explicit opt-in path. It must not be part of the default validation entrypoint.

## Scenario Matrix

- Before-agent-start rejection when working context is missing, ambiguous, unavailable, or otherwise not resolvable.
- Successful invocation when runtime resolves working context from a default source supplied by the entrypoint or test harness.
- Before-agent-start rejection when `coding-core` cannot be expanded into matching tool declarations and execution bindings.
- Tool declaration snapshot refresh preserves the default `coding-core` declarations and bindings for ordinary coding-agent invocations.
- Explicit specialized mode may omit some or all default `coding-core` declarations only when runtime accepts that mode.
- Fake-provider end-to-end invocation where the model requests `read`, `edit`, `write`, and `bash`, then returns a final answer.
- Live observation shows `agent_start`, `turn_start`, `message_start`, tool execution events, message completion, turn completion, and `agent_end`.
- Final material resolves to evidence-backed agent-invocation facts without requiring tests to inspect storage schemas.
- Tool-result facts are linked to assistant tool requests, tool execution outcomes, and loop-visible tool-result messages.
- Tests for the first implementation slice must not require skills, memory, instruction-file discovery, or context-file discovery unless a later spec makes those features part of the default capability.

## Validation Boundaries

- Acceptance tests should compare behavior and semantic invariants, not storage field layouts or provider payload shapes.
- Tests should avoid brittle snapshots of prompt text, generated lists, provider catalogs, or workflow prose.
- Tests must keep fake provider state, resource state, temporary files, and environment changes isolated and cleaned up.
- Real-provider smoke tests may check basic compatibility, but failures there must not block the deterministic default validation path unless explicitly requested.
