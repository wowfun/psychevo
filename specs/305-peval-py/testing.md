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
- analysis report import into Trial-cell artifacts

Out of scope:

- live providers, ACP servers, official benchmark harnesses, Docker, remote
  network services, or browser automation against live providers
- default Rust workspace validation changes
- `skills/peval-py` content assertions inside `tools/peval-py/tests`

## Deterministic Coverage

Tests use only temporary directories, local fixtures, Python standard-library
SQLite, and checked-in JSONL fixture files.

The unittest suite is split by behavior surface instead of keeping all coverage
in one large module. Shared fixture paths, fake adapter entry points, custom
test adapters, JSON script extraction, and SQLite fixture builders live in a
non-discovered support module under `tools/peval-py/tests`; discovered
`test_*.py` modules group config/adapter coverage, source conversion coverage,
report/HTML coverage, and CLI/input-table smoke coverage.

Architecture refactors must keep tests at deep module seams. Workspace state
tests should exercise workspace services and repositories through behavior
visible to CLI or serve workflows, not private SQL helper details. Serve tests
should exercise controller payloads and local HTTP behavior rather than handler
implementation branches. Asset tests must verify ordered bundle loading,
embedded render options, Leaderboard Summary assets, and JavaScript syntax for
every split asset file.

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
  issuing Agent step, mark tool meta as failed, count tool errors per failed
  tool call even when multiple failures share one step, and do not prevent
  later successful tool calls from being represented.
- unmatched tool results remain visible, produce a warning, and do not increase
  tool-call error totals.
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
- CLI help describes `-p/--path` as a generic source path that accepts JSONL,
  report JSON, trajectory artifacts, Trial cells, and descendants rather than
  as JSONL-only input.
- path adapters can handle `-p/--path` without the default JSONL loader.
- `-p/--path` can read an exported ATIF JSON trajectory object directly without
  reparsing it through a message adapter, and does not require the configured
  default adapter to be installed.
- `-p/--path` can read a Trial cell artifact directory containing
  `agent/trajectory.json` and `agent/trajectory_meta.json` for `view trajectory`
  inspect/raw output and `export trajectory`. Tests cover direct unregistered
  artifact snapshots, registered artifact paths that preserve workspace source
  metadata, inferred workspace context from `<workspace>/runs/...`,
  canonicalization of literal `<cell-dir>/**` and `<cell-dir>/**/*` inputs,
  shell-expanded descendant inputs inside a cell, cell-path precedence over
  unrelated `-r/-a/-d/-s/-i` and DB listing flags, and malformed `runs/...` cell
  directories that report the missing artifact files instead of adapter DB
  lookup errors. Git Bash and WSL coverage includes accessible `C:/...` Trial
  cell artifact paths mapped through `/mnt/<drive>/...`, `.peval/state.json`
  source metadata reads for mapped workspace cells, and unmapped Windows
  absolute-like paths not being resolved under the current directory. Raw report
  output for cell directory input includes `artifact_ref` while preserving the
  original `data_ref`; inspect v2 omits provenance-only metadata, and exported
  ATIF trajectory output does not include that metadata-only reference.
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
- adapter config accepts reserved `default_db_path`, resolves relative values
  against the TOML file that defines them, expands `~`, treats POSIX absolute
  paths, Windows drive paths, and UNC paths as absolute, stores current-home
  paths with `~` when writing adapter defaults, keeps the remaining raw adapter
  options available to adapters, and lets `-d @adapter` expand to the configured
  DB while rejecting missing defaults or conflicting `-a dN=...` selectors.
- malformed JSONL lines fail with a clear line-number diagnostic.
- ATIF step ids are sequential and tool observations link to source tool calls.
- final metrics aggregate available usage, accounting, turn, tool-call, and
  tool-error facts.
- report JSON contains the v19 subset top-level fields.
- report JSON records active `duration_ms` plus `wall_duration_ms` for each
  Trial in `trajectory_meta[]`; HTML/CSV comparison rows derive those values at
  render/export time instead of storing duplicate JSON rows.
- peval-py report JSON can be ingested as a canonical snapshot source without a
  message adapter.
- report JSON can derive Hermes active model/tool durations from strictly
  matched `agent.log` timing while preserving the existing JSON v19 shape.
- retained sessions with multi-hour idle gaps keep the full `wall_duration_ms`
  but exclude that idle gap from active `duration_ms`.
- active duration fallback counts short missing-metadata timestamp spans and
  excludes spans longer than 600,000 ms.
- repeated `-p` inputs in `view trajectory` create one trajectory per input
  without writing a persisted comparison summary.
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
  `alias`/`label`/`source_alias`, `agent_name`, `agent_version`, and `model`;
  row-level adapter, alias, and agent/model fields override command defaults for
  only that row.
- input table relative path and DB values resolve against the table file's
  directory; row notes bind to the expanded row index unless they use explicit
  `N=TEXT` syntax, and report notes bind to index `0`.
- input table validation fails clearly for unknown or duplicate headers, rows
  with both path and DB, rows with neither path nor DB, path rows with
  session ids, unsupported `.xls`, and `.xlsx` files when `openpyxl` is not
  importable.
- `--source-alias N=TEXT` binds display-only aliases to expanded input sessions,
  rejects duplicate and out-of-range aliases, writes `source_alias` to
  `trajectory_meta[]` and leaderboard rows, and does not change source identity,
  `trajectory.session_id`, `trial_key`, or Evidence/Input Source paths.
- with multiple DB inputs, `-s dN=ID` binds session ids to the one-based DB
  input, while bare `-s ID` fails clearly.
- with one DB input, bare `-s ID` remains compatible.
- DB session selection accepts `-s #N`, ID-first bare numeric `-s N`, and
  multi-DB `-s dN=#M` index selectors against the adapter session list.
- `view trajectory --list/-l` prints `#`, `session_id`, and `name` for DB
  inputs and exits without rendering a report.
- inspect-mode tool error summaries list `step_id`, `tool_call_id`, and tool
  name. `--steps VALUE` adds matching `selected_steps` and accepts comma lists
  or inclusive `start:end` ranges; `--tool-call ID` works independently and
  adds matching `selected_tool_calls` with the corresponding tool result when
  retained data provides one.
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
- cached analysis enrichment reads the exact
  `runs/<eval-slug>/<agent-id>/<session-id>/<trial-key>/analysis.{json,md}`
  cell derived from `trajectory_meta.trial_key`, prefers `agent_name` over
  adapter id for the path, and merges cached status, compatible
  `relative_path`, optional `summary`, optional `md_report`, per-format
  `relative_paths`, and typed incremental analysis fields into the Trial's
  computed `annotations.analysis[]` item. Tests verify every Trial receives
  `analysis_metrics.auto` without cached analysis, cached analysis updates the
  same item to `status = "cached"`, `analysis.json.status` maps to
  `analysis_status`, `analysis.json.metrics` and compiled
  `analysis.json.extra.metrics` map to flat keys under `analysis_metrics`,
  `analysis_metrics.auto` remains present and peval-py-owned while omitting
  direct canonical facts such as outcome status, durations, turns, tool counts,
  token totals, token breakdowns, raw cost, tokens per turn, and tools per turn,
  unknown top-level fields and recognized fields with incompatible types are ignored,
  session-root analysis artifacts are ignored, and valid Markdown survives when
  sibling JSON is malformed.
- `peval-py import analysis` requires an existing workspace root, a Trial cell
  `--run-path`, and at least one `--path/-p` analysis input. Tests verify JSON
  analysis report import writes compiled `analysis.json` with `subject` derived from
  `runs/<eval-slug>/<agent-id>/<session-id>/<trial-key>`, defaults omitted
  input status to `analyzed`, preserves non-standard input fields under
  `extra`, keeps imported `subject`, `metrics`, and `commands` under `extra`
  without overriding the compiled `subject`, does not synthesize top-level
  metrics or commands, and keeps the imported artifact recognized by later
  `view trajectory -r DIR`.
- JSON analysis import tests cover `extra` object merging with non-standard
  top-level fields, deterministic top-level override on duplicate extra keys,
  report generation ignoring `extra`, and non-object `extra` failing
  without partial writes.
- JSON analysis import tests cover `--json` warnings for top-level
  `subject`, `metrics`, `commands`, `analysis_status`, and
  `analysis_metrics`, plus standard input fields nested under `extra`. Tests
  verify warnings do not block import, input `metrics` later appears as flat
  imported keys under `analysis_metrics`, input `analysis_metrics` or `auto` cannot
  overwrite `analysis_metrics.auto`, unknown custom fields remain silent, and
  default text output does not include warnings.
- `peval-py import analysis` infers input format from suffix. Tests verify
  `.json` writes `analysis.json`, `.md` and `.markdown` write `analysis.md`,
  Markdown-only import does not create `analysis.json`, JSON plus Markdown
  writes both artifacts, duplicate JSON/Markdown inputs fail, unsupported
  suffixes fail, invalid JSON fails, and failures do not leave partial imported
  files.
- import path validation tests cover relative and absolute in-workspace
  `--run-path` values, outside-workspace paths, paths outside `runs/`, too-short
  run paths, and machine-readable `--json` output with selected run path and
  written artifact paths.
- cell-local peval manual notes read the exact
  `runs/<eval-slug>/<agent-id>/<session-id>/<trial-key>/notes.md` cell derived
  from `trajectory_meta.trial_key`, prefer `agent_name` over adapter id for the
  path, write `annotations.notes[]` with `source = "cell"`, `label =
  "notes.md"`, Markdown body, and a note `source_ref.relative_path`, render
  before CLI/table Trial notes, stay separate from `annotations.analysis[]`,
  and ignore session-root notes.
- report artifact aggregation preserves and remaps both cached analysis and
  cell-local notes when serve composes active source artifacts.
- serve state persists Trial agent artifacts as `agent/trajectory.json` and
  `agent/trajectory_meta.json`, stores only source management overlays in
  `.peval/state.json`, derives source identity and display summary from the
  cell path plus agent artifacts, and does not require a workspace `state.db`
  or SQLite trial table. Complete artifact-only Trial cells do not create an
  empty `.peval/state.json` during source discovery. Ordinary path/session/DB
  imports also do not create `.peval/state.json` unless the request carries
  alias/tags/archive/error overlay data; after import they are artifact-only
  non-refreshable sources. Clearing the last overlay field removes the state
  file.
- serve source display metadata tests cover `source_alias` and `source_tags`
  persistence in `.peval/state.json`, projection through `source_payload()` and
  served `trajectory_meta[]`, clearing values, and preserving existing display
  metadata when a source is re-imported.
- serve active report composition reads current cell-local `analysis.json`,
  `analysis.md`, and `notes.md` from each active source's stored Trial artifact
  cell path for all active sources, including snapshots and imported artifact
  sources whose latest status is `error`; tests cover added and deleted analysis
  files across repeated `active_report()` calls and `/api/report` reloads
  without `refresh_sources()`.
- report JSON uploads materialize matching Trial annotations into cell-local
  `notes.md`, `analysis.json`, and `analysis.md`; multiple Trial notes or
  analysis entries merge deterministically, typed incremental analysis fields
  are preserved when they can be merged without ambiguity, and report-level
  notes are ignored.
- serve source payloads include stored Trial identity and
  `last_turn_finished_at_ms` from `trajectory_meta.finished_at_ms`, and source
  aliases remain display-only.
- report JSON omits persisted comparison projections, including
  `comparison.summary`, `comparison.selected_trial_key`,
  `comparison.leaderboard.entries`, legacy `session_heatmap.rows`, and
  `session_table.rows`, `scope`, and `path_selections`; HTML still renders
  Leaderboard and Trajectory Overview by synthesizing rows from canonical Trial
  facts even for single-Trial reports, and CSV export uses the same runtime rows.
- converter output keeps `trajectory[]` ATIF-v1.7 compatible: provider
  `usage` and `accounting` maps appear under `steps[].metrics.extra`, aggregate
  custom metrics appear under `final_metrics.extra`, and no non-ATIF metrics
  keys appear at the standard metrics roots.
- direct ATIF JSON path input rejects non-ATIF metric root fields instead of
  passing them through to report JSON.
- report JSON omits per-step `trajectory_meta[].steps[].data_preview`; HTML
  step summaries and overview tooltips synthesize previews directly from
  `trajectory.steps[]` content and do not use meta preview fallbacks.
- HTML escapes text, safely embeds JSON, exposes one step visibility toggle,
  and renders peval-style tool names, tool execution timing, and observations
  inside the corresponding Agent step.
- HTML expanded step bodies sort mixed tool-call and observation blocks by
  per-block metadata timestamp while leaving reasoning and message content
  before those activity blocks.
- HTML Analysis metrics render structured JSON values without dumping arrays or
  objects into one-line metric cells: automatic step/tool/model latency
  distributions render together as a compact vertical box plot with duration
  labels inside the chart and category labels below the x-axis, and imported
  array/object metrics render as compact tables or collapsed details.
- HTML renderer source templates, CSS, and JavaScript live in package asset
  files and are still inlined into the emitted offline HTML report.
- static HTML reports use the default report presentation mode and do not
  render serve-only source manager controls, Leaderboard row-selection
  checkboxes, Leaderboard export controls, or active/archived source-state
  controls.
- serve UI HTML mode reuses the static report body while rendering a compact
  source/status toolbar, modal source manager, a Leaderboard row-selection
  checkbox column, a header select-visible checkbox, and one Leaderboard
  `Export` menu with `Table`, `JSON Report`, and `HTML Report` choices.
  Leaderboard and Trajectory Overview also render synchronized serve-only
  `Show archived` and bulk Archive/Activate controls. Clicking a Leaderboard
  session row selects that Trial and opens the step drawer on its first User
  step when one exists; sessions without a User step remain selectable without
  opening stale step details.
- serve UI source manager renders Session/ATIF/runs path, DB, and input-table
  forms, JSONL/ATIF JSON/report JSON upload affordance, a native file picker
  trigger for the Path textarea, explicit refresh controls only where
  provenance remains refreshable, active/archive/delete controls,
  non-refreshable artifact labels, and latest source status without a
  persistent sidebar, duplicate form titles, or add-form alias fields.
- serve UI source manager renders a DB Inspect control, adapter single-choice
  controls defaulting to `auto`, session multi-select table, select-all-visible
  control, and add-selected action only in serve mode.
- serve UI selected Trial Notes section renders Edit/Add notes controls only in
  serve mode for refreshable sources, saves through the source-specific notes
  endpoint, rerenders from the returned mutation payload, keeps CLI/table notes
  read-only, hides save controls for artifact-only/imported/snapshot sources,
  and escapes raw HTML in Markdown notes.
- serve HTTP exposes `POST /api/db-sessions` for local DB inspection. Tests cover
  `.hermes`, `.psychevo`, and `.opencode` path-token adapter inference,
  explicit adapter retry after failed inference, ambiguous path errors,
  unsupported session-list adapters, missing DB diagnostics, and same-origin
  rejection.
- serve HTTP `POST /api/sources` accepts DB `session_ids` arrays and creates one
  independent artifact-only source/trial per selected session while preserving
  the existing single `session_id` payload behavior.
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
  same-origin JSON POST rules, writes `notes.md` to the selected source's
  persisted Trial cell, refreshes the source snapshot immediately, and returns
  the standard `{ sources, report }` mutation payload.
- serve UI row-selection state is independent from selected-Trial state:
  checkbox clicks stop row selection, row clicks still update the selected
  Trial, Trajectory Overview row checkboxes mirror the same selection state as
  Leaderboard row checkboxes, and Trajectory Overview rows keep following
  filtered and sorted Leaderboard rows rather than checkbox state.
- serve main-view reports stay full-width over active readable sources:
  initial `GET /`, refresh, add/reload, source action, alias, and notes
  mutations return or embed a full active-source report, while explicit
  `GET /api/report?source_key=KEY` still returns a single-source report. Tests
  cover duplicate raw Trial keys that are uniquified in the full report and
  Source Manager row selection mapping back to the corresponding uniquified
  Trial without clearing comparison panels.
- serve startup binds the HTTP listener before importing explicit CLI sources
  or scanning workspace Trial cells. Tests cover a loading empty shell before a
  delayed initial load completes, a top-toolbar scanning status instead of a
  misleading normal empty-source status, and a full `/api/sources` envelope
  after the background load finishes.
- serve main-view archived mode is lazy and mutually exclusive with active mode:
  `GET /api/report?source_state=archived` returns archived readable sources,
  `GET /api/report` remains active by default, `source_key` report loads still
  return single-source detail, archived reports are fetched only when the browser
  first switches modes, and switching is disabled with a status message when the
  target readable source count is zero.
- serve UI bulk source-state actions use only checked rows currently visible in
  `leaderboardRows()`: tests cover hidden checked rows being ignored, Source
  Manager selection not affecting the payload, successful archive/activate
  clearing row selection and moving rows out of the current mode, automatic
  switching to the target mode when the current mode becomes empty after a batch
  action, same-origin protection, unknown source errors, and invalid payload
  rejection for `POST /api/sources/state`. HTTP tests keep backend payload
  semantics stable: `/api/sources/state` may return an empty report for the
  requested `report_source_state`; the browser owns the automatic fallback load.
- serve UI export helpers use visible checked rows when any currently visible
  rows are checked, otherwise the current visible filtered and sorted rows.
  Checked rows hidden by filters remain selected in UI state but are excluded
  from the current export scope until visible again.
- HTML does not render the old Summary, Session Heatmap, or Session Table
  labels in multi-session reports.
- HTML renders Report Notes, Leaderboard, Trajectory Overview, selected Trial
  details, selected-state cues, note snippets, selected Trial notes, and safe
  note Markdown for reports with one or more Trials.
- Leaderboard and Trajectory Overview comparison panels render only their
  primary heading, without duplicate eyebrow text; `Leaderboard` remains English
  in localized reports.
- Multi-session HTML does not render the old Visible Heatmap panel, metric
  controls, visible-grid, or session-axis layout.
- Leaderboard renders Agent from the trajectory agent name with adapter fallback,
  shows canonical Session separately from Session Alias, exposes sortable Last
  Turn End from `trajectory_meta.finished_at_ms`, and active duration, tokens,
  Tool Calls, and Turns cells show per-column metric intensity classes while
  Cost remains unshaded.
- Leaderboard exposes multi-value filters for Session, Agent, Model, Result,
  and `Analysised`; filter options come from the complete row set, values within
  a column are OR-ed, filtered columns are AND-ed, metric shading uses filtered
  visible rows, the Trajectory Overview reuses the same filtered and sorted
  rows, and filter buttons render inline to the right of each filterable column
  label.
- Leaderboard Summary renders below Leaderboard and above Trajectory Overview in
  static and serve HTML only when at least two rows are available. Tests cover
  that its transposed table and separate vertical box plots use only
  `leaderboardRows()` visible rows, row selection does not affect the summary, no
  separate visible-row count badge is rendered, model-call duration sums only
  non-estimated agent/assistant step durations per Trial, tool error rate treats
  no-tool rows as missing while preserving valid `0%` rows, and single-Trial
  reports render Leaderboard and Trajectory Overview without rendering the
  comparison summary section.
- Empty report runtime coverage verifies comparison panels can be absent without
  crashing selected-Trial rendering, and notes editor rendering never reads
  `markdown` from a null editor state.
- Trajectory Overview renders one session row per currently filtered and sorted
  Leaderboard row, preserving the same row count and order, aligns step nodes by
  the largest visible step count, renders neutral lettered nodes for `S`
  system, `U` user, `A` agent, and `?` unknown roles, renders positive step
  durations as very low-contrast ten-level per-Trial background-shade heat
  classes on nodes while leaving untimed or zero-duration nodes neutral, includes
  duration text in node title/aria labels, and row/node clicks update the
  selected Trial panel without resetting the Leaderboard or Trajectory Overview
  internal scroll positions. Leaderboard and Trajectory Overview vertical scroll
  progress stays synchronized in both directions while keeping Leaderboard
  horizontal scroll independent.
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
- HTML cached Markdown rendering tests cover Analysis `md_report` headings,
  strong/emphasis, inline code, fenced code, unordered lists, escaped script
  content, and GFM-style pipe tables with left, center, and right alignment.
  Tests verify `analysis.md` pipe tables render as `<table>` markup rather than
  plain paragraphs and that malformed table-like text remains readable.
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
  trace with latency-bearing stages and near-zero user/system markers, keep the
  fixed ECharts 6.0.0 CDN build for static reports, load the local-first
  `/assets/echarts/6.0.0/echarts.min.js` script in serve mode with CDN fallback,
  show a readable Waterfall fallback if ECharts is unavailable, omit
  retained-session idle gaps from the Timeline, preserve true model and tool wall start/end values in the detail
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
  the embedded JSON v19 report data. Timeline bar, user/system marker with a
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
  configured adapter default DB paths are exposed to the DB form, source alias
  edit controls render only in the source list, left-side add/upload forms omit
  alias inputs, the Source Manager source list scrolls independently, the
  language select renders only in serve mode, Export and table filter submenus
  stay open for inside clicks, close on outside clicks, and do not apply this
  outside-click behavior to Timeline or Step collapsible sections.
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
  scenario alias, `import analysis`, localized HTML output from
  `[defaults].locale = "zh-CN"`, and short flags including `-p`, `-a`, `-i`,
  `-n`, and `-o`.
- `view trajectory` defaults to `-m inspect` and emits fixed
  `inspect_schema_version: 2` JSON backed by pandas DataFrame projections.
  `view trajectory -m raw` preserves the full JSON/HTML report behavior.
  Inspect tests cover direct report/trajectory/trajectory_meta JSON inputs,
  JSONL, DB sessions, saved workspace snapshots, default `--head 2` and
  `--tail 2`, `--top`, `--source`, default 3000-character inspect previews,
  `--max-content-chars` preview bounding, `--steps` exact and comma/range
  selection, `--steps` suppressing default digest sections, malformed step
  selector diagnostics, help text for `--max-content-chars` and `--steps`,
  independently used `--tool-call`, omitted empty fields, second-based duration
  values, step/tool duration distributions, tool errors, and raw-mode rejection
  for inspect-only flags. CLI tests also cover raw-only rejection for
  `--agent-name`, `--agent-version`, `--model`, and `--no-redact`
  in default inspect mode, plus successful use of those flags with `view
  trajectory -m raw`.
- CLI and config tests verify `--trajectory-id` is not exposed by `view`,
  `export`, or `serve` help and is not parsed as a defaults override, while
  conversion still emits a generated ATIF `trajectory_id` and existing
  trajectory JSON IDs remain readable.
- CLI input tests cover `view trajectory -r DIR` and
  `export trajectory -r DIR` loading an existing peval-py workspace config from
  outside the workspace, including adapter `default_db_path` expansion through
  `-d @adapter`; `view trajectory -r DIR` recognizing cached
  `analysis.json` / `analysis.md`; `view trajectory --list -r DIR -d @adapter`
  using the root-selected config; direct Trial cell path input reading
  `.peval/state.json` source aliases and current cell-local overlays; and
  `view/export trajectory -r DIR` failing clearly when `DIR` is missing or does
  not contain `peval-py.toml`.
- init tests verify `peval-py init` creates only `<workspace>/peval-py.toml` and
  `<workspace>/logs/`, preserves existing valid `peval-py.toml`
  adapter defaults, writes built-in Psychevo, OpenCode, and
  Hermes default DB paths with `~` for new workspaces, rejects invalid
  peval-py TOML, and does not create `peval.toml`, `runs/`, `datasets/`,
  `scripts/`, default templates, `$PSYCHEVO_HOME/peval-config.toml`, or
  `.gitignore`.
- saved-workspace tests verify peval-py workspace discovery from current-or-parent
  `peval-py.toml`, explicit `--root` and `PEVAL_ROOT` handling as a peval-py
  root override without requiring `peval.toml`, `<workspace>/peval-py.toml` defaults,
  no workspace `state.db` creation, canonical cell-derived stable source keys,
  `.peval/state.json` minimal overlay writes, older flat state files
  being read best-effort until the next mutation rewrites them, source alias and tag
  storage without changing stable keys,
  duplicate imports resolving to the same cell updating one source,
  active/archive lifecycle, JSONL refresh/import logging, latest
  canonical Trial snapshots including refreshed cached analysis JSON/Markdown
  and cell-local notes, refresh-log rows, and no non-peval-py table writes.
- HTML interaction tests cover Leaderboard visible-scope search, all-session
  search over active and archived sources, disabled mixed-state batch
  Archive/Activate, inline alias/tag editing, existing-tag quick selection,
  flattened Any tag filters,
  startup loading status rendering and ready-state recovery,
  inline edit click isolation from row selection, first-User-step drawer
  selection from Leaderboard rows, Path picker textarea filling with
  newline-separated absolute paths while preserving existing input on
  cancel/error, export scope after search, and source-key mapping after
  filtering.
- Timeline HTML tests cover `N.M` numbering for Waterfall labels/tooltips and
  Detail Table rows, including multiple Timeline items derived from one source
  step and stable `#` sorting by trace order.
- CLI smoke tests cover `init --root`, `init --root --json`, `serve -p`,
  `serve -d`, `serve -i`, persistent save-and-refresh behavior, default
  `58010..58029` port fallback, strict explicit-port failure, config-free
  defaults, and missing-workspace diagnostics.
- HTTP tests use only the Python standard library and temporary workspaces. They
  verify `/`, report JSON, source listing, add source, archive/activate, refresh,
  JSONL upload, ATIF JSON upload, report JSON upload, unsupported upload
  rejection, 20 MiB upload cap rejection, no CORS header, JSON POST requirement,
  same-origin rejection for mutating APIs, `/api/config/locale` TOML updates,
  `/api/config/adapter-default-db` TOML updates and clears, Source Manager HTML
  regeneration with updated adapter defaults, recursive external `runs/` import
  through `/api/sources`, `/api/sources/{source_key}/alias`, local native path
  picker results and unavailable-picker errors through `/api/path-picker`, batch
  `/api/sources/state`, and the ECharts cached asset route using fake
  cache/download paths rather than real network.
- serve UI HTML and interaction tests verify the near-full-screen Source Manager
  workbench structure, adapter default DB configuration controls, mutable
  adapter default state in `report.js`, and continued DB form autofill from
  configured adapter defaults.
- legacy top-level `report` and `convert` commands are rejected.
- translated evaluation docs exist under `docs/i18n/zh-CN/...`, the peval-py
  tool README translation exists beside `tools/peval-py/README.md`, English
  docs link to their Chinese counterparts, Chinese docs link to translated
  pages when available, and spec links still target canonical specs.
- a fixture-backed smoke creates a temporary peval-py workspace, writes
  `runs/default/agent-a/common_session/<trial-key>/analysis.json` with a
  top-level `summary` and representative typed whitelist fields, writes sibling
  `analysis.md`, runs `view tr -m raw` with `-r <workspace>` against
  `tools/peval-py/tests/fixtures/common_session.jsonl` with `-a opencode
  --agent-name agent-a -f json`, and verifies the generated report contains a
  matching `annotations.analysis[]` item with both `summary` and `md_report`
  plus the structured fields.

## Validation

Separate skill package validation:

```sh
python /home/kevin/.codex/skills/.system/skill-creator/scripts/quick_validate.py skills/peval-py
python -m compileall skills/peval-py/scripts
```

The primary peval-py package validation command is:

```sh
UV_PROJECT_ENVIRONMENT=../../.local/peval-py-venv uv run --project tools/peval-py python -m unittest discover -s tools/peval-py/tests
```

Smoke validation should also run representative CLI commands against fixtures
and inspect generated JSON with:

```sh
python -m json.tool <output.json>
```

The repository Rust broad gate remains separate from peval-py Python
validation. Do not add Python package execution to
`cargo xtask ci run --profile rust-broad`, and do not use that Rust gate as the
default validation path for peval-py-only changes.

## Related Topics

- [305 peval-py](spec.md)
