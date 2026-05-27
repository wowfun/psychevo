---
name: 300. peval CLI View Attachment
psychevo_self_edit: deny
---

Define view generation and comparison behavior for `peval`.

This attachment is part of [300 peval CLI](spec.md).

## Scope

- cell fact input sources
- HTML, Markdown, and JSON output roles
- redaction defaults
- view-based comparison and inspection behavior

## View Inputs

Views are derived from artifact v6 cell facts under
`runs/<benchmark-id>/<agent-id>/<task-id>/<cell-key>/run.json`. They must not
depend on active agent sessions, mutable benchmark workspaces, generated
dashboards, visible indexes, hidden caches, or v2 per-invocation run summaries.

`peval view` is the single built-in reporting, comparison, and inspection
surface. It projects selected cell facts into summary, matrix, usage, and
grouped views. The default scope is the selected benchmark. `--path`
may narrow the scope to a benchmark, agent, task, or exact cell directory, and
filters are applied after path scoping.

Markdown is the default terminal and log format. HTML is a local human review
format. JSON is the machine DTO format and may expose raw artifact paths so
automation can perform explicit follow-up reads.

## Visible Content

A view should show the selected matrix shape, pass/fail counts, setup and
runtime failures, scores, duration, model/candidate identifiers, cost or usage
when available, and status aggregation. View renderers read metrics from the
structured metrics fields. They do not recalculate token usage, cost, duration,
turn counts, or tool counts from human logs.

Markdown and HTML views do not inline or list raw trajectory, prompt, model
output, tool output, evaluator stdout/stderr, or full process log bodies. Coding
views do not display code diffs by default. If a lower domain stores workspace
or patch material, view rendering must still treat it as sensitive raw
diagnostic material unless a later spec explicitly changes that policy.

## Redaction

Views redact credential values, HTTP headers, non-allowlisted environment
variables, host-specific secret paths, and provider keys by default. Raw export
must be a separate explicit diagnostic operation; it is not implied by
Markdown, HTML, or default JSON views.

## Comparison

Comparison is a view query over cell facts. Grouping and matrix projection align
cells by canonical benchmark, task-set, task, factor, and candidate identity. They
should surface missing cells, regressions, improvements, setup/runtime/scoring
failures, and usage deltas without rerunning agents or evaluators.

## Related Topics

- [090 Artifacts](../090-evaluation/artifacts.md)
- [300 Commands](commands.md)
