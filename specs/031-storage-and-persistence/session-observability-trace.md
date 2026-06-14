---
name: 031. Session Observability Trace Sidecar
psychevo_self_edit: deny
---

Define the durable observability sidecar for Psychevo sessions.

## Boundary

The session observability trace is an append-only JSONL sidecar for runtime
observation events. It is not the session transcript, not model context, not a
compaction input, and not an export/share source of truth. SQLite `sessions` and
`messages` remain the retained session truth.

For a persistent state database, each session trace is stored at
`<state_root>/sessions/<session_id>/events.jsonl`, where `state_root` is the
parent directory of `StateRuntime.db_path()`. `PSYCHEVO_DB=:memory:` disables the
trace sidecar. Session identifiers used in trace paths must be validated before
joining filesystem paths.

## Record Shape

Each line is one self-contained JSON object with this envelope:

- `schema_version`: integer trace schema version, currently `2` for new
  writers; readers may still return older `1` records
- `seq`: per-session monotonic sequence number
- `event_id`: unique event id
- `session_id`: owning session id
- `invocation_id`: accepted runtime invocation id
- `turn_index`: current turn index when known
- `kind`: typed event kind
- `timestamp_ms`: wall-clock event timestamp from the local runtime clock
- `monotonic_offset_ms`: offset from invocation start when known
- `source`: producer, currently `runtime`
- `correlation`: typed ids such as `tool_call_id`, `tool_name`, `message_role`,
  `generation_id`, `parent_tool_call_id`, or `child_session_id`
- `redaction_state`: `redacted` for the default trace writer
- `payload`: bounded typed payload

Version 2 records compact lifecycle, timing, and summary facts by default:
`run_start`, `agent_start`, `agent_end`, `turn_start`, `turn_end`,
`generation_start`, `generation_end`, `tool_execution_start`,
`tool_execution_end`, `message_end`, `reasoning_end`, and `run_summary`.
High-frequency live stream events such as `message_update`,
`tool_call_pending`, `reasoning_delta`, and `tool_execution_update` are not
persisted by the default writer. They are counted in `run_summary` as
coalesced observations. `message_start` is also omitted from the default durable
trace because `message_end` is the retained message lifecycle fact.
`run_summary` is a trace-level accounting footer. It is not the transcript, not
the timing source of truth, and not a duplicated tool or generation index.

Lifecycle and timing facts should be preserved under normal conditions, but the
writer may drop observability events under backpressure rather than blocking
user-facing execution.

## Payload Policy

The default trace writer stores redacted typed payloads only. It must not store
raw provider requests or responses, full unbounded tool outputs, image data
URLs, unbounded streamed text/reasoning deltas, or hidden raw diagnostics.

Tool arguments and results are stored as summaries only: tool name/id, start or
end timing, outcome, selected title fields such as `cmd`, `path`, `url`,
`name`, and `query`, and shape facts such as field counts, item counts,
character counts, byte counts, or truncation flags. They must not store body
previews for large command output, file content, fetch content, or write
content, and default traces must not persist UI display policy metadata such as
tool title-key lists. Object summaries keep type, field count, and selected
title fields; they do not need full field-name inventories.

`message_end` stores only role, timestamp, and a bounded message summary. It
does not duplicate usage, accounting, or message metadata; SQLite `messages`
remains the retained transcript truth, and `generation_end` remains the compact
usage/timing fact for model calls. `run_start` stores compact provider/model/
mode, permission, reasoning, source, skill, and context summaries, not full
local paths, database paths, base URLs, raw credential-related fields, or
catalog/debug inventory counts.

`run_summary` stores only accounting that cannot be reconstructed reliably from
the compact lifecycle facts. Its payload contains `summary_kind:
"accounting_footer"`, coalesced event counts, dropped counts by reason and
kind, bounded per-turn coalesced accounting with `turn_index` and
`coalesced_events`, low-cardinality coalesced counts by tool name, and
`omitted_counts` for bounded arrays. It must not duplicate observed/persisted
event counts, per-tool-call coalesced details, generation summaries, or tool
start/end timing, outcome, argument summaries, or result summaries already
represented by lifecycle facts.

## Reliability

Trace write, read, cleanup, enrichment, and debug rendering are observational
work only. They must not block or fail user-facing session execution, message
persistence, transcript rendering, export/share, or ordinary UI interaction.
Trace absence must not degrade the user's primary experience.

Trace write failures must not fail the agent loop or block final message
persistence. They must be observable as a warning or final diagnostic for that
invocation. Readers ignore a malformed final JSONL line and return a warning;
malformed non-final lines are reported as warnings and skipped.

When appending to an existing trace, writers continue from the largest valid
`seq` in the file. Session delete attempts to remove the session trace
directory, but cleanup failure must not cancel the session deletion result;
callers with a diagnostic surface may report a warning. Archive and restore
preserve trace files. Resetting the state root removes traces with the rest of
local state.

## Consumers

Gateway may expose trace records through a bounded `thread/trace` read API for
debugging surfaces. Workbench may render those records in its Debug panel.
Evaluation adapters may prefer trace timing over restored message metadata when
converting retained Psychevo sessions. Session export and share artifacts do not
include trace records in this version.

Direct trace JSONL conversion is supported for version 1 traces that include
message payloads. Version 2 compact traces are timing and debug sidecars, not
standalone transcript exports; consumers that need full transcript content must
read SQLite session messages instead.
