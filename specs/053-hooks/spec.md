---
name: 053. Hooks
psychevo_self_edit: deny
---

Define hook authority before hook declarations enter runtime execution.

## Scope

- hook event identity and lifecycle semantics
- hook declaration normalization and source provenance
- hook trust status and diagnostics
- the difference between observation, request, contribution, and event-scoped direct effects
- relationship between profile, project, agent, plugin, managed, and runtime hook sources

Out of scope:
- plugin package manifests, installation, marketplace catalogs, and stores
- concrete Rust storage schemas, UI rendering, remote hook services, or network transports
- provider payload mutation, durable permission grants, sandbox confinement, or credential handling

## Hook Model

Hooks are a runtime-owned extension module. Sources contribute hook
declarations; runtime normalizes, trusts, matches, and executes them through the
shared hook module.

A hook source is a managed policy, profile, selected-agent, project, plugin,
plugin worker, or runtime-owned source that declares handlers for named runtime
events. A hook declaration is a candidate handler. Discovery does not make it
trusted, executable, model-visible, or allowed to mutate runtime state.

The canonical declaration shape is Codex-style: `hooks.<Event>[]` contains
matcher groups, and each matcher group contains a `matcher` plus a `hooks[]`
handler list. Diagnostics and evidence report normalized event, matcher-group,
and handler metadata.

## Event Catalog

Hook event names are case-sensitive product names. The Codex-aligned core
catalog is:

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

Psychevo also keeps these product events:

- `SessionEnd`: explicit close behavior for long-lived UI, Gateway, daemon, and
  replay contexts where cleanup is not the same as `Stop`.
- `PostLLMCall`: provider-adjacent observation after a model response is
  received, while preserving raw provider output and signed reasoning.
- `Notification`: redacted, actionable runtime notification hooks for product
  surfaces that are not ordinary transcript messages.

## Source Trust

Managed hooks are trusted by policy.

Profile hooks and selected-agent hooks are trusted configuration for the active
invocation. They still produce metadata and run summaries, but do not require
per-hook hash review.

Project hooks and plugin hooks require accepted source policy plus per-hook
normalized-hash review before they run. Project hooks require trusted project
configuration. Plugin hooks require the plugin package to be enabled and the
hook definition to be trusted.

Hook trust status values are:

- `managed`
- `trusted`
- `untrusted`
- `modified`

Untrusted and modified hooks are listed for review and skipped at execution
time. A current invocation may opt into a dangerous one-shot trust bypass for
project and plugin hooks, but disabled hooks remain disabled and the bypass must
not persist trusted hashes or enablement state.

Profile-owned hook state is stored in active profile configuration under
`hooks.state.<hook_key>`. Each state record contains `enabled` and
`trusted_hash`. The state key is derived from normalized source identity,
normalized event, matcher, handler type, and a declaration index scoped to that
source/event/matcher group. Display order is not part of the key. A changed
definition can then be reported as `modified` instead of disappearing as a new
untrusted hook, and inserting unrelated sources, events, or matcher groups does
not invalidate existing profile hook state. The trusted hash is derived from the
normalized event, matcher, and single-handler definition so TOML and JSON
equivalents converge. CLI and UI review commands must write only profile hook
state, never project files, plugin packages, SQLite session state, or source
hook declarations.

## Handler Types

The first-class handler types are:

- `command`: local command adapter with JSON payload on stdin.
- `worker`: plugin worker hook adapter.
- `prompt`: typed context or instruction contribution through context assembly.
- `agent`: delegation to a named agent or subagent interface.

Unsupported handler types and unavailable adapters are skipped with
source-qualified diagnostics.

Worker handlers call the plugin worker hook method and require package
enablement plus an accepted hook source. Prompt handlers contribute typed
context candidates to context assembly. Agent handlers delegate through the
agent/subagent interface with bounded timeout and turn limits. These adapters
are hidden behind the hook module interface; callers must not execute worker,
prompt, or agent handlers directly.

## Authority

Hooks do not own provider payload semantics, permission policy, sandbox policy,
session state, registry state, durable transcript facts, or future registry
views.

Most hook effects are observation, typed request, or typed context effects.
The owning runtime boundary decides whether a request changes the current
invocation. Context and prompt contributions enter context assembly; they do
not write directly into the system prompt or provider payload.

Tool hooks are the direct-effect exception. `PreToolUse` runs before permission
and resource checks. It may block the current call or update only the current
call's hook-facing input; permission and resource policy evaluate the effective
input. `PermissionRequest` may answer one approval request but must not persist
a grant. `PostToolUse` may report diagnostics or feedback but cannot
retroactively change the permission decision.

`PostLLMCall` may contribute display/projected reasoning or typed feedback
while preserving raw provider output and signed reasoning. `PreCompact` may
contribute compaction guidance. `Notification` payloads must be redacted to the
minimum actionable message.

## Execution

Runtime executes matching trusted hook declarations with:

- normalized event name
- source identity and source kind
- invocation, turn, or session scope where available
- canonical cwd
- handler type
- hook-facing typed payload

For one event occurrence, matching handlers run concurrently. Reporting remains
ordered by source display order, matcher-group order, and handler order. Any
block or deny decision wins. Current-call input updates resolve by completion
order before permission and resource policy evaluate the effective request.

Hook failures degrade the handler or source unless the hook contract defines a
blocking outcome. Runtime must not crash the agent loop because a hook handler
failed, timed out, emitted invalid JSON, or exited non-zero.

Command handlers receive one JSON payload on stdin, execute in the canonical
run cwd by default, use a default timeout of 600 seconds with a one-second
minimum, and capture bounded stdout and stderr. Structured JSON stdout is the
primary response contract. Exit code `2` blocks events whose contract supports
blocking when structured output does not provide a stronger result.

## Evidence

Hook evidence uses run summaries, not ordinary transcript messages and not a
full durable audit ledger by default. A run summary includes event, handler
type, scope, source, display order, status, status message, start/end time,
duration, bounded output entries, and diagnostics.

Hook metadata includes event, matcher, handler type, source, plugin id when
relevant, display order, enabled state, managed state, normalized hash, and
trust status.

## Related Topics

- [140 Hook Runtime](../140-hook-runtime/spec.md) defines runtime execution slices.
- [054 Plugins](../054-plugins/spec.md) defines plugin package boundaries.
- [150 Plugin Runtime](../150-plugin-runtime/spec.md) defines plugin hook source loading.
- [006 Context Assembly](../006-context-assembly/spec.md) defines context projection.
- [007 Tool Surface](../007-tool-surface/spec.md) defines tool dispatch.
- [041 Permissions](../041-permissions/spec.md) defines permission gates.
- [045 Sandbox](../045-sandbox/spec.md) defines sandbox confinement status.
