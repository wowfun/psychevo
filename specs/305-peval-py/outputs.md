# peval-py Outputs

## Outputs

`export trajectory` writes a single ATIF trajectory object. `view trajectory`
writes either JSON or HTML:

- JSON is a self-contained peval view v19 subset with `schema_version`,
  `includes`, `trajectory`, and `trajectory_meta`. View reports include
  `annotations.analysis[]` for every Trial, and notes or cached analysis add to
  the same `annotations` object.
- Every object in `trajectory[]` must remain ATIF-v1.7 compatible. Report or
  provider fields that are not part of the ATIF root, step, tool-call,
  observation, metrics, or final-metrics schemas must live in the matching ATIF
  `extra` object or in `trajectory_meta[]`; report generation must not repair
  a non-ATIF trajectory after conversion.
- Converter-owned provider metrics must use ATIF extension points. Step-level
  `usage` and `accounting` maps live under `steps[].metrics.extra`, while
  aggregate turns, tool counts/errors, usage, and accounting live under
  `final_metrics.extra`. ATIF-standard flat token and cost fields remain at the
  standard `metrics` / `final_metrics` level.
- Direct ATIF JSON path input is validated against the same v1.7 allowlist
  before report construction. Files that put non-standard fields such as
  `usage`, `accounting`, or tool-count aliases at `metrics` /
  `final_metrics` root fail clearly instead of being passed through.
- JSON v19 removes generated-report data that is no longer a persistent fact
  source. It does not emit `comparison.summary`,
  `comparison.selected_trial_key`, `comparison.leaderboard.entries`,
  `session_heatmap.rows`, `session_table.rows`, `scope`, or
  `path_selections`. The HTML renderer and CSV export synthesize comparison
  rows at runtime from same-index `trajectory[]`, `trajectory_meta[]`, and
  `trajectory.final_metrics` facts. The default selected Trial is the first
  non-passed Trial, otherwise the first Trial.
- `trajectory_meta` stays session-oriented. It keeps adapter, timing, status,
  failure, score, warning, input source, event count, prompt availability, and
  per-step timing/tool/observation metadata. It must not emit peval matrix/task
  placeholder fields such as `matrix_cell_key`, `benchmark`, `cell_root_relative`,
  `case_id`, `task_set_id`, `task_id`, `task_family`, `score_passed`, or
  `score_details`. `trajectory_meta[]` may include display-only `source_alias`;
  aliases must not change `trajectory.session_id`, `trial_key`, input source
  paths, or source identity. Per-step metadata must not persist
  `data_preview`; HTML step summaries and overview tooltips synthesize previews
  at render time from canonical `trajectory.steps[]` content. Trial-level
  `duration_ms` is active model/tool work time,
  not the first-to-last session wall span. When runtime trace timing separates
  model generation from tool execution, active duration is model generation
  duration plus tool execution duration rather than a duplicated outer step
  span. For adapters such as OpenCode or Hermes, tool execution timing may come
  from adapter message/part timestamps, same-database events, message metadata,
  or a strictly matched current-source log such as Hermes `agent.log`; these
  sources populate the existing per-step and per-tool timing metadata without
  adding extra timing fields. OpenCode DB model timing from assistant/tool
  boundaries is an estimate and must be displayed as estimated rather than as
  provider API latency.
  When no explicit model generation duration is available, peval-py must not
  present inferred model timing as exact; HTML Timeline may estimate the model
  span from adjacent message/tool timestamps and must visibly prefix displayed
  start, end, and duration values with `≈`. The original wall span is retained
  as `wall_duration_ms = finished_at_ms - started_at_ms`. Leaderboard rows are
  runtime-only HTML/CSV projections and use the same active `duration_ms` plus
  `wall_duration_ms` facts from `trajectory_meta[]`.
- `annotations.analysis[]` is present for every Trial and carries deterministic
  report-time derived metrics under `analysis_metrics.auto`. Automatic metric
  groups can include `tooling`, `cost`, and `latency`; empty groups such as
  `cost` or `latency` are omitted when no values are available. They are
  computed only from `trajectory.final_metrics`, `trajectory_meta[]`, and
  per-step/per-tool metadata. Automatic metrics may include ratios, percentiles,
  top-N summaries, and other derived values, but must not duplicate
  direct facts such as status, warning counts, event counts, prompt availability,
  active or wall duration, steps, turns, tool calls/errors, token totals, token
  breakdowns, or raw cost. When cached analysis also exists for that Trial, its
  typed fields merge into the same analysis annotation and any cached/imported
  metrics remain flat keys in `analysis_metrics`; `analysis_metrics.auto`
  remains owned by report generation.
- `trajectory.final_metrics.extra.total_tool_errors` counts failed modeled tool
  calls, not failed Agent steps or standalone tool-result rows. A tool call is
  counted as failed when its matched observation marks the corresponding
  per-tool metadata status as `error`; unmatched tool results remain visible as
  observations with warnings but do not increase this tool-call numerator.
- HTML is emitted as a single offline file with inline CSS and JavaScript,
  while the source HTML templates, CSS, and JavaScript live in package asset
  files instead of large Python strings. It renders the selected Trial trajectory, step rows,
  reasoning, message, tool-call, observation, metrics cues, and one combined
  Expand all / Collapse all control. Expanded step bodies keep reasoning and
  message content first, then render tool-call and observation blocks in
  ascending per-block `timestamp_ms` order when metadata timestamps are
  available; untimed blocks keep their original relative order after timed
  blocks. The page head contains only the localized report title; agent/model
  and metric summaries stay inside the Run and Result sections instead of
  appearing as a separate top banner. Report typography uses a 15px body text
  baseline, with compact labels, chips, table headers, and code blocks no
  smaller than 12px.
- HTML keeps page-level titles for navigation but avoids repeated titles inside
  a single context. The selected Trial Analysis panel does not repeat
  `computed` or `cached` as a chip plus heading, and automatic metrics render
  directly as metric groups rather than under an extra `Auto Metrics` heading.
  Tooltip `title` attributes are reserved for hidden identity or interaction
  hints, not for repeating visible table labels.
- HTML Analysis metric rendering is structure-first. Known
  `analysis_metrics.auto` groups use dedicated renderers: scalar tooling and
  cost metrics render with human-readable labels; latency distributions for
  step, tool, and measured model durations render together in one compact
  vertical box plot with a shared duration scale. Key duration values are
  labeled directly inside the plot, and category labels sit below the x-axis
  instead of above the plot. Imported metrics
  keep their original JSON data in the report payload, but the HTML renderer
  displays scalars as key-value rows, arrays of objects as compact tables, and
  nested or mixed structures inside `<details>` instead of squeezing full JSON
  strings into metric cells.
- HTML cached Markdown rendering is intentionally lightweight but must make
  common `analysis.md` reports readable. It supports section headings, inline
  code, emphasis, strong text, unordered lists, fenced code blocks, and
  GFM-style pipe tables with optional alignment markers. Markdown headings in
  the Analysis panel render as dark, bold section titles within the panel
  hierarchy rather than as muted body text. Pipe tables render as real HTML
  tables inside a horizontally scrollable wrapper, reuse report table tokens,
  escape cell content, and keep malformed table-like text readable as normal
  paragraphs. The renderer must not add a third-party Markdown dependency or
  change report JSON.
- HTML data tables use content-adaptive column widths with a safe maximum
  column width instead of fixed column tracks. Wide tables may still scroll
  horizontally inside their table shell, but narrow content such as compact
  labels should not reserve oversized empty columns, and long cell content must
  not expand a column without bound. The browser renderer uses one shared data
  table layer for Leaderboard and Timeline Detail Table rendering, sorting,
  filtering, numeric metric shading, empty states, table headers, cells, and
  row state. Each table has isolated table state keyed by table id so sort and
  filter changes in one table do not affect the other table or the report JSON
  payload.
- HTML renders Timeline Waterfall and Timeline Detail Table diagnostics inside
  the selected Trial trajectory, after Notes/Evidence and before the final
  Steps list. These diagnostics are derived in the browser from the existing
  `trajectory_meta.steps` timing/tool metadata and matching ATIF steps; they
  must not introduce new report fields or mutate the embedded report payload.
  Timeline diagnostics are a performance trace rather than a second Steps view:
  the browser derives flat `stages` for latency-bearing work and `markers` for
  near-zero contextual events. Waterfall and Detail Table use the same flat
  `stages` list in the same order and do not express nested step/tool hierarchy.
  Near-zero user/system steps (`duration_ms` missing or no more than 50 ms)
  render only as chart markers; longer user/system processing can render as
  `Input processing` or `System context processing` stages. Model generation
  steps render as `Model: <model_name>` when a model name is known, otherwise
  `Model`; if model timing is inferred from adjacent timestamps rather than
  explicit metadata, Timeline table and tooltip values are prefixed with `≈`.
  Order-only source timestamps must not produce estimated model stages. Tools
  with explicit timing metadata render as `Tool: <name>`, including measured
  zero-duration tools with `0.0%` active share; tools with missing or null
  timing are omitted from the Timeline. Failed tool stages are
  categorized as `Error`. Retained-session idle gaps are
  intentionally omitted from Timeline diagnostics; they remain represented by
  Trial `wall_duration_ms` outside the Timeline.
- Timeline Waterfall uses the fixed ECharts 6.0.0 CDN build from
  `https://cdn.jsdelivr.net/npm/echarts@6.0.0/dist/echarts.min.js`. It renders a
  cumulative active-latency Gantt with a shared x-axis, category-colored
  rectangular bars, per-stage duration labels on or beside bars, user/system
  markers, and tooltips containing true wall start/end, active offsets,
  duration, percent of active Timeline duration, category, and source refs.
  Static report mode loads this CDN script directly. Serve UI mode loads
  `/assets/echarts/6.0.0/echarts.min.js` first, then falls back to the same CDN
  URL if the local asset cannot load. The serve asset endpoint reads
  `<workspace>/.cache/echarts/6.0.0/echarts.min.js`; on cache miss it may fetch
  the fixed CDN URL with standard-library HTTP, write the file atomically, and
  then serve it. If ECharts is unavailable, the Waterfall shows a readable
  fallback message while the Detail Table still renders. The chart left gutter is sized from the
  rendered y-axis labels instead of using a large fixed margin, so short labels
  do not leave excessive empty space. The active-latency x-axis uses stable
  nice tick intervals and interval-aware labels, so short traces do not collapse
	  multiple ticks into the same rounded value.
- Timeline Detail Table preserves true wall start, end, duration, and
  active-share values. It uses heuristic categories
  (`I/O`, `Agent`, `Network`, `External`, `Tool`, and `Error`) derived from step
  source, tool names/titles, and status. The table does not render a separate
  visible Category column; instead, the Stage value is tinted by its category so
  type can be scanned without widening the table. Waterfall labels, tooltips,
  and Detail Table stage cells use compact structural labels and must not
  display step message or reasoning previews, which remain available in the
  Steps content for diagnostics. Red Timeline color is reserved for `Error`;
  non-error `External` work uses a neutral category color. Timeline Waterfall
  and Timeline Detail Table are separate, default-expanded collapsible
  sections. Clicking a Timeline Waterfall bar, a Waterfall user/system marker
  with a source `step_id`, or a Timeline Detail Table row opens the existing
  right-side Step details drawer for the corresponding source step, including
  in single-session reports that do not render Leaderboard or Trajectory
  Overview rows. This interaction does not change Timeline row order, selected
  Trial semantics, or JSON payload shape.
  The selected Detail Table row uses one subtle row background and one left
  edge indicator on the first cell; it must not draw repeated vertical
  selection bars across every table column. Sortable data-table headers cycle
  through ascending, descending, and no-sort states; the no-sort state restores
  the filtered rows to their source order. Timeline Detail Table supports
  sorting by `#`, `Stage`, `Start`, `End`, `Duration`, and `Active Share`,
  filters only by `Stage` text, applies metric shading only to `Duration` and
  `Active Share`, and renders `Active Share` with an in-cell proportional fill
  based on the same active-share percentage. It does not render a separate
  Distribution column. Timeline Detail Table sorting and filtering do not
  affect Timeline Waterfall row order, bars, markers, or active-latency axis.
  Timeline diagnostic section shells are structural carriers in the report body
  and must not use pink or tinted filled panel backgrounds; they may keep
  borders and spacing while chart/table surfaces stay readable.
- The HTML renderer has two presentation modes over the same report body:
  static report mode and serve UI mode. Static report mode is the default used
  by `view trajectory --format html` and must not show import controls,
  leaderboard row checkboxes, or report-export controls. Serve UI mode reuses
  the same Leaderboard, Trajectory Overview, selected Trial trajectory, Step
  details drawer, state transitions, and visual tokens, then adds only
  serve-specific controls around that body. Serve-mode table export writes the
  current row selection, or all currently visible filtered rows when no rows are
  selected, as a default `.xlsx` workbook rather than CSV. JSON report and HTML
  report exports keep their existing behavior.

Single-session HTML renders the current Run, Result, Evidence, and Steps
sections. Multi-session HTML renders Report Notes, Leaderboard, Trajectory
Overview, then the selected Trial trajectory. The comparison panels are
runtime-only projections synthesized from `trajectory[]`, `trajectory_meta[]`,
and `trajectory.final_metrics`; they are not stored in report JSON. They render
one primary section title without a duplicate eyebrow label. `Leaderboard` is a
preserved report UI term and remains English in localized reports.
`peval-py` treats each input session as one Trial. Multi-session HTML no longer
renders a separate Visible Heatmap panel. The Leaderboard shows the canonical
session id, a separate Session Alias column that displays `source_alias` or
`-`, agent, model, result, Last Turn End, active duration, turns, tools, tokens,
cost, HTML-only `Analysised`, and notes. Last Turn End is the Trial's
`trajectory_meta.finished_at_ms`;
missing values render as `-` and sort after present values. The
Agent column uses the trajectory agent name and falls back to the adapter id
when the trajectory does not provide an agent name. The Session, Agent, Model,
Result, and `Analysised` columns provide multi-value filters whose values are
collected from the complete Leaderboard row set. Empty selections are
equivalent to no filter, values within one column are OR-ed, and multiple
filtered columns are AND-ed.
Filtering happens before sorting and before metric shading. If filters hide the
currently selected Trial and visible rows remain, HTML selects the first visible
Trial; if filters hide all rows, the selected Trial detail remains visible but
no Leaderboard row is selected. Leaderboard active duration, tokens, Tool Calls,
and Turns cells show per-column metric intensity directly as cell background
shading; each metric column computes its own scale from the currently visible
filtered rows, missing values remain unshaded, and Cost is not shaded. The
filter control appears inline on the right side of the filtered column label,
similar to a spreadsheet table header, instead of occupying a second header
line. `Analysised` displays `True` only when the Trial's analysis annotation
points to cached cell artifacts named `analysis.md` or `analysis.json` through
`relative_paths.md`, `relative_paths.json`, or `relative_path`; computed-only
analysis annotations display `False`. This projection is not written to report
JSON or `trajectory_meta[]`. The Leaderboard table body and Trajectory Overview
list cap their vertical viewport at roughly 10 rows and scroll after that
without truncating rows, filters, sorting, selection, metric shading, or export
scope. The rendered comparison sections must not show benchmark, task,
task-set, task-family, or matrix task-axis fields.

Serve UI mode keeps the report body as the primary mental model rather than
turning the page into a separate dashboard. It shows a compact source/status
toolbar with a persistent language select above the report title and opens
source management in a near-full-screen workbench modal dialog.
The modal supports Session/ATIF path, DB, and input-table source forms, upload
of JSONL, ATIF JSON, or peval-py report JSON snapshots, explicit refresh,
active/archive/delete source lifecycle, and per-source status display. The
modal exposes an adapter default SQLite DB configuration strip above the source
forms and source list; saving a non-empty path updates that adapter's
`default_db_path`, while saving or clearing an empty value removes that default.
Session/ATIF path and DB path fields accept one or more whitespace-separated
paths and honor single- or double-quoted paths; they import refreshable local
paths and do not upload file contents. The DB Inspect action still inspects
  exactly one DB path at a time. Adapter choices in the source manager are
  compact single-select dropdowns in each source form's action row, immediately
  before the add/upload action area, with `auto` as the default; `auto` omits
  the adapter override and lets existing inference/default adapter rules apply.
  When an adapter has a configured `default_db_path`, the DB form exposes that
  path as a default so the user can inspect/import without retyping the SQLite
  path. Source add forms accept an optional alias field. The DB form includes an
  Inspect action that lists adapter-owned sessions in the modal, supports
  checkbox multi-select, and adds selected sessions as independent DB sources.
	  Failed source imports show a transient error containing the concrete server
	  message and must not persist the failed source or show it in the source list.
	  Source aliases can be edited from the source list and affect only display in
	  the source list, the Leaderboard Session Alias column, Trajectory Overview,
	  and selected Trial summary; Evidence/Input Source continues to show the
	  original path. Source Manager rows also expose the latest stored Trial's
	  Last Turn End using `trajectory_meta.finished_at_ms`, without requiring a
	  source refresh.
	  It does not add a persistent left sidebar or reduce the report body width.
  Serve-only controls use the same color, radius, typography, and panel tokens as
  static reports but sit at a lower visual priority than report content.
  Source Manager form shells are structural carriers rather than filled cards:
  they must not use pink or tinted filled panel backgrounds, while text inputs,
  uploads, and menu surfaces remain solid enough to read.

`serve` does not refresh sources on startup unless source flags were supplied on
that invocation. The page opens from the latest canonical Trial artifacts
and marks sources with their latest status. When composing the active served
report for the initial `/` page render or `/api/report`, `serve` re-reads
current workspace-side cell `analysis.json`, `analysis.md`, and `notes.md` from
each active source's stored Trial artifact cell path and overlays those
annotations on the stored artifacts without mutating the stored trajectory or
requiring the original source file/DB session to refresh successfully. This
scan includes non-refreshable report snapshots and observes both added and
deleted analysis files on the next page/API reload.
Refresh is explicit from the source manager or through source flags on the
`serve` command.

In serve UI mode, the selected Trial Notes section shows `Edit notes` when the
selected Trial maps to a refreshable source with an existing cell-local note and
`Add notes` when the selected Trial maps to a refreshable source without one.
The editor only edits the peval cell `notes.md`; CLI and input-table notes
remain read-only. Snapshot and uploaded non-refreshable sources must not expose
a save entry point, though their persisted notes still render read-only.

Serve HTTP APIs are same-origin local APIs. The server must not enable CORS.
Mutating APIs require JSON `POST` requests and must reject non-same-origin
`Origin` or `Referer` headers. Localhost binding is the only network exposure
model in this version; there is no token, account, or remote-host authentication
surface.

`GET /assets/echarts/6.0.0/echarts.min.js` serves the workspace ECharts cache.
It must not expose arbitrary file paths. On cache miss, the endpoint may fetch
the fixed ECharts CDN URL, create parent directories under
`<workspace>/.cache/echarts/6.0.0/`, write atomically, and return the cached
script. If fetching fails, the endpoint returns an error so the browser can fall
back to CDN.

`POST /api/config/locale` accepts JSON `{ "locale": "en|en-US|zh|zh-CN" }`,
normalizes the locale, writes top-level `locale` to `<workspace>/peval-py.toml`,
updates the running serve config, and returns the normalized locale. The browser
then reloads the page so embedded i18n messages are regenerated.

`POST /api/config/adapter-default-db` accepts JSON
`{ "adapter": "ADAPTER_ID", "default_db_path": "PATH" }`, validates the adapter
against the available adapter registry, writes or clears
`[adapters.<adapter-id>].default_db_path` in `<workspace>/peval-py.toml`,
updates the running serve config, and returns the updated adapter default DB
map. Blank `default_db_path` clears the adapter default. The save endpoint does
not require the SQLite file to exist; DB import and inspect remain responsible
for checking usable DB paths.

`POST /api/db-sessions` accepts JSON `{ "db": "PATH", "adapter": "optional" }`.
The DB path resolves like other serve source paths: relative paths are resolved
under the workspace root and absolute paths are expanded directly. Without an
explicit adapter, the endpoint uses generic path-token adapter inference only;
if no adapter or more than one adapter matches, it fails clearly and asks for an
adapter. On success it returns the resolved DB path, adapter id, whether the
adapter was inferred, and session rows with one-based `index`, `session_id`, and
`name`. `POST /api/sources` continues to accept a single `session_id` and also
accepts `session_ids` for DB payloads; each selected session creates or updates
one independent refreshable source before the response report is rebuilt. For
path and DB payloads, a single string may contain multiple paths parsed with
Windows-safe shell-like quoting: quoted paths may contain spaces, unquoted
Windows drive paths such as `C:\Users\me\state.db` keep their backslashes,
Windows drive and UNC paths are treated as absolute-like paths and are not
resolved under the workspace root, and POSIX hosts may map existing drive paths
through `/mnt/<drive>/...`. New source imports are all-or-nothing: if any
newly submitted source fails to load, convert, or refresh, the endpoint returns
a JSON error and does not persist any source from that request. Refreshing an
already persisted source keeps the existing artifact directory when conversion
fails, and records the latest status/error/log entry. If refreshing a persisted
source converts to a different canonical cell identity, refresh fails and keeps
the existing source/artifact unchanged instead of silently changing one source
into another Trial. `POST
/api/sources/{source_key}/alias` accepts JSON `{ "alias": "..." }`, updates only
the display alias for that source, rebuilds the composed report payload from
stored snapshots, and does not refresh or mutate the original source file or DB.
An empty alias clears the alias. `POST /api/sources/{source_key}/delete` deletes
only peval-py state for that source and refresh-log rows; it never deletes the
original local file or DB. Because serve enforces one source per Trial cell, it
also deletes that source's persisted Trial cell artifacts.

`POST /api/sources/{source_key}/notes` accepts JSON `{ "markdown": "..." }`,
requires the same JSON POST and same-origin checks as other mutating APIs, and
writes UTF-8 `notes.md` only for refreshable sources with a persisted Trial
cell. The Markdown payload is limited to 1 MiB after UTF-8 encoding. On success,
the server writes `<artifact_dir>/notes.md`, refreshes that source immediately,
and returns the standard mutation payload `{ sources, report }`. Saving an empty
string writes an empty `notes.md`; delete semantics are not part of v1.

In serve UI mode, the Leaderboard may add a row-selection checkbox column at
the start of the existing full column set. Header and row checkboxes control
export selection only; they must not change the selected Trial, open the Step
details drawer, or change the filtered/sorted row set. Clicking a Leaderboard
row remains the canonical selected-Trial interaction. The Trajectory Overview
continues to follow the currently filtered and sorted Leaderboard rows and
does not follow checkbox state.

  Serve UI mode renders one Leaderboard `Export` menu in the panel header. Its
  menu items are `Table`, `JSON Report`, and `HTML Report`. All serve exports use
  the same row scope rule: visible checked rows when at least one currently
  visible row is checked, otherwise the current filtered and sorted visible row
  set. Checked rows hidden by filters remain checked in UI state but are excluded
  from the current export scope until they become visible again. JSON and HTML
  exports create report subsets for that same export scope; table export defaults
  to CSV and must not introduce an Excel dependency.
  Export and data-table filter menus close when the user clicks outside the open
  menu or opens another menu, while clicks inside the menu keep it open. This
  submenu behavior applies only to menu-like `<details>` controls and must not
  auto-close Timeline diagnostic sections or Step detail sections.

The Trajectory Overview section below the Leaderboard renders one row per
session in the same order as the currently filtered and sorted Leaderboard
rows; its row count and row order must exactly match the rendered Leaderboard.
Long trajectories wrap their fixed-size nodes onto additional lines inside the
row instead of forcing a single horizontally scrolling node track.
Each row shows a compact left-to-right node track where each ATIF step is one
node. Overview nodes use a neutral visual style and show source initials:
`S` for system, `U` for user, `A` for agent, and `?` for unknown or unsupported
sources. Nodes with positive `trajectory_meta.steps[].duration_ms` also render a
subtle, very low-contrast ten-level duration heat background shade scaled against
the slowest timed step in that same visible Trial; untimed or zero-duration nodes
remain neutral, and selected node styling stays stronger than heat styling. Node
title and aria text include the step duration when available. All rows share a
grid width based on the largest step count among visible sessions, so nodes at
the same step index align vertically and shorter trajectories leave empty
positions at the end. Clicking a Trajectory Overview row selects that Trial.
Clicking a node selects that Trial and opens a fixed right-side Step details
drawer showing the same expanded step markup and block content used by the
final Steps section. On desktop, the drawer uses a wider inspection width than
the initial compact rail so longer reasoning, tool, and observation content can
be read without excessive wrapping. The widened drawer
must not obscure the middle report content: when it opens on desktop, the page
layout reserves the drawer's right-side width and constrains the main workspace
to the remaining viewport. Its expanded step layout is content-sized: the step
summary stays at the top, short visible content blocks do not stretch merely to
fill the drawer, and long block payloads scroll inside their own blocks. When
browser zoom or a short viewport leaves less vertical room than the drawer
content needs, the drawer itself remains scrollable so lower blocks and controls
stay reachable instead of being clipped. The drawer supports a close button,
Escape, and
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
execution in the selected Trial, and elapsed chips scale against
`wall_duration_ms` when available, then the selected Trial first-to-last
timestamp span, and finally the active `duration_ms`.

HTML localization covers the report title, report-level chrome, comparison
section titles, metric labels, comparison table headers, comparison filters,
drawer chrome, comparison empty states, buttons, aria labels, comparison status
labels, and the selected Trial summary, notes, result, and evidence sections.
Only the final selected Trial Steps detail section remains English in this
version. English is the default.
Simplified Chinese is selected with `defaults.locale = "zh-CN"` or the `zh`
alias. Serve UI mode also exposes a language select that writes the normalized
top-level workspace `locale`; static reports remain config-driven and there is
still no CLI locale flag. In Simplified Chinese reports, domain terms such as Run, Result, Notes,
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

Timing comes from message metadata when available. For Psychevo messages,
`metadata_json.elapsed_ms` on assistant rows is the preferred assistant step
duration, and `metadata_json.elapsed_ms` on tool-result rows is the preferred
tool execution duration. If explicit metadata is absent, converters and report
projection may fall back to timestamp spans only when the span is non-negative
and no more than 600,000 ms. Longer timestamp-derived spans are treated as
human idle or unknown retained-session delay and are excluded from active
duration. Explicit source durations are trusted even when they exceed that
fallback cap.

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

When a peval-py workspace root is known, report generation may also read peval
cell manual notes from
`<workspace>/runs/<analysis_eval_slug>/<agent-id>/<session-id>/<cell_key>/notes.md`.
These notes are Trial annotations, not Analysis. `<session-id>` is the
displayed trajectory session id, and `<agent-id>` is the input `agent_name`
when provided, otherwise the effective adapter id. `<cell_key>` is the
Trial's `trajectory_meta.trial_key` after safe path-segment normalization.
Missing, unreadable, invalid UTF-8, or oversized note files are silently omitted
so ordinary view/export/serve workflows keep rendering. Valid cell notes enter
`annotations.notes[]` with `trial_key`,
`source = "cell"`, `label = "notes.md"`, `markdown`, and a
`source_ref = { kind = "note", label = "notes.md", relative_path = "..." }`
object. For the same Trial, cell notes render before CLI or input-table notes.
CLI and input-table notes continue to use `source = "cli"` and labels such as
`CLI note N`.

Report generation creates a computed analysis annotation for each Trial before
reading workspace cached analysis. The computed annotation has `status =
"computed"`, the Trial key, and `analysis_metrics.auto`. The `auto.tooling`
group records tool error rate and distinct tool names. The `auto.cost` group
records cost per 1k tokens when both cost and token totals are available. The
`auto.latency` group records min, q1, p50, q3, p95, and max for step duration,
tool execution duration, and measured model duration when observed values are
available. Model duration is derived from agent/assistant step timing and
excludes known boundary-estimate timing sources so inferred model spans are not
presented as exact latency.

When a peval-py workspace root is known, report generation may enrich the same
Trial analysis annotations with Rust peval cached analysis from
`<workspace>/runs/<analysis_eval_slug>/<agent-id>/<session-id>/<cell_key>/analysis.json`
and `analysis.md`. The default `analysis_eval_slug` is `default`; `<session-id>`
is the displayed session id; `<agent-id>` is the input `agent_name` when
provided, otherwise the effective adapter id. `<cell_key>` is the Trial's
`trajectory_meta.trial_key` after safe path-segment normalization. Missing
files, malformed JSON, or unreadable Markdown are silently omitted so ordinary
view/export/serve workflows keep rendering. Valid cached analysis updates the
matching `annotations.analysis[]` entry to `status = "cached"` and may add
`relative_path`, optional `summary` from the analysis JSON top-level `summary`
field, optional typed incremental fields from `analysis.json`, optional
`md_report` from `analysis.md`, and optional `relative_paths` with `json` and
`md` entries. `relative_path` is retained for compatibility and points to JSON
when present, otherwise Markdown. `status` on the annotation remains the cache
source status; `analysis.json.status` is exposed as `analysis_status`, and
`analysis.json.metrics` or compiled `analysis.json.extra.metrics` is exposed as
flat imported keys in `analysis_metrics`. If a cached artifact contains
`analysis_metrics` or an `auto` metric object, those fields must not replace
`analysis_metrics.auto`. Absolute paths are not exposed, ATIF trajectory data
is not changed, and peval-py never
executes analysis agents. The explicit `peval-py import analysis` command is
the only CLI path that writes compiled analysis cache files. In `serve`, the
active report composition path refreshes only this workspace-side annotation
overlay; the persisted trajectory snapshot remains the last successful source
conversion.
