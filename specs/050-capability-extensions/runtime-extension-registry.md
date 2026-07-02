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

`ExtensionData` is the scoped store for extension-private state.

Psychevo may keep separate stores for session, thread, and turn scopes when an
effect needs durable or scoped extension state. Contributors receive only the
store scopes and typed host inputs they are allowed to use. Contributors do not
receive mutable access to core runtime state, provider credentials, permission
policy, sandbox policy, transcript storage, or future registry snapshots.

## Contributor Slots

The intended contributor slots are:

- `McpServerContributor`: resolves runtime MCP servers and preserves source
  provenance before MCP tools enter the tool surface.
- `ContextContributor`: contributes prompt fragments or world-state fragments
  through context assembly.
- `ThreadLifecycleContributor`: observes or seeds thread-level lifecycle state.
- `TurnLifecycleContributor`: observes or seeds turn-level lifecycle state.
- `TurnInputContributor`: contributes turn-local user-context fragments.
- `ConfigContributor`: observes committed effective configuration changes.
- `TokenUsageContributor`: observes model token-usage checkpoints.
- `ToolContributor`: exposes native tools owned by an extension feature.
- `ToolLifecycleContributor`: observes accepted tool execution without owning
  tool payload policy.
- `ApprovalReviewContributor`: can claim a rendered approval-review prompt and
  return a review decision.
- `TurnItemContributor`: post-processes parsed turn items through an ordered
  host-owned slot.

Psychevo may add contributor slots only when a new effect cannot be expressed
through these slots without making an existing slot ambiguous. New slots must
remain host-owned, typed, scoped, and evidence-friendly.

## Composition

Runtime features compose from contributors rather than from source forms.

Examples:

- Skills may install context contributors, turn-input contributors, tool
  contributors, and lifecycle contributors.
- Memory may install context contributors, tool contributors, lifecycle
  contributors, and token-usage contributors.
- MCP inputs primarily install `McpServerContributor` entries; resulting MCP
  tools still pass through the tool surface.
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
