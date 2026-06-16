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
- saved-workspace `serve` state and local HTTP behavior
- minimal peval-py serve state initialization from `peval-py init`

Out of scope:

- live providers, ACP servers, official benchmark harnesses, Docker, remote
  network services, or browser automation against live providers
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
  supports explicit `--session-id` selection, prefers same-DB `event` timing for
  tool execution when available, marks assistant/tool boundary model timing as
  an OpenCode estimate, and falls back to part row timing without warnings when
  event timing is unavailable.
- Hermes DB conversion reads current `sessions` and `messages` tables, includes
  stored `sessions.system_prompt` as a system step, defaults to the most
  recently active, ended, or started session when `--session-id` is omitted,
  supports explicit `--session-id` selection, and marks Hermes DB message
  timestamps as order-only so active model/tool durations remain unknown unless
  explicit elapsed/start/end timing metadata exists or current Hermes
  `logs/agent.log` API/tool timing strictly matches the DB transcript.
- adapters used with `--db` that support neither native DB input nor record
  conversion fail with a clear unsupported-input diagnostic.
- locale config defaults to English, accepts the `en-US`, `zh-CN`, and `zh`
  aliases, reads top-level `locale` from discovered `peval-py.toml`, overlays
  explicit `-c/--config` without resetting unspecified workspace values, and
  rejects unsupported values with a clear config error.
- analysis config defaults `analysis_eval_slug` to `default`, reads the top-level
  key from discovered `peval-py.toml`, and allows explicit `-c/--config` to
  override only that key without resetting other workspace values.
- malformed JSONL lines fail with a clear line-number diagnostic.
- ATIF step ids are sequential and tool observations link to source tool calls.
- final metrics aggregate available usage, accounting, turn, tool-call, and
  tool-error facts.
- report JSON contains the v18 subset top-level fields.
- report JSON records active `duration_ms` plus `wall_duration_ms` for each
  Trial and comparison leaderboard row.
- peval-py report JSON can be ingested as a canonical snapshot source without a
  message adapter.
- report JSON can derive Hermes active model/tool durations from strictly
  matched `agent.log` timing while preserving the existing JSON v18 shape.
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
- DB session selection accepts `-s #N`, ID-first bare numeric `-s N`, and
  multi-DB `-s dN=#M` index selectors against the adapter session list.
- `view trajectory --list/-l` prints `#`, `session_id`, and `name` for DB
  inputs and exits without rendering a report.
- `view trajectory --list-interactive/-li` requires a TTY and exactly one DB,
  accepts comma/range input such as `1,3-4` and `all`, treats blank input as
  cancel, and renders the selected sessions.
- adapter inference for direct `-p` and `-d` inputs uses available adapter ids
  as complete path components or filename tokens only; explicit bare `-a`,
  per-input selectors, and manifest row adapters override inference, while
  ambiguous path matches fail clearly.
- `export trajectory` rejects multiple sessions clearly.
- `export trajectory` accepts `-i/--input-table` only when the expanded input
  set contains exactly one session.
- JSONL view inputs without an embedded session id use the file stem as the
  displayed session id.
- `-n/--note 0=TEXT` creates report-level notes, `-n/--note N=TEXT` attaches
  to the one-based session index, repeated notes preserve CLI order, and
  out-of-range indexes fail clearly.
- cached analysis enrichment reads exactly one cell directory matching
  `runs/<eval-slug>/<agent-id>/<session-id>/*/analysis.{json,md}`, prefers
  `agent_name` over adapter id for the path, writes `annotations.analysis[]`
  with cached status, compatible `relative_path`, optional `summary`,
  optional `md_report`, and per-format `relative_paths`, and silently omits
  missing or ambiguous cell matches while keeping valid Markdown when sibling
  JSON is malformed.
- cell-local peval manual notes read exactly one
  `runs/<eval-slug>/<agent-id>/<session-id>/*/notes.md` cell, prefer
  `agent_name` over adapter id for the path, write `annotations.notes[]` with
  `source = "cell"`, `label = "notes.md"`, Markdown body, and a note
  `source_ref.relative_path`, render before CLI/table Trial notes, stay
  separate from `annotations.analysis[]`, and silently omit missing or
  ambiguous note cells.
- report snapshot aggregation preserves and remaps both cached analysis and
  cell-local notes when serve composes active source snapshots.
- comparison JSON contains one canonical `leaderboard.entries` row list, omits
  legacy duplicate `session_heatmap.rows` and `session_table.rows`, and does not
  emit benchmark, task, task-set, task-family, matrix task-axis, row `selected`,
  or derived `successful_tool_calls` fields.
- HTML escapes text, safely embeds JSON, exposes one step visibility toggle,
  and renders peval-style tool names, tool execution timing, and observations
  inside the corresponding Agent step.
- HTML renderer source templates, CSS, and JavaScript live in package asset
  files and are still inlined into the emitted offline HTML report.
- static HTML reports use the default report presentation mode and do not
  render serve-only source manager controls, Leaderboard row-selection
  checkboxes, or Leaderboard export controls.
- serve UI HTML mode reuses the static report body while rendering a compact
  source/status toolbar, modal source manager, a Leaderboard row-selection
  checkbox column, a header select-visible checkbox, and one Leaderboard
  `Export` menu with `Table`, `JSON Report`, and `HTML Report` choices.
- serve UI source manager renders Session/ATIF path, DB, and input-table forms,
  JSONL/ATIF JSON/report JSON upload affordance, explicit refresh controls,
  active/archive/delete controls, non-refreshable snapshot labels, and latest
  source status without a persistent sidebar or duplicate form titles.
- serve UI source manager renders a DB Inspect control, adapter single-choice
  controls defaulting to `auto`, session multi-select table, select-all-visible
  control, and add-selected action only in serve mode.
- serve UI selected Trial Notes section renders Edit/Add notes controls only in
  serve mode for refreshable sources, saves through the source-specific notes
  endpoint, rerenders from the returned mutation payload, keeps CLI/table notes
  read-only, hides save controls for snapshot sources, and escapes raw HTML in
  Markdown notes.
- serve HTTP exposes `POST /api/db-sessions` for local DB inspection. Tests cover
  `.hermes`, `.psychevo`, and `.opencode` path-token adapter inference,
  explicit adapter retry after failed inference, ambiguous path errors,
  unsupported session-list adapters, missing DB diagnostics, and same-origin
  rejection.
- serve HTTP `POST /api/sources` accepts DB `session_ids` arrays and creates one
  independent refreshable source/trial per selected session while preserving the
  existing single `session_id` payload behavior.
- serve HTTP `POST /api/sources` accepts shell-quoted multi-path strings for
  path and DB payloads, rejects malformed quoted input clearly, treats `auto`
  adapter as no override, and persists no new source/trial/log rows when a
  submitted source fails to load, convert, or refresh. Path parsing tests cover
  quoted paths, unquoted Windows drive paths with backslashes, `C:/...` paths,
  UNC paths, relative workspace paths, Windows absolute-like paths not being
  prefixed with the workspace root, and POSIX `/mnt/<drive>/...` fallback when
  the mapped path exists. `/api/db-sessions` and `/api/sources` both cover this
  shared parsing behavior.
- serve HTTP exposes `POST /api/sources/{source_key}/delete`; tests verify it
  removes only peval-py state rows for the source, keeps source files untouched,
  returns clear errors for unknown sources, and is covered by the same-origin
  mutating API checks.
- serve HTTP exposes `POST /api/sources/{source_key}/notes`; tests verify it
  rejects snapshots and unknown sources, enforces the 1 MiB Markdown limit and
  same-origin JSON POST rules, overwrites an existing unique notes cell, saves
  beside a unique analysis cell when no notes cell exists, creates
  `peval-py-notes/notes.md` when no cell exists, returns a clear error for
  ambiguous note or analysis cells, refreshes the source snapshot immediately,
  and returns the standard `{ sources, report }` mutation payload.
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
  system, `U` user, `A` agent, and `?` unknown roles, renders positive step
  durations as very low-contrast ten-level per-Trial background-shade heat
  classes on nodes while leaving untimed or zero-duration nodes neutral, includes
  duration text in node title/aria labels, and row/node clicks update the
  selected Trial panel.
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
  is unavailable, suppress estimated model stages for order-only source
  timestamps, avoid duplicating tool spans as model duration, render Timeline
  Waterfall and Timeline Detail Table as default-expanded collapsible sections,
  keep measured zero-duration tool stages visible while omitting tools with
  missing timing,
  render heuristic category colors on Detail Table Stage values instead of a
  separate visible Category column, visible Waterfall duration labels, and active-share values, size the
  Waterfall left gutter from y-axis
  labels instead of a large fixed margin, use stable interval-aware x-axis ticks
  that avoid repeated rounded labels on short traces, keep message previews out
  of Waterfall labels/tooltips and Detail Table Stage cells, and do not mutate
  the embedded JSON v18 report data. Timeline bar, user/system marker with a
  source `step_id`, and Detail Table row clicks open the existing Step details
  drawer for the corresponding source step without changing the selected Trial,
  including in single-session reports that have no Leaderboard rows to
  synchronize against. The selected Detail Table row uses a single first-cell
  indicator rather than repeated vertical bars in every cell.
  Timeline Detail Table tests cover sortable `#`, `Stage`, `Start`, `End`,
  `Duration`, and `Active Share` columns, sortable table headers cycling
  through ascending, descending, and no-sort states, Stage-only filtering,
  `Duration` and `Active Share` metric shading, `Active Share` in-cell
  proportional fill, no Distribution column, and no Category header or filter.
  Timeline Waterfall tests verify that table
  sorting/filtering state does not drive the Waterfall trace order or chart
  data.
  Timeline color tests reserve red for `Error` and keep non-error `External`
  stages on a neutral color. CSS tests verify Timeline section shells do not
  use the old pink/tinted filled background.
- HTML Trajectory Overview tests verify fixed-size nodes can wrap onto multiple
  rows for long trajectories while preserving node order, selected-node state,
  and click targets.
- serve UI HTML and interaction tests verify Source Manager form shells do not
  use the old pink/tinted filled background, source adapters render as compact
  single-select dropdowns in each form action row rather than radio groups,
  Export and table filter submenus stay open for inside clicks, close on outside
  clicks, and do not apply this outside-click behavior to Timeline or Step
  collapsible sections.
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
- init tests verify `peval-py init` creates only `<workspace>/peval-py.toml` and
  migrated `<workspace>/state.db`, preserves existing valid `peval-py.toml`
  state DB paths, rejects invalid peval-py TOML, and does not create
  `peval.toml`, `runs/`, `datasets/`, `scripts/`, default templates,
  `$PSYCHEVO_HOME/peval-config.toml`, or `.gitignore`.
- saved-workspace tests verify peval-py workspace discovery from current-or-parent
  `peval-py.toml`, explicit `--root` and `PEVAL_ROOT` handling as a peval-py
  root override without requiring `peval.toml`, `<workspace>/peval-py.toml` defaults,
  `<workspace>/state.db` creation, `peval_py_*` migrations, stable source keys,
  source update instead of duplicate append, active/archive lifecycle, latest
  canonical Trial snapshots including refreshed cached analysis JSON/Markdown
  and cell-local notes, refresh-log rows, and no non-peval-py table writes.
- CLI smoke tests cover `init --root`, `init --root --json`, `serve -p`,
  `serve -d`, `serve -i`, persistent save-and-refresh behavior, default
  `58010..58029` port fallback, strict explicit-port failure, config-free
  defaults, and missing-workspace diagnostics.
- HTTP tests use only the Python standard library and temporary workspaces. They
  verify `/`, report JSON, source listing, add source, archive/activate, refresh,
  JSONL upload, ATIF JSON upload, report JSON upload, unsupported upload
  rejection, 20 MiB upload cap rejection, no CORS header, JSON POST requirement,
  and same-origin rejection for mutating APIs.
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
