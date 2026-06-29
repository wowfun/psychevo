---
name: 0004. Hook System
status: proposed
date: 2026-06-27
psychevo_self_edit: deny
---

## Context

Psychevo already has early hook support through agent and plugin declarations,
but that support is a narrow command-runner slice. The product is still
pre-release, so Psychevo can replace that slice with a coherent hook system
before users depend on accidental ordering, trust, payload, or evidence
behavior.

ADR 0002 defines the capability contribution mechanism. ADR 0003 defines the
plugin product layer above it. Hooks need their own ADR because they sit at
runtime lifecycle seams and can influence prompts, tool calls, permission
requests, compaction, notification, and provider-adjacent events. Without a
single hook module, each source form would invent its own event names, trust
rules, mutation authority, and diagnostics.

Codex is the primary design reference: hook declarations use event matcher
groups, non-managed command hooks are trusted by normalized hash, matching
handlers run concurrently, and the UI receives structured started/completed run
summaries. Reasonix contributes useful lifecycle coverage and operational
ergonomics: `PostLLMCall`, `SessionEnd`, `Notification`, compact stdout/stderr
diagnostics, and clear timeout/block behavior.

## Decision

Psychevo will make hooks a runtime-owned module. Capability sources may
contribute hook declarations, but the runtime hook module owns normalization,
source provenance, trust, matching, execution, event-scoped effects, diagnostics,
and run summaries.

The hook interface is intentionally small. Callers assemble hook sources for an
invocation, ask the hook module to run a named event with a typed payload, and
receive a typed response plus run summaries. The implementation may contain
command, worker, prompt, and agent adapters, but callers do not bypass the hook
module to run those adapters directly.

Hook effects are scoped to the lifecycle event that produced them. A hook may
block a current action, approve or deny a single permission request, update the
current tool-call input, contribute context through context assembly, add
compaction guidance, report feedback, or emit diagnostics. A hook must not
rewrite raw provider payloads, mutate durable transcript facts, grant future
permissions, widen sandbox authority, edit provider credentials, replace future
capability snapshots, or write directly to registries.

## Event Catalog

Psychevo standardizes the combined Codex and Reasonix event catalog:

- `SessionStart`
- `SessionEnd`
- `SubagentStart`
- `SubagentStop`
- `UserPromptSubmit`
- `PreToolUse`
- `PermissionRequest`
- `PostToolUse`
- `PostLLMCall`
- `PreCompact`
- `PostCompact`
- `Notification`
- `Stop`

Event names are case-sensitive product names. Implementations may accept
compatibility aliases while reporting the normalized event name in metadata,
diagnostics, and run summaries.

`PreToolUse` runs before permission and resource checks. If a hook updates the
current-call input, permission and resource policy evaluate that effective
request, not the model's original request. `PermissionRequest` can decide only
the current approval request and cannot persist a grant. `PostToolUse` observes
the completed call and may add feedback, but it cannot retroactively change the
permission decision that allowed the call.

`PostLLMCall` runs after a model turn completes but before model output is
projected for display or future context. It may contribute display/projected
reasoning or typed feedback while preserving raw provider output and signed
reasoning. `PreCompact` may contribute compaction guidance. `Notification`
payloads must be redacted to the minimum actionable message.

## Declarations And Sources

The canonical declaration shape is Codex-style:

```json
{
  "hooks": {
    "PreToolUse": [
      {
        "matcher": "Bash",
        "hooks": [
          {
            "type": "command",
            "command": "python3 .psychevo/hooks/pre_tool.py",
            "statusMessage": "Checking tool request",
            "timeout": 30
          }
        ]
      }
    ]
  }
}
```

Each event maps to an ordered list of matcher groups. A matcher group contains
an optional `matcher` and one or more handlers. Omitted, empty, or `*` matchers
match every event occurrence. Tool and permission events match on tool name.
Compaction events match on trigger. Session and subagent events match on their
start or stop kind when available. Events without a meaningful matcher ignore
the matcher with a diagnostic.

Hook sources include managed policy, profile configuration, selected-agent
definitions, project configuration, plugin packages, plugin workers, and future
runtime-owned sources. Existing Psychevo agent and plugin hook shapes may be
accepted as compatibility input, but runtime normalizes them into the canonical
event, matcher-group, handler shape before trust or execution.

## Trust Model

Managed hooks are trusted by policy and cannot be disabled by ordinary user
hook review.

Profile hooks and selected-agent hooks are trusted configuration for the active
invocation. They still produce metadata and run summaries, but they do not need
per-hook hash review.

Project hooks and plugin hooks require accepted source policy plus per-hook
normalized-hash review before they run. Project hooks require trusted project
configuration. Plugin hooks require the plugin to be enabled, the `hooks`
capability family to be enabled, and the specific hook definition to be trusted
by normalized hash. Installing or enabling a plugin does not automatically trust
or run plugin hooks.

The hook metadata interface includes `HookTrustStatus` values:

- `managed`
- `trusted`
- `untrusted`
- `modified`

`modified` means the source identity is known but the normalized hook definition
hash no longer matches the trusted hash. Untrusted and modified hooks are listed
for review and skipped at execution time unless an explicit one-shot bypass is
added by a later spec.

## Handler Family

The first-class handler types are:

- `command`: runs a local command adapter with the hook payload on stdin.
- `worker`: calls a plugin worker hook adapter through the Psychevo worker
  protocol.
- `prompt`: contributes typed context or instruction candidates through context
  assembly.
- `agent`: delegates the hook event to a named agent or subagent interface.

Unsupported handler types, unavailable adapters, invalid commands, malformed
worker responses, invalid prompt contributions, and unavailable agents become
source-qualified diagnostics. They must not crash the agent loop.

## Runtime Interfaces

The hook module owns these interface names:

- `HookEventName`
- `HookSourceKind`
- `HookHandlerType`
- `HookMatcherGroup`
- `HookHandler`
- `HookPayload`
- `HookResponse`
- `HookMetadata`
- `HookTrustStatus`
- `HookRunSummary`

`HookMetadata` includes event name, matcher, handler type, source kind, source
identity, plugin id when relevant, display order, enabled state, managed state,
normalized hash, source path when available, timeout, status message, and trust
status.

`HookPayload` is event-specific but always includes the normalized event name,
scope, canonical cwd, invocation or turn identity when available, and
redacted event data. Tool payloads include tool name and hook-facing input.
Provider-adjacent payloads must not expose more provider data than the event
contract needs.

`HookResponse` supports only event-scoped effects: continue, block, one-shot
permission decision, current-call input update, typed context contribution,
tool feedback, compaction guidance, notification side effect, and diagnostic
entries.

## Execution Semantics

For one event occurrence, runtime discovers all enabled, trusted, matching
handlers and launches them concurrently. One matching handler must not prevent
another matching handler from starting.

Run summaries and UI presentation are ordered by source display order, matcher
group order, and handler order. Any block or deny decision wins. Current-call
input updates are resolved by completion order and then checked by permission
and resource policy. Diagnostic text remains bounded and source-qualified.

Handler timeouts are part of the normalized handler definition. A later spec may
define exact defaults per handler type and event. Until then, command handlers
must have bounded execution and bounded stdout/stderr capture.

Hook failures degrade only the hook handler or source unless the event contract
defines a blocking result. Runtime must continue the agent loop whenever the
hook contract allows it.

## Boundaries

Hooks do not own provider protocol semantics. They may observe provider-adjacent
events, report diagnostics, and contribute typed context or feedback. They must
not rewrite raw provider payloads or signed provider reasoning.

Hooks do not own context projection. Prompt and context effects enter context
assembly as typed candidates, and context assembly decides model visibility.

Hooks do not own permission or sandbox policy. `PreToolUse` may change the
current request before permission evaluation, and `PermissionRequest` may answer
one approval request, but permission policy still enforces the runtime ceiling
and sandbox policy still describes actual confinement.

Hooks do not own plugin policy. Plugin packages may contribute hooks only after
plugin policy enables the plugin and hook family; hook trust review still
applies to project and plugin hook definitions.

## Evidence And Diagnostics

Psychevo adopts the Run Summaries model. Hook runs emit structured start and
completion summaries with id, event, handler type, scope, source, source path
when available, display order, status, status message, started/completed times,
duration, bounded output entries, and diagnostics.

Run summary entry kinds include:

- `warning`
- `stop`
- `feedback`
- `context`
- `error`

Hook run summaries are diagnostic/evidence records, not ordinary transcript
messages. They should be available to UI, CLI, doctor, and debugging surfaces,
but Psychevo should not persist a full hook audit ledger by default. Final
model-visible tool results remain normal tool evidence.

## Migration And Spec Alignment

ADR 0002 keeps the general contribution mechanism. ADR 0004 owns hook event
catalog, declaration shape, trust, execution, payload, effect, and evidence
rules.

ADR 0003 keeps plugin package, store, policy, and worker-system decisions.
Plugin-packaged hooks are hook contributions governed by this ADR.

Specs 053 and 140 define hook authority and runtime behavior against this ADR.
Specs 054 and 150 define how plugin packages and plugin runtime feed hook
contributions into the same hook module. Existing command-only implementation
behavior is a compatibility slice, not the final hook system contract.

## Consequences

This design is larger than a simple shell-hook runner. The benefit is one
inspectable hook module with consistent source identity, trust review, matching,
execution, and evidence across agents, plugins, projects, profiles, and future
managed sources.

Concurrent execution makes completion timing part of the effect resolution
model. The benefit is that one slow or blocking hook cannot prevent another
matching hook from starting, and UI/evidence can still present stable
declaration order.

Hash trust adds friction for project and plugin hooks. The benefit is that
cloned repositories and third-party packages cannot silently start executing
new local commands merely because they are discovered.

## Open Questions

- Exact Rust type shapes and protocol wire names for the hook interfaces.
- Default timeouts per event and handler type.
- UI affordances for reviewing project and plugin hook hashes.
- Whether a future one-shot bypass should exist for deterministic automation.
