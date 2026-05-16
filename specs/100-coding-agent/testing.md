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

Automation vocabulary and generic validation boundaries follow
[060 Automation](../060-automation/spec.md).

There is currently no required validation command for this capability because
this repository slice is specification-only. When code exists, this topic's
default validation path should use deterministic local harnesses and fake or
test providers.

Manual real-provider smoke validation is allowed only as live opt-in validation.

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
- Tests for the first implementation slice cover AGENTS-named project
  instruction discovery, injection, warnings for legacy assistant memory files,
  and evidence projection. They must not require skills, memory, legacy
  assistant imports/rules, third-party instruction globs, remote instruction
  URLs, or other context-file discovery unless a later spec makes those
  features part of the default capability.

## Validation Boundaries

- Acceptance tests should compare behavior and semantic invariants, not storage field layouts or provider payload shapes.
- Tests should avoid brittle snapshots of coding-agent prompt text, generated
  workflow prose, or implementation-private provider payloads.
- Fake-provider end-to-end tests should preserve evidence-backed final material
  without requiring tests to inspect storage schemas directly.
