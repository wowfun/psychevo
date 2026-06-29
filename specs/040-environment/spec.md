---
name: 040. Environment
psychevo_self_edit: deny
---

# 040. Environment

Define Psychevo's local host environment and authority boundary for runtime and
caller-facing operations.

This topic is the source of truth for how Psychevo describes the local
environment it operates in: cwd, filesystem, process, network, platform,
environment-variable, temporary-directory, and cache exposure. It does not grant
authority by itself. [041 Permissions](../041-permissions/spec.md) owns policy
gates, and [045 Sandbox](../045-sandbox/spec.md) owns sandbox enforcement below
those gates.

## Scope

- local host environment vocabulary used by runtime, Gateway, CLI, TUI,
  Workbench, and future shell surfaces
- cwd and workspace-root assumptions at the environment boundary
- filesystem, process, network, environment-variable, temporary-directory, and
  cache exposure categories
- platform capability constraints that affect local execution
- relationship between environment facts, permission policy, resource gates,
  and sandbox enforcement
- user-visible environment diagnostics and fail-closed expectations when local
  environment capabilities are unavailable

Out of scope:

- permission profiles, approval policy, persistent grants, dangerous-command
  policy, or policy precedence, which are owned by
  [041 Permissions](../041-permissions/spec.md)
- operating-system sandbox isolation, writable-root enforcement, shell child
  confinement, or `/sandbox` reporting, which are owned by
  [045 Sandbox](../045-sandbox/spec.md)
- concrete tool schemas, command names, terminal rendering, JSON fields, or
  public API shapes
- provider authentication, credential storage, secret redaction policy, remote
  policy services, or hosted execution environments
- concrete storage schemas, trace formats, retained logs, or replay behavior

## Environment Boundary

The local environment is the host context Psychevo can observe or act within
when running locally. It includes the process cwd, known workspace roots,
filesystem paths exposed to resource and tool operations, inherited environment
variables, local process execution, platform capabilities, network reachability,
temporary directories, and development cache locations.

Environment description is not authority. A path, executable, network endpoint,
temporary directory, cache root, or platform capability being present only means
it may be considered by an owning policy or tool. Permissions, resource gates,
runtime mode, and sandbox policy decide whether it may be used.

The process cwd is the default local scope for workspace-oriented behavior,
but it is not the universal security boundary. Specs that use workspace roots
must say whether they mean caller intent, default path resolution, permission
profile defaults, sandbox writer roots, Gateway source scope, or UI grouping.

## Authority Relationship

Psychevo separates environment facts from authority decisions:

- [009 Resource Surface](../009-resource-surface/spec.md) owns resource facts,
  resource operations, access gates, and resource decisions.
- [041 Permissions](../041-permissions/spec.md) owns runtime permission policy
  before local resource operations and tool execution.
- [045 Sandbox](../045-sandbox/spec.md) constrains writes and shell children
  after an operation has permission to be attempted.

These layers must not collapse into one another. A permission allow does not
mean the sandbox can be bypassed. A sandbox writable root does not mean a tool
has permission to mutate it. A resource fact does not become model-visible
unless context assembly or another owning spec admits it.

Runtime mode may further constrain local authority. `plan` remains read-only
for model-visible mutation even when the host environment contains writable
paths or executable tools.

## Host Exposure

Filesystem exposure includes readable or writable local paths, workspace roots,
temporary directories, cache directories, and any host paths surfaced by product
or shell integrations. Host path exposure must preserve the distinction between
display labels, user-selected files, canonical local paths, and model-visible
content.

Process exposure includes shell execution, stdin/stdout/stderr handling,
long-running child sessions, yielded stdin, platform-specific process
capabilities, and executable discovery. Shell execution remains subject to
permissions and sandbox enforcement when those layers apply.

Network exposure includes ordinary provider calls, local Gateway transports,
MCP transports, shell-originated network risk, and future product transports.
This topic names exposure categories only; it does not define network policy or
network sandboxing.

Environment-variable exposure is sensitive by default. Environment variables
may guide local configuration, provider discovery, cache discovery, or isolated
test behavior, but specs that persist, display, or model-expose environment
material must define a safe redaction or opt-in boundary.

Temporary and cache roots are host conveniences, not durable truth sources.
Specs that use them must avoid creating hidden execution truth that cannot be
reconstructed from durable evidence or declared configuration.

## Platform Capability

Local execution may depend on platform capabilities such as Unix process
signals, pseudo-terminals, filesystem canonicalization behavior, native sandbox
backends, browser file-selection limits, or shell-host APIs. When a required
platform capability is unavailable, the owning spec must define whether the
operation degrades, becomes unsupported, or fails closed.

Platform differences should be reported as environment capability differences
instead of being hidden behind generic runtime failures. Deterministic local
validation may use fake or skipped capability checks, but live host capability
validation must not depend on real credentials or global host state unless a
spec explicitly opts in.

## Related Topics

- [000 Foundation](../000-foundation/spec.md) defines the upstream project
  foundation and implementation-neutral principles.
- [004 Runtime Contract](../004-runtime-contract/spec.md) defines runtime
  assembly and permission wiring.
- [009 Resource Surface](../009-resource-surface/spec.md) defines resource
  facts, access gates, and resource decisions.
- [020 Interfaces](../020-interfaces/spec.md) defines caller-facing invocation
  and observation boundaries.
- [025 CLI](../025-cli/spec.md) defines process-oriented command-line
  environment boundaries.
- [041 Permissions](../041-permissions/spec.md) defines local permission policy
  gates and approvals.
- [045 Sandbox](../045-sandbox/spec.md) defines sandbox enforcement for local
  writes and shell children.
