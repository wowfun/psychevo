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
- config-selected English and Simplified Chinese HTML report UI localization
- translated canonical docs under `docs/i18n/<locale>/...`
- localized tool README files beside their original README files
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

The command supports path and DB input sources:

- `-p, --path PATH` reads one source file. By default this accepts JSONL with
  one JSON object per line or an exported ATIF JSON trajectory object. Adapters
  may parse other path formats directly.
- `-d, --db PATH` reads an adapter-owned SQLite persistence format. `view
  trajectory` may repeat `-d` to compare sessions across adapters.
- `-i, --input-table PATH` reads a structured input manifest and appends its
  rows after any direct `-p/--path` and `-d/--db` inputs. CSV and JSON manifests
  use only the Python standard library. `.xlsx` manifests are supported only
  when the optional `openpyxl` package is importable; `.xls` is unsupported and
  must fail with guidance to use `.xlsx` or CSV.

`view trajectory` may mix repeated `-p` and repeated `-d` inputs in one
invocation, and may also use repeated `-i/--input-table` manifests. Each loaded
path, DB session, or manifest row becomes one Trial in the report. `export
trajectory` remains single-session only and must fail clearly when the effective
input set contains more than one session.

`-s, --session-id` is valid only when at least one `-d, --db` input is present.
With one DB input, bare `-s ID` remains valid and may be repeated to compare
multiple sessions from that DB in `view trajectory`. With multiple DB inputs,
session ids must use `-s dN=ID`, where `N` is the one-based DB input index.
Repeating `-s dN=ID` selects multiple sessions from that DB. A DB input without
explicit session ids lets its adapter choose its default or latest session.

The command surface follows a peval-style verb and scenario shape:

- `peval-py view trajectory ...` writes a peval-compatible JSON or HTML report
  for one or more sessions.
- `peval-py export trajectory ...` writes a single ATIF trajectory object.

`tr` is an accepted alias for `trajectory`, so `peval-py view tr ...` and
`peval-py export tr ...` are equivalent to the long scenario form.

Common trajectory flags use both long and short forms:

- `-c, --config PATH`
- `-a, --adapter ADAPTER`
- `-i, --input-table PATH`
- `-o, --output [PATH]`
- `-f, --format json|html` for `view trajectory`
- `-n, --note N=TEXT` for `view trajectory`, where `0` is report-level and
  positive one-based indexes attach to the ordered input sessions

`-a ADAPTER` sets the default adapter for all inputs. `-a pN=ADAPTER` overrides
the adapter for the one-based path input `N`; `-a dN=ADAPTER` overrides the
adapter for the one-based DB input `N`. `-a` may be repeated. The default
adapter starts from config, the last bare `-a ADAPTER` overrides that default,
and selector forms override only their matching input. Invalid selectors,
duplicate selectors, selectors that reference missing inputs, and unknown
adapter ids must fail clearly and list available adapters for unknown ids.
Selector forms apply only to directly supplied `-p/--path` and `-d/--db`
inputs. Manifest rows use their own `adapter` or `a` column for row-level
adapter selection, falling back to the effective default adapter when omitted.

Input table manifests are input lists, not batch job runners: they do not
introduce per-row output paths or multiple command executions. CSV manifests use
the first row as a header, read with `utf-8-sig`, preserve cell newlines, skip
blank data rows, and resolve relative `path` or `db` values relative to the
manifest file's directory. JSON manifests may be a top-level array of row
objects or an object with `rows` and optional `report_notes`. `.xlsx` manifests
use the active worksheet with the first row as a header. Headers are normalized
by removing leading dashes, lowercasing, and converting hyphens and spaces to
underscores. Supported manifest columns are `path`/`p`, `db`/`d`,
`session_id`/`session`/`s`, `adapter`/`a`, `note`/`notes`/`n`,
`report_note`/`report_notes`, `agent_name`, `agent_version`, and `model`.
Unknown or duplicate columns must fail clearly. Each non-blank row must provide
exactly one of `path` or `db`; `session_id` is valid only for `db` rows. A DB
with multiple selected sessions is represented by multiple manifest rows.
Existing CLI `--agent-name`, `--agent-version`, and `--model` values are
defaults for every session; manifest row values override those defaults only
for that row's conversion.

When `-o/--output` is omitted, commands write to stdout. When `-o/--output` is
present without a path, the default file name includes the effective adapter and
session identity. Single-session `export trajectory` writes
`trajectory-<adapter>-<session>.json`. Single-session `view trajectory` writes
`report-<adapter>-<session>.html`, or `report-<adapter>-<session>.json` when
`--format json` is set. Multi-session `view trajectory` writes
`report-<adapter>-sessions-<count>.<format>` when every session uses the same
adapter, or `report-multi-adapter-sessions-<count>.<format>` when multiple
adapters are present. Unsafe filename characters are replaced with `-`, and
missing session ids fall back to `session`.

`export trajectory` remains single-session only. Multiple path inputs, multiple
DB inputs, mixed path/DB inputs, or multiple selected DB sessions must fail
clearly for export.

TOML config uses `defaults.adapter` for the input adapter default. Older
`defaults.agent` config keys may be accepted for local compatibility, but the
public CLI and docs use `adapter`. `defaults.locale` selects the generated HTML
report UI locale and is config-only; there is no CLI locale flag. Supported
values are `en`, `en-US`, `zh-CN`, and `zh`; `en-US` normalizes to `en`, and
`zh` normalizes to `zh-CN`. Unsupported locale values must fail with a clear
config error. Adapter-specific options live under `[adapters.<adapter-id>]`.
`peval-py` passes each effective adapter's raw option table through to that
adapter and does not define adapter-specific CLI flags.

JSONL accepts either direct message objects or wrapper objects containing
`message`, optional `usage`, optional `metadata`, optional `accounting`, and
optional `session_seq`. Exported ATIF JSON path input preserves the trajectory
object as the canonical data source, does not require a selected adapter, and
uses `atif` as the report metadata adapter id to mark passthrough input.
It rebuilds only minimal report sidecar step metadata; peval-only timing
metadata that is not present in ATIF is not reconstructed.

SQLite `--db` input is interpreted by the effective adapter for that DB input.
Adapters may implement native database conversion for their own
retained-session persistence. If an adapter does not implement native DB
conversion but does
implement record conversion, `peval-py` may use the configured generic
`messages` table mapping and then call `convert(records, config)`. That generic
mapping reads `session_seq`, `message_json`, `usage_json`, `metadata_json`, and
accounting columns ordered by `session_seq`. Table and column names supplied by
config must be SQL identifiers, not raw SQL fragments.
For generic SQLite inputs, the selected `--session-id` is report metadata even
when it is not duplicated inside individual message rows. ATIF output must set
`session_id` from that selected id. Native DB adapters may define their own
session selection behavior. Psychevo defaults to the most recently updated
session from the `sessions` table, OpenCode defaults to the most recently
updated session, and Hermes defaults to the session with the most recent active
message, ending, or start time when `--session-id` is omitted.

## Adapters

`-a, --adapter` selects the default adapter or a per-input adapter selector.
Built-in adapters are always available:

- `psychevo` supports current Psychevo retained messages with
  `role=user|assistant|tool_result`, user text blocks, assistant text,
  reasoning, tool-call blocks, and current Psychevo SQLite persistence with
  `sessions` and `messages` tables.
- `opencode` supports the common single-session message JSONL shape and current
  OpenCode SQLite persistence with `session`, `message`, and `part` tables.
- `hermes` supports the common single-session message JSONL shape and current
  Hermes SQLite persistence with `sessions` and `messages` tables.

Third-party adapters register through installed Python package entry points in
the `peval_py.adapters` group. The entry point name is the adapter id after
lowercase normalization. Unknown adapters fail with a diagnostic that lists
available adapter ids, and duplicate ids fail clearly instead of shadowing an
existing adapter.

Adapters may implement the normal record conversion contract,
`convert(records, config)`, for JSONL and generic SQLite inputs. Adapters that
need to parse a source file directly may also implement `convert_path(path,
config)`. Adapters that own a SQLite persistence format may implement
`convert_db(path, session_id, config)`.
For `-p/--path`, `peval-py` first recognizes exported ATIF JSON trajectory
objects. Otherwise it calls `convert_path` when the effective adapter provides
it, then falls back to reading JSONL into `MessageRecord` values and calling
`convert`. For `-d/--db`, `peval-py` calls `convert_db` when the effective
adapter provides it; otherwise it reads the configured generic SQLite
`messages` rows and requires a record adapter. An adapter used with `--db` that
supports neither native DB input nor record conversion must fail with a clear
unsupported-input diagnostic.

Adapters may preserve source metadata in report sidecars, but ATIF output must
stay standard and must not include peval-only fields.

## Outputs

`export trajectory` writes a single ATIF trajectory object. `view trajectory`
writes either JSON or HTML:

- JSON is a self-contained peval view v17 subset with `schema_version`,
  `includes`, `scope`, `path_selections`, `trajectory`, and
  `trajectory_meta`. Multi-session view reports also include `comparison`;
  reports with notes also include `annotations`.
- HTML is emitted as a single offline file with inline CSS and JavaScript,
  while the source CSS and JavaScript live in package asset files instead of a
  large Python string. It renders the selected Trial trajectory, step rows,
  reasoning, message, tool-call, observation, metrics cues, and one combined
  Expand all / Collapse all control. The page head contains only the localized
  report title; agent/model and metric summaries stay inside the Run and Result
  sections instead of appearing as a separate top banner. Report typography
  uses a 15px body text baseline, with compact labels, chips, table headers,
  and code blocks no smaller than 12px.

Single-session HTML renders the current Run, Result, Evidence, and Steps
sections. Multi-session HTML renders Report Notes, Leaderboard, Trajectory
Overview, then the selected Trial trajectory. The comparison panels render one
primary section title without a duplicate eyebrow label. `Leaderboard` is a
preserved report UI term and remains English in localized reports.
`peval-py` treats each input session as one Trial. Multi-session HTML no longer
renders a separate Visible Heatmap panel. The Leaderboard shows session, agent,
model, result, duration, turns, tools, tokens, cost, and notes. The Agent column
uses the trajectory agent name and falls back to the adapter id when the
trajectory does not provide an agent name. The Session, Agent, Model, and
Result columns provide multi-value filters whose values are collected from the
complete Leaderboard row set. Empty selections are equivalent to no filter,
values within one column are OR-ed, and multiple filtered columns are AND-ed.
Filtering happens before sorting and before metric shading. If filters hide the
currently selected Trial and visible rows remain, HTML selects the first visible
Trial; if filters hide all rows, the selected Trial detail remains visible but
no Leaderboard row is selected. Leaderboard duration, tokens, Tool Calls, and
Turns cells show per-column metric intensity directly as cell background
shading; each metric column computes its own scale from the currently visible
filtered rows, missing values remain unshaded, and Cost is not shaded. The
filter control appears inline on the right side of the filtered column label,
similar to a spreadsheet table header, instead of occupying a second header
line. The rendered comparison sections must not show benchmark, task, task-set,
task-family, or matrix task-axis fields.

The Trajectory Overview section below the Leaderboard renders one row per
session in the same order as the currently filtered and sorted Leaderboard
rows; its row count and row order must exactly match the rendered Leaderboard.
Each row shows a compact left-to-right node track where each ATIF step is one
node. Overview nodes use a neutral visual style and show source initials:
`S` for system, `U` for user, `A` for agent, and `?` for unknown or unsupported
sources. All rows share a grid width based on the largest step count among
visible sessions, so nodes at the same step index align vertically and shorter
trajectories leave empty positions at the end. Clicking a Trajectory Overview
row selects that Trial. Clicking a node selects that Trial and opens a fixed
right-side Step details drawer showing the same expanded step markup and block
content used by the final Steps section. On desktop, the drawer uses a wider
inspection width than the initial compact rail so longer reasoning, tool, and
observation content can be read without excessive wrapping. The widened drawer
must not obscure the middle report content: when it opens on desktop, the page
layout reserves the drawer's right-side width and constrains the main workspace
to the remaining viewport. Its expanded step layout stretches to the available
drawer height: the step summary stays at the top, visible content blocks share
the remaining height where possible, and long block payloads scroll inside their
own blocks instead of leaving unused drawer space below. When browser zoom or a
short viewport leaves less vertical room than the drawer content needs, the
drawer itself remains scrollable so lower blocks and controls stay reachable
instead of being clipped. The drawer supports a close button, Escape, and
clicking blank page space outside the drawer. Clicking another node replaces
the drawer content; filtering that hides the drawer's selected node closes the
drawer. On narrow screens, the drawer appears as a bottom sheet. Node titles
provide step id, role, and a short preview.

Step token chips prefer real per-step metrics from the trajectory. When a
visible step lacks per-step token metrics, HTML may show an estimated token
chip to avoid an empty visual rail. Estimated step tokens are UI-only: they are
derived while rendering HTML, are visibly marked with an estimated indicator,
and must not be written back into ATIF trajectories or report JSON. When the
optional `tiktoken` package is importable, the renderer may use it for the
visible step text; otherwise it falls back to a deterministic standard-library
byte-length estimate.
Estimated chips must resolve against the selected Trial identity and render in
the final Steps rail for any visible step without real step metrics, including
user and system steps.

Steps timing chips may show a UI-only proportional fill in HTML. The fill is
computed in the browser from the selected Trial metadata and is not written
back to ATIF or report JSON. Step duration chips scale against the slowest step
in the selected Trial, tool execution chips scale against the slowest tool
execution in the selected Trial, and elapsed chips scale against the selected
Trial wall duration when available, falling back to the largest elapsed step
value.

HTML localization covers the report title, report-level chrome, comparison
section titles, metric labels, comparison table headers, comparison filters,
drawer chrome, comparison empty states, buttons, aria labels, comparison status
labels, and the selected Trial summary, notes, result, and evidence sections.
Only the final selected Trial Steps detail section remains English in this
version. English is the default.
Simplified Chinese is selected with `defaults.locale = "zh-CN"` or the `zh`
alias. In Simplified Chinese reports, domain terms such as Run, Result, Notes,
Evidence, Steps, events, Session, variant, evaluator, reasoning, selected
trial trajectory, cache read, and cache write remain English, as do metric/tool
labels such as Turns, Tool Calls, and tool success / total. Report JSON schema,
adapter ids, model names, session ids, note text, tool names, raw warnings, and
stored status values remain unchanged.

User-facing translated Markdown for canonical docs lives under
`docs/i18n/<locale>/...`. Tool README translations live beside their original
README files, such as `tools/peval-py/README.zh-CN.md`. The canonical English
docs remain in their original locations. Chinese docs link to Chinese
translated pages when available and fall back to the canonical English target
when a page has not been translated. Spec links continue to point at
`specs/...` unless translated specs are introduced later.

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
`failed`; adapter is the effective adapter id for that input session.

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
notes must be escaped before Markdown display and must not execute. Manifest
`note`/`notes`/`n` values without `N=` bind to that row's expanded session
index; values with `N=TEXT` reuse the CLI note syntax. Manifest `report_note`
or `report_notes` values are report-level notes equivalent to `-n 0=TEXT`.
JSON note fields may be strings or arrays of strings.

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
