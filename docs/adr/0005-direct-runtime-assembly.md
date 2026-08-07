---
name: 0005. Direct Runtime Assembly
status: accepted
date: 2026-07-26
psychevo_self_edit: deny
supersedes: 0002. Runtime Extension Registry
---

## Context

ADR 0002 predicted that many independent in-process contributors would need a
shared mutable runtime registry and scoped extension-private state. The shipped
implementation has no such owners. Host code discovers sources once, wraps
static values as contributors, reads them once during invocation assembly, and
discards `ExtensionData`. Contribution projections are constructed during the
same pass but have no production consumer.

The real variation is already owned by tools, MCP, skills, hooks, agents, and
plugins. Preserving a generic registry between source acceptance and those
modules adds types and allocations without isolating policy or lifetime.

## Decision

Host code builds one immutable `ExtensionAssembly` value directly from the
effective invocation inputs. Its fields are accepted source-qualified values
for existing owning modules. Those modules continue to own conflicts, trust,
permissions, startup, context visibility, dispatch, evidence, and diagnostics.

There is no `ExtensionRegistry`, `ExtensionData`, generic contributor trait
family, frozen registry view, or generic `ContributionProjection`. A new
abstraction at this boundary requires at least two real consumers with different
implementations or lifetimes; possible future variation is not sufficient.

Tool declarations from every accepted source are compiled once through the
tool-surface owner. Extension sidecars, skill scanners, and MCP connections are
runtime resources with explicit invocation, lease, or Thread lifetimes, not
registry entries. The Extension host defined by ADR 0006 owns processes and
protocol; it does not reintroduce a mutable contribution registry.

## Consequences

Source provenance and all product features remain. The host-to-owner call graph
becomes explicit and compile-time checked, and invocation assembly avoids
wrappers that were immediately unwrapped. Adding a new declaration family
requires choosing its owning module instead of adding a generic slot.

This decision deliberately gives up a speculative in-process extension SDK.
Psychevo remains extensible through Plugin declarations, Extension sidecars,
MCP, hooks, skills, Agents, providers, and typed product interfaces.
