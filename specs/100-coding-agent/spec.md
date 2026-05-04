---
name: 100. Coding Agent
psychevo_self_edit: deny
---

Define the built-in coding-agent capability assembled by `psychevo-runtime`.

## Scope

- runtime-owned `coding-agent` capability semantics
- minimum invocation requirements for coding-agent agent invocations
- required session and working-context boundary
- default coding toolset selection
- relationship between coding intent, working context, tools, and final result material
- evidence-backed completion expectations for coding-agent completion

Out of scope:
- CLI commands, flags, rendering, terminal UI, process behavior, or exit codes
- project-root discovery, instruction-file discovery, workspace policy, or context-source discovery
- approval UX, sandbox behavior, deny lists, dangerous-command policy, or concrete resource policy
- memory, skills, evaluation, self-evolution, self-modification loops, training, or workflow search
- concrete provider behavior, prompt wording, prompt templates, or model-selection policy
- standalone crate boundaries, Rust APIs, payload schemas, storage schemas, or wire formats

## Capability Contract

`coding-agent` is a built-in capability target resolved by runtime. It is not a separate Rust crate boundary. Its future implementation belongs inside `psychevo-runtime` as a built-in capability module that assembles lower-level runtime, tool, context, resource, and evidence contracts.

A coding-agent invocation accepts a coding intent from the caller and operates inside a runtime-resolved session and working context. The coding intent is the caller's requested software work. The working context is the runtime-bound resource and context boundary that the coding tools operate through.

Runtime may resolve the working context from caller input, session continuity, entrypoint state, process environment, or other runtime-owned inputs. This spec does not define source priority, project-root discovery, instruction-file discovery, workspace policy, or context-source discovery.

Runtime must reject a `coding-agent` invocation before `agent_start` when the session boundary cannot be provided or the working context cannot be resolved into one unique, available, runtime-bound resource and context boundary. Session failure is a session-start rejection; working-context assembly failure is a before-agent-start rejection. Neither is a failed agent invocation.

The default toolset for `coding-agent` is `coding-core`, defined by [110 Coding Core Tools](../110-coding-core-tools/spec.md). Runtime must reject a `coding-agent` invocation before `agent_start` when the required `coding-core` toolset cannot be assembled.

Toolset hints may constrain or supplement runtime selection. They must not silently remove the minimum `coding-core` capability from a `coding-agent` invocation unless the caller explicitly requests an invocation without that minimum capability and runtime accepts that specialized mode.

The default `coding-core` selection is the minimum initial capability for ordinary coding-agent invocations. Model-visible tool declaration snapshots may refresh between generation requests under [007 Tool Surface](../007-tool-surface/spec.md), but refresh must not silently remove the accepted minimum capability. Any later loss, degradation, or omission of required core tool declarations or bindings must remain observable.

## Runtime Assembly

Runtime resolves the `coding-agent` capability target during agent-invocation assembly. Capability target selection, toolset selection, context projection, resource surface wiring, and evidence sink wiring stay owned by their foundation specs.

`coding-agent` may contribute instruction context, attached context candidates, or summary context candidates to context assembly. [006 Context Assembly](../006-context-assembly/spec.md) owns whether those candidates become model-visible.

Instruction-file discovery, context-file discovery, skills, and memory are not part of the minimum coding-agent contract. Later implementations may feed those materials through context candidates, memory candidates, or capability contributions owned by [006 Context Assembly](../006-context-assembly/spec.md), [010 Memory System](../010-memory-system/spec.md), and [050 Capability Extensions](../050-capability-extensions/spec.md).

`coding-agent` uses the agent-invocation scoped tool surface assembled by runtime. Toolset expansion, refreshable tool declaration snapshots, and tool declaration visibility stay owned by [007 Tool Surface](../007-tool-surface/spec.md). Source identity, activation, availability, degraded state, and conflicts for contributed capability material stay owned by [050 Capability Extensions](../050-capability-extensions/spec.md).

Coding tools operate through the runtime-bound resource surface. [009 Resource Surface](../009-resource-surface/spec.md) owns resource decisions and their observability boundaries.

## Completion

A completed coding-agent invocation produces caller-facing final result material. At minimum, normal completion must provide a caller-facing answer and evidence-backed final material reachable through retained session, evidence, message, and material relationships.

The answer format is not stable in this spec. Product entrypoints may render summaries, changed files, validation output, or errors differently, but they must preserve access to evidence-backed final facts and material.

Self-evolution is a later capability domain. Any future evaluate-modify-retain-or-discard loop must be specified separately on top of durable evidence, session continuity, optional memory, toolsets, and resource boundaries. It is not part of this coding-agent slice.

## Related Topics

- [004 Runtime Contract](../004-runtime-contract/spec.md) defines agent-invocation assembly and capability target resolution.
- [006 Context Assembly](../006-context-assembly/spec.md) defines model-visible context projection for coding-agent inputs and candidates.
- [007 Tool Surface](../007-tool-surface/spec.md) defines agent-invocation scoped tool surface and toolset expansion semantics.
- [009 Resource Surface](../009-resource-surface/spec.md) defines resource decisions for working context and tool operations.
- [020 Interfaces](../020-interfaces/spec.md) defines caller-facing invocation, observation, and completion semantics.
- [040 Storage and Persistence](../040-storage-and-persistence/spec.md) defines material retrieval through session and evidence relationships.
- [050 Capability Extensions](../050-capability-extensions/spec.md) defines capability source, availability, and conflict boundaries.
- [100 Runtime Assembly](runtime-assembly.md) defines the first implementation slice assembly contract.
- [110 Coding Core Tools](../110-coding-core-tools/spec.md) defines the required `coding-core` toolset.
