---
name: 300. peval CLI View Attachment
psychevo_self_edit: deny
---

Define view generation and comparison behavior for `peval`.

This attachment is part of [300 peval CLI](spec.md).

## Scope

- Trial fact input sources
- HTML, Markdown, and JSON output roles
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
trajectory, ATIF, log, analysis, diff, and grouped views. The default scope is
the selected benchmark. `--path` may narrow the input to a benchmark, agent,
task, or exact cell directory, and filters are applied after path scoping.

Report profiles are optional named view definitions declared as
`[reports.<key>]` in eval, workspace, or user config. The first supported
profile field is `[reports.<key>.analysis]`, which overrides `[analysis]`
defaults for explicit analysis runs. Profile lookup uses user, workspace, then
eval merge order, so eval-local settings have the highest precedence.

Markdown is the default terminal and log format. HTML is a local human review
format. JSON is the machine DTO format and exposes structured references so
automation can perform explicit follow-up reads.
`-i all` is a parser-level alias for the complete static diagnostic report:
`summary,matrix,usage,warnings,artifacts,trajectory,atif,logs,analysis,diff`.
`timeline` is removed from the public include surface in schema v7; requesting
it is a clear error rather than an alias. View DTOs serialize the expanded list
and use view schema v7 for the Trial/MatrixCell diagnostic sections.

Schema v7 top-level JSON contains `summary`, `matrix`, `trials`, and section
arrays keyed by `trial_key`: `trajectory`, `atif`, `artifacts`, `logs`,
`analysis`, and `diff`. `matrix` contains task and agent axes plus cells. Each
MatrixCell has `matrix_cell_key`, `trial_keys`, and representative metric
values from the latest Trial. Each Trial has deterministic `trial_key =
"<matrix_cell_key>:t001"` for the current single-attempt layout. Public DTOs
must not serialize the legacy `cell_key` field.

Physical references are represented as `ViewDataRef` values with `kind`,
`label`, `relative_path`, `mime`, `size_bytes`, and available hash or mtime
metadata. Static JSON, Markdown, and HTML omit absolute paths; the local service
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

Static JSON and Markdown views show bounded summaries and references by
default. HTML also shows bounded summaries by default, and may inline only
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
fields are present; ACP adapters accumulate thought chunks, agent message
chunks, tool calls, tool updates, and observations. Unmapped lifecycle events
remain available through peval timeline metadata. When the original prompt is
not retained, derived ATIF remains structurally valid and marks
`extra.prompt_unavailable = true`. View schema v7 treats trajectory as the
primary visual section. It contains ATIF-derived step summaries, duration bars,
token/cost bars, tool-call and tool-error summaries, unmapped event counts, and
a self-rendered SVG graph of ATIF step flow. Reports should render complete
trajectory visuals up to roughly 50 Trials or 1000 steps; above that threshold
they degrade to summaries, references, and collapsed expansion.

ATIF keeps the `ATIF-v1.7` schema string while including Psychevo additive
producer fields for turns, tool calls/errors, usage details, cost, and
accounting. Step metrics, final metrics, structured cell metrics, and the view
usage/accounting sections must agree for normalized values. Explicit `-i atif`
or `-i all` includes full ATIF in JSON. Markdown and HTML render full ATIF in
collapsed details so the report remains readable.

## Redaction

Views redact credential values, HTTP headers, non-allowlisted environment
variables, host-specific secret paths, and provider keys by default. Raw export
must be a separate explicit diagnostic operation; it is not implied by
Markdown, HTML, or default JSON views. A user clicking `Run analysis` in the
local service is explicit authorization for the configured analysis agent to
read the relevant Trial artifact directory and optional task directory
read-only; the service, not the agent, owns analysis cache writes.

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
terms and schema v7 data. It defaults to the whole workspace, lazy-loads
benchmark and Trial details, writes only analysis cache files, and protects
file routes with localhost binding, a generated token, containment checks,
MIME checks, and a 1 MiB raw/detail limit. The built-in UI uses offline local
HTML, CSS, and native JavaScript. Its visual direction is a light "scientific
evaluation workbench": mid-high density, desktop-first, mobile-readable,
report-quality overview, linked matrix detail drawers, and trajectory graph to
step-accordion highlighting.

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
