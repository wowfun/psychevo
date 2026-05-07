---
name: 001. Architecture
psychevo_self_edit: deny
---

Define Psychevo's system-level architecture boundaries.

## Scope

- Rust workspace structure for the agent substrate
- primary architecture components and their Rust crate mapping
- component ownership boundaries
- runtime coordination responsibilities
- allowed direct interaction paths
- dependency direction between architecture layers

Out of scope:
- event, trace, provider, tool, session, or persistence schemas
- concrete trait, function, CLI, or file format APIs
- built-in tool names or tool behavior
- replay, evaluation, memory, skill, extension, or self-evolution behavior

## Architecture Principles

- Layering over bundling. Psychevo separates provider protocol, agent execution, runtime assembly, persistence, and transport instead of bundling product concerns into lower layers.
- Component specialization. Each primary architecture component owns one system-level responsibility area and must not absorb responsibilities from adjacent components.
- Runtime is the coordination and library surface. Session coordination, agent-invocation assembly, resource surface wiring, tool surface assembly, context assembly, durable evidence, persistence, and replay wiring converge in `psychevo-runtime`; non-CLI entry points should depend on runtime libraries directly instead of routing through command-line transport.
- Transport is replaceable. CLI parsing, terminal rendering, stdin/stdout behavior, exit codes, and environment handling must remain outside the core runtime and lower layers.
- Large crate implementations should be organized internally by owned responsibility instead of collecting unrelated behavior in a single root source file. Root crate files may act as facades that re-export stable public surfaces while private modules keep implementation details near their owning boundary.

## Internal Module Layout

`psychevo-runtime` may expose public module namespaces for its runtime-owned
responsibility areas, such as run assembly, provider configuration resolution,
SQLite-backed state, event projection, context pruning, and built-in tool
assembly. Root-level re-exports may preserve established caller paths while the
module layout makes ownership boundaries explicit.

`psychevo-cli` should keep process and terminal concerns in transport-owned
modules. CLI argument parsing, environment/path setup, command handlers, and
TUI rendering or event handling may be split into internal modules, but agent
execution, provider behavior, resource rules, and durable persistence semantics
must remain in lower layers.

## Primary Architecture Components

The primary architecture components are the Rust workspace crates listed below. Each component has an ownership boundary and prohibited knowledge boundary.

### `psychevo-ai`

Owns:
- model and provider protocol abstractions
- provider request/response normalization
- fake provider support for deterministic local validation
- real provider integration boundaries

Must not know:
- agent loop policy
- concrete coding tools
- runtime resource surface policy
- sessions, traces, replay, evaluation, or self-evolution
- CLI or terminal behavior

### `psychevo-agent-core`

Owns:
- model-agnostic agent execution
- agent lifecycle events
- tool traits and tool execution hooks
- stop conditions, turn limits, and abort handling

Must not know:
- concrete coding tools
- runtime resource surface policy
- durable trace or session storage
- context assembly policy outside the agent loop
- evaluation, memory, skill generation, or self-evolution
- CLI or terminal behavior

### `psychevo-runtime`

Owns:
- session coordination
- agent-invocation assembly
- built-in runtime capability modules specified by capability specs
- resource surface wiring
- agent-invocation scoped tool surface assembly
- capability extension resolution into agent-invocation scoped selections
- model context assembly
- durable execution records, persistence, and replay wiring
- the stable library surface for future non-CLI entry points

Must not know:
- CLI parsing, terminal rendering, stdin/stdout framing, or process exit behavior
- UI-specific interaction mechanics
- entry-point-specific modes that can be implemented separately

### `psychevo-cli`

Owns:
- command-line argument parsing
- environment and process-level setup
- terminal/event rendering
- exit code behavior
- construction of the runtime from CLI inputs

Must not own:
- agent loop behavior
- provider protocol behavior
- coding tool behavior
- resource surface rules
- durable record, persistence, or replay semantics
- long-lived business logic

## Dependency Direction

Dependencies between primary architecture components must point inward:

```text
psychevo-cli -> psychevo-runtime -> psychevo-agent-core -> psychevo-ai
```

Allowed dependency rules:
- `psychevo-cli` may depend on `psychevo-runtime`.
- `psychevo-runtime` may depend on `psychevo-agent-core` and `psychevo-ai`.
- `psychevo-agent-core` may depend on `psychevo-ai`.
- `psychevo-ai` must not depend on higher Psychevo crates.

Allowed direct interaction rules:
- `psychevo-cli` may directly interact with `psychevo-runtime` only.
- `psychevo-runtime` may directly interact with `psychevo-agent-core`, `psychevo-ai`, agent-invocation scoped tool surface bindings, and runtime-owned durable records.
- `psychevo-runtime` may resolve capability extension contributions into agent-invocation scoped selections.
- `psychevo-runtime` may implement and assemble built-in capability modules, such as capability specs that explicitly place their implementation in runtime. Concrete capability behavior remains owned by those capability specs.
- `psychevo-runtime` may own SQLite persistence for the first implementation slice without adding a new crate.
- `psychevo-agent-core` may directly interact with `psychevo-ai` and tool abstractions supplied by runtime.

Prohibited dependency rules:
- lower layers must not depend on higher layers
- `psychevo-agent-core` must not depend on `psychevo-runtime` or `psychevo-cli`
- `psychevo-runtime` must not depend on `psychevo-cli`
- business logic must not be introduced into `psychevo-cli`

## Related Topics

- [000 Foundation](../000-foundation/spec.md) defines the upstream project foundation and implementation-neutral principles.
- [002 Agent Execution](../002-agent-execution/spec.md) defines agent-core execution semantics and core event families.
- [003 AI Protocol](../003-ai-protocol/spec.md) defines provider-neutral generation semantics for `psychevo-ai`.
- [004 Runtime Contract](../004-runtime-contract/spec.md) defines session coordination, agent-invocation assembly, and evidence sink wiring.
- [005 Durable Evidence](../005-durable-evidence/spec.md) defines durable evidence semantics for sessions and agent invocations.
- [006 Context Assembly](../006-context-assembly/spec.md) defines model context assembly and transformation boundaries.
- [007 Tool Surface](../007-tool-surface/spec.md) defines agent-invocation scoped tool surface semantics.
- [008 Session Continuity](../008-session-continuity/spec.md) defines the session boundary for continuity and persistence.
- [009 Resource Surface](../009-resource-surface/spec.md) defines runtime-owned resource surface and resource decision semantics.
- [010 Memory System](../010-memory-system/spec.md) defines optional memory boundaries outside architecture layering.
- [020 Interfaces](../020-interfaces/spec.md) defines caller-facing interface layer semantics.
- [030 State and Data Model](../030-state-and-data-model/spec.md) defines cross-cutting semantic state relationships.
- [040 Storage and Persistence](../040-storage-and-persistence/spec.md) defines storage and persistence boundaries.
- [050 Capability Extensions](../050-capability-extensions/spec.md) defines capability contribution boundaries resolved by runtime.
- [100 Coding Agent](../100-coding-agent/spec.md) defines a runtime-owned built-in capability target.
