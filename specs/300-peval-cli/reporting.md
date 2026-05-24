---
name: 300. peval CLI Reporting Attachment
psychevo_self_edit: deny
---

Define report generation and comparison behavior for `peval`.

This attachment is part of [300 peval CLI](spec.md).

## Scope

- report input sources
- HTML, Markdown, and JSON output roles
- redaction defaults
- compare and replay report behavior

## Report Inputs

Reports are derived from run summaries, case result documents, trajectory
indexes, scorer details, and diagnostic metadata. They must not depend on
active agent sessions or mutable benchmark workspaces.

HTML is the primary human analysis format for local review. Markdown is the
compact text format for logs, pull requests, and terminal summaries. JSON is
the machine summary format. The first implementation does not generate CSV.

## Visible Content

A report should show the matrix shape, run status, pass/fail counts, setup and
runtime failures, scores, duration, model/candidate identifiers, cost or usage
when available, and links to local detailed artifacts.

Coding reports do not display code diffs by default. If a lower domain stores
workspace or patch material, report rendering must still treat it as sensitive
raw diagnostic material unless a later report spec explicitly changes that
policy.

## Redaction

Reports redact credential values, HTTP headers, non-allowlisted environment
variables, host-specific secret paths, and provider keys by default. A raw
export mode must be explicit and should be unavailable from `report` unless the
user names both the source artifact root and the raw output intent.

## Comparison

Comparison reports align cases by canonical suite, task, factor, and candidate
identity. They should surface regressions, improvements, newly skipped cases,
and newly failing setup/runtime/scoring phases separately.

Replay output is generated from stored trajectory events and case artifacts.
It may summarize command, agent, scorer, and diagnostic events, but it does not
re-run any part of the case.

For the Psychevo adapter, replay input includes the `pevo run --format json`
observation stream captured during the run. Report and dashboard surfaces may
link to that local trajectory, but they must not inline the raw observation
stream by default.

## Static Dashboard

The persistent store may include a static HTML dashboard and per-run HTML
reports. The dashboard is a local review surface: it shows recent runs,
dataset inventory, latest shortcuts, pass/fail counts, and local artifact
links. It may use inline CSS and small inline JavaScript for filtering,
sorting, expansion, comparison selection, and dataset/run linking.

Dashboard pages do not inline raw trajectories, prompts, model outputs, tool
outputs, or full logs by default. They may expose abnormal cases through
filters and short structured summaries while linking to canonical artifacts
for detailed inspection.

## Related Topics

- [090 Artifacts](../090-evaluation/artifacts.md)
- [300 Commands](commands.md)
