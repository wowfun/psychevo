---
name: 050. Direct Runtime Extension Assembly
psychevo_self_edit: deny
---

Define the host-owned invocation assembly step for accepted extension inputs.

## Scope

- direct immutable `ExtensionAssembly`
- authority-qualified selected capability roots
- one assembly call for root, reconstruction, and child-agent entrypoints
- explicit handoff to existing owning runtime modules

Out of scope:
- a public extension SDK or in-process plugin ABI
- a mutable runtime registry
- generic contributor traits or extension-private data stores
- a second contribution/evidence projection

## Assembly

Host code constructs `ExtensionAssembly` once from the invocation cwd,
effective environment, plugin policy, selected capability roots, static MCP
inputs, and static runtime tools.

The result contains the accepted, source-qualified values already consumed by
the tool, MCP, skill, hook, agent, and plugin modules. It does not wrap those
values in one-use contributor objects. Each owning module performs its own
conflict resolution, trust, permission, availability, and diagnostic work.

Run, request reconstruction, and child-agent entrypoints consume this result.
They must not independently reload plugins or reinterpret selected capability
roots after assembly.

## Capability Root

One selected root carries only:

- stable selector id;
- authority;
- authority-owned locator/path.

Compatibility profile and source identity come from the authority/provider
that resolved the root. The root value does not duplicate a display kind,
source kind, resource kind, and local path for the same fact.

Local and Codex authorities enforce their own containment and resource access.
Consumers do not coerce a non-local locator into a host path.

## Diagnostics And Evidence

Assembly returns the diagnostics already owned by the source loaders and domain
modules. It does not create a generic list of accepted/omitted facts or persist
a frozen registry snapshot. Prompt-prefix, tool-surface, MCP, hook, skill,
agent, and plugin evidence remain in their owning contracts.

## Lifetime

The value is immutable for one invocation. Executable resources referenced by
the value use their actual owners:

- skill runtime: invocation;
- Extension sidecars: the fingerprint-keyed host lifecycle defined by
  [058 Extensions](../058-extensions/spec.md);
- MCP runtime: materialized Thread;
- Codex authority: Application/Gateway process with per-Turn generation lease.

The assembly value itself owns no background task, process, database state, or
shutdown protocol.
