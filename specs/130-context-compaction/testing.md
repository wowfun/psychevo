---
name: 130. Context Compaction Testing
psychevo_self_edit: deny
---

# 130. Context Compaction Testing

Define deterministic acceptance coverage for runtime-owned context compaction,
checkpoint persistence, compacted context projection, summary generation, TUI
`/compact` behavior, and Gateway/Workbench/Channels compaction routing.

CI/CD vocabulary and generic validation boundaries follow
[065 CI/CD](../065-ci-cd/spec.md).

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
- Gateway transcript projection shows completed checkpoints as `Session
  compacted` diagnostic divider entries with collapsed review detail and without
  emitting visible user/assistant summary messages.
- Persisted transcript messages remain in authoritative `session_seq` order
  when timestamps collide or move backward; checkpoint dividers merge at their
  `created_after_session_seq` structural boundary and turn terminals merge at
  their persisted `lastCommittedSeq` boundary.
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
- Gateway `thread/compact/start`, Workbench `/compact`, and Channels `/compact`
  all route through native runtime compaction rather than ordinary prompt
  submission.
- Direct Codex waits past `thread/compact/start` acknowledgement for the
  matching `contextCompaction` item completion. EOF/process exit before that
  item completes returns one typed failure and never a false compacted result.
- Direct Codex rejects custom summary instructions before native delivery and,
  on success, records one projection-only divider without hiding or rewriting
  local messages. Context assembly ignores that marker.
- Direct OpenCode returns an explicit unavailable result until adapter-owned
  compaction exists.
- Gateway derives compaction backend identity from its authoritative
  thread/source binding; an omitted or forged native `runtimeRef` cannot compact
  a direct or peer-backed mirror transcript.
- A Channels `/compact` accepted during an active turn is atomically queued
  ahead of later prompts while result waiting and reply delivery do not block
  the channel poll loop.
- Successful Gateway auto-compaction exposes the exact new checkpoint divider
  in the live turn completion and emits transient compacting activity without
  replaying older checkpoints or persisting the activity row.

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
- Gateway `thread/compact/start` resolves an explicit thread id or source-bound
  thread, serializes behind active work, returns structured compact/no-op/error
  results, and refreshes transcript projection from the checkpoint table.
- Gateway compact-boundary coverage binds a thread/source to a peer or direct
  backend and proves that omitted or forged native runtime assertions return
  unavailable without writing a checkpoint.
- Workbench `/compact [instructions]` command execution returns a compact action
  and calls `thread/compact/start`; it does not call `turn/start`.
- Workbench transcript rendering shows the divider as collapsed read-only
  checkpoint detail with the generated summary behind disclosure.
- Channels `/compact [instructions]` calls the same Gateway operation and sends
  concise result text; it does not enqueue `Compact this session` as a prompt.
  Active-turn coverage proves the request is accepted, the poll loop remains
  responsive to approval/stop work, and compaction runs before a later prompt.
- Runtime profile compaction coverage verifies native and direct Codex route to
  their runtime-owned operations, while OpenCode profiles report unavailable.
- Gateway native auto-compaction after a completed turn performs a bounded
  precheck, shows transient compacting status when due, and projects the
  exact newly-created checkpoint divider in live `committedEntries` on success.
- Transcript projection coverage uses deliberately inverted and colliding
  timestamps to prove persisted messages retain `session_seq` order and
  checkpoint and terminal diagnostics are inserted after their persisted
  structural sequence boundary.

## Validation

Relevant narrow validation:

- `cargo test -p psychevo-runtime`
- `cargo test -p psychevo-gateway`
- `cargo test -p psychevo-cli`
- `pnpm --filter @psychevo/workbench test`

Broad deterministic validation:

- `cargo xtask ci run --profile rust-broad`
- `cargo xtask ci run --profile visual`
- `cargo xtask live run --all --env shared`

## Validation Boundaries

- Tests should use fake or test providers for summary generation and
  context-overflow retry behavior.
- Tests must isolate `PSYCHEVO_HOME`, `PSYCHEVO_DB`, config files, cwds,
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
