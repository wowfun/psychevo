---
name: 100. Coding Agent Implementation Plan
psychevo_self_edit: deny
---

Plan the future implementation of the built-in `coding-agent` capability. This plan is not a command-line product plan and does not implement code by itself.

## Phase 1: Session and Agent-Invocation Assembly

- Add a runtime-owned capability target for `coding-agent` inside `psychevo-runtime`; do not create a new crate.
- Accept coding intent and resolve a session plus working context through runtime-owned inputs.
- Allow working context resolution from caller input, session continuity, entrypoint state, process environment, or other runtime-owned inputs without defining resolver precedence in this spec.
- Reject invocations before `agent_start` when the session boundary cannot be provided, the working context is not resolvable, or the required `coding-core` toolset cannot be assembled.
- Resolve default `coding-core` toolset selection as the minimum initial capability for ordinary coding-agent invocations.

## Phase 2: Context, Tool, and Resource Wiring

- Wire coding-agent context candidates into context assembly without making them automatically model-visible.
- Assemble expanded model-visible tool declaration snapshots from the selected toolsets and matching execution bindings.
- Refresh tool declaration snapshots between generation requests according to [007 Tool Surface](../007-tool-surface/spec.md) without silently removing the accepted minimum `coding-core` capability.
- Ensure all `coding-core` tool bindings operate through the runtime-bound working context and resource surface.
- Project resource denials, deferrals, timeouts, bounds, and aborts as before-agent-start rejection or observable tool-result errors according to the owning boundary.

## Phase 3: Evidence and Completion

- Persist agent-invocation assembly facts for capability target, selected toolsets, expanded tools, working context resolution, and assembly failures or degraded selections.
- Persist final tool request, execution outcome, and tool-result relationships through durable evidence.
- Return caller-facing completion with answer material and evidence-backed final facts or artifact material reachable through session, evidence, message, and material relationships.

## Phase 4: Product Entrypoints

- Keep CLI behavior, rendering, flags, and exit codes outside this capability spec.
- Build future CLI support in a later product spec that routes through the runtime library surface.
- Keep instruction-file discovery, context-file discovery, memory, skills, and self-evolution out of the first implementation slice. Later implementations may connect them through context, memory, or capability contribution boundaries.

## Validation

- Use deterministic fake-provider validation as the default implementation gate.
- Keep real-provider smoke validation manual and opt-in.
- Update this plan when concrete runtime APIs, module names, or validation entrypoints exist.

## Related Topics

- [100 Coding Agent](spec.md) defines the stable capability contract.
- [100 Testing](testing.md) defines acceptance scenarios and validation expectations.
- [110 Coding Core Tools](../110-coding-core-tools/spec.md) defines the required toolset assembled by this capability.
