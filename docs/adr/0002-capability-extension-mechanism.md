---
name: 0002. Capability Extension Mechanism
status: Proposed
date: 2026-05-23
psychevo_self_edit: deny
---

## Context

Psychevo needs a capability contribution mechanism before it adds more plugin
surfaces. A plugin system without a shared contribution model would make each
new source define its own identity, selection, dispatch, permission, and
evidence behavior. That would make extension behavior harder to inspect and
harder to evolve.

Codex points toward a runtime-owned boundary: raw capability identity is
separate from model-visible names, dispatch routes through one controlled path,
deferred capabilities can be discovered before they enter the visible tool
surface, and execution permissions stay separate from capability selection.

Hermes points toward contributor ergonomics: manifests are understandable,
extension kinds are typed, registration happens through constrained host
facades, availability checks make missing dependencies visible, and plugin or
hook loading can be debugged without reading the core runtime.

This ADR does not define package installation, marketplaces, hot reload,
external source protocols, or a stable extension ABI. It defines the mechanism
Psychevo should have before those product surfaces grow.

## Proposed Direction

Psychevo should introduce a runtime-owned contribution mechanism that normalizes
capability sources, contributions, activation, availability, selection,
model-visible exposure, dispatch, hooks, and evidence.

The first shared model should cover tools, skills, agents, and providers.
Context, memory, and resource contributions should align later, after the first
four capability classes prove the boundary.

Specialized managers should remain where they own real domain semantics. The
provider registry, for example, should keep provider and model resolution
responsibilities. It should still participate in the same source identity,
selection, conflict, and evidence vocabulary as tools, skills, and agents.

## Mechanism Decisions

The contribution registry comes before plugin product work. Plugins, MCP
servers, bundled capabilities, and trusted local scripts are source forms. They
should feed the same contribution mechanism instead of bypassing it.

Every contribution needs a stable raw identity and a model-visible name. The
raw identity is used for provenance, conflict handling, dispatch, and evidence.
The model-visible name is the prompt-facing handle. Same visible-name conflicts
must be rejected, omitted, namespaced, or resolved by an explicit replacement
policy. Silent override is not acceptable.

Direct and deferred exposure should use a hybrid model. The runtime may keep
catalogs, candidate summaries, and search metadata current without showing
every capability to the model. An execution-capable deferred contribution must
enter the model-visible surface before the model can call it.

Registry selection and execution permission are separate decisions. The
registry decides which contributions are active, available, conflict-resolved,
and visible for an invocation. Permission and resource policy decide whether a
selected operation may execute.

Hooks should be controlled event points on the registry and dispatch path. They
should be easy to write and debug, but they should not gain open-ended authority
to rewrite the visible surface, bypass permissions, or mutate runtime-owned
state outside the event contract.

Evidence should stay compact. Psychevo should record source identity, selected
contributions, omitted or degraded contributions that affected assembly,
conflicts that affected selection, visibility decisions, and dispatch trace
facts. It should not persist every discovered candidate by default.

## Worked Example

A future external source contributes a `repo_lint` tool.

First, the source is discovered and assigned a raw source identity. The tool
contribution receives its own raw contribution identity, a proposed
model-visible name, schema metadata, availability check, execution binding, and
declared toolset membership.

Second, the registry evaluates activation, availability, and conflicts. If
another contribution already claims the visible name `repo_lint`, the registry
does not replace it silently. It reports the conflict and either omits the new
tool or exposes a namespaced name according to an explicit policy.

Third, the tool may remain deferred. The runtime can include it in a searchable
catalog summary without adding it to the model-visible tool list. If a later
search or runtime policy activates it for the invocation, the tool declaration
enters the visible surface before the model may call it.

Fourth, the model calls the visible tool name. Dispatch resolves the visible
name back to the raw contribution identity, runs pre-dispatch hooks, checks
permission and resource policy, invokes the source-owned execution binding, and
runs post-dispatch hooks.

Fifth, Psychevo records compact evidence: the selected source, the visibility
decision, any omitted conflicting candidates, the permission outcome, and the
dispatch result. It does not store every unrelated candidate discovered from
the same source.

## Milestones

1. Normalize internal capabilities. Built-in tools, skills, agents, and
   providers should flow through shared source, contribution, selection, and
   evidence vocabulary without changing user-visible behavior.
2. Define the model-visible surface. Add direct and deferred exposure semantics,
   including minimal search or activation behavior for deferred capabilities.
3. Prepare for multiple source forms. MCP, plugin manifests, bundled packages,
   and future trusted local scripts should fit the same source and contribution
   contract, without committing to a source priority order.
4. Route extension power through controlled paths. Hooks, availability checks,
   conflict reporting, dispatch, and evidence should pass through the registry
   or the owning specialized manager.

## Tradeoffs

This mechanism adds structure before Psychevo has a broad plugin surface. The
cost is extra design and migration work around current built-in capabilities.
The benefit is that future extensions inherit one identity, selection,
permission, dispatch, and evidence model.

The design also limits convenient override behavior. Contributors may need to
use namespaces or explicit replacement policy instead of claiming an existing
visible name. That friction is intentional because silent override makes
invocation evidence and safety review weaker.

The hybrid direct/deferred model adds one more step before some tools can run.
It keeps prompts smaller and makes the visible execution surface inspectable for
each invocation.

## Decision Questions

Question: What is the minimum raw identity shape for a contribution?

Proposed default: use a source identity plus a source-local contribution id,
with the visible name stored as separate display and prompt metadata.

Question: Which deferred activation path should ship first?

Proposed default: ship a small search or activation surface before adding
automatic selection policies. Runtime-maintained catalogs are allowed, but
execution-capable tools still need visible activation.

Question: How much replacement policy should v1 support?

Proposed default: reject same visible-name conflicts unless a built-in
capability owner defines an explicit replacement rule. Do not provide general
last-writer-wins override.

Question: How should provider contributions enter this model?

Proposed default: keep provider resolution in the provider manager, and make
provider entries emit contribution facts for source identity, selection,
availability, and evidence.
