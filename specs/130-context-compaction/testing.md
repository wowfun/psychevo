---
name: 130. Context Compaction Testing
psychevo_self_edit: deny
---

# 130. Context Compaction Testing

Define deterministic acceptance coverage for runtime-owned context compaction,
checkpoint persistence, compacted context projection, summary generation, and
TUI `/compact` behavior.

Automation vocabulary and generic validation boundaries follow
[060 Automation](../060-automation/spec.md).

## Long-Term Acceptance Contract

- Effective configuration applies the documented `compression.*` defaults and
  rejects invalid values before a compaction attempt mutates state.
- Disabled compaction, disabled automatic compaction, below-threshold context,
  side chats, missing sessions, insufficient history, and unsafe cut points
  leave the transcript and checkpoint table unchanged.
- Completed compaction appends a checkpoint without deleting or rewriting
  original transcript messages.
- Runtime selects the latest checkpoint that is valid for the current transcript
  boundary and ignores later checkpoints invalidated by undo or revert state.
- Compacted context projection prepends hidden summary context and then includes
  retained messages from the checkpoint boundary onward.
- Cut-point selection preserves the latest user task, keeps recent context by
  token budget, and does not split assistant tool-call messages from required
  tool-result messages.
- Summary generation uses a no-tools provider request, omits hidden reasoning
  and provider metadata, truncates large tool material, redacts obvious
  probable secrets, and frames the summary as reference-only continuity
  context.
- Manual `/compact [instructions]` passes optional instructions into summary
  generation and reports bounded completion feedback.
- Automatic compaction triggers only when bounded latest context usage meets
  the configured threshold or reserve-space rule.
- Context-overflow recovery may compact and retry the same prompt once. A
  second overflow is reported normally.

## Deterministic Tests

Required runtime coverage:

- Configuration parsing and resolution for `enabled`, `auto`,
  `threshold_percent`, `reserve_tokens`, `keep_recent_tokens`, `model`, and
  `reasoning_effort`.
- Provider/model resolution for default invocation model, configured summary
  model, configured reasoning effort, and `none` reasoning suppression.
- No-op results for disabled configuration, disabled auto mode, side chats,
  too little history, below-threshold usage, and no safe boundary.
- Checkpoint append shape, latest-valid checkpoint lookup, and invalidation
  after undo or revert boundaries.
- Projection from compacted sessions without transcript deletion or visible
  transcript summary messages.
- Safe cut-point selection across ordinary messages and assistant tool-call /
  tool-result groups.
- Repeated compaction that incorporates the previous summary and newly compacted
  messages from the previous kept boundary.
- Summary input construction that redacts obvious secrets, truncates large tool
  payloads, omits hidden reasoning/provider metadata, includes manual
  instructions when present, and uses no tools.
- Empty or failed summary provider responses leave the session unchanged and
  return a bounded failure.
- `pevo run` preflight compaction before prompt submission and one retry after
  provider context-overflow errors.
- Child-agent automatic compaction while temporary `/btw` side chats remain
  excluded.

Required TUI and command coverage:

- Slash parsing and menu discovery for `/compact` and
  `/compact [instructions]`.
- Fullscreen manual compaction starts when idle, queues behind an active turn,
  and runs ahead of later queued prompts.
- Fullscreen compaction rejects side chats and reports bounded command
  feedback.
- Automatic TUI compaction schedules after completed turns only when latest
  context usage is bounded and due.
- Prompts submitted during compaction queue until compaction completes.
- Completion feedback includes before/after token estimates and a folded
  display-only summary row.
- Scripted TUI prints bounded compaction feedback without requiring a terminal
  UI.

## Validation

Relevant narrow validation:

- `cargo test -p psychevo-runtime`
- `cargo test -p psychevo-cli`

Broad deterministic validation:

- `scripts/validate-rust.sh broad`

## Validation Boundaries

- Tests should use fake or test providers for summary generation and
  context-overflow retry behavior.
- Tests must isolate `PSYCHEVO_HOME`, `PSYCHEVO_DB`, config files, workdirs,
  and any process environment changes.
- Assertions should target behavior, token-estimate invariants, checkpoint
  validity, and projected context shape rather than provider prompt wording or
  storage field order.
- Live provider, billing, and provider-specific retry validation remain opt-in.

## Related Topics

- [130 Context Compaction](spec.md) defines the functional contract.
- [006 Context Assembly](../006-context-assembly/spec.md) defines hidden
  summary context projection.
- [031 SQLite Persistence](../031-storage-and-persistence/sqlite-persistence.md)
  defines checkpoint storage.
- [120 Provider Registry Testing](../120-provider-registry/testing.md) covers
  provider and compression configuration resolution.
- [270 UI Interaction Testing](../270-ui-interaction/testing.md) covers shared
  slash-command behavior.
- [210 pevo TUI Testing](../210-pevo-tui/testing.md) covers the fullscreen
  terminal projection.
