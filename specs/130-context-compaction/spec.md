---
name: 130. Context Compaction
psychevo_self_edit: deny
---

# 130. Context Compaction

Define runtime-owned context compaction for long-running sessions.

## Scope

- automatic context compaction trigger policy
- manual TUI `/compact` behavior
- manual Gateway/Workbench/Channels `/compact` behavior
- compacted session-context projection
- compacted checkpoint transcript projection
- compaction checkpoint persistence semantics
- auxiliary summary model configuration
- context-overflow recovery behavior

Out of scope:
- export/share projection of compaction checkpoints
- branch or child-session rotation for compaction
- exact provider prompt wording beyond required summary safety properties
- live-provider validation, billing policy, or provider-specific retry policy

## Configuration

Compaction is enabled by default. The effective TOML configuration may define:

- `compression.enabled`, default `true`
- `compression.auto`, default `true`
- `compression.threshold_percent`, default `70`
- `compression.reserve_tokens`, default `16384`
- `compression.keep_recent_tokens`, default `20000`
- `compression.model`, optional provider/model selection for summary generation
- `compression.reasoning_effort`, optional summary-model reasoning effort;
  `none` suppresses the provider reasoning field
- `auxiliary.compression.provider` and `auxiliary.compression.model`, preferred
  provider/model selection for summary generation from GUI model settings

When `auxiliary.compression.model` is absent, runtime falls back to legacy
`compression.model`; when both are absent, runtime uses the current invocation
model for summary generation. When the configured auxiliary or legacy
compression model cannot be resolved or fails during summary generation,
runtime must leave the session unchanged and report the compaction failure.

## Checkpoints

Native Psychevo compaction stores completed checkpoints separately from
transcript messages. An ACP Agent's compaction remains Agent-owned when a
certified typed action or command is advertised and does not create a Psychevo
checkpoint.
Original session messages remain authoritative transcript material and are not
deleted or rewritten by compaction.

A checkpoint records the session, creation time, reason, summary text, summary
model/provider, token estimates, optional manual instructions, the first
retained session message sequence, the sequence boundary after which the
checkpoint was created, and implementation metadata.

Runtime uses only the latest checkpoint that is still valid for the current
effective transcript boundary. Undo or revert state can make a later checkpoint
inapplicable without deleting the checkpoint row.

## Projection

Context assembly for a compacted session prepends the latest valid summary as
hidden summary context and then includes retained messages from the checkpoint's
first retained message sequence onward. It does not expose the summary as a
durable user prompt, assistant answer, or visible transcript message.

For native Psychevo sessions, Gateway transcript projection exposes completed
checkpoints as durable diagnostic divider entries. The divider label is
`Session compacted`; it is not a user or assistant message and must not be
included in future model-visible context. The divider stores checkpoint facts
in metadata/detail, including the reason, trigger, token estimates,
provider/model, timestamp, checkpoint id, and first kept session sequence.
Generated summary text is reviewable only as collapsed detail. Agent-owned ACP
compaction does not project a local divider because the Agent owns its history
and context.

Persisted transcript messages remain ordered by their authoritative
`session_seq`; wall-clock timestamps must not reorder messages. Synthetic
checkpoint entries are merged at their structural
`created_after_session_seq` boundary, with deterministic placement for
multiple synthetic entries at the same boundary. Turn terminals persist and
merge at their `lastCommittedSeq` boundary. Timestamp is presentation metadata
and a tie-breaker only within an already-established structural boundary; it
must never infer a checkpoint or terminal boundary.

Cut-point selection must preserve the latest user task, keep recent context by
token budget, and avoid splitting assistant tool-call messages from their
required tool-result messages. If no safe cut point exists, compaction is a
no-op and must not write a checkpoint.

Summary generation uses a no-tools provider request. The summarization input
may include the previous compaction summary plus newly summarized messages, but
must omit hidden reasoning and provider metadata, truncate large tool material,
and redact obvious probable secrets. The generated summary must be framed as
reference-only continuity context, not active instructions from the user.

## Triggers

Automatic compaction applies to main run/TUI sessions and child-agent sessions.
Temporary `/btw` side threads are excluded.

For non-interactive `pevo run`, runtime checks compaction before submitting the
prompt. It does not perform normal post-completion compaction before process
exit.

Fullscreen TUI schedules background compaction after a completed turn when the
latest context usage meets the configured threshold or reserve-space rule. If no
bounded latest context usage is available, automatic TUI compaction is not
scheduled. If a prompt is submitted while compaction is running, the prompt is
queued until compaction completes.

Gateway schedules native post-turn compaction after a completed native Psychevo
turn when the bounded latest context usage meets the configured threshold or
reserve-space rule. The Gateway must use the same runtime compaction operation
as manual compaction and must expose transient compacting activity while the
operation is running. When auto-compaction succeeds, the exact newly created
checkpoint divider is included in the live turn completion
`committedEntries`; clients must not need a later `thread/read` to discover it,
and older checkpoints must not be replayed in that completion. Gateway
auto-compaction remains native-only in this slice.

If a provider returns a context-overflow error, runtime may compact prior
context and retry the same prompt once. A second overflow is reported normally.

## Gateway Operation

Gateway exposes manual compaction only through descriptor-gated
`thread/action/run` with action
`{ kind: "compact", instructions? }` for a concrete public Thread. The operation
serializes through the same per-thread/source activity queue as turns, so manual
compaction waits behind an active turn and ahead of later queued prompts.

The Gateway compaction boundary derives the effective backend identity from the
authoritative thread/source runtime binding. Client-supplied `runtimeRef` is a
consistency assertion only: omission, the native default, or a forged native
value must never authorize Psychevo compaction of an Agent-owned ACP transcript.
An ACP binding routes through its immutable effective Profile and negotiated
typed action; a binding without certified compaction returns unavailable.

During an active Channels turn, `/compact` is accepted and atomically enqueued
before the command handler returns. The channel poll loop waits for the
compaction result and sends its reply in background work, so approvals, stop
requests, and later inbound messages remain processable. A later prompt cannot
overtake the already accepted compaction request.

The structured result includes `accepted`, `threadId`, `compacted`, `reason`,
`message`, `checkpoint`, `tokensBefore`, `tokensAfter`, `summaryProvider`,
`summaryModel`, and unavailable/error state. `checkpoint` contains durable
checkpoint facts plus collapsed review summary text when a checkpoint is
created. Missing sessions, side chats, disabled compaction, no safe cut point,
and below-threshold automatic attempts return accepted but uncompacted results
without mutating state.

ACP compaction routes through the Agent Session action descriptor and reports
success only after the Agent's terminal command/update proves completion; an
acknowledgement alone is not observation. If the advertised command does not
accept custom instructions, `/compact <instructions>` returns typed unsupported
guidance before delivery. Agent-owned completion returns transient structured
status only: it does not read or rewrite local Native messages, append a
`session_compactions` row, fabricate summary/token facts, or project a local
checkpoint divider. EOF or failure likewise produces no local checkpoint state.

## TUI Command

`/compact [instructions]` performs manual compaction for the current main
session. If a turn is running, the manual compaction request is queued behind
that turn and ahead of later queued prompts. Side chats reject
`/compact`.

Fullscreen TUI reports compaction completion with before/after token estimates
and a folded summary row. The row is display-only and is not persisted as a
transcript message.

## Workbench and Channels

Workbench keeps manual compaction slash-only in this slice. `/compact
[instructions]` routes through `thread/action/run`; it must not submit
`Compact this session` or any other ordinary user prompt. The composer context
popover does not add a compact button in this slice.

Channels route `/compact [instructions]` through the same native Gateway
operation and reply with concise success, no-op, or unavailable text. Channels
must not submit compaction as an ordinary prompt.

Manual and automatic Gateway compaction publish transient compacting activity
for the affected thread while work is queued or running and clear it on every
terminal path. This activity is not persisted as a transcript entry.

Checkpoint dividers are read-only. Clicking or expanding a divider may show the
reason, trigger, token before/after, provider/model, timestamp, first kept
sequence, and collapsed summary. Correcting a summary is done by rerunning
`/compact [instructions]`; already-active checkpoints are not edited.

## Attachments

- [Testing](testing.md) defines acceptance scenarios and validation expectations.

## Related Topics

- [006 Context Assembly](../006-context-assembly/spec.md) defines summary
  context as model-visible context.
- [008 Session Continuity](../008-session-continuity/spec.md) defines sessions
  as continuity boundaries.
- [031 SQLite Persistence](../031-storage-and-persistence/sqlite-persistence.md)
  defines the storage shape for checkpoints.
- [120 Provider Registry](../120-provider-registry/spec.md) defines provider
  and model configuration resolution.
- [270 UI Interaction](../270-ui-interaction/spec.md) defines shared
  slash-command interaction behavior.
- [210 pevo TUI Interaction](../210-pevo-tui/interaction.md) defines the
  fullscreen terminal projection of that behavior.
