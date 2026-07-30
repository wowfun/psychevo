---
name: 140. Hook Runtime
psychevo_self_edit: deny
---

Define runtime-owned hook execution for the target hook system.

## Scope

- shared runtime execution for hook handlers
- hook source records supplied by profiles, projects, selected capability roots,
  agents, plugins, and managed policy
- canonical hook declaration normalization
- trust-aware matching and execution
- structured run summaries and bounded diagnostics
- command, worker, prompt, and agent handler execution through one hook module

Out of scope:
- hosted hook catalogs, graphical hook editing, hot reload, remote hook services, or stable SDKs
- provider payload mutation or general session mutation
- whole-process sandboxing for command hooks or plugin worker hooks
- persistent full hook audit logs

## Runtime Contract

`psychevo::hooks` owns hook normalization, trust filtering, matching,
handler execution, and run-summary construction. Agent code, plugin code,
profile config, project config, selected capability roots, and managed policy
may produce hook source descriptors, but they must not execute hook handlers
directly.

A hook source descriptor contains:

- source id
- source kind: managed, profile, project, capability_root, plugin, worker, or
  runtime
- optional display name
- source path when available
- plugin id when relevant
- canonical hook declaration data
- trust facts available before normalized-hash review

Agent declarations map their real source into this list: project agents are
project, global/profile agents are profile, built-ins are managed, and
generated or explicit agents are runtime. Their path/source id is preserved;
there is no blanket trusted agent source kind.

Runtime normalizes all accepted declaration shapes into event matcher groups
with handler lists. Unsupported or malformed declarations become diagnostics
and are not executed.

Runtime accepts hook declarations from inline `hooks.<Event>` configuration and
from `hooks.json` files discovered beside profile and project configuration
layers. Sources are additive; higher-precedence configuration does not erase
lower-precedence hook declarations.

Codex package hooks execute under the explicit semantic profile
`codex-plugin/8604689e`. That profile owns Codex event aliases, generated input
schemas, package/data environment variables, source ordering, matcher behavior,
normalized trust hashes, concurrent execution, and event-specific output
folding. Psychevo-only hook events and handler types remain outside the Codex
profile and must not change its conformance outcomes.

Codex-authority install and reviewed upgrade may establish trust for the exact
fingerprint returned by that mutation. A subsequent package-content change,
external mutation, or unexpected authority version invalidates the trust fact
before the hook source reaches this runtime. This runtime remains the sole owner
of local Codex hook execution and never delegates the same handler back through
the Codex broker.

Runtime exposes a metadata/listing interface that reports every normalized
handler, including handlers skipped for disabled state, untrusted hash, modified
hash, unsupported handler type, unavailable adapter, malformed command, or
policy-disabled plugin capability.

Runtime call sites use typed hook methods and outcomes, not ad hoc inspection
of generic response JSON. The generic event runner may remain as an internal
adapter or test helper, but the public module interface for execution is the
typed event surface: pre-tool, permission, post-tool, session/subagent start,
user-prompt submit, compact, stop/subagent-stop, post-LLM, notification, and
session-end outcomes.

## Handler Execution

Runtime recognizes these handler types:

- `command`
- `worker`
- `prompt`
- `agent`

The current implementation slice executes command, worker, and prompt handlers
through adapters hidden behind the hook module interface. Agent handlers
normalize, list, match, and skip with structured adapter-unavailable
diagnostics until an agent hook adapter is defined.

Command handlers receive a JSON payload on stdin, run in the canonical cwd
unless overridden by a safe handler field, and have bounded timeout plus bounded
stdout/stderr capture. Worker handlers receive the same semantic payload through
the plugin worker protocol. Prompt handlers return typed context candidates for
the current turn only. Agent handlers are declared but not executable in this
slice.

Command execution uses `tokio::process`. One `HookRuntime` admits at most eight
active handlers across all concurrently running event occurrences; the permit
pool is owned by the runtime, not by one `run_event` call. Command and worker
stdout/stderr each retain at most the first
4,096 bytes while continuing to drain the child pipes, then mark the retained
value truncated. Bounded output must remain valid UTF-8 after truncation. If
output is larger than the capture limit, runtime truncates at a valid character
boundary and appends a visible truncation marker; malformed input bytes degrade
through lossy UTF-8 conversion instead of crashing the agent loop.

Each command handler owns a process tree through the same minimal
`ProcessTreeGuard` used by plugin materialization: a Unix process group or a
Windows kill-on-close Job. One absolute handler deadline covers direct-child
wait, stdout/stderr EOF, and cleanup. On expiry runtime terminates the whole
tree, gives bounded readers at most one additional second to drain, aborts
readers that still retain inherited descriptors, and returns the already
captured bounded output. A descendant cannot keep a timed-out handler alive.

Structured command stdout may return:

- `continue: false` with `stopReason` to block the current event when the event
  supports blocking
- `decision: "allow" | "deny"` for `PermissionRequest`
- `updatedInput` for `PreToolUse`
- `context`, `feedback`, `compactionGuidance`, `systemMessage`, and
  `suppressOutput` as event-scoped effects
- `modelContent` for `PostToolUse`, replacing only the model-visible tool
  result content for the current tool call

Unsupported fields produce diagnostics. They do not become durable transcript
facts and do not mutate future permission, sandbox, provider, or capability
state.

For a single event occurrence, matching trusted handlers launch concurrently
under the runtime's eight-way limit. Run summaries and effects fold in source
display order, matcher-group order, and handler order. Any block or deny
decision wins. Otherwise later declarations overwrite scalar update fields and
array fields append in declaration order; completion timing never changes the
effective result.

## Tool Hook Semantics

Before a tool call, runtime runs matching `PreToolUse` handlers before
permission and resource checks. A `PreToolUse` handler may:

- continue without changes
- block only the current tool call
- update only the current call's hook-facing input
- add event-scoped diagnostics or typed context feedback

Permission and resource policy evaluate the effective tool request after
`PreToolUse` resolution. A hook must not persist permission grants, mutate
future registry views, or widen sandbox authority.

`PermissionRequest` handlers run when runtime is about to ask for approval.
They may allow, deny, or provide no decision for the current request only.

After a tool call, runtime runs matching `PostToolUse` handlers with the tool
name, effective input, bounded output summary, and success state. `PostToolUse`
may add diagnostics or feedback and may replace the current tool result's
model-visible content through `modelContent`. It cannot retroactively change
permission, execution, raw tool JSON, attachments, or future dispatch state.

`UserPromptSubmit` prompt and agent handlers may return typed `context`
candidates. Runtime injects accepted context as hidden turn-local contextual
user messages for the current agent invocation only. Hook context must not
rewrite the user's submitted prompt, mutate the persisted prompt prefix, or
persist as reusable skill, memory, or project instruction state.

Command handlers use exit code `2` as a block signal for blocking command
events when structured output does not provide a stronger result. Other
non-zero exits are diagnostics unless a later handler response schema defines a
stronger event-specific result.

`PermissionRequest` hook decisions run before the existing approval handler.
Any hook denial fails the current request. A hook allow is one-shot and may
skip the user or smart approval handler for the current request, but it must not
persist an allow-always rule or a session grant. If no hook decides, the normal
approval path runs. When a denying hook includes structured `feedback`, that
feedback is the visible denial reason; blocked reasons and diagnostics are
fallback reasons.

## Lifecycle Hook Semantics

`SessionStart`, `SessionEnd`, `SubagentStart`, `SubagentStop`,
`UserPromptSubmit`, `PostLLMCall`, `PreCompact`, `PostCompact`,
`Notification`, and `Stop` use event-specific payloads defined by 053 Hooks.

`SessionStart`, `SubagentStart`, and `UserPromptSubmit` produce typed outcomes
with run summaries, `should_stop`, optional `stopReason`, and turn-local
context candidates. Accepted context is injected as hidden contextual user
messages for the current invocation only. If `should_stop` is true, the owning
call site rejects or aborts the current input before ordinary model execution.

`PreCompact` produces a typed outcome with run summaries and `should_stop`; a
stopped pre-compact aborts compaction before summarization. `PostCompact` is
observation-only. Its stop, failure, timeout, or malformed output is a warning
and cannot reverse a committed checkpoint or change a successful compaction
result into an error.

`Stop` and `SubagentStop` produce typed outcomes with run summaries,
`should_stop`, `should_block`, optional reasons, and continuation fragments.
When blocked, runtime feeds the continuation fragments back as turn-local
context and continues the current loop under the existing turn budget and
cancellation wiring. Runtime must carry a reentrancy flag so a stop hook cannot
recursively block forever without becoming observable as a bounded turn failure.

`PostLLMCall` must preserve raw provider output and signed reasoning even when
a hook contributes display/projected reasoning or feedback. It must not rewrite
the final assistant answer in this slice. `SessionEnd` is cleanup/diagnostic
only. `Notification` payloads must be redacted before they leave runtime.

`SessionStart`, `SessionEnd`, `UserPromptSubmit`, `PostLLMCall`,
`PreCompact`, `PostCompact`, `Notification`, and `Stop` are runtime lifecycle
events. Unsupported lifecycle adapters must produce bounded diagnostics, not
silent omissions.

## Evidence And Diagnostics

Hook runtime returns run summaries with:

- run id
- event name
- handler type
- source id and source kind
- plugin id when relevant
- display order
- status: running, completed, failed, blocked, stopped, or skipped
- trust status
- exit code or handler error when available
- bounded output entries
- elapsed time when available

Run summaries are diagnostic/evidence records and must not be projected as
ordinary transcript facts. Runtime may surface them through TUI, Workbench,
CLI doctor, run warnings, and plugin diagnostics without adding a full durable
hook audit table.

Runtime command hooks and plugin worker hooks are best-effort and are not
whole-process sandboxed in the current sandbox model. Diagnostics must say so
when a user asks for plugin or hook doctor output.

## Acceptance Coverage

Deterministic local validation must cover normalization, stable keys and
hashes, trust/modified/untrusted states, current-invocation bypass, matcher
behavior, eight-way concurrent launch, declaration-order summaries and input
rewrites, block/deny precedence, timeout and output bounding, permission
ordering, one-shot permission decisions, plugin hook gating, worker adapter
diagnostics, prompt/agent effect scoping, session/user-prompt context
injection, compact interruption, stop-hook continuation and reentrancy,
provider-output preservation, and notification redaction.

Live hook validation must use realistic local hook scripts in an isolated
profile/cwd: prompt secret scanning, tool input rewriting, permission
allow/deny, post-tool feedback, compaction guidance, notification redaction,
and a plugin-packaged hook requiring trust. Live validation must not use real
provider calls unless explicitly requested.

## Related Topics

- [053 Hooks](../053-hooks/spec.md) defines hook authority.
- [150 Plugin Runtime](../150-plugin-runtime/spec.md) defines plugin hook source loading.
- [250 UI Display Model](../250-ui-display-model/spec.md) defines transcript projection.
