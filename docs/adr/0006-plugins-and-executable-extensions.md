---
name: 0006. Plugins And Executable Extensions
status: accepted
date: 2026-08-06
psychevo_self_edit: deny
supersedes: 0003. Plugin Packages
---

## Context

One Plugin concept currently carries incompatible responsibilities:
distribution metadata, Codex-compatible declarative components, executable
workers, command registration, and potential UI integration. That makes
installation, enablement, trust, process lifetime, and permission difficult to
explain. Most executable capabilities enhance Psychevo's CLI/TUI/GUI rather
than acting as independent applications.

Pi demonstrates a low-friction top-level Extension lifecycle and temporary
local loading. Codex demonstrates marketplace-qualified Plugin identity and
explicit package management. MCP Apps demonstrates a portable rich-UI boundary
without importing third-party modules into a host frontend.

## Decision

Psychevo separates Plugin and Extension.

A Plugin is a marketplace-owned, manifest-first declarative package. Adding it
materializes the package disabled. Enablement is explicit, and every declared
skill, MCP server/App, hook, Agent, or toolset still passes through its owning
runtime policy. A Plugin has no worker or in-process executable ABI.

An Extension is a precompiled executable sidecar with one root
`psychevo.extension.json`. Installing it immediately enables and trusts its
exact fingerprint. The host starts it lazily and communicates only over the
versioned `psychevo-extension/1` stdio protocol. Direct commands appear as
`pevo <command>` and execute through host `command/run`; built-ins win and
duplicate command activation fails.

An Extension may carry at most one co-root Plugin. The reverse is forbidden,
and the two keep independent policy and trust. This accommodates distributions
that need executable interaction plus optional declarative content without
making Plugin installation executable.

Profile Extension management uses Pi-style top-level `install`, `remove`,
`list`, `update`, `config extension`, and `-e` temporary loading. Plugin
management uses Codex-style `plugin add <plugin>@<marketplace>`, explicit
enable/disable, and nested marketplace management.

Host-rendered typed effects are the default UI contribution. Rich UI uses
sandboxed MCP Apps. Arbitrary JavaScript/React imports, native dynamic modules,
and unrestricted host callback objects are excluded.

WeChat, Telegram, and Feishu/Lark adapters ship as three independent first-party
Extension artifacts. Gateway retains configuration, routing, Thread/outbox
state, and supervision; SDK and transport code moves to the artifacts. The Rust
CLI defaults to `full = acp + gateway + desktop` but can omit those product
surfaces at build time. Extension support remains in the base CLI.

## Consequences

Package acquisition, declarative activation, executable trust, and owning
runtime permission now have distinct user-visible states. Static manifests
make command discovery and conflict checks possible without eager process
startup. A five-minute cancelable idle lease lets interactive hosts reuse a
sidecar while CLI calls remain deterministic one-shot processes.

The cost is a process and protocol boundary for executable capabilities and
separate release artifacts per supported target. That boundary is intentional:
it keeps Rust and frontend ABIs private, permits lazy installation, and lets the
host validate every effect. ADR 0005 still applies: Extensions feed immutable
owning-module assembly and explicit runtime resources; they do not create a
generic mutable registry.
