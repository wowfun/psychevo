---
name: 050. Runtime Extension Registry
psychevo_self_edit: deny
---

Define the Codex-aligned runtime interface for extension effects.

## Scope

- host-owned `ExtensionRegistry`
- scoped `ExtensionData`
- typed contributor installation
- invocation-scoped frozen registry views
- compact source-qualified registry evidence

Out of scope:
- public extension SDKs or ABI stability
- plugin manifest schemas, marketplace records, worker wire messages, or CLI
  commands
- concrete hook payloads, tool schemas, provider protocols, context schemas, or
  storage schemas
- third-party in-process registration APIs

## Registry Model

`ExtensionRegistry` is the host-owned runtime interface for extension effects.
It is built by Psychevo host code from effective configuration, selected
sources, plugin policy, managed policy, interface inputs, and current runtime
facts.

Sources do not mutate the registry directly. A built-in feature, plugin
package, MCP declaration, selected agent, skill root, profile setting, project
setting, Gateway input, ACP session input, or managed policy may cause
contributors to be installed only through host-owned mapping code.

Once built for an invocation or session scope, callers interact with typed
contributor lists rather than source-specific loaders. Source discovery,
manifest parsing, policy overlay, compatibility mapping, package installation,
MCP startup, skill loading, hook trust, and provider setup can be complex, but
accepted effects must reach runtime through typed contributors or owning
runtime modules.

Host code must assemble those accepted effects through one runtime-owned
extension assembly step before downstream runtime modules consume them. The
assembly input includes the invocation cwd, effective environment, plugin
policy, selected capability roots, static MCP inputs, and static runtime tools.
The assembly output contains a frozen `ExtensionRegistry` plus owning-module
inputs for skills, agents, hooks, toolsets, warnings, and compact contribution
projection facts.

Run entrypoints, reload/reconstruction entrypoints, and child-agent entrypoints
must consume this assembly output instead of each reloading plugins, selected
capability roots, MCP inputs, and runtime tools independently. This preserves a
single source of truth for Codex-style host-owned extensibility while still
letting skills, hooks, MCP, agents, and tool surface own their final semantics.

## ExtensionData

`ExtensionData` is the scoped store for extension-private state. The current
registry owns session and thread stores because installed built-in contributors
use those lifetimes. Turn storage is added only with a real turn-scoped owner
and call site.

Contributors receive only the store scopes and typed host inputs they are
allowed to use. Contributors do not receive mutable access to core runtime
state, provider credentials, permission policy, sandbox policy, transcript
storage, or future registry snapshots.

## Contributor Slots

The registry exposes only contributor slots with a current host call site and
at least one real owner:

- `McpServerContributor`: resolves source-qualified MCP server candidates for
  one frozen thread selection before MCP startup.
- `ToolContributor`: exposes native tool bindings owned by a built-in feature
  or accepted plugin worker.

Delegated capability sessions are currently owned directly by the Gateway
broker. A `ThreadLifecycleContributor` is not exposed until that broker and a
second real owner share a typed lifecycle call site; an identity-only slot
would provide no usable extensibility.

Contributor inputs and outputs are typed behavior, not marker identities. The
host invokes contributors in registration order. When an owning module permits
replacement, a later contribution for the same owned identity wins and the
replacement remains visible in source-qualified diagnostics.

Psychevo must not publish empty marker traits for hypothetical context, turn,
config, token, tool-lifecycle, approval, or item effects. A new slot is added
only when an existing feature owns the effect, the host has a concrete call
site, and its behavior can be tested through the registry interface. This is a
pre-release replacement of the earlier shallow marker surface, not a stable
third-party Rust interface.

## Composition

Runtime features compose from contributors rather than from source forms.

Examples:

- MCP inputs primarily install `McpServerContributor` entries; resulting MCP
  tools still pass through the tool surface.
- An exported Psychevo MCP server is an interface adapter; it uses runtime
  entrypoints and does not install contributors or feed exported tools back
  into the accepted invocation surface.
- Plugin packages declare resources and extension sources; host code maps
  accepted declarations into contributors or owning modules.
- Hooks are handled by the runtime hook module; hook effects enter lifecycle,
  tool, approval, context, feedback, and diagnostic routes only through
  event-scoped contracts.

## Runtime Boundaries

The registry does not grant ambient authority.

Contributors must not rewrite raw provider payloads, mutate provider
credentials, persist permission grants, widen sandbox authority, replace future
registry views, write directly to transcript facts, or inject model context
outside context assembly.

Tool exposure, dispatch, permission checks, resource checks, provider
resolution, context projection, persistence, and hook execution remain owned by
their runtime modules. Contributors may provide inputs or observations to those
modules, but the owning module decides the final effect.

The registry may carry tools whose model exposure is `direct`, `deferred`, or
`hidden`. `direct` tools are eligible for the next generation-request tool
snapshot. `deferred` tools have execution bindings in the accepted invocation
surface but are not model-visible until the agent loop activates them through
the tool router. `hidden` tools are executable only through host-owned routes
that explicitly bypass model-visible dispatch.

Tool exposure remains a property of the accepted execution binding plus
runtime source-family policy, not of a plugin manifest. When synthetic
`tool_search` is enabled, direct MCP and plugin-worker bindings may be adapted
to `deferred`; host-owned runtime tools stay direct by default. Bindings that
already declare `deferred` or `hidden` keep that exposure.

Registry selection and execution permission are separate. A selected tool,
hook, MCP server, context fragment, approval reviewer, or turn item contributor
is not automatically allowed to execute sensitive work.

## Snapshots

An accepted invocation uses a frozen registry view. Background refresh may
discover new source facts, but it must not change the model-visible or
executable surface already accepted for the active turn.

A later turn or session may build a new registry from fresh facts. Calls to
removed or unavailable raw routes should fail as unavailable or degraded through
the owning module rather than silently changing the accepted view.

## Evidence

Registry evidence should stay compact and source-qualified. Psychevo records
facts that affected assembly or execution, such as:

- selected contributors
- omitted unavailable contributors
- degraded contributors that changed assembly
- visible-name conflicts
- approval-review decisions
- hook run summaries
- tool dispatch summaries
- source identities for accepted effects

Psychevo should not persist every discovered candidate, every manifest field,
or every full contributor payload by default. Detailed inventories belong in
diagnostic surfaces.

## Related Topics

- [050 Capability Extensions](spec.md) defines the broad source and declaration vocabulary.
- [004 Runtime Contract](../004-runtime-contract/spec.md) defines invocation assembly.
- [005 Durable Evidence](../005-durable-evidence/spec.md) defines durable evidence semantics.
- [006 Context Assembly](../006-context-assembly/spec.md) defines context projection.
- [007 Tool Surface](../007-tool-surface/spec.md) defines tool declaration and dispatch semantics.
- [053 Hooks](../053-hooks/spec.md) defines hook authority.
- [054 Plugins](../054-plugins/spec.md) defines plugin package boundaries.
- [056 MCP](../056-mcp/spec.md) defines MCP source and dispatch semantics.
