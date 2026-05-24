---
name: 130. Context Compaction
psychevo_self_edit: deny
---

# 130. Context Compaction

Define runtime-owned context compaction for long-running sessions.

## Scope

- automatic context compaction trigger policy
- manual TUI `/compact` behavior
- compacted session-context projection
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

When `compression.model` is absent, runtime uses the current invocation model
for summary generation. When `compression.model` is present but cannot be
resolved or fails during summary generation, runtime must leave the session
unchanged and report the compaction failure.

## Checkpoints

Compaction stores completed checkpoints separately from transcript messages.
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

Cut-point selection must preserve the latest user task, keep recent context by
token budget, and avoid splitting assistant tool-call messages from their
required tool-result messages. If no safe cut point exists, compaction is a
no-op and must not write a checkpoint.

Summary generation uses a no-tools provider request. The summarization input
may include the previous compaction summary plus newly summarized messages, but
must omit hidden reasoning and provider metadata, truncate large tool material,
and redact obvious secret-like values. The generated summary must be framed as
reference-only continuity context, not active instructions from the user.

## Triggers

Automatic compaction applies to main run/TUI sessions and child-agent sessions.
Temporary `/btw` side sessions are excluded.

For non-interactive `pevo run`, runtime checks compaction before submitting the
prompt. It does not perform normal post-completion compaction before process
exit.

Fullscreen TUI schedules background compaction after a completed turn when the
latest context usage meets the configured threshold or reserve-space rule. If no
bounded latest context usage is available, automatic TUI compaction is not
scheduled. If a prompt is submitted while compaction is running, the prompt is
queued until compaction completes.

If a provider returns a context-overflow error, runtime may compact prior
context and retry the same prompt once. A second overflow is reported normally.

## TUI Command

`/compact [instructions]` performs manual compaction for the current main
session. If a turn is running, the manual compaction request is queued behind
that turn and ahead of later queued prompts. Side conversations reject
`/compact`.

Fullscreen TUI reports compaction completion with before/after token estimates
and a folded summary row. The row is display-only and is not persisted as a
transcript message.

## Attachments

- [Testing](testing.md) defines acceptance scenarios and validation expectations.

## Related Topics

- [006 Context Assembly](../006-context-assembly/spec.md) defines summary
  context as model-visible context.
- [008 Session Continuity](../008-session-continuity/spec.md) defines sessions
  as continuity boundaries.
- [040 SQLite Persistence](../040-storage-and-persistence/sqlite-persistence.md)
  defines the storage shape for checkpoints.
- [120 Provider Registry](../120-provider-registry/spec.md) defines provider
  and model configuration resolution.
- [212 pevo TUI Interaction Slash Commands](../212-pevo-tui-interaction/slash-commands.md)
  defines slash-command interaction behavior.
