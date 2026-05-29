---
name: 090. Evaluation Artifacts Attachment
psychevo_self_edit: deny
---

Define foundation rules for evaluation artifacts, trajectories, reports, and
privacy.

This attachment is part of [090 Evaluation](spec.md).

## Scope

- artifact classes and source-of-truth boundaries
- trajectory retention
- local raw data and shareable report privacy
- workspace retention principles

Out of scope:

- concrete report rendering
- CLI output paths and flags
- domain-specific patch, screenshot, or binary artifacts

## Artifact Classes

Evaluation artifacts are grouped into:

- cell-level run fact documents
- trajectory event streams
- evaluator or harness logs
- environment diagnostics
- derived views

Structured cell run facts are the source of truth for automated comparison,
reporting, and reuse. A cell is one semantic benchmark/agent/task/factor
combination. Views are derived from cell facts plus the view renderer version.

Artifact v8 records are cell-oriented. Each cell fact separates benchmark
identity, semantic fingerprint, stable case identity, candidate identity,
task-set identity, expanded factors, terminal status, score, metrics, and
artifact links. Views are logical projections over those facts and must not
require reshaping the physical artifact layout.

Metrics are part of structured cell facts. Duration, turns, tool calls,
tool errors, token/cache usage, provider accounting, and optional cost are read
from metrics fields. When an adapter cannot provide a value, the metric is
unknown rather than fabricated. Aggregated view metrics are derived from cell
metrics and should preserve unknown values where aggregation would be
misleading.

Cell facts may include non-fatal `warnings`. Warnings record adapter or
contract diagnostics such as lossy usage extraction, missing optional updates,
or degraded capability inputs. They do not change score or terminal status by
themselves.

## Trajectories

Trajectory artifacts record normalized events in execution order. Lower specs
may export additional trajectory formats for training or analysis, but such
exports are derived from the canonical event stream.

Trajectory events should identify source, timestamp or sequence order, case id,
candidate identity, task identity, and event kind. When an adapter cannot
provide full detail, it should emit a lossy event or collector diagnostic
instead of fabricating precise behavior.

When an evaluated agent exposes its own structured observation stream, the
evaluation trajectory should retain those source events as local trajectory
events instead of collapsing them into a single process-finished marker. Raw
source events may contain prompts, model output, tool arguments, and tool
results, so they are local diagnostic artifacts and remain behind the report
privacy boundary.

## Privacy

Local cell artifacts may retain raw prompts, model outputs, tool outputs, and
environment logs for debugging. Shareable reports must redact secrets,
credential values, HTTP headers, provider keys, and non-allowlisted environment
variables by default.

Raw public export must require an explicit opt-in in the concrete CLI or report
layer. Redaction must prefer omission or stable placeholders over partial
secret exposure.

## Retention

Lower specs define concrete artifact locations and retention defaults. The
foundation rule is that artifact retention must be explicit enough for a user
to clean local run state and to understand whether failed workspaces or raw
logs remain on disk.

Domain-specific code patches, screenshots, or final workspaces are not
foundation artifacts. A domain spec must opt into storing them.

## Persistent Store

Evaluation implementations may provide a workspace for cell run artifacts,
dataset inventories, and workspace scripts. Structured cell facts remain the
source of truth. This slice intentionally has no workspace cache contract:
readers scan cell facts directly. User-visible index, latest, dashboard, and
per-invocation run summary files from older implementations are legacy derived
artifacts, not authoritative data.

Eval configs map current results under
`runs/<benchmark-id>/<agent-id>/<task-id>/<short-fingerprint>/`. The cell identity does
not include the eval config id or path, so separate eval configs can reuse the
same semantic benchmark/agent/task/factor result. Store-relative paths must not
escape the store root. Explicit external output roots are isolated escape
hatches and do not participate in workspace reuse.

`peval view` is the built-in reporting, comparison, and inspection surface. It
may render Markdown, JSON, or HTML over selected cell facts. Static Markdown and
HTML views must not inline raw trajectory, prompt, model output, tool output,
evaluator log, or environment log bodies by default. JSON views may expose
artifact paths and derived summaries so automation can inspect local files
explicitly. Local server/detail endpoints are explicit diagnostic reads and may
show full local bodies after containment, redaction, and size checks.

Service and view APIs follow the same privacy boundary. Default views expose
bounded summaries, not raw trajectory, prompt, model output, tool output,
evaluator stdout/stderr, or full logs. Raw diagnostic reads require an explicit
raw-access method or local viewer endpoint. Derived ATIF trajectories,
timelines, analysis files, and discovered diffs are views over existing cell
artifacts; adding them must not change artifact reuse semantics unless a later
artifact schema explicitly opts in.

## Related Topics

- [090 Schema](schema.md)
- [300 Reporting](../300-peval-cli/reporting.md)
- [350 Scoring](../350-coding-evaluation/scoring.md)
