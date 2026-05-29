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
surface. It projects selected cell facts into summary, matrix, usage, artifact,
trajectory, trajectory metadata, log, analysis, diff, and grouped views. The
default scope is the selected benchmark. `--path` may narrow the input to a
benchmark, agent, task, or exact cell directory, and filters are applied after
path scoping.

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
`summary,matrix,usage,warnings,artifacts,trajectory,trajectory-meta,analysis`.
`timeline`, `atif`, `logs`, and `diff` are removed from the public include
surface; requesting them is a clear error rather than an alias. `atif` fails
with guidance to use `trajectory`. View DTOs serialize the expanded list and
use view schema v12 for the Trial,
MatrixCell, trajectory, and trajectory metadata sections.

Schema v12 top-level JSON contains `summary`, `matrix`, `leaderboard`, `trials`,
`trajectory`, `trajectory_meta`, and section arrays keyed by `trial_key`:
`artifacts` and `analysis`. Artifacts contain only absolute path lists plus an
optional error. `matrix` contains task and agent
axes plus cells. Each MatrixCell has `matrix_cell_key`, `trial_keys`, and
representative metric values from the latest Trial for the task/agent/model
identity. Each Trial has deterministic `trial_key = "<matrix_cell_key>:t001"`
for the current single-attempt cell layout. Public DTOs must not serialize the
legacy `cell_key` field or absolute cell-root paths in static JSON/HTML. Trial
DTOs include run timestamps, candidate model, relative cell-root locator, score
pass/message/details, and a bounded redacted prompt preview when a prompt
artifact is retained.

The schema v12 `leaderboard` is a first-class machine-readable report section.
Entries are grouped by `agent_id` plus model name. Aggregate rows expose total
trials, successes, pass rate, average score, average duration, total tokens,
total cost, task breakdown, and trial rows. Default ranking orders by pass rate
or score descending, then by average duration ascending, then by tokens and
cost ascending. Trial detail rows are represented by `trial_keys`; renderers
join those keys against the top-level `trials` array.

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
when available, and status aggregation. View renderers read metrics from the
structured metrics fields. They do not recalculate token usage, cost, duration,
turn counts, or tool counts from human logs.

Usage and accounting are separate projections. Usage shows provider token
counts and cost. Accounting shows the runtime accounting mirror fields when any
cell provides them. When no accounting data exists, the accounting section is
omitted rather than filled with placeholder values.

Warnings are a separate report section when present. Warnings are read from
cell facts and do not change pass/fail aggregation.

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
prompt unavailability in trajectory metadata. View schema v12 treats
`trajectory` as standard ATIF v1.7 data and moves peval-only UI hints into
`trajectory_meta`. Metadata contains the trajectory `data_ref`, total events,
unmapped events, grouped step count, system/reasoning exposure flags, per-step
label/summary/timing/truncation/tool-status hints, and the optional step graph.
Step counts represent visible transcript rows (`user`, explicit `system`, and
`agent` rows), while tool-call and tool-error counts remain separate metrics.
Reports should render complete trajectory visuals up to roughly 50 Trials or
1000 steps; above that threshold they degrade to summaries, references, and
bounded expansion.

ATIF keeps the `ATIF-v1.7` schema string and must not carry peval-only
producer fields such as view labels, event counts, raw source metadata,
tool-status titles, or UI timings. Step metrics, final metrics, structured cell
metrics, and the view usage/accounting sections must agree for normalized
values. HTML renders evidence as an always visible ledger and does not collapse
message, reasoning, or evidence blocks.

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
cells by canonical benchmark, task-set, task, factor, and candidate identity. They
should surface missing cells, regressions, improvements, setup/runtime/scoring
failures, and usage deltas without rerunning agents or evaluators.

Harbor-style matrix comparison is the default multi-job shape. Rows represent
tasks and columns represent agents or candidate identities. Cell color/value use
the configured primary metric; when not configured, score or pass rate is the
primary metric. Hover and detail views expose status, duration, usage, cost,
turns, tool calls, and failure class.

`peval serve` is a Harbor-inspired local workspace viewer, but it uses peval
terms and schema v12 data. It defaults to the whole workspace, lazy-loads
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
content, the preview is `(Empty Message)`. Step duration represents the current
Step's observed span, using an explicit grouped-step end timestamp when the
adapter exposes one; it must not display the gap since the previous Step as the
current Step's duration. Expanding a row reveals ordinary non-collapsible
Reasoning, Message or System Prompt, Tool Calls, Observations, and Metrics
blocks when those fields exist. Step Metrics omit unavailable key/value pairs
rather than rendering placeholder dashes. ACP adapters should surface pending
tool-call argument generation separately from tool execution start/end when the
underlying runtime exposes it. Report renderers label `step span` separately
from `tool time`; a long model interval before a tool call must not be presented
as a long tool execution. System Prompt appears
only for explicitly exposed system/system_prompt/system_message events and is
never synthesized from hidden runtime instructions. Artifact entries should not
emphasize file sizes unless file-size diagnosis is the selected task. Desktop
is the primary layout target; narrow screens remain a readable single-column
report.

The HTML workbench also renders the schema v12 leaderboard as flat comparison
tables rather than per-agent/model groups. The default aggregate table has one
row per agent/model/task breakdown so runs can be compared horizontally, and a
single global Trial details table remains expandable below it. Leaderboard
tables align columns across header, filter, and body rows. Enumerated identity
columns such as agent, model, task, family, status, and cell root support
compact multi-select exact filtering only. No selected values means all values
are visible. Numeric result columns such as rank, trial counts, success counts,
resolution rate, score, duration, tokens, and cost support sorting only, with a
visibly distinct sort direction indicator. Result headers use compact labels
and fixed numeric column widths so the label and sort indicator do not wrap into
each other. The old diagnostic sections are reduced to an always-visible,
de-duplicated Evidence Ledger for `-i all`, rather than competing with the
primary leaderboard, matrix, trajectory, and outcome surfaces. The Evidence
Ledger orders derived analysis before raw artifact paths. Artifacts render as
absolute path lists only; logs and diff are not separate report sections.
Single-Trial evidence tables omit repeated Trial identity columns; multi-Trial
tables may show short Trial identifiers.

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
- [300 Commands](commands.md)
