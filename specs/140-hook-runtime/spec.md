---
name: 140. Hook Runtime
psychevo_self_edit: deny
---

Define runtime-owned hook execution for the target hook system.

## Scope

- shared runtime execution for hook handlers
- hook source records supplied by profiles, projects, agents, plugins, and managed policy
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

`psychevo-runtime::hooks` owns hook normalization, trust filtering, matching,
handler execution, and run-summary construction. Agent code, plugin code,
profile config, project config, and managed policy may produce hook source
descriptors, but they must not execute hook handlers directly.

A hook source descriptor contains:

- source id
- source kind: managed, profile, project, agent, plugin, worker, or runtime
- optional display name
- source path when available
- plugin id when relevant
- canonical hook declaration data
- trust facts available before normalized-hash review

Runtime normalizes all accepted declaration shapes into event matcher groups
with handler lists. Unsupported or malformed declarations become diagnostics
and are not executed.

Runtime accepts hook declarations from inline `hooks.<Event>` configuration and
from `hooks.json` files discovered beside profile and project configuration
layers. Sources are additive; higher-precedence configuration does not erase
lower-precedence hook declarations.

Runtime exposes a metadata/listing interface that reports every normalized
handler, including handlers skipped for disabled state, untrusted hash, modified
hash, unsupported handler type, unavailable adapter, malformed command, or
policy-disabled plugin capability.

## Handler Execution

Runtime recognizes these handler types:

- `command`
- `worker`
- `prompt`
- `agent`

The current implementation slice may execute only command handlers and skip
other handler types with structured diagnostics. The target runtime must route
worker, prompt, and agent handlers through adapters hidden behind the hook
module interface.

Command handlers receive a JSON payload on stdin, run in the canonical cwd
unless overridden by a safe handler field, and have bounded timeout plus bounded
stdout/stderr capture. Worker handlers receive the same semantic payload through
the plugin worker protocol. Prompt handlers return typed context candidates.
Agent handlers delegate through the agent/subagent interface and return typed
hook responses.

Bounded command and worker stdout/stderr must remain valid UTF-8 after
truncation. If output is larger than the capture limit, runtime truncates at a
valid character boundary and appends a visible truncation marker; malformed
input bytes degrade through lossy UTF-8 conversion instead of crashing the agent
loop.

Structured command stdout may return:

- `continue: false` with `stopReason` to block the current event when the event
  supports blocking
- `decision: "allow" | "deny"` for `PermissionRequest`
- `updatedInput` for `PreToolUse`
- `context`, `feedback`, `compactionGuidance`, `systemMessage`, and
  `suppressOutput` as event-scoped effects

Unsupported fields produce diagnostics. They do not become durable transcript
facts and do not mutate future permission, sandbox, provider, or capability
state.

For a single event occurrence, matching trusted handlers launch concurrently.
Run summaries are reported in source display order, matcher-group order, and
handler order. Any block or deny decision wins. Current-call input updates
resolve by completion order.

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
may add diagnostics or feedback but cannot retroactively change permission or
execution.

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

`PostLLMCall` must preserve raw provider output and signed reasoning even when
a hook contributes display/projected reasoning or feedback. `PreCompact`
stdout or structured guidance becomes compaction guidance. `Notification`
payloads must be redacted before they leave runtime.

`SessionStart`, `SessionEnd`, `UserPromptSubmit`, `PostLLMCall`,
`PreCompact`, `PostCompact`, `Notification`, and `Stop` are runtime lifecycle
events even when the first implementation slice reports some handler families
as skipped. Unsupported lifecycle adapters must produce bounded diagnostics,
not silent omissions.

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
behavior, concurrent launch, declaration-order summaries, completion-order input
rewrites, block/deny precedence, timeout and output bounding, permission
ordering, one-shot permission decisions, plugin hook gating, worker adapter
diagnostics, prompt/agent effect scoping, provider-output preservation, and
notification redaction.

Live hook validation must use realistic local hook scripts in an isolated
profile/cwd: prompt secret scanning, tool input rewriting, permission
allow/deny, post-tool feedback, compaction guidance, notification redaction,
and a plugin-packaged hook requiring trust. Live validation must not use real
provider calls unless explicitly requested.

## Related Topics

- [053 Hooks](../053-hooks/spec.md) defines hook authority.
- [150 Plugin Runtime](../150-plugin-runtime/spec.md) defines plugin hook source loading.
- [250 UI Display Model](../250-ui-display-model/spec.md) defines transcript projection.
