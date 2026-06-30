---
name: 0002. Runtime Extension Registry
status: proposed
date: 2026-06-30
psychevo_self_edit: deny
---

## Context

Psychevo needs extension power without letting every source form invent its own
runtime path. Skills, memory, MCP servers, plugins, hooks, tools, provider
adjuncts, and future agent integrations all need to influence an invocation,
but they should not each receive a separate way to change prompts, tools,
permissions, lifecycle, or evidence.

The simplest long-term shape is one host-owned extension interface with typed
contributor slots. Codex demonstrates this model well: the host builds an
`ExtensionRegistry`, extensions register typed contributors, contributors run
only at host-owned lifecycle points, and extension-private state lives in
`ExtensionData` rather than in core runtime objects.

Psychevo should align with that concept directly. Capability names, packages,
and source forms are product concerns. Runtime authority is expressed through
typed contributors.

## Decision

Psychevo will use a host-owned `ExtensionRegistry` as the runtime interface for
extension effects.

The registry is built by Psychevo host code from the effective configuration,
selected sources, and product policy for a session or invocation. After the
registry is built, callers interact with typed contributor lists instead of
calling source-specific systems directly. A plugin, skill root, MCP server,
profile setting, project setting, built-in feature, or managed policy may cause
contributors to be installed, but those sources do not mutate the registry
themselves.

`ExtensionData` is the scoped store for extension-private state. Psychevo keeps
separate stores for scopes such as session, thread, and turn when an effect
needs durable extension state. Contributors receive the store scopes they are
allowed to use, plus typed host inputs, instead of receiving mutable access to
the runtime.

The registry interface is intentionally smaller than the implementation behind
it. Source discovery, policy, compatibility parsing, package installation,
MCP startup, skill loading, hook trust, and provider setup can be complex, but
the runtime should see their accepted effects through typed contributors.

## Contributor Slots

The intended registry has Codex-aligned contributor categories:

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

Psychevo may add contributor categories only when a new effect cannot be
expressed through these slots without making an existing slot shallow or
ambiguous. New categories must remain host-owned, typed, and scoped.

## Composition Model

Extensions are composed from contributors rather than from broad capability
categories.

Examples:

- A skill system is a combination of context contributors, turn-input
  contributors, tool contributors, and optional lifecycle contributors.
- A memory system is a combination of context contributors, tool contributors,
  lifecycle contributors, and token-usage observers.
- An MCP integration is primarily an `McpServerContributor`; any resulting
  tools still enter the normal tool surface.
- A plugin package is a source of declarations that host code maps into
  contributors.
- Hooks are a specialized runtime module whose declarations are supplied by
  sources and whose effects enter lifecycle, tool, approval, context, and
  diagnostic slots.

This keeps the product model understandable: sources provide possible effects;
the host decides which effects become typed contributors; the runtime invokes
contributors only at known points.

## Runtime Boundaries

The `ExtensionRegistry` does not grant ambient authority.

Contributors must not rewrite raw provider payloads, mutate provider
credentials, persist permission grants, widen sandbox authority, replace
future registry snapshots, write directly to durable transcript facts, or
inject model context outside context assembly.

Tool exposure, dispatch, permission checks, resource checks, provider
resolution, context projection, and persistence remain owned by their runtime
modules. Contributors may supply inputs or observations to those modules, but
the owning module decides the final effect.

Registry selection and execution permission are separate. A selected tool,
hook, MCP server, context fragment, or approval reviewer is not automatically
allowed to execute sensitive work.

## Snapshots And Evidence

An accepted invocation uses a frozen extension view. Background refresh may
discover new source facts, but it must not change the model-visible or
executable surface already accepted for the active turn. A later turn or
session may build a new registry from fresh facts.

Evidence should stay compact and source-qualified. Psychevo records the facts
that affected assembly or execution: selected contributors, omitted or degraded
contributors that changed the result, visible-name conflicts, approval-review
decisions, hook summaries, tool dispatch summaries, and source identities.

Psychevo should not persist every discovered candidate, every manifest field,
or every full contributor payload by default. Detailed inventories belong in
diagnostic surfaces, not ordinary transcript history.

## Non-Goals

This ADR does not define a public extension SDK, package format, marketplace,
worker protocol, hook schema, storage schema, tool schema, provider protocol,
or UI. Those are product and spec decisions layered above or below the runtime
extension registry.

It also does not require every future feature to become externally
extensible. Built-in features should use the same internal contributor model
when that improves locality and evidence, even if no third-party extension
point is exposed.

## Consequences

This design makes extension behavior less ad hoc. The cost is that source
forms must be adapted into contributors before they affect a run. The benefit
is one inspectable runtime interface for prompts, tools, MCP servers,
lifecycle, approvals, turn items, and diagnostics.

The registry becomes the main architecture decision; plugins, skills, hooks,
and MCP are source or module designs that feed it. That keeps Psychevo closer
to Codex's conceptual model while leaving product-specific policy outside the
core interface.
