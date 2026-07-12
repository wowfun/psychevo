---
name: 031. SQLite Persistence Attachment
psychevo_self_edit: deny
---

Define the default SQLite persistence implementation contract for the first implementation slice.

This attachment is part of [031 Storage and Persistence](spec.md). It is not an independently numbered spec and does not introduce a new public interface.

## Scope

- SQLite-backed session and message persistence
- default first-slice schema shape
- minimal SQLite behavior requirements
- schema version boundary
- bounded write contention behavior
- relationship-based retrieval of retained material

Out of scope:
- full-text search, trigram search, vector search, indexes beyond implementation need, pagination, sorting, or query language design
- branch trees, fork UI, retry UI, undo UI, merge behavior, or transcript search
- artifact storage engines, external blob stores, checkpoint stores, or provider credential stores
- migration framework design beyond a minimal schema version boundary
- Rust APIs, payload schemas, public identifiers, CLI behavior, SDK behavior, or transport behavior

## Schema Shape

The default first-slice SQLite shape contains:
- `sessions`
- `messages`
- `context_evidence`
- `agent_edges`
- `session_compactions`
- `gateway_source_bindings`
- `gateway_activities`
- `gateway_live_events`
- `gateway_live_snapshots`
- `gateway_control_commands`
- `gateway_turn_terminals`
- `gateway_turn_deliveries`
- `gateway_channel_outbox`
- `automations`
- `automation_runs`

The default first-slice SQLite shape does not create:
- a separate per-invocation execution-root table
- `facts`
- `refs`
- `artifacts`
- a complete per-turn event-log table

This attachment defines implementation shape, not public contract shape. The
first implementation slice uses the columns below as an internal contract, not
as a long-term public storage API. Storage must still preserve the logical
record fields required by [030 Session Record Model](../030-state-and-data-model/session-record-model.md).

## Sessions

The `sessions` storage shape persists logical session record material from [030 Session Record Model](../030-state-and-data-model/session-record-model.md).

Required semantics include:
- one row or equivalent durable unit per persistent session
- session identity suitable for relating messages to the session
- lifecycle timestamps and optional ended state
- optional parent session relationship
- counters or summaries needed by implementation
- metadata space for model and working-context summaries

Reopening the same persistent session uses the existing session row rather than
creating a new execution root. Opening, resuming, or viewing a session is not
activity for `updated_at_ms`; persisting new transcript material is activity
and may clear ended or archived state.

The first implementation slice stores these session columns:

- `id` text primary key, UUID v7
- `source` text
- `parent_session_id` text nullable
- `cwd` text
- `model` text
- `provider` text
- `started_at_ms` integer
- `updated_at_ms` integer
- `ended_at_ms` integer nullable
- `end_reason` text nullable
- `archived_at_ms` integer nullable
- `message_count` integer
- `tool_call_count` integer
- `title` text nullable
- `metadata_json` text nullable

## Gateway Source Bindings

The `gateway_source_bindings` storage shape persists Gateway source-to-thread
routing facts from [021 Gateway](../021-gateway/spec.md). It is not a
second transcript or execution truth source.

Required semantics include:
- one active binding per deterministic source key
- durable relationship from source key to current Psychevo thread/session
- raw source identity retained for routing and local debugging
- optional visible label retained for UI projection
- backend kind and optional backend-native id for future peer-agent backends
- an unbound source draft for the top-level Agent Definition, Runtime Profile,
  and typed control values; binding clears the draft atomically
- timestamps and lineage metadata for reset/rebind audit

The first implementation slice stores these columns:

- `source_key` text primary key
- `source_kind` text
- `raw_identity_json` text
- `visible_name` text nullable
- `thread_id` text foreign key
- `backend_kind` text
- `backend_native_id` text nullable
- `draft_agent_ref` text nullable
- `draft_profile_ref` text nullable
- `draft_control_values_json` text nullable
- `created_at_ms` integer
- `updated_at_ms` integer
- `lineage_json` text nullable

## Gateway Runtime Coordination

Gateway runtime coordination tables persist local coordination facts needed for
cross-surface status, recovery, and control. They are not ordinary transcript
material and must not become model-visible context.

`gateway_runtime_bindings` stores the immutable `RunnableTarget` captured for a
public thread. A resolved row contains nullable `agent_ref` (`NULL` denotes the
explicit default Agent), non-null `agent_fingerprint` and
`agent_definition_json`, Runtime Profile identity/fingerprint/revision/snapshot,
Adapter identity, ownership, optional native session identity, and binding
revision. Repeating the same capture is idempotent; changing either Agent or
Runtime Profile evidence is an immutable-binding conflict. Schema v28 is a
reset cutover, so no legacy row is accepted without Agent snapshot evidence.

The row also owns the common Thread control state. `thread_preferences_json`
stores user preferences independently from `runtime_observed_json`, and
`control_revision` advances through compare-and-set writes without mutating the
immutable binding revision. Callers must supply both expected binding and
control revisions. A stored preference, an Adapter acknowledgement, and a
runtime observation remain distinct facts; an acknowledgement never writes an
observed value unless the Adapter supplied that observation.

`gateway_activities` stores the active or recently settled Gateway activity
claim for a running turn or shell command. Required semantics include:
- durable ownership for a running activity across surfaces
- relationship to a thread, source key, and turn/activity identity when known
- status sufficient to distinguish running, queued, completed, failed,
  interrupted, superseded, and released states
- lease and owner fields sufficient to detect stale foreign owners
- queued-turn count for status projection
- optional intent metadata for local recovery/debug projection

For Agent turns, `gateway_activities.intent_json` may retain non-content routing
and recovery fields, but it must not outlive the bounded delivery ledger as
another copy of user content. Confirmed delivery removes input from both the
delivery and generic activity rows in one store transaction. The unique
terminal performs the same scrub as a fallback.

`gateway_live_events` stores a bounded retained boundary-event relay buffer for
foreign-surface replay. It is an observation buffer, not the transcript source
of truth. High-frequency transcript updates such as entry deltas and repeated
entry updates are not retained in this append buffer.

`gateway_live_snapshots` stores coalesced latest live transcript observations
for foreign-surface replay while an activity is running. A row is keyed by the
running activity, turn, and transcript entry identity, and is updated in place
with a revision counter. Consumers must reconcile snapshots against durable
message and terminal facts and may discard stale snapshots after bounded
retention or activity completion.

`gateway_control_commands` stores cross-process control requests for activities
owned by another Gateway process. It is a command mailbox for interrupt,
takeover, steering, permission, and clarify control paths, not a transcript or
audit log.

`gateway_turn_terminals` stores the narrow accepted-turn terminal lifecycle fact
defined by [030 Turn Lifecycle](../030-state-and-data-model/turn-lifecycle.md).
Required semantics include:
- one terminal fact per accepted turn identity
- relationship to the owning thread/session
- status `completed`, `failed`, or `interrupted`
- optional outcome and bounded display error message
- timestamps sufficient for transcript diagnostic ordering
- optional metadata for local projection only

Failed and interrupted terminal facts may be projected as diagnostic/status
rows by product transcript views. They must not be stored as assistant messages
or counted as loop-visible transcript messages.

`gateway_turn_deliveries` is the implementation-neutral bounded delivery ledger
for accepted Agent prompts. It retains prompt content only until the selected
Adapter confirms delivery. Confirmation clears content and preserves the hash,
delivery state, binding revision, and timestamps. Unknown delivery keeps enough
state for reconciliation but never authorizes an automatic resend.

`gateway_channel_outbox` retains an Agent final payload only while the
target platform has not acknowledged delivery. Acknowledgement clears the
payload and preserves its hash, delivery state, and timestamps. Neither table
is transcript content, model context, or an alternate history owner.

Rows in `gateway_live_events` and `gateway_live_snapshots` are transient
cross-process delivery material. The unique terminal clears them; reconnect
obtains content from the thread's declared History owner.

## Automations

The `automations` and `automation_runs` storage shapes persist local product
automation facts from [060 Automation](../060-automation/spec.md). They are
coordination and inspection records, not a replacement for transcript,
message, or Gateway activity records.

`automations` stores one definition per local automation task. Required
semantics include:
- durable task identity
- cwd scope
- target kind, either project automation or thread heartbeat
- optional target thread id for thread heartbeats
- title and prompt text
- structured schedule JSON
- enabled state
- execution policy JSON including permission mode and sandbox default
- optional model and reasoning-effort selection
- last-run, next-run, and bounded error projection fields
- timestamps for creation and update

The first implementation slice stores these automation columns:

- `id` text primary key, UUID v7
- `cwd` text
- `kind` text, `project` or `thread_heartbeat`
- `target_thread_id` text nullable
- `title` text
- `prompt` text
- `schedule_json` text
- `enabled` integer boolean
- `execution_json` text
- `model` text nullable
- `reasoning_effort` text nullable
- `source_key` text nullable
- `created_at_ms` integer
- `updated_at_ms` integer
- `last_run_at_ms` integer nullable
- `next_run_at_ms` integer nullable
- `last_status` text nullable
- `last_error` text nullable

`automation_runs` stores bounded run coordination and result projection for one
attempt to run a task. Required semantics include:
- durable relationship to one automation task
- status sufficient to distinguish running, completed, failed, skipped, and
  interrupted
- trigger source such as schedule or manual run
- started/completed timestamps
- target thread/source details when known
- bounded error message for inspection

The first implementation slice stores these run columns:

- `id` text primary key, UUID v7
- `automation_id` text foreign key
- `trigger` text
- `status` text
- `started_at_ms` integer
- `completed_at_ms` integer nullable
- `thread_id` text nullable
- `source_key` text nullable
- `error` text nullable
- `metadata_json` text nullable

## Agent Edges

The `agent_edges` storage shape persists parent-to-child agent coordination
edges. It complements `sessions.parent_session_id`; it does not replace session
lineage and is not a separate agent-run truth source.

Required semantics include:
- one incoming edge per child agent session
- stable relationship from parent session to child session
- lifecycle coordination status independent of the child session's completed,
  failed, or interrupted execution status
- metadata space for runtime projection fields such as durable agent id,
  task name, agent definition name, role, and depth

The first implementation slice stores these columns:

- `parent_session_id` text
- `child_session_id` text primary key
- `status` text, `open` or `closed`
- `created_at_ms` integer
- `updated_at_ms` integer
- `metadata_json` text nullable

## Session Compactions

The `session_compactions` storage shape persists completed context compaction
checkpoints. It is context-projection state, not transcript material.

Required semantics include:
- durable relationship to one session
- one row or equivalent durable unit per successful compaction checkpoint
- summary text that may be projected as hidden summary context
- session sequence boundaries for retained messages and checkpoint validity
- reason, token estimates, summary model/provider, and optional manual
  instruction metadata

Compaction checkpoints must not delete or rewrite rows in `messages`, must not
increment `sessions.message_count`, and must not appear in default transcript
retrieval. Runtime may ignore checkpoints that are newer than the current
effective undo/revert boundary.

The first implementation slice stores these compaction columns:

- `id` integer primary key autoincrement
- `session_id` text foreign key
- `created_at_ms` integer
- `reason` text
- `summary_text` text
- `first_kept_session_seq` integer
- `created_after_session_seq` integer
- `tokens_before` integer nullable
- `tokens_after` integer nullable
- `summary_provider` text
- `summary_model` text
- `instructions` text nullable
- `metadata_json` text nullable

## Messages

The `messages` storage shape persists logical message record material from [030 Session Record Model](../030-state-and-data-model/session-record-model.md).

Required semantics include:
- durable relationship to one session
- role and loop-visible material
- timestamp or durable ordering material
- optional assistant tool-call material
- optional tool-result relationship aids
- optional token count, finish, outcome, reasoning, model, or provider metadata
- optional normalized usage metrics and allowlisted provider metadata associated
  with completed assistant messages

The first implementation slice must preserve the logical message field support required by [030 Session Record Model](../030-state-and-data-model/session-record-model.md), including tool-call fields, tool-result relationship aids, local folded reasoning blocks, and model/provider metadata when present.

The first implementation slice stores large tool material in message material or metadata when practical. External artifact storage belongs to a later attachment or spec.

The first implementation slice stores these message columns:

- `id` integer primary key autoincrement
- `session_id` text foreign key
- `session_seq` integer
- `role` text
- `timestamp_ms` integer
- `message_json` text
- `content_text` text nullable
- `tool_call_id` text nullable
- `tool_name` text nullable
- `tool_calls_json` text nullable
- `finish_reason` text nullable
- `outcome` text nullable
- `model` text nullable
- `provider` text nullable
- `usage_json` text nullable
- `metadata_json` text nullable
- `context_input_tokens` integer nullable
- `billable_input_tokens` integer nullable
- `billable_output_tokens` integer nullable
- `reasoning_tokens` integer nullable
- `cache_read_tokens` integer nullable
- `cache_write_tokens` integer nullable
- `reported_total_tokens` integer nullable
- `estimated_cost_nanodollars` integer nullable
- `pricing_source` text nullable
- `pricing_tier` text nullable
- `cost_status` text nullable
- `pricing_missing_reason` text nullable
- `pricing_version` text nullable

`message_json` is the authoritative retained message material. The other
message columns are relationship, query, and summary aids. Reasoning is stored
only in `message_json` as assistant content blocks; provider wire fields such
as `reasoning_content` are request projections and are not persisted as
separate columns.

`usage_json` stores normalized usage metrics when reported by a provider.
`metadata_json` stores allowlisted provider metadata suitable for local debug
projection and local per-message metric facts such as tool-result elapsed
duration. Neither column is part of transcript content, and neither may be
serialized into sanitized transcript messages.

Dedicated accounting columns store structured token and local estimated-cost
facts derived from `usage_json` and resolved model pricing. They are summary
aids for stats and UI projection, not billing records. `NULL` estimated cost
means pricing was unknown unless `cost_status` explicitly says the request was
included. `0` means pricing was known and free. `cost_status` is one of
`estimated`, `free`, `included`, or `unknown`; aggregate views may add `mixed`
when a window contains both priced and unknown messages. `pricing_source`,
`pricing_tier`, `pricing_missing_reason`, and `pricing_version` describe the
local price metadata used to produce the estimate and must not be interpreted as
provider billing records.

Message ordering is authoritative by `(session_id, session_seq)`, not by
timestamp. The first implementation enforces `UNIQUE(session_id, session_seq)`.

## Context Evidence

The `context_evidence` storage shape persists model-visible runtime injections
that are not loop-visible transcript messages. It is durable evidence for
agent-invocation assembly, not retained message material.

Required semantics include:
- durable relationship to one session and one accepted user prompt
- deterministic ordering among evidence items for that prompt
- role used in the model-facing context projection
- source kind and source identity sufficient for local audit
- retained model-visible injected text
- optional metadata for source-specific facts

The first implementation slice stores these context evidence columns:

- `id` integer primary key autoincrement
- `session_id` text foreign key
- `prompt_session_seq` integer
- `context_seq` integer
- `role` text
- `source_kind` text
- `source_name` text nullable
- `source_path` text nullable
- `provider_group` text nullable
- `provider_block_index` integer nullable
- `context_kind` text nullable
- `timestamp_ms` integer
- `content_text` text
- `metadata_json` text nullable

Context evidence is anchored to `messages(session_id, session_seq)` for the
accepted prompt and enforces `UNIQUE(session_id, prompt_session_seq,
context_seq)`. `provider_group` and `provider_block_index` preserve hidden
contextual-user grouping for reconstructed provider requests. Deleting the
prompt message deletes its context evidence. Context evidence does not increment
`sessions.message_count`, is not returned by message transcript retrieval, and
is not used by default session resume.

## SQLite Behavior

SQLite persistence should enable WAL mode when supported.

SQLite persistence must enable foreign key enforcement when relationships depend on foreign keys.

Writes that persist one accepted message or one bounded session update must use bounded write transactions.

Schema versioning must exist through `PRAGMA user_version` or a minimal schema metadata mechanism.

SQLite persistence must handle write contention with bounded retry and backoff or surface a bounded storage failure to runtime. Retry loops must not be unbounded.

SQLite persistence should perform periodic WAL checkpoint work when supported by the deployment shape.

Storage failures that affect session or message persistence must be observable to runtime or caller-facing layers that depend on persistence.

The current implementation uses `PRAGMA user_version = 28`, WAL, foreign keys,
short busy timeouts, `BEGIN IMMEDIATE`, bounded jitter retry, and best-effort
periodic `wal_checkpoint(PASSIVE)` every 50 successful writes.

Long-running processes should open SQLite state once through the cloneable
`StateRuntime` handle and reuse that handle across high-level runtime calls.
Path-based database opening remains a low-level initialization and migration
concern, not the default high-level runtime contract. The fullscreen TUI must
avoid idle high-frequency database polling; live agent reload checks are
rate-limited to at most once every 250 ms while preserving immediate checks
after session switches.

Version 28 is the pre-release Native/ACP Application Architecture cutover. The
minimum supported version is also 28. Earlier schemas, captured direct bindings,
and direct-only coordination rows are intentionally not migrated; opening them
fails with the standard `pevo init --reset-state` recovery instruction, whose
backup-before-recreate behavior remains unchanged.

The version 23 slice adds local product automation definition and run
coordination tables.

The historical version 22 slice added `gateway_live_snapshots` for coalesced
live transcript observation replay. Its migration path is no longer reachable
through the version-28 minimum.

The pre-release current-working-directory naming cutover to `cwd` is a state
reset boundary, not a compatibility migration. Development databases created
with the former column name must be rebuilt with `pevo init --reset-state`
instead of being silently rewritten on open; the runtime then creates the
current `cwd` schema from first principles.

The historical version 21 slice added structured cost status, pricing
missing-reason, and pricing version columns for local accounting projections.
It is documentation of schema lineage, not a supported opening boundary.

The version 11 slice creates `session_compactions` for completed context
compaction checkpoints. The checkpoints affect runtime context projection but
do not rewrite message transcript rows.

The version 8 slice creates `agent_edges` for durable parent-to-child agent
coordination. The edge tracks `open`/`closed` coordination state separately
from child session completion.

The historical version 7 slice added contextual-user grouping columns in
`context_evidence`; earlier version-3/4/6 migration steps are retained only as
schema lineage. No schema below version 28 is accepted by the current opener.

## Retrieval

Final result material, tool result material, and artifact material retained in the first slice are retrieved through session, evidence, message, and material relationships.

This attachment does not define public locators, path formats, cursor formats, database keys, or transport payloads.

## Related Topics

- [031 Storage and Persistence](spec.md) defines the storage boundary.
- [030 State and Data Model](../030-state-and-data-model/spec.md) defines semantic state relationships.
- [030 Session Record Model](../030-state-and-data-model/session-record-model.md) defines logical session and message records.
- [005 Durable Evidence](../005-durable-evidence/spec.md) defines evidence semantics.
- [100 Runtime Assembly](../100-coding-agent/runtime-assembly.md) defines the first runtime wiring that writes these records.
