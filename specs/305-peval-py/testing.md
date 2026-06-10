---
name: 305. peval-py Testing
psychevo_self_edit: deny
---

# 305. peval-py Testing

Define deterministic validation for the `peval-py` Python CLI.

## Scope

- Python package unit tests
- fixture-backed `peval-py` smoke tests
- JSONL and SQLite input behavior
- ATIF and peval-compatible report shape
- session-comparison view behavior and CLI notes

Out of scope:

- live providers, ACP servers, official benchmark harnesses, Docker, or network
  services
- default Rust workspace validation changes

## Deterministic Coverage

Tests use only temporary directories, local fixtures, Python standard-library
SQLite, and checked-in JSONL fixture files.

The unittest suite is split by behavior surface instead of keeping all coverage
in one large module. Shared fixture paths, fake adapter entry points, custom
test adapters, JSON script extraction, and SQLite fixture builders live in a
non-discovered support module under `tools/peval-py/tests`; discovered
`test_*.py` modules group config/adapter coverage, source conversion coverage,
report/HTML coverage, and CLI/input-table smoke coverage.

Coverage must verify:

- Psychevo SQLite `messages` extraction orders rows by `session_seq`.
- Psychevo DB conversion reads current `sessions` and `messages` tables,
  defaults to the most recently updated session when `--session-id` is omitted,
  and supports explicit `--session-id` selection.
- SQLite `--session-id` is preserved as `trajectory.session_id`.
- Psychevo JSONL conversion preserves user, assistant, reasoning, tool-call,
  and tool-result material.
- matched tool-result observations are nested under the Agent step that issued
  the tool call.
- matched tool-call failures in the middle of a session remain nested under the
  issuing Agent step, mark tool meta as failed, count tool errors, and do not
  prevent later successful tool calls from being represented.
- unmatched tool results remain visible and produce a warning.
- assistant and tool execution duration prefer `metadata.elapsed_ms` and fall
  back to timestamp differences only when the fallback span is no more than
  600,000 ms.
- common JSONL conversion works through the OpenCode and Hermes adapters.
- built-in adapter registry discovery includes Psychevo, OpenCode, and Hermes.
- installed Python entry points in the `peval_py.adapters` group can register a
  third-party adapter without editing the core adapter list.
- duplicate adapter ids and unknown adapter ids fail with clear diagnostics.
- adapter TOML tables under `[adapters.<adapter-id>]` are passed to the
  effective adapter as raw options, including when CLI `--adapter` overrides
  the configured default.
- `-a ADAPTER` applies one default adapter to all inputs, while `-a pN=ADAPTER`
  and `-a dN=ADAPTER` override individual path and DB inputs.
- invalid adapter selectors, duplicate selectors, out-of-range selectors, and
  unknown effective adapter ids fail with clear diagnostics.
- path adapters can handle `-p/--path` without the default JSONL loader.
- `-p/--path` can read an exported ATIF JSON trajectory object directly without
  reparsing it through a message adapter, and does not require the configured
  default adapter to be installed.
- DB adapters can handle `-d/--db` without the generic SQLite `messages` loader.
- OpenCode DB conversion reads current `session`, `message`, and `part` tables,
  defaults to the most recently updated session when `--session-id` is omitted,
  and supports explicit `--session-id` selection.
- Hermes DB conversion reads current `sessions` and `messages` tables, includes
  stored `sessions.system_prompt` as a system step, defaults to the most
  recently active, ended, or started session when `--session-id` is omitted,
  and supports explicit `--session-id` selection.
- adapters used with `--db` that support neither native DB input nor record
  conversion fail with a clear unsupported-input diagnostic.
- locale config defaults to English, accepts the `en-US`, `zh-CN`, and `zh`
  aliases, and rejects unsupported values with a clear config error.
- malformed JSONL lines fail with a clear line-number diagnostic.
- ATIF step ids are sequential and tool observations link to source tool calls.
- final metrics aggregate available usage, accounting, turn, tool-call, and
  tool-error facts.
- report JSON contains the v18 subset top-level fields.
- report JSON records active `duration_ms` plus `wall_duration_ms` for each
  Trial and comparison leaderboard row.
- retained sessions with multi-hour idle gaps keep the full `wall_duration_ms`
  but exclude that idle gap from active `duration_ms`.
- active duration fallback counts short missing-metadata timestamp spans and
  excludes spans longer than 600,000 ms.
- repeated `-p` inputs in `view trajectory` create one trajectory per input
  and include a session-oriented comparison summary.
- `-d` with repeated `-s` reads each SQLite session independently through the
  effective adapter.
- repeated `-d` inputs may use different adapters and generate one comparison
  report.
- JSONL path and DB input families can be mixed in `view trajectory`.
- `-i/--input-table` reads CSV, JSON top-level arrays, and JSON
  `{ rows, report_notes }` manifests, appending rows after direct CLI inputs in
  manifest order.
- input table rows support `path`/`p`, `db`/`d`, `session_id`/`session`/`s`,
  `adapter`/`a`, `note`/`notes`/`n`, `report_note`/`report_notes`,
  `agent_name`, `agent_version`, and `model`; row-level adapter and
  agent/model fields override command defaults for only that row.
- input table relative path and DB values resolve against the table file's
  directory; row notes bind to the expanded row index unless they use explicit
  `N=TEXT` syntax, and report notes bind to index `0`.
- input table validation fails clearly for unknown or duplicate headers, rows
  with both path and DB, rows with neither path nor DB, path rows with
  session ids, unsupported `.xls`, and `.xlsx` files when `openpyxl` is not
  importable.
- with multiple DB inputs, `-s dN=ID` binds session ids to the one-based DB
  input, while bare `-s ID` fails clearly.
- with one DB input, bare `-s ID` remains compatible.
- `export trajectory` rejects multiple sessions clearly.
- `export trajectory` accepts `-i/--input-table` only when the expanded input
  set contains exactly one session.
- JSONL view inputs without an embedded session id use the file stem as the
  displayed session id.
- `-n/--note 0=TEXT` creates report-level notes, `-n/--note N=TEXT` attaches
  to the one-based session index, repeated notes preserve CLI order, and
  out-of-range indexes fail clearly.
- comparison JSON contains one canonical `leaderboard.entries` row list, omits
  legacy duplicate `session_heatmap.rows` and `session_table.rows`, and does not
  emit benchmark, task, task-set, task-family, matrix task-axis, row `selected`,
  or derived `successful_tool_calls` fields.
- HTML escapes text, safely embeds JSON, exposes one step visibility toggle,
  and renders peval-style tool names, tool execution timing, and observations
  inside the corresponding Agent step.
- HTML renderer source CSS and JavaScript live in package asset files and are
  still inlined into the emitted offline HTML report.
- static HTML reports use the default report presentation mode and do not
  render serve-only import controls, Leaderboard row-selection checkboxes, or
  Leaderboard export controls.
- serve UI HTML mode reuses the static report body while rendering a collapsed
  import panel above the report title, a Leaderboard row-selection checkbox
  column, a header select-visible checkbox, and a split Leaderboard export
  control for rows, JSON report, and HTML report.
- serve UI row-selection state is independent from selected-Trial state:
  checkbox clicks stop row selection, row clicks still update the selected
  Trial, and Trajectory Overview rows keep following filtered and sorted
  Leaderboard rows rather than checkbox state.
- serve UI export helpers use visible checked rows when any currently visible
  rows are checked, otherwise the current visible filtered and sorted rows.
  Checked rows hidden by filters remain selected in UI state but are excluded
  from the current export scope until visible again.
- HTML does not render the old Summary, Session Heatmap, or Session Table
  labels in multi-session reports.
- HTML renders Report Notes, Leaderboard, Trajectory Overview, selected Trial
  details, selected-state cues, note snippets, selected Trial notes, and safe
  note Markdown for multi-session reports.
- Leaderboard and Trajectory Overview comparison panels render only their
  primary heading, without duplicate eyebrow text; `Leaderboard` remains English
  in localized reports.
- Multi-session HTML does not render the old Visible Heatmap panel, metric
  controls, visible-grid, or session-axis layout.
- Leaderboard renders Agent from the trajectory agent name with adapter fallback,
  and active duration, tokens, Tool Calls, and Turns cells show per-column
  metric intensity classes while Cost remains unshaded.
- Leaderboard exposes multi-value filters for Session, Agent, Model, and Result;
  filter options come from the complete row set, values within a column are
  OR-ed, filtered columns are AND-ed, metric shading uses filtered visible rows,
  the Trajectory Overview reuses the same filtered and sorted rows, and filter
  buttons render inline to the right of each filterable column label.
- Trajectory Overview renders one session row per currently filtered and sorted
  Leaderboard row, preserving the same row count and order, aligns step nodes by
  the largest visible step count, renders neutral lettered nodes for `S`
  system, `U` user, `A` agent, and `?` unknown roles, and row/node clicks
  update the selected Trial panel.
- Clicking a Trajectory Overview node opens a Step details drawer that reuses
  the final Steps section's expanded step markup, supports close and Escape,
  closes when the user clicks blank page space outside the drawer, swaps content
  when another node is clicked, and closes when filtering hides the selected
  node. The drawer is wide enough for readable inspection on desktop; expanded
  step blocks use their natural content height, short message/tool/observation
  blocks do not stretch into large empty panels, and long payloads scroll inside
  their own blocks. When the desktop drawer is open, the main workspace reserves
  the drawer width so report content is not hidden behind it. The drawer remains
  scrollable when browser zoom or short viewports make its content taller than
  the available screen height.
- HTML renders the peval-style Run, Result, Evidence, and Usage Breakdown
  sections for single-session reports.
- HTML report typography keeps the body text baseline at 15px and compact
  labels, chips, table headers, and code blocks at 12px or larger.
- HTML timed chips in the rendered Steps rail can show proportional fill for
  step duration, elapsed time, and tool execution time, while missing or zero
  timing values keep the plain chip style. Elapsed fill uses `wall_duration_ms`
  before falling back to first-to-last timestamps or active `duration_ms`.
- HTML data tables use content-adaptive column widths with a safe maximum column
  width; long content must not force unbounded columns, and narrow compact-value
  columns must not reserve fixed oversized space. Shared table renderer tests
  verify that Leaderboard and Timeline Detail Table use the same table model for
  rendering, sorting, filtering, metric shading, empty states, headers, cells,
  and row state while keeping isolated table state by table id.
- HTML Timeline Waterfall and Timeline Detail Table diagnostics render from
  existing selected-Trial step/tool timing metadata, derive a flat performance
  trace with latency-bearing stages and near-zero user/system markers, load the
  fixed ECharts 6.0.0 CDN build for the Waterfall, show a readable Waterfall
  fallback if ECharts is unavailable, omit retained-session idle gaps from the
  Timeline, preserve true model and tool wall start/end values in the detail
  table, render model generation as `Model: <model_name>` or `Model`, render
  estimated model timing with visible `≈` prefixes when explicit model duration
  is unavailable, avoid duplicating tool spans as model duration,
  render heuristic category colors on Detail Table Stage values instead of a
  separate visible Category column, visible Waterfall duration labels, and active-share values, size the
  Waterfall left gutter from y-axis
  labels instead of a large fixed margin, use stable interval-aware x-axis ticks
  that avoid repeated rounded labels on short traces, keep message previews out
  of Waterfall labels/tooltips and Detail Table Stage cells, and do not mutate
  the embedded JSON v18 report data. Timeline bar and Detail Table row clicks
  open the existing Step details drawer for the corresponding source step
  without changing the selected Trial. The selected Detail Table row uses a
  single first-cell indicator rather than repeated vertical bars in every cell.
  Timeline Detail Table tests cover sortable `#`, `Stage`, `Start`, `End`,
  `Duration`, and `Active Share` columns, sortable table headers cycling
  through ascending, descending, and no-sort states, Stage-only filtering,
  `Duration` and `Active Share` metric shading, `Active Share` in-cell
  proportional fill, no Distribution column, and no Category header or filter.
  Timeline Waterfall tests verify that table
  sorting/filtering state does not drive the Waterfall trace order or chart
  data.
  Timeline color tests reserve red for `Error` and keep non-error `External`
  stages on a neutral color.
- HTML shows visibly marked estimated token chips for steps that lack real
  token metrics, preserves exact token chips when real step metrics exist, can
  use an optional `tiktoken` module, falls back to a deterministic byte-length
  estimate, resolves estimates through the selected Trial key in the rendered
  Steps rail, and does not mutate report JSON data while rendering.
- HTML report title and comparison UI labels remain English by default and
  switch to Simplified Chinese only when the normalized locale is `zh-CN`, while
  the selected Trial Run, Result, Notes, and Evidence sections also localize and
  only the final Steps detail section remains English. Simplified Chinese
  reports preserve selected domain terms in English, including Run, Result,
  Notes, Evidence, Steps/events, Session, variant, evaluator, reasoning,
  selected trial trajectory, Turns, Tool Calls, tool success / total, cache
  read, and cache write.
- failed tool-call chips use the shared failure styling without applying that
  styling to later successful tools.
- step duration covers matched observations and is not computed as the
  previous-step gap.
- CLI smoke commands cover `view trajectory`, `export trajectory`, the `tr`
  scenario alias, localized HTML output from `[defaults].locale = "zh-CN"`, and
  short flags including `-p`, `-a`, `-i`, `-n`, and `-o`.
- legacy top-level `report` and `convert` commands are rejected.
- translated evaluation docs exist under `docs/i18n/zh-CN/...`, the peval-py
  tool README translation exists beside `tools/peval-py/README.md`, English
  docs link to their Chinese counterparts, Chinese docs link to translated
  pages when available, and spec links still target canonical specs.

## Validation

The primary validation command is:

```sh
UV_PROJECT_ENVIRONMENT=../../.local/peval-py-venv uv run --project tools/peval-py python -m unittest discover -s tools/peval-py/tests
```

Smoke validation should also run representative CLI commands against fixtures
and inspect generated JSON with:

```sh
python -m json.tool <output.json>
```

The repository broad validation script remains Rust-only for this feature. Do
not add Python package execution to `scripts/validate.sh broad`.

## Related Topics

- [305 peval-py](spec.md)
