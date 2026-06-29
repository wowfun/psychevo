---
name: 0002. Capability Extension Mechanism
status: proposed
date: 2026-05-31
psychevo_self_edit: deny
---

## Context

Psychevo needs a capability contribution mechanism before it adds more plugin,
gateway, IM, desktop, and peer-agent surfaces. A plugin or client integration
surface without a shared contribution model would make each source define its
own identity, selection, dispatch, permission, and evidence behavior. That
would make extension behavior harder to inspect and harder to evolve.

Pi shows why an agent runtime can become extensible: the runtime owns a host
facade, contributions are typed, source provenance is visible, and contributors
can debug registration without reading the core loop. Psychevo should learn
from that shape without inheriting open-ended authority. Extensions should not
rewrite provider payloads directly or mutate runtime-owned state outside a
bounded event contract.

Codex points toward a runtime-owned execution boundary. Raw capability identity
is separate from model-visible names, dispatch routes through one controlled
path, deferred capabilities can be discovered before they enter the visible
tool surface, and execution permissions stay separate from capability
selection. Its hook model also provides a useful lightweight evidence shape:
hook runs have started/completed summaries, while final tool results remain
tool evidence.

Hermes points toward contributor ergonomics. Manifests are understandable,
extension kinds are typed, registration happens through constrained host
facades, availability checks make missing dependencies visible, and plugin or
hook loading can be debugged without reading the core runtime.

This ADR does not define package installation, marketplaces, hot reload,
external source protocols, or a stable extension ABI. It records the mechanism
Psychevo should have before those product surfaces grow. Plugin product-system
decisions are recorded separately in ADR 0003 so this ADR can remain the
capability contribution mechanism.

## Decision

Psychevo will introduce a runtime-owned contribution mechanism that normalizes
capability sources, contributions, activation, availability, selection,
model-visible exposure, dispatch, hooks, and evidence.

The first shared model includes tools, skills, agents and peer agents,
providers, context, memory, and resources. Each category keeps its owning
semantics. Tool surface semantics stay with 007, AI protocol semantics stay
with 003, context projection stays with 006, resource decisions stay with 009,
memory semantics stay with 010, and skills stay with 055. The contribution
mechanism supplies shared source, selection, conflict, snapshot, and evidence
vocabulary; it does not replace the owning specs.

Specialized managers remain where they own real domain semantics. The provider
manager keeps provider and model resolution. Context assembly decides model
visibility. Resource policy decides resource access. Tool dispatch owns
executable tool contracts. Those managers still participate in the shared
source identity, selection, conflict, and evidence vocabulary.

## Mechanism Decisions

The contribution registry comes before plugin product work. Plugins, MCP
servers, bundled capabilities, trusted local scripts, Gateway inputs, IM
connectors, ACP session inputs, and peer-agent adapters are source forms. They
must feed the same contribution mechanism instead of bypassing runtime
normalization.

Session or invocation origin is not the same thing as capability source
provenance. Runtime records should be able to distinguish a root interface,
automation or exec entrypoint, Gateway or app-server entrypoint, internal run,
subagent or peer-agent run, and custom or unknown origin. Capability provenance
should identify the contributing source and the raw contribution or route.
Lineage must be linkable for subagents, peer agents, internal/background work,
and Gateway-originated sessions.

Every contribution needs raw identity and model-visible identity. Raw identity
covers source identity, source-local contribution or route identity, lifetime
or snapshot identity, and optional package, path, plugin, display, or lineage
metadata. Model-visible identity is the prompt-facing handle. Same visible-name
conflicts are rejected or namespaced by default. A source may request
replacement, but an owning policy must authorize it. There is no general
last-writer-wins override.

Direct and deferred exposure use a hybrid model. Runtime may keep catalogs,
candidate summaries, and search metadata current without showing every
capability to the model. An execution-capable deferred contribution must enter
the model-visible surface before the model can call it.

Agent-invocation scoped selection uses a frozen snapshot. Background refresh
may update catalogs, connection state, and availability facts, but it must not
change the model-visible or executable capability surface for an active
session. New or removed capabilities become visible after reset or in a new
session. Calls to a removed or unavailable raw route should fail as unavailable
or degraded through the owning boundary rather than silently changing the
visible surface.

Registry selection and execution permission are separate decisions. The
registry decides which contributions are active, available, conflict-resolved,
selected, and visible for an invocation. Permission and resource policy decide
whether a selected operation may execute.

Gateway, IM, ACP, web UI, and desktop clients provide session-scoped source
inputs. They may submit dynamic tool candidates or other scoped contributions,
but runtime validates identity, namespace, schema, conflicts, selection, and
snapshot membership. Client-provided capability metadata does not imply trust,
activation, selection, or permission approval.

Provider adapters may be contribution sources, but this ADR only covers their
identity, availability, selection, conflict, and evidence facts. Provider and
model resolution remain with the provider manager and the provider-neutral
protocol in 003.

Codex, OpenCode, and similar systems should enter as peer-agent or agent
contributions. They may be selectable, schedulable, or exposed as invocation
tools. They are not AI providers unless a provider manager explicitly adapts
them as providers under 003.

Resources are cataloged or listed by source-qualified raw route, then read
through explicit resource or tool operations governed by 009 and projected by
006. They are not injected into model context by discovery alone. Memory
providers are composite contribution sources: they may provide context
candidates, memory candidates or providers, tools, lifecycle observations, and
memory mutations, but each effect must pass through 006, 010, or 007 as
appropriate. External memory providers conflict by default; built-in memory may
remain additive unless 010 defines a composition rule.

## Hooks

Hooks are capability contributions that attach handlers to controlled runtime
lifecycle events. This ADR only requires hook declarations to enter the shared
contribution mechanism with source identity, selection state, conflict
diagnostics, and compact evidence.

ADR 0004 owns the hook event catalog, declaration shape, trust review,
execution semantics, payloads, event-scoped effects, and run summaries. Specs
053 and 140 define the hook authority and runtime slices against that ADR.

Hook contributions still obey the owning runtime boundaries. Provider protocol
semantics remain owned by 003. Context projection remains owned by 006. Tool
surface and dispatch remain owned by 007. Permission and sandbox decisions
remain owned by 041 and 045. A hook contribution does not grant provider,
permission, sandbox, registry, session-state, or future-snapshot authority by
being discovered.

## Evidence

Evidence should stay compact. Psychevo should record source identity, selected
contributions, omitted or degraded contributions that affected assembly,
conflicts that affected selection, visibility decisions, and dispatch trace
facts. It should not persist every discovered candidate by default.

Hook evidence is governed by ADR 0004. This ADR requires only that hook
evidence remains source-qualified and compact, and that final model-visible
tool results remain normal tool evidence.

## Worked Example

A future external source contributes a `repo_lint` tool.

First, runtime discovers the source and assigns raw source identity. The tool
contribution receives a raw contribution identity, proposed model-visible name,
schema metadata, availability signal, execution binding, lifetime or snapshot
metadata, and declared toolset membership.

Second, runtime evaluates activation, availability, and conflicts. If another
contribution already claims the visible name `repo_lint`, runtime does not
replace it silently. It either rejects the new visible name, exposes a
namespaced name, or applies an owner-authorized replacement policy.

Third, the tool may remain deferred. Runtime can include it in a searchable
catalog summary without adding it to the model-visible tool list. If search or
runtime policy activates it for the invocation, the tool declaration enters the
visible surface before the model may call it.

Fourth, the model calls the visible tool name. Dispatch resolves the visible
name back to raw identity, runs matching hook events through the runtime hook
module defined by ADR 0004, checks permission and resource policy, and invokes
the execution binding.

Fifth, Psychevo records compact evidence: selected source identity, visibility
decision, omitted conflicting candidates that affected selection, permission
outcome, hook run summaries, and the final model-visible dispatch result.

## Milestones

1. Normalize internal capabilities. Built-in tools, skills, agents, providers,
   context candidates, memory candidates, and resource candidates should flow
   through shared source, contribution, selection, snapshot, and evidence
   vocabulary without changing user-visible behavior.
2. Define the model-visible surface. Add direct and deferred exposure semantics,
   catalog refresh boundaries, and session reset semantics for visible
   capability snapshots.
3. Prepare for multiple source forms. MCP, plugin manifests, bundled packages,
   Gateway session inputs, ACP session inputs, IM connectors, and peer-agent
   adapters should fit the same source and contribution contract.
4. Route extension power through controlled paths. Hooks, availability checks,
   conflict reporting, dispatch, and evidence should pass through runtime or
   the owning specialized manager.

## Tradeoffs

This mechanism adds structure before Psychevo has a broad plugin surface. The
cost is migration work around current built-in capabilities. The benefit is
that future extensions inherit one identity, selection, permission, dispatch,
and evidence model.

The design limits convenient override behavior. Contributors may need to use
namespaces or request owner-authorized replacement instead of claiming an
existing visible name. That friction keeps invocation evidence and safety
review inspectable.

The hybrid direct/deferred model adds one more step before some tools can run.
It keeps prompts smaller and makes the visible execution surface inspectable for
each invocation.

Delegating hook specifics to ADR 0004 keeps this ADR focused on contribution
normalization. The cost is one more document to read when a capability source
contributes hooks. The benefit is that hook event, trust, execution, and
evidence rules can evolve without re-opening the whole contribution mechanism.

## Open Questions

The mechanism direction is settled in this ADR. Remaining questions are
implementation details:

- exact Rust types, API boundaries, and SDK facades for sources,
  contributions, registries, and snapshots
- concrete field names and persistence shape for source records, snapshot
  records, and evidence records
- hook review and UI rendering details that ADR 0004 leaves to later specs
- reset, refresh, and catalog UX for CLI, TUI, web UI, desktop app, Gateway,
  IM connectors, and ACP
- exact naming and validation rules for session-scoped dynamic tools and
  peer-agent invocation tools
