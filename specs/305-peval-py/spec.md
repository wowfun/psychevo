---
name: 305. peval-py
psychevo_self_edit: deny
---

# 305. peval-py

Define `peval-py`, the lightweight Python edition of `peval`. The current
capability is offline agent trajectory export and reporting; future
capabilities may add more evaluation-adjacent inspection scenarios under the
same command tree.

## Scope

- offline trajectory export of one session from JSONL or SQLite `messages` rows
- ATIF v1.7 trajectory projection
- single-session and session-comparison JSON/HTML report generation
- adapter-specific message readers for Psychevo, OpenCode, and Hermes
- deterministic local tests for the `peval-py` package

Out of scope:

- agent execution, benchmark execution, scoring, reruns, or `peval` workspace
  mutation
- live providers, network services, ACP server startup, or official benchmark
  harnesses
- benchmark/task comparison matrices; `peval-py` comparison is session-first
  and does not introduce benchmark or task axes
- generic runtime debug tables as canonical sources for v1 conversion

## Position

The CLI lives under `tools/peval-py/` and is runnable with `uv`. Its console
command is `peval-py`. It is a simplified Python companion to the Rust `peval`
CLI that is lightweight enough to install and use on its own. It is
independent from the Rust workspace and has no runtime dependencies outside the
Python standard library.

The tool reads existing retained session material and produces derived files.
It must not update Psychevo state databases, peval workspaces, benchmark
artifacts, or user config.

## Inputs

The command supports one input source family per run:

- `-p, --path PATH` reads one JSONL file containing one JSON object per line.
  `view trajectory` may repeat `-p` to compare sessions.
- `-d, --db PATH` plus `-s, --session-id ID` reads SQLite `messages` rows.
  `view trajectory` may repeat `-s` against the same DB to compare sessions.

JSONL and SQLite input families cannot be mixed in one invocation. `-s,
--session-id` is valid only with `-d, --db`.

The command surface follows a peval-style verb and scenario shape:

- `peval-py view trajectory ...` writes a peval-compatible JSON or HTML report
  for one or more sessions.
- `peval-py export trajectory ...` writes a single ATIF trajectory object.

`tr` is an accepted alias for `trajectory`, so `peval-py view tr ...` and
`peval-py export tr ...` are equivalent to the long scenario form.

Common trajectory flags use both long and short forms:

- `-c, --config PATH`
- `-a, --adapter psychevo|opencode|hermes`
- `-o, --output [PATH]`
- `-f, --format json|html` for `view trajectory`
- `-n, --note N=TEXT` for `view trajectory`, where `0` is report-level and
  positive one-based indexes attach to the ordered input sessions

When `-o/--output` is omitted, commands write to stdout. When `-o/--output` is
present without a path, the default file name includes the selected adapter and
session identity. Single-session `export trajectory` writes
`trajectory-<adapter>-<session>.json`. Single-session `view trajectory` writes
`report-<adapter>-<session>.html`, or `report-<adapter>-<session>.json` when
`--format json` is set. Multi-session `view trajectory` writes
`report-<adapter>-sessions-<count>.<format>`. Unsafe filename characters are
replaced with `-`, and missing session ids fall back to `session`.

`export trajectory` remains single-session only. Multiple `-p` values or
multiple `-s` values must fail clearly for export.

TOML config uses `defaults.adapter` for the input adapter default. Older
`defaults.agent` config keys may be accepted for local compatibility, but the
public CLI and docs use `adapter`.

JSONL accepts either direct message objects or wrapper objects containing
`message`, optional `usage`, optional `metadata`, optional `accounting`, and
optional `session_seq`.

SQLite reads only the configured `messages` table in v1. The default Psychevo
mapping reads `session_seq`, `message_json`, `usage_json`, `metadata_json`, and
accounting columns ordered by `session_seq`. Table and column names supplied by
config must be SQL identifiers, not raw SQL fragments.
For SQLite inputs, the selected `--session-id` is report metadata even when it
is not duplicated inside individual message rows. ATIF output must set
`session_id` from that selected id.

## Adapters

`-a, --adapter` selects one adapter:

- `psychevo` supports current Psychevo retained messages with
  `role=user|assistant|tool_result`, user text blocks, assistant text,
  reasoning, and tool-call blocks.
- `opencode` and `hermes` are separate adapter modules from v1. They support a
  common single-session message JSONL shape and provide agent-specific defaults
  so native formats can be added without changing the CLI surface.

Adapters may preserve source metadata in report sidecars, but ATIF output must
stay standard and must not include peval-only fields.

## Outputs

`export trajectory` writes a single ATIF trajectory object. `view trajectory`
writes either JSON or HTML:

- JSON is a self-contained peval view v17 subset with `schema_version`,
  `includes`, `scope`, `path_selections`, `trajectory`, and
  `trajectory_meta`. Multi-session view reports also include `comparison`;
  reports with notes also include `annotations`.
- HTML is a single offline file with inline CSS and JavaScript. It renders the
  selected Trial trajectory, step rows, reasoning, message, tool-call,
  observation, metrics cues, and one combined Expand all / Collapse all control.
  The page head contains only `Agent Trajectory Report`; agent/model and metric
  summaries stay inside the Run and Result sections instead of appearing as a
  separate top banner.

Single-session HTML renders the current Run, Result, Evidence, and Steps
sections. Multi-session HTML follows the Rust `peval view` report structure:
Report Notes, Visible Heatmap, Leaderboard, then the selected Trial trajectory.
`peval-py` treats each input session as one Trial. The Visible Heatmap supports
metric switching for duration, tokens, tool calls, and turns. It uses a
session/trial row axis: each input session occupies one row with a left-side
session label and one heatmap cell, so large comparisons grow vertically rather
than as an unbounded horizontal row. The Leaderboard shows session,
adapter/model, result, duration, turns, tools, tokens, cost, and notes. The
rendered comparison sections must not show benchmark, task, task-set,
task-family, or matrix task-axis fields.

Report timing, tool/observation grouping, and trajectory row visualization
follow [340 Trajectory](../340-agent-evaluation/trajectory.md). This spec
defines the standalone CLI input and projection behavior rather than a separate
trajectory display semantic.

The ATIF schema string is `ATIF-v1.7`. Step ids are sequential. Step `source`
is one of `system`, `user`, or `agent`. Tool observations use
`source_call_id` to reference the originating tool call when known.
When a tool-result message has a `tool_call_id` matching a prior assistant
tool call, the observation is attached to that assistant Agent step instead of
being emitted as a separate observation-only step. Unmatched tool results remain
standalone Agent observation steps and add a conversion warning.

Tool timing comes from message metadata when available. For Psychevo messages,
`metadata_json.elapsed_ms` on tool-result rows is the preferred tool execution
duration. If absent, converters may fall back to the elapsed wall time between
the assistant tool-call timestamp and the tool-result timestamp.

Single-session report defaults use deterministic peval-compatible placeholders
for eval-only fields: benchmark, case, task-set, task, and task family are
`session`; status is `passed` unless conversion warnings or errors require
`failed`; adapter is the selected adapter id.

Multi-session report rows are ordered by input order. Each input session is one
trial. Trial keys are deterministic from the displayed session id, with
collision suffixes when repeated ids appear. If a JSONL input does not contain a
session id in the message, metadata, or wrapper, its displayed session id falls
back to the JSONL file stem. The default selected session is the first failed
session, otherwise the first session.

`-n/--note 0=TEXT` adds a report-level note. `-n/--note N=TEXT` attaches a note
to the one-based input session index after ordering. Repeated notes append in
CLI order. Invalid note syntax or out-of-range indexes must fail clearly. JSON
preserves note `markdown` text; HTML renders report notes, Leaderboard note
snippets, and selected Trial notes with peval-style note markup. Raw HTML in
notes must be escaped before Markdown display and must not execute.

## Redaction

Reports redact obvious secret-bearing keys, authorization headers, bearer
tokens, and common provider key patterns by default. `--no-redact` disables
redaction explicitly. Redaction applies before writing JSON and before
embedding report data in HTML.

## Related Topics

- [300 peval CLI](../300-peval-cli/spec.md)
- [300 Reporting](../300-peval-cli/reporting.md)
- [340 Agent Evaluation](../340-agent-evaluation/spec.md)
- [340 Trajectory](../340-agent-evaluation/trajectory.md)
