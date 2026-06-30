---
name: 0004. Runtime Hooks
status: proposed
date: 2026-06-30
psychevo_self_edit: deny
---

## Context

Hooks are the extension mechanism for event-scoped runtime decisions. They are
not a general plugin API, a provider-payload editor, a permission store, or a
second tool runtime.

Codex provides the core model: hook declarations name runtime events, event
matcher groups select handlers, handlers run through host-owned execution, and
the UI receives structured hook run summaries. Psychevo should align with that
model while keeping a few product-specific long-term events for provider
projection, notifications, and explicit session close behavior.

Because hooks can block tool calls, answer permission requests, and contribute
context, they need a dedicated runtime module rather than source-specific
execution paths.

## Decision

Psychevo will own hooks through a runtime hook module.

Sources may contribute hook declarations, but the hook module owns
normalization, source provenance, trust review, matching, execution,
event-scoped effects, diagnostics, and hook run summaries. Callers do not run
hook commands, workers, prompt handlers, or agent handlers directly.

Hook declarations are candidates until accepted by source policy and hook
trust. Accepted hooks can affect only the current event occurrence. They do not
mutate future extension snapshots, grant persistent permissions, rewrite raw
provider payloads, widen sandbox authority, or persist transcript facts.

## Event Catalog

The core event catalog follows Codex:

- `SessionStart`
- `UserPromptSubmit`
- `PreToolUse`
- `PermissionRequest`
- `PostToolUse`
- `PreCompact`
- `PostCompact`
- `SubagentStart`
- `SubagentStop`
- `Stop`

Psychevo also keeps these long-term product events:

- `SessionEnd`: explicit close behavior for long-lived UI, gateway, daemon, and
  replay contexts where cleanup is not the same as `Stop`.
- `PostLLMCall`: provider-adjacent observation after a model response is
  received, while preserving raw provider output and signed reasoning.
- `Notification`: redacted, actionable runtime notification hooks for product
  surfaces that are not ordinary transcript messages.

Event names are product names. Compatibility aliases may be accepted by
implementation specs, but evidence and diagnostics report normalized event
names.

## Declarations And Handlers

The canonical declaration shape is Codex-style: an event maps to ordered
matcher groups, each matcher group has an optional matcher, and each group
contains one or more handlers.

The first-class handler families are:

- `command`: runs a local command adapter with a typed JSON payload.
- `worker`: calls a plugin worker hook adapter.
- `prompt`: contributes typed context or instruction candidates.
- `agent`: delegates the event to an agent or subagent interface.

Handler families are adapters hidden behind the hook module interface. A source
may declare a handler, but the runtime decides whether the handler is enabled,
trusted, supported, and executable for the current invocation.

## Trust Model

Managed hooks are trusted by policy.

Profile hooks and selected-agent hooks are trusted configuration for the active
invocation. They still produce metadata and run summaries.

Project hooks and plugin hooks require accepted source policy plus per-hook
normalized-hash review before execution. Installing or enabling a plugin does
not automatically trust plugin hooks. Trust review is for execution authority,
not for model visibility or package installation.

Untrusted, modified, unsupported, disabled, or unavailable hooks are listed for
review or diagnostics and skipped by default.

## Event-Scoped Effects

Hook responses support only event-scoped effects:

- continue the current event
- block the current prompt, tool call, compaction, or stop event when the event
  contract allows blocking
- update only the current tool-call input before permission and resource checks
- allow or deny one current permission request
- contribute typed context or instruction candidates through context assembly
- contribute tool feedback, user feedback, compaction guidance, notification
  effects, or diagnostics

Any block or deny decision wins for the current event. Current-call input
updates are resolved for the current call only and then evaluated by permission
and resource policy.

Hooks must not persist permission grants, directly edit provider credentials,
rewrite raw provider payloads, mutate durable transcript facts, replace future
tool snapshots, or change future registry membership.

## Execution And Summaries

For one event occurrence, the hook module finds enabled, trusted, matching
handlers and starts them without letting one selected handler prevent another
selected handler from starting. Reporting remains stable by declaration order,
even if completion order differs.

Hook failures degrade the handler or source unless the event contract defines a
blocking result. Timeouts, invalid output, unsupported handler types,
unavailable workers, and unavailable agents become bounded diagnostics rather
than agent-loop crashes.

Hook run summaries are structured diagnostic records. A summary identifies the
event, handler type, source, trust status, run status, bounded output,
diagnostics, and timing facts when available. Summaries may be shown in CLI,
Workbench, doctor, trace, or debug surfaces, but they are not ordinary
transcript messages.

## Boundaries

Hooks participate in the extension system but do not replace it.

Hook declarations may come from plugin packages, profiles, projects, selected
agents, managed policy, or future runtime sources. Those sources still enter
through package policy, profile/project policy, or the `ExtensionRegistry`.

Context effects enter context assembly. Tool effects enter tool dispatch and
permission flow. Approval decisions answer only one approval request. Provider
effects are observation or projection only and preserve raw provider output.
Notification effects are redacted before reaching user-facing surfaces.

## Non-Goals

This ADR does not define exact JSON schemas, CLI commands, review UI, timeout
defaults, stdout and stderr limits, worker wire messages, durable storage
tables, or hosted hook catalogs.

It also does not require whole-process sandboxing for command hooks or worker
hooks. If sandboxing becomes available, hook diagnostics must describe what is
actually confined.

## Consequences

Hooks become a small, powerful module instead of a general escape hatch. The
cost is stricter event contracts and trust review. The benefit is that policy,
context, tool, permission, notification, and lifecycle effects remain
inspectable, scoped, and compatible with the Codex-style runtime model.
