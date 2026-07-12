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
- gateway orchestration responsibilities
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
- Runtime is the Native Psychevo execution kernel. Native agent-invocation
  assembly, resource and tool surface wiring, context assembly, and Native
  durable evidence converge in `psychevo-runtime`.
- Gateway is the application kernel and caller-facing orchestration surface.
  Interactive entrypoints route thread context, immutable agent/runtime
  binding, turns, controls, queueing, interaction requests, delivery state,
  history projection, and observation through `psychevo-gateway` instead of
  reimplementing those semantics per surface.
- Native Psychevo Agents and external ACP Agents are equal execution Adapters
  behind one Gateway-owned Agent Session seam. ACP is an external protocol at
  that seam; it is not Psychevo's internal application interface and Native is
  not lowered through ACP.
- Transport is replaceable. CLI parsing, terminal rendering, stdin/stdout behavior, exit codes, and environment handling must remain outside the core runtime and lower layers.
- Large crate implementations should be organized internally by owned responsibility instead of collecting unrelated behavior in a single root source file. Root crate files may act as facades that re-export stable public surfaces while private modules keep implementation details near their owning boundary.

## Internal Module Layout

Large source files should be split by durable ownership boundaries, not by
mechanical line-count slices. Generated files, lockfiles, snapshots, and
baseline inventories are not ordinary module-layout targets.

Extracted implementation files must be named for the responsibility they own.
Placeholder split names such as `part_001.rs`, `chunk-a.ts`, or other purely
ordinal buckets are not acceptable refactor endpoints, even when they satisfy
line-count thresholds. If a file cannot be split further without obscuring the
owning boundary, the facade or entrypoint role must be explicit in the file name
or documented by the owning spec.

For ordinary source and specification files under `apps/`, `crates/`,
`packages/`, and `specs/`, production modules should normally remain below 900
lines after a structural refactor. Test modules should normally remain below
1200 lines. A file that exceeds those limits must either be a generated artifact
or a documented facade/entrypoint whose remaining size is explained by a stable
public boundary. Generated artifacts may be split only by changing their
generator or source schema organization; checked-in generated files are never
manually edited as a refactor shortcut.

Crate roots may act as facades. Established root-level re-exports should remain
stable unless the owning topic intentionally changes the public interface.
Private helper modules should use the narrowest practical visibility, normally
`pub(super)` or `pub(crate)`.

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
- capability-extension declaration acceptance and runtime extension registry assembly
- model context assembly
- durable execution records, persistence, and replay wiring
- the stable library surface for future non-CLI entry points

Must not know:
- CLI parsing, terminal rendering, stdin/stdout framing, or process exit behavior
- UI-specific interaction mechanics
- entry-point-specific modes that can be implemented separately
- transport source routing, IM-specific routing keys, Web/Desktop connection identity, or gateway queue ownership

`psychevo-runtime` may own shared interface-neutral command metadata when the
metadata must be projected by multiple product surfaces, such as CLI, TUI, ACP,
and future WebUI entrypoints. Runtime-owned command metadata describes command
identity, argument shape, status, and output kind; concrete parsing, terminal
rendering, editor protocol payloads, and process behavior remain owned by the
entrypoint crates.

### `psychevo-gateway`

Owns:
- the `ThreadApplication` Module used by every interactive caller
- the sole caller-facing typed turn request and its lowering into
  runtime-internal `RunOptions`; caller Adapters never construct `RunOptions`
  or the application queue envelope
- the `AgentSessionHost` Module and its Native and outbound ACP Adapters
- transport-neutral Thread/Turn orchestration over Native and ACP Agents
- source identity normalization and source-to-thread mapping
- active-turn queue, steer, interrupt, and reset coordination
- caller-facing permission and clarify request rendezvous
- canonical live event and item projection for product surfaces
- immutable Agent Definition and Runtime Profile binding
- delivery classification, product history ownership/fidelity, and interaction
  brokering
- outbound ACP process, connection, and session supervision

Must not own:
- agent loop behavior
- provider protocol behavior or provider/model resolution
- coding tool behavior
- runtime permission policy
- capability selection semantics
- context assembly semantics
- Native durable evidence schemas or Native replay semantics
- concrete CLI, TUI, ACP, Web, desktop, or IM rendering/protocol behavior

### `psychevo-acp`

Owns:
- inbound ACP server packaging over stdio for the first product slice
- ACP request and notification handling according to [027 ACP](../027-acp/spec.md)
- ACP projection of gateway/runtime sessions, observations, permissions, commands,
  auth, model/mode choices, config options, and MCP source inputs
- construction of gateway calls from ACP inputs

`psychevo-acp` is a caller-side Adapter. It must not own or be reused as the
outbound ACP Agent Adapter; the latter lives behind Gateway's Agent Session
seam and has the opposite protocol role.

Inbound ACP, CLI, TUI, Web/Desktop, and Channels submit turns through the same
`ThreadApplication.run_turn` Interface. Surface-specific environment,
presentation, and interaction choices are typed caller intent on that request;
runtime state handles, native session ids, internal delegates, and queue
delivery policy remain private to Gateway.

Must not own:
- agent loop behavior
- provider protocol behavior
- coding tool behavior
- runtime permission policy
- capability selection semantics
- durable record, persistence, or replay semantics
- CLI or TUI rendering behavior

### `psychevo-cli`

Owns:
- command-line argument parsing
- environment and process-level setup
- terminal/event rendering
- exit code behavior
- construction of gateway calls from CLI inputs

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
psychevo-cli -> psychevo-gateway -> psychevo-runtime -> psychevo-agent-core -> psychevo-ai
psychevo-acp -> psychevo-gateway -> psychevo-runtime -> psychevo-agent-core -> psychevo-ai
                                 -> outbound ACP Agent processes
```

Allowed dependency rules:
- `psychevo-cli` may depend on `psychevo-runtime`.
- `psychevo-acp` may depend on `psychevo-runtime`.
- `psychevo-cli` and `psychevo-acp` may depend on `psychevo-gateway`.
- `psychevo-gateway` may depend on `psychevo-runtime`.
- `psychevo-gateway` may depend on the ACP SDK and launch configured outbound
  ACP Agent processes through structured process configuration.
- `psychevo-runtime` may depend on `psychevo-agent-core` and `psychevo-ai`.
- `psychevo-agent-core` may depend on `psychevo-ai`.
- `psychevo-ai` must not depend on higher Psychevo crates.

Allowed direct interaction rules:
- Interactive `psychevo-cli` and `psychevo-acp` work should interact with `psychevo-gateway` for thread/turn orchestration and may interact with `psychevo-runtime` for non-interactive administrative helpers that are not gateway semantics.
- `psychevo-gateway` may directly interact with `psychevo-runtime` through its
  Native Adapter and with external ACP Agents through its outbound ACP Adapter.
- Workbench, Channels, CLI/TUI, and inbound `psychevo-acp` must interact with
  the same Gateway application uses cases and must not select an Adapter by
  implementation name.
- `psychevo-runtime` may directly interact with `psychevo-agent-core`, `psychevo-ai`, agent-invocation scoped tool surface bindings, and runtime-owned durable records.
- `psychevo-runtime` may accept capability-extension declarations and assemble
  the runtime extension registry for an invocation.
- `psychevo-runtime` may implement and assemble built-in capability modules, such as capability specs that explicitly place their implementation in runtime. Concrete capability behavior remains owned by those capability specs.
- `psychevo-runtime` may own SQLite persistence for the first implementation slice without adding a new crate.
- `psychevo-agent-core` may directly interact with `psychevo-ai` and tool abstractions supplied by runtime.

Agent definitions and subagent orchestration are first-class orchestration
concepts, not core loop concepts. `psychevo-runtime` owns their resolution in
the first implementation slice; a future agent-orchestration crate may own that
layer as long as dependency direction and transport separation remain intact.

Prohibited dependency rules:
- lower layers must not depend on higher layers
- `psychevo-agent-core` must not depend on `psychevo-runtime`, `psychevo-cli`, or `psychevo-acp`
- `psychevo-runtime` must not depend on `psychevo-gateway`, `psychevo-cli`, or `psychevo-acp`
- `psychevo-gateway` must not depend on `psychevo-cli` or `psychevo-acp`
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
- [021 Gateway](../021-gateway/spec.md) defines transport-neutral gateway orchestration.
- [030 State and Data Model](../030-state-and-data-model/spec.md) defines cross-cutting semantic state relationships.
- [031 Storage and Persistence](../031-storage-and-persistence/spec.md) defines storage and persistence boundaries.
- [050 Capability Extensions](../050-capability-extensions/spec.md) defines
  capability-extension source, declaration, and registry boundaries resolved by
  runtime.
- [051 Agents](../051-agents/spec.md) defines reusable agent definitions and selected-agent orchestration semantics.
- [100 Coding Agent](../100-coding-agent/spec.md) defines a runtime-owned built-in capability target.
- [027 ACP](../027-acp/spec.md) defines the Agent Client Protocol boundary.
- [230 pevo-acp](../230-pevo-acp/spec.md) defines the concrete ACP server
  packaging for the `pevo` product.
