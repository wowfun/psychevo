---
name: 110. Coding Core Tools Implementation Plan
psychevo_self_edit: deny
---

Plan the future implementation of the `coding-core` toolset. This plan describes implementation direction, not current code.

## Phase 1: Toolset Assembly

- Register `coding-core` as a built-in runtime toolset contribution.
- Expand `coding-core` into exactly `read`, `edit`, `write`, and `bash`.
- Reject or mark unavailable the toolset when any required declaration or execution binding cannot be assembled.
- Keep toolset selection separate from model-visible tool declaration snapshots; the model sees only expanded tools exposed through [007 Tool Surface](../007-tool-surface/spec.md).

## Phase 2: Working Context and Resource Wiring

- Bind all four tools to the runtime-resolved working context accepted for the coding-agent invocation.
- Route file-like and process-like operations through the runtime resource surface.
- Convert resource denials, deferrals, timeouts, bounds, and aborts into before-agent-start rejection or model-visible JSON error results.
- Keep approval, sandbox, dangerous-command, path policy, and deny-list mechanics outside this spec.

## Phase 3: Tool Behavior

- Implement `read` as bounded text reading with pagination/range support, stable result fields, and binary/image refusal.
- Implement `write` as create-or-complete-replace behavior with parent directory creation when allowed.
- Implement `edit` with semantic `replace` and `patch` modes; keep patch updates limited to existing targets.
- Implement `bash` as foreground bounded command execution with observable exit status, timeout, abort, truncation, and the minimum non-zero explanation table.

## Phase 4: Evidence and Observation

- Emit tool execution lifecycle events through agent execution.
- Preserve raw model-visible JSON tool results and tool outcome summaries for observation and durable evidence.
- Link tool requests, execution outcomes, resource decisions, and tool-result messages in durable evidence.

## Validation

- Use deterministic fake-provider scenarios for default validation.
- Avoid real shell, host filesystem, or user configuration dependencies in default tests unless isolated by the test harness.
- Keep real-provider and live-resource smoke paths explicit and opt-in.

## Related Topics

- [110 Coding Core Tools](spec.md) defines the stable toolset and tool behavior contract.
- [110 Testing](testing.md) defines acceptance scenarios and validation expectations.
- [100 Coding Agent](../100-coding-agent/spec.md) consumes this toolset as the default coding-agent minimum.
