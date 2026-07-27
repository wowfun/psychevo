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
- Framework is the Psychevo application and execution kernel. Thread and Turn
  authority, Native agent-invocation assembly, resource and tool surface
  wiring, context assembly, durable evidence, interaction rendezvous, and
  accepted-work supervision converge in the public `psychevo` crate.
- Gateway is a product Adapter host, not an application kernel. Web, desktop,
  channel, automation, App Server, and outbound ACP concerns live in
  `psychevo-gateway`, while all Thread and Turn use cases call an injected
  `psychevo::Client`.
- Native Psychevo Agents and external ACP Agents are equal execution Adapters
  behind one Framework-owned Agent Session seam. ACP is an external protocol at
  that seam; it is not Psychevo's internal application interface and Native is
  not lowered through ACP.
- Transport is replaceable. CLI parsing, terminal rendering, stdin/stdout behavior, exit codes, and environment handling must remain outside the core runtime and lower layers.
- Large crate implementations should be organized internally by owned responsibility instead of collecting unrelated behavior in a single root source file. Root crate files expose named module namespaces; they must not become item-level compatibility facades that obscure ownership.
- Optional compilation seams must correspond to a real dependency or startup-cost difference. Product taxonomy alone is not a reason to introduce a Cargo feature.
- `psychevo-gateway` owns one default-on `native-channels` seam for native
  channel adapter dependencies. `psychevo-cli` forwards that feature and omits
  the channel setup command when built without default features. The default
  product remains complete; the no-default build is an executable dependency
  graph invariant, not a separate edition.

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

Crate roots expose named module namespaces and only the smallest truly
crate-wide primitives, such as the crate error and result types. Public domain
types remain in their owning modules instead of being item-re-exported from the
crate root. Private helper modules use the narrowest practical visibility,
normally `pub(super)` or `pub(crate)`.

`psychevo` exposes the small high-level Framework Interface defined by
[080 Framework and SDK](../080-sdk/spec.md). Run assembly, provider
configuration resolution, SQLite-backed state, event projection, context
pruning, and built-in tool assembly remain internal Framework modules.
`StateRuntime` is the single internal state Module. SQLite connections, schema
helpers, and transaction helpers remain implementation details; no public store
handle, repository family, or pass-through state facade is added.

`application.rs` declares the public Framework vocabulary and facade. Private
modules own Application lifecycle, Thread operations, Turn execution, Agent
Session adaptation, accepted-work supervision and serialization, interaction
rendezvous, and bounded event journaling as defined by
[Runtime Scalability And Ownership](runtime-scalability.md). Implementations
move behind those seams; callers still learn one Framework Interface. This is
an ownership split, not a second Application interface.

The production `StateRuntime` Interface is asynchronous and backed by one
runtime-owned SQLite connection pool. Callers await semantic state operations;
they never borrow a connection, hold a database lock, select a pool member, or
implement busy retry. In-memory runtime state that performs no database I/O may
remain synchronous behind the same Module. The pool does not create a second
database, read repository, actor, or storage ownership layer.

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

### `psychevo`

Owns:
- `Application`, its cloneable in-process `Client`, and Thread/Turn authority
- accepted-turn supervision, controls, queueing, and interaction rendezvous
- Native and injected external Agent Session Adapter coordination
- agent-invocation assembly
- built-in runtime capability modules specified by capability specs
- resource surface wiring
- agent-invocation scoped tool surface assembly
- direct capability-extension declaration assembly into owning runtime modules
- model context assembly
- durable execution records, persistence, replay wiring, and canonical
  Thread/Turn projection
- the stable Rust Framework and SDK surface

Must not know:
- CLI parsing, terminal rendering, stdin/stdout framing, or process exit behavior
- UI-specific interaction mechanics
- transport framing, IM-specific routing keys, or Web/Desktop connection identity

`psychevo` may own shared interface-neutral command metadata when the
metadata must be projected by multiple product surfaces, such as CLI, TUI, ACP,
and future WebUI entrypoints. Runtime-owned command metadata describes command
identity, argument shape, status, and output kind; concrete parsing, terminal
rendering, editor protocol payloads, and process behavior remain owned by the
entrypoint crates.

### `psychevo-gateway`

Owns:
- Web, desktop, channel, automation, and App Server transport Adapters
- wire identity normalization and mapping to Framework Thread identifiers
- product and protocol event projection from typed Framework events
- transport connection ownership, authentication, delivery, and reconnect
- outbound ACP process and connection supervision behind a Framework
  `AgentSessionAdapter`
- the private Gateway protocol schema crate and generated wire types

Must not own:
- Thread or Turn authority, accepted-work supervision, or a parallel queue
- agent loop behavior
- provider protocol behavior or provider/model resolution
- coding tool behavior
- runtime permission policy
- capability selection semantics
- context assembly semantics
- durable evidence schemas or replay semantics
- concrete CLI, TUI, ACP, Web, desktop, or IM rendering/protocol behavior

### `psychevo-acp`

Owns:
- inbound ACP server packaging over stdio for the first product slice
- ACP request and notification handling according to [027 ACP](../027-acp/spec.md)
- ACP projection of Framework Threads, observations, permissions, commands,
  auth, model/mode choices, config options, and MCP source inputs
- construction of Framework Client calls from ACP inputs

`psychevo-acp` is a caller-side Adapter. It must not own or be reused as the
outbound ACP Agent Adapter; the latter is implemented by Gateway behind the
Framework Agent Session seam and has the opposite protocol role.

Inbound ACP, CLI, TUI, Web/Desktop, Channels, and Automations submit turns
through the same `psychevo::Client` Interface. Surface identity, environment
facts, presentation, and interaction choices are typed caller intent; state
handles, Native session ids, internal delegates, run options, event persistence
sinks, and queue delivery policy remain private to Framework.

Starting a Turn returns accepted public Thread/Turn identity plus a
`TurnHandle`. Application supervision, not ownership of that handle, owns the
accepted Turn. Web may return acceptance without awaiting completion, while
synchronous callers may await the same handle. Dropping a handle never cancels
accepted work.

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
- construction of Framework Client calls from CLI inputs

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
psychevo-agent-core -> psychevo-ai
psychevo -> psychevo-agent-core + psychevo-ai
psychevo-gateway -> psychevo + psychevo-gateway-protocol
                  -> outbound ACP Agent processes
psychevo-acp -> psychevo
psychevo-cli -> psychevo + psychevo-gateway + psychevo-acp
```

Allowed dependency rules:
- `psychevo-cli`, `psychevo-acp`, and `psychevo-gateway` may depend on
  `psychevo`.
- `psychevo-cli` may depend on the private `psychevo-gateway` and
  `psychevo-acp` product crates to package their commands.
- `psychevo-acp` must not depend on `psychevo-gateway`.
- `psychevo-gateway` may depend on the ACP SDK and launch configured outbound
  ACP Agent processes through structured process configuration.
- `psychevo` may depend on `psychevo-agent-core` and `psychevo-ai`.
- `psychevo-agent-core` may depend on `psychevo-ai`.
- `psychevo-ai` must not depend on higher Psychevo crates.

Allowed direct interaction rules:
- Interactive CLI/TUI, Gateway, and inbound ACP work interacts with the same
  `psychevo::Client` use cases.
- `psychevo-gateway` may provide an outbound ACP Agent Session Adapter to
  `psychevo::Application`.
- Workbench, Channels, CLI/TUI, and inbound `psychevo-acp` must not select an
  Agent Session Adapter by
  implementation name.
- `psychevo` may directly interact with `psychevo-agent-core`, `psychevo-ai`,
  agent-invocation scoped tool surface bindings, and Framework-owned durable
  records.
- `psychevo` may accept capability-extension declarations and assemble
  direct owning-module assembly for an invocation.
- `psychevo` may implement and assemble built-in capability modules, such as
  capability specs that explicitly place their implementation in Framework.
  Concrete capability behavior remains owned by those capability specs.
- `psychevo` may own SQLite persistence without adding a new crate.
- `psychevo-agent-core` may directly interact with `psychevo-ai` and Tool
  abstractions supplied by Framework.

Agent definitions and subagent orchestration are first-class orchestration
concepts, not core loop concepts. `psychevo` owns their resolution in
the first implementation slice; a future agent-orchestration crate may own that
layer as long as dependency direction and transport separation remain intact.

Prohibited dependency rules:
- lower layers must not depend on higher layers
- `psychevo-agent-core` must not depend on `psychevo`, `psychevo-cli`, or `psychevo-acp`
- `psychevo` must not depend on `psychevo-gateway`, `psychevo-cli`, or `psychevo-acp`
- `psychevo-gateway` must not depend on `psychevo-cli` or `psychevo-acp`
- `psychevo-acp` must not depend on `psychevo-gateway`, `psychevo-agent-core`,
  or `psychevo-ai`
- `psychevo-cli` and `psychevo-gateway` must not depend directly on
  `psychevo-agent-core` or `psychevo-ai`
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
- [080 Framework and SDK](../080-sdk/spec.md) defines the public Framework,
  Rust SDK, App Server, and Python SDK boundaries.
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
