---
name: 040. SQLite Persistence Attachment
psychevo_self_edit: deny
---

Define the default SQLite persistence implementation contract for the first implementation slice.

This attachment is part of [040 Storage and Persistence](spec.md). It is not an independently numbered spec and does not introduce a new public interface.

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

The default first-slice SQLite shape does not create:
- a separate per-invocation execution-root table
- `facts`
- `refs`
- `artifacts`

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
- `workdir` text
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
means pricing was unknown; `0` means pricing was known and free.

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

The current implementation uses `PRAGMA user_version = 12`, WAL, foreign keys,
short busy timeouts, `BEGIN IMMEDIATE`, bounded jitter retry, and best-effort
periodic `wal_checkpoint(PASSIVE)` every 50 successful writes.

Long-running processes should open SQLite state once through the cloneable
`StateRuntime` handle and reuse that handle across high-level runtime calls.
Path-based database opening remains a low-level initialization and migration
concern, not the default high-level runtime contract. The fullscreen TUI must
avoid idle high-frequency database polling; live agent reload checks are
rate-limited to at most once every 250 ms while preserving immediate checks
after session switches.

The version 11 slice creates `session_compactions` for completed context
compaction checkpoints. The checkpoints affect runtime context projection but
do not rewrite message transcript rows.

The version 8 slice creates `agent_edges` for durable parent-to-child agent
coordination. The edge tracks `open`/`closed` coordination state separately
from child session completion.

The version 7 slice creates contextual-user grouping columns in
`context_evidence` for new databases and supported migrations. It still migrates
version 6 databases by adding those columns, still migrates version 4 databases
by adding message accounting columns, and still migrates version 3 state
databases by adding `sessions.archived_at_ms` before applying version 5
additions. It does not
automatically migrate version 1 or version 2 state databases. Opening an older
state database must fail with an explicit cutover/reset instruction instead of
silently mutating retained state.

## Retrieval

Final result material, tool result material, and artifact material retained in the first slice are retrieved through session, evidence, message, and material relationships.

This attachment does not define public locators, path formats, cursor formats, database keys, or transport payloads.

## Related Topics

- [040 Storage and Persistence](spec.md) defines the storage boundary.
- [030 State and Data Model](../030-state-and-data-model/spec.md) defines semantic state relationships.
- [030 Session Record Model](../030-state-and-data-model/session-record-model.md) defines logical session and message records.
- [005 Durable Evidence](../005-durable-evidence/spec.md) defines evidence semantics.
- [100 Runtime Assembly](../100-coding-agent/runtime-assembly.md) defines the first runtime wiring that writes these records.
