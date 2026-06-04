---
name: 300. peval CLI View Attachment
psychevo_self_edit: deny
---

Define view generation and comparison behavior for `peval`.

This attachment is part of [300 peval CLI](spec.md).

## Scope

- Trial fact input sources
- HTML and JSON output roles
- redaction defaults
- view-based comparison and inspection behavior

## View Inputs

Views are derived from artifact v8 cell facts under
`runs/<benchmark-id>/<agent-id>/<task-id>/<short-fingerprint>/run.json`. They must not
depend on active agent sessions, mutable benchmark workspaces, generated
dashboards, visible indexes, hidden caches, or v2 per-invocation run summaries.
The physical layout remains a cell directory, but the public view model calls a
single execution fact a Trial and a comparison position a MatrixCell.

`peval view` is the built-in static reporting, comparison, and inspection
surface. It projects selected cell facts into a deduplicated core plus
role-based projections: `core`, `comparison`, `annotations`, and
`attachments`. The default scope is the selected benchmark. `--path/-p` may be
repeated to compare explicit path selections. Each selected path may narrow the input to a
benchmark, agent, task, or exact cell directory. The path is a view-only
variant group; descendant cells inherit the group's automatic path label.
Filters are applied after all selected path groups are expanded and unioned.
Any explicit path that resolves to zero cell facts fails before filtering, and
the same cell must not be selected by multiple explicit path groups.

Report profiles are optional named view definitions declared as
`[reports.<key>]` in eval, workspace, or user config. The first supported
profile field is `[reports.<key>.analysis]`, which overrides `[analysis]`
defaults for explicit analysis runs. Profile lookup uses user, workspace, then
eval merge order, so eval-local settings have the highest precedence.

HTML is the default local human review format. JSON is the machine DTO format
and exposes structured references so automation can perform explicit follow-up
reads. Markdown view generation is removed; callers that request `markdown`,
`md`, or an `.md`/`.markdown` output path receive a clear error that points to
HTML and JSON.
`-i all` is a parser-level alias for the complete static diagnostic report:
`core,comparison,annotations,attachments`. With no explicit include, the
default is `core,comparison,annotations`. Legacy section include names
(`summary`, `matrix`, `usage`, `warnings`, `artifacts`, `trajectory`,
`trajectory-meta`, `notes`, and `analysis`) are removed from the public include
surface in schema v17 and fail clearly with guidance to the role-based include
names. `timeline`, `atif`, `logs`, and `diff` also fail clearly.

Schema v17 top-level JSON always contains `schema_version`, expanded
`includes`, `scope`, `path_selections`, `trajectory`, and `trajectory_meta`.
When included, `comparison`, `annotations`, and `attachments` are role-based
sections keyed by `trial_key`. `trajectory` is standard ATIF v1.7 and is the
authority for transcript content plus ATIF metrics such as token, tool, turn,
and cost totals. `trajectory_meta` is the peval sidecar for information ATIF
cannot express: Trial identity, variant, relative cell root, result status,
score, score details, warnings, started/finished timing, source
`trajectory.jsonl` reference, event counts, prompt unavailability, derivation
errors, and per-step timing/truncation/tool-status hints.
Shared trajectory timing and visualization semantics are defined in
[340 Trajectory](../340-agent-evaluation/trajectory.md). This attachment
defines peval-specific view schema, comparison, and rendering behavior around
those shared semantics.

`comparison` contains `summary`, `groups`, `matrix`, `leaderboard`, and
`default_metric`. These are projections over core Trials and must carry only
axis/group identities, aggregate values, metric values, and `trial_keys`.
Single-Trial facts are resolved by joining those keys against
`trajectory_meta` and `trajectory.final_metrics`. Matrix cells include
`matrix_cell_key`, `trial_keys`, `representative_trial_key`, axes, optional
variant id/label, and comparison metric values. `default_metric` is the first
initial heatmap metric, in UI metric order `score`, `duration`, `tokens`,
`tools`, `turns`, whose numeric values are not all identical across visible
matrix cells. If no metric varies, it is `score`. Leaderboard entries are
grouped by `agent_id` plus model name and, in multi-path mode, variant id.
Default ranking orders by pass rate or score descending, then by average
duration ascending, then by tokens and cost ascending.

HTML renderers keep matrix cells aggregated when `trial_keys` contains more
than one Trial, but selected-Trial state is exact. Heatmap selected state and
visibility checks use membership in `trial_keys`, not only the
`representative_trial_key`. Clicking a heatmap cell chooses the representative
Trial for that cell; clicking a Trial details row chooses that exact Trial. The
selected-Trial panel shows a compact sibling switcher for all Trials in the
current matrix cell when there is more than one, and the Trial details table
adds a short Trial identity column only for reports that contain a multi-Trial
matrix cell.

The HTML Agent / Model Comparison table is a per-task comparison projection.
Its score, duration, token, and cost columns use per-Trial averages for the
row, so repeated Trials in the same matrix cell do not inflate token or cost
values. Machine-readable leaderboard entry totals may still expose summed
token and cost fields for overall accounting, and the Trial details table shows
exact single-Trial values.

`annotations` contains report notes, Trial notes, and cached analysis. Manual
notes come from cell-local `notes.md` files or repeatable
`--note INDEX=TEXT` CLI entries, are report-only metadata, and never mutate
`run.json` or `analysis.json`. `--note 0=TEXT` creates report notes;
`--note N=TEXT` for `N >= 1` targets the one-based Trial index in the current
filtered view. `attachments` contains artifact `ViewDataRef` values keyed by
`trial_key`; static JSON and HTML omit absolute paths.

Physical references are represented as `ViewDataRef` values with `kind`,
`label`, `relative_path`, `mime`, `size_bytes`, and available hash or mtime
metadata. Static JSON and HTML omit absolute paths; the local service
may add `absolute_path` and token-scoped `access_url` in service-only DTOs.

When `-o`/`--output` is present without a path, views are written under
`<workspace>/views/` by mirroring the selected `runs/` scope and adding
`index.<ext>`. Absolute `--path` scopes outside the workspace `runs/` tree must
use an explicit output path.

## Visible Content

A view should show the selected matrix shape, pass/fail counts, setup and
runtime failures, scores, duration, model/candidate identifiers, cost or usage
when available, and status aggregation. View renderers read metrics from ATIF
metrics and peval sidecar result fields. They do not recalculate token usage,
cost, duration, turn counts, or tool counts from human logs.

Usage and accounting are ATIF metric data. Comparison tables may aggregate
those values, but they must not duplicate per-Trial usage rows. Warnings are
part of the `trajectory_meta` result sidecar for the relevant Trial and do not
change pass/fail aggregation.

Each new run retains the task prompt as `prompt.md` in the cell root so views do
not require retaining the whole workspace to show the user-visible task input.
Older runs without that artifact remain valid and render
`extra.prompt_unavailable = true` in derived ATIF. Static JSON views show
bounded summaries and references by default. HTML also shows bounded summaries
by default, and may inline only
physical data that directly improves visualization or readability: trajectory
data, small image artifacts, and key diff hunks. Ordinary logs and artifacts
remain summary plus references. Full prompt, model output, tool output,
evaluator stdout/stderr, process log, and large image bodies are exposed
through the local server or explicit raw/detail reads with root containment and
a 1 MiB inline limit. Coding diff views discover existing `*.diff` or `*.patch`
files first and fall back to tool-result diffs from the trajectory.

Trajectory JSONL remains the canonical local event stream. ATIF v1.7 is derived
on demand for view/server/export use. Command adapters map user, assistant,
tool-call, tool-result, and usage/accounting events into ATIF steps when those
fields are present. ACP adapters project thought chunks, agent message chunks,
tool calls, tool updates, and observations into fine-grained ATIF steps instead
of one opaque aggregate step. System prompts are represented only when an agent
or adapter explicitly exposes a system/system_prompt/system_message event;
peval does not infer or synthesize system instructions. Unmapped lifecycle
events remain available through peval timeline metadata. ACP tool calls and
observations are grouped into the current agent transcript step by
`toolCallId`; they do not create independent Steps. Step counts therefore
represent visible transcript rows (`user`, explicit `system`, and `agent`
rows), while tool-call and tool-error counts remain separate metrics. When the original
prompt is not retained, derived ATIF remains structurally valid and records
prompt unavailability in trajectory metadata. View schema v17 treats
`trajectory` as standard ATIF v1.7 data and moves only ATIF gaps into
`trajectory_meta`. Metadata contains the trajectory `data_ref`, source event
counts, prompt unavailability, Trial identity/result/locator fields, and
per-step timing/truncation/tool-status hints. It does not repeat step source,
label, summary, tool names, token totals, cost, tool counts, system/reasoning
exposure, or graph fields that renderers can derive from ATIF.
Step counts represent visible transcript rows (`user`, explicit `system`, and
`agent` rows), while tool-call and tool-error counts remain separate metrics.
Reports should render complete trajectory visuals up to roughly 50 Trials or
1000 steps; above that threshold they degrade to summaries, references, and
bounded expansion.

ATIF keeps the `ATIF-v1.7` schema string and must not carry peval-only producer
fields such as view labels, event counts, raw source metadata, tool-status
titles, or UI timings. ATIF final metrics are the authority for normalized
token, tool, turn, and cost values. HTML renders only selected-Trial evidence;
it does not render a report-wide evidence ledger.

## Redaction

Views redact credential values, HTTP headers, non-allowlisted environment
variables, host-specific secret paths, and provider keys by default. Raw export
must be a separate explicit diagnostic operation; it is not implied by HTML or
default JSON views. System prompt, task prompt, message, and reasoning/thought
blocks use the same redaction as other bounded previews. HTML trajectory step
rows may expand, but the message, reasoning, and evidence blocks inside them
are ordinary visible sections rather than nested collapsed controls. A user
clicking `Run analysis` in the local service is explicit authorization for the
configured analysis agent to read the relevant Trial artifact directory and
optional task directory read-only; the service, not the agent, owns analysis
cache writes.

## Comparison

Comparison is a view query over cell facts. Grouping and matrix projection align
cells by canonical benchmark, task-set, task, factor, candidate identity, and
view-derived path variant identity when explicit path groups are selected. They
should surface missing cells, regressions, improvements, setup/runtime/scoring
failures, and usage deltas without rerunning agents or evaluators.

Harbor-style matrix comparison is the default multi-job shape. Rows represent
tasks and columns represent agents or candidate identities. Cell color/value use
the configured primary metric; when not configured, score or pass rate is the
primary metric. Hover and detail views expose status, duration, usage, cost,
turns, tool calls, and failure class.

`peval serve` is a Harbor-inspired local workspace viewer, but it uses peval
terms and schema v17 data. It defaults to the whole workspace, lazy-loads
benchmark and Trial details, writes only analysis cache files, and protects
file routes with localhost binding, a generated token, containment checks,
MIME checks, and a 1 MiB raw/detail limit. The built-in UI uses offline local
HTML, CSS, and native JavaScript. Its visual direction is a light "scientific
evaluation workbench": mid-high density, desktop-first, mobile-readable,
report-quality overview, linked matrix detail drawers, and Harbor-style
transparent trajectory rows with expandable message, reasoning, tool-call,
observation, and metrics blocks.

Static HTML report prototypes explore the same local diagnostic direction
without requiring a server or build chain. Candidate reports should be
single-file, zero-dependency HTML with inline CSS and native JavaScript, aimed
at local failure investigation and batch comparison. The preferred prototype
direction is the matrix-first workbench represented by
`.local/design/candidates/peval-html-prototype-05-transparent-trace.html`:
the leaderboard is the first report section, its aggregate filters drive the
heatmap, and metric switching stays near the heatmap. The report is a
single-column diagnostic surface; selected-trial
outcome, metrics, and paths live in the trajectory panel rather than a right
sidebar. In this shape, heatmap cell hue
continues to represent status, while a bounded five-step shade scale represents
the selected metric's relative position among currently visible cells. A
visible Trial cell with a missing selected metric still uses its status hue at a
middle shade and displays `-`; only absent matrix slots use the neutral empty
style.
Selecting a MatrixCell should reveal the Trial trajectory directly below the
matrix as Harbor-style ATIF transcript rows. The trajectory panel should show
only `Run`, `Result`, and `Steps` sections before the step list. `Run`
contains identity, session, started-to-finished timing, duration, step/event
counts, and system/reasoning exposure. `Result` contains status, score,
evaluator message, tokens, turns, tool successful/total calls, and cost; score
details belong in Evidence rather than the main panel. Artifact paths belong in Evidence
rather than the main trajectory panel. Each step row should show source, model,
duration/elapsed, token/cost, tool-call, and tool-error cues when those fields
are available. It must not synthesize per-step token usage from Trial-final
usage totals; ACP agents that expose only final prompt usage show total tokens
in Result and Usage evidence, while individual Step rows omit unavailable token
cues. The collapsed Step row preview uses only the Step message content, not
reasoning, tool-call, or observation summaries; when a Step has no message
content, the preview is `(No Message)`. The collapsed row rail follows
[340 Trajectory](../340-agent-evaluation/trajectory.md): summary chips such as
`N/M tools`, compact `M.Nk tok` token counts, step span, and elapsed offset stay
on the first line, while ordered `tool name + execution time` chips move to a
second line. Detailed usage sections keep full numeric values. Step span and
elapsed render as separate chips labeled `step <duration>` and
`elapsed <duration>`; `tool` timing remains absent from the first-line summary.
Expanded Steps do not render a separate Metrics block; Tool Calls show `tool
exec` directly after the tool name when execution duration is available. Step duration represents the current
Step's observed span, using an explicit grouped-step end timestamp when the
adapter exposes one; it must not display the gap since the previous Step as the
current Step's duration. Expanding a row reveals ordinary non-collapsible
Reasoning, Message or System Prompt, Tool Calls, and Observations blocks when
those fields exist. ACP adapters should surface pending
tool-call argument generation separately from tool execution start/end when the
underlying runtime exposes it. `psychevo-acp` carries runtime timing through
ACP `_meta.psychevo.toolTiming.startedAtMs` and `elapsedMs`; v17 trajectory
metadata prefers that elapsed runtime timing for `tool exec` and marks the
duration source as `runtime_meta`. When old trajectory data lacks that metadata,
reports keep the existing ACP event timestamp fallback and mark the source as
`event_timestamps`. Report renderers label `step span` separately from `tool
exec`; a long model interval before a tool call must not be presented as a long
tool execution. System Prompt appears
only for explicitly exposed system/system_prompt/system_message events and is
never synthesized from hidden runtime instructions. Artifact entries should not
emphasize file sizes unless file-size diagnosis is the selected task. Desktop
is the primary layout target; narrow screens remain a readable single-column
report.

The HTML workbench also renders the schema v17 comparison leaderboard as flat comparison
tables rather than per-agent/model groups. The default aggregate table has one
row per agent/model/task breakdown so runs can be compared horizontally, and a
single global Trial details table remains expandable below it. Leaderboard
tables align columns across header, filter, and body rows. Enumerated identity
columns such as agent, model, task, family, and status support compact
multi-select exact filtering only. No selected values means all values are
visible. Numeric result columns such as trial counts, success counts, pass rate,
score, duration, tokens, and cost support sorting only, with a visibly distinct
sort direction indicator. Result headers use compact labels and fixed numeric
column widths so the label and sort indicator do not wrap into each other.
All HTML duration presentations, including heatmap metric values, table
duration cells, selected-Trial duration summaries, expanded tool generation
timing, tool execution timing, and Step rail `step`/`elapsed` chips, use seconds
with one decimal place. Unknown durations render `-`; values above one minute
may use minute-plus-seconds notation while preserving one decimal on seconds.
The selected-Trial Run summary follows Harbor's wall-clock model and displays
wall duration from `finished_at_ms - started_at_ms`, falling back to stored
Trial duration only when timestamps are unavailable. This prevents the Run
summary from comparing a monotonic case metric against wall-clock trajectory
elapsed values. Matrix cells, leaderboards, and sorting still use the stored
case metric duration.
The Variant column is conditional: aggregate and Trial details tables render it
only when at least one visible row has a real path variant id or label. Reports
without explicit path variants must not show an all-`-` Variant column or
filter control.
Report-wide row-count status bars are omitted. Both aggregate and Trial details
table body rows are clickable and update the selected Trial panel. The Trial
details table omits long path columns; selected Trial metadata such as the cell
root is shown below the tables instead.
The selected Trial Steps header provides one combined Expand all / Collapse all
toggle for that Trial's transcript rows. The toggle expands all rows when any
row is collapsed and collapses all rows when every row is open. It only changes
the current step list's `<details>` state; it does not alter table expansion,
filters, selected metric, or report data.

The old report-wide diagnostic sections are not rendered as an Evidence Ledger
in HTML, even for `-i all`. Included annotations and attachments remain in the
JSON DTO, and HTML projects only the currently selected Trial's supplemental
evidence into the selected Trial panel. That panel must not repeat information
already shown in its Run and Result summaries; supplemental evidence covers
cell root, score details, warnings, and artifacts for the selected Trial only.
Usage breakdown is read from ATIF final metrics rather than a separate usage
section. Artifacts render with compact relative labels and avoid layout-breaking
absolute paths in the main table surface. Logs and diff are not separate report
sections.

Manual notes are lightweight human commentary for static reports. A cell may
contain `notes.md`; when the `annotations` include is active, its Markdown text is
read only from the cell root, bounded to the same 1 MiB text limit as other
view previews, and exposed as a `ViewNoteReport` with `trial_key`, `source`,
`label`, `markdown`, and optional `source_ref`. `peval view --note INDEX=TEXT`
adds temporary CLI notes for this render only. Index `0` creates a report-level
note exposed in `report_notes`; indexes `1..N` target the current filtered
Trial order exposed by the view and fail clearly when out of range. If both
`notes.md` and CLI notes apply to the same Trial, `notes.md` renders first and
CLI notes append in argument order. HTML renders report notes beneath the title,
Trial notes in the comparison tables' final `Notes` column with hoverable full
text, and the selected Trial's notes in the Trial panel. Table note summaries
and selected Trial notes display note text only; source labels, `notes.md`
prefixes, and Trial keys are not shown as note headers. HTML renders supported
Markdown after escaping raw HTML so scripts and unsafe links are not executed.

Cached Trial analysis remains a derived artifact but is not rendered as a
standalone Evidence Ledger section. When analysis is included, the selected
Trial panel shows that Trial's cached analysis status and summary; missing
analysis renders a compact "No cached analysis." placeholder.

Analysis is a cached derived artifact, not a cell fact. `[analysis]` provides
defaults and `[reports.<key>.analysis]` may override them through eval,
workspace, then user precedence. The configured `agent` references a peval
agent id. Rubrics may be declared inline or by `rubric_path`; the default rubric
contains `reward_hacking`, `task_specification`, and `failure_diagnosis`.
Analysis is explicit-button only. The service supports Trial analysis and
batch analysis over the current filter's failed Trials, defaults to concurrency
4, supports progress and cancel, validates structured agent output, retries
once after invalid output, and writes `analysis.json` with `trial_name`,
`summary`, `checks`, schema/status/timestamp metadata, rubric id, input
fingerprint, references, and error state. It must not save raw agent output,
the full prompt, or full context.

## Related Topics

- [090 Artifacts](../090-evaluation/artifacts.md)
- [340 Trajectory](../340-agent-evaluation/trajectory.md)
- [300 Commands](commands.md)
