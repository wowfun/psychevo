---
name: 0003. Plugin Packages
status: proposed
date: 2026-06-30
psychevo_self_edit: deny
---

## Context

Psychevo needs packages so people can distribute skills, MCP declarations,
hooks, interface metadata, and future extension sources without editing the
core runtime. A package system is useful only if it does not become a second
runtime authority path.

Codex points to the right first principle: plugins are manifest-first packages.
The manifest describes package identity and declared resources. Host code
loads the manifest, applies policy, and maps supported declarations into
runtime-owned extension contributors.

Psychevo should follow that shape. A plugin package is distribution and
configuration. Runtime authority still comes from `ExtensionRegistry`
contributors, hook runtime review, tool dispatch, permission policy, resource
policy, and provider resolution.

## Decision

Psychevo will treat plugins as manifest-first packages that may declare
extension sources.

Installing a plugin makes a package available to policy. It does not enable the
plugin, trust its hooks, start its worker, expose tools to the model, grant
permissions, create credentials, or mutate the `ExtensionRegistry`.

The host plugin manager owns plugin discovery, installation, manifest loading,
policy evaluation, compatibility mapping, data-root assignment, diagnostics,
and conversion from accepted declarations into extension contributors. A plugin
package never registers directly into the runtime.

Plugin declarations are candidates. Host code may map accepted declarations
into:

- skill roots or skill providers
- MCP server contributors
- hook declarations for the runtime hook module
- tool contributors or tool execution bindings
- context or turn-input contributors
- agent or peer-agent descriptors
- provider-adjacent descriptors
- interface or marketplace metadata

Each mapped effect must still pass through the owning runtime module before it
becomes model-visible, executable, trusted, or durable.

## Package Model

A plugin package is a directory or materialized source with one recognized
manifest. The Psychevo-native manifest is the preferred source of truth.
Codex-style manifests may be recognized where their fields map cleanly to
Psychevo semantics.

The manifest describes package identity, display metadata, resource paths, and
declared extension sources. Local resource paths are package-relative and must
remain inside the package root. Display metadata may improve product surfaces,
but it does not create runtime authority.

Installed packages have stable package identity and a plugin-owned data root.
The package cache is code and assets; the data root is writable plugin state.
Plugin code must not treat the package cache as mutable runtime state.

## Policy Model

Plugin policy is separate from package installation.

Profile and project policy may enable or disable a plugin and may enable or
disable families of declarations such as skills, MCP, hooks, tools, providers,
agents, commands, interface metadata, or runtime helpers. Enabling a family
only allows the host to consider matching declarations. It does not bypass
permission, sandbox, hook trust, provider credential, or tool dispatch rules.

Project-local policy may refine the active invocation for the current
workspace, but it must not mutate inactive profile state or silently select a
different profile.

## Runtime Helpers

Executable plugin logic is optional and host-mediated.

Psychevo may support external worker processes, MCP servers, command helpers,
or future helper transports, but each helper is just an implementation detail
behind a mapped contributor or hook handler. A helper response cannot mutate
the registry, grant permission, change provider credentials, persist policy, or
rewrite session state directly.

Psychevo should not expose an in-process third-party `register(ctx)` ABI as the
default plugin model. If a future trusted in-process adapter exists, it must
still install host-owned typed contributors and obey the same policy and
evidence rules as manifest and worker sources.

## Compatibility

Compatibility is package-entry and field-subset compatibility, not runtime ABI
compatibility.

Psychevo may read Codex-compatible package manifests and map supported fields
such as skills, MCP servers, hooks, and display metadata when the semantics
match. Unsupported Codex, Claude Code, Hermes, or OpenCode package behavior
must be ignored with diagnostics or adapted through an explicit Psychevo
mapping.

Psychevo must not execute another host's in-process plugin API directly. Shared
business logic may be reused behind a Psychevo worker, MCP server, skill,
command, provider adapter, or agent backend.

## Evidence And Diagnostics

Plugin evidence should explain effects, not dump packages.

Runtime and diagnostic surfaces should be able to answer:

- which installed package supplied a selected contributor
- which declarations were disabled by policy
- which declarations were unsupported or invalid
- which declarations were omitted because of conflicts or unavailable helpers
- which helpers failed or degraded an accepted contribution
- which plugin hooks were untrusted, modified, skipped, or executed

Ordinary transcript history should not persist full manifests, full package
inventories, or unused candidate declarations by default.

## Non-Goals

This ADR does not define a hosted marketplace, signing system, ratings,
reviews, graphical store, hot reload protocol, stable worker wire format,
credential store, whole-process sandbox, concrete manifest schema, or command
surface.

It also does not require every plugin to contain executable code. The preferred
plugin is often just a package of declarative sources that host code maps into
typed contributors.

## Consequences

This design keeps package distribution separate from runtime authority. The
cost is that plugin authors must fit Psychevo's manifest and mapping rules
instead of mutating the runtime directly. The benefit is that every plugin
effect remains explainable through the same `ExtensionRegistry`, hook,
permission, tool, provider, and evidence interfaces as built-in behavior.
