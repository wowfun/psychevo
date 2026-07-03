# Agent Trajectory Analysis Guide

Use this guide when the user asks to evaluate one agent session, compare multiple sessions, write an analysis report, or import an existing analysis report into a Trial cell.

## Analysis Method

Start from the task, then inspect the trajectory evidence:

1. Identify the user's goal, success criteria, constraints, and any expected deliverable.
2. For large JSON/JSONL inputs, use `view tr` inspection and `--steps` / `--tool-call` only when specific steps or a tool call need more evidence before reading raw files or rendering a full report.
3. Read the final response and outcome first, then trace backward through major steps, tool calls, warnings, and retries.
4. Choose the relevant analysis dimensions instead of applying every metric mechanically.
5. Write evidence-backed findings with step ids, tool names, final metrics, trajectory metadata, or report sections.
6. Recommend concrete fixes: prompt, orchestration, tool description, argument construction, stop condition, or evaluation coverage.

Do not hide uncertainty. Call out missing trajectory fields, redacted data, unavailable timing, conversion warnings, or assumptions.

## Evidence Sources

Use whichever source the user provided:

- peval-py report JSON or HTML
- ATIF `trajectory.json`
- JSONL trajectory input
- saved workspace snapshot inspected with default `view tr` or rendered as a full report when needed
- Trial cell artifact path under `runs/<eval>/<agent>/<session>/<cell>`

When the user provides a Trial cell path, glob under it, or descendant inside it, use `view tr -p <cell-dir>` to narrow large trajectory evidence when helpful, then read `agent/trajectory.json`, `agent/trajectory_meta.json`, and existing `analysis.json` / `analysis.md` directly for accuracy-critical findings. If the path names a session directory and it contains exactly one cell, analyze that cell; if it contains multiple cells, ask which cell to target.

## Analysis Dimensions

| Dimension | What To Inspect |
| --- | --- |
| Task success | Whether the requested outcome was completed, partially completed, blocked, or abandoned. |
| Trajectory quality | Plan coherence, step efficiency, unnecessary detours, repeated work, and whether the agent used the evidence it gathered. |
| Tool use | Tool choice, argument correctness, result interpretation, missing calls, extra calls, and tool-call ordering. |
| Failure recovery | Tool errors, invalid outputs, retries, fallback strategy, loop behavior, and whether recovery changed the outcome. |
| Grounding | Whether claims in the final answer are supported by tool outputs or trajectory evidence. |
| Response quality | Completeness, clarity, instruction following, user-facing caveats, and usefulness of next steps. |
| Performance | Wall latency, active duration, number of turns, tool-call count, waiting gaps, and long-running stages. |
| Cost | Token usage, model/provider mix, repeated expensive calls, and reported monetary cost when available. |
| Comparison | For multiple trajectories, compare outcome, path length, failure mode, tool behavior, duration, tokens, and cost. |

Prefer qualitative findings tied to concrete evidence over fixed scores. If a numeric judgment is useful, explain the scale and cite the evidence that moved the rating.

## What To Fix

| Finding | Likely Fix |
| --- | --- |
| Task incomplete | Fix orchestration, add missing tool access, clarify success criteria, or prevent premature final responses. |
| Inefficient trajectory | Tighten planning instructions, remove redundant checks, improve stop conditions, or make constraints explicit. |
| Poor tool use | Improve tool descriptions, parameter documentation, argument construction, and selection policy. |
| Hallucinated or unsupported claims | Require grounding in tool output, add final-response verification, or expose missing evidence clearly. |
| Weak final response | Strengthen response-format instructions, completeness checks, and user-facing caveats. |
| High cost or latency | Batch compatible work, reduce retries, cap search breadth, use cheaper models where appropriate, or stop after sufficient evidence. |
| Regression across sessions | Compare the changed step, tool, prompt, model, or input condition before changing the evaluation bar. |

## Find Cell Identity

Skip this when the user already provided a cell path. If the cell path is missing, use a peval-py report instead of guessing session and agent identities.

When the workspace has saved snapshots, list saved sources with `view tr -r <workspace> -d <workspace>/state.db --list` before deriving cell identity.

To derive full Trial cell paths from a report, run:

```sh
python <skill-dir>/scripts/report_tools.py subjects <workspace>/report.json --workspace <workspace>
```

The helper prints one JSON object per Trial with raw report identities and a normalized `run_path`. If multiple Trials are present and the user did not identify one, ask which Trial to target before importing analysis reports.

## Analysis Report Output

Create an analysis report only when the user asks for one or when a report is needed before `peval-py import analysis`. Respect the requested output path.

- Use JSON for concise machine-readable judgment: summary, status, findings, recommendations, limitations, confidence, and optional metrics.
- Use Markdown for narrative review notes, evidence discussion, root-cause analysis, or material meant for humans.
- Use both only when they are complementary; do not duplicate the same analysis twice.
- Do not write final cell `analysis.json` or `analysis.md` files by hand. Publish through `peval-py import analysis`.

JSON template:

```json
{
  "summary": "One concise paragraph summarizing the outcome, highest-impact findings, and recommended next action.",
  "status": "analyzed",
  "findings": [
    {
      "severity": "high",
      "title": "Short finding title",
      "evidence": ["Step, tool call, metric, warning, or report section"],
      "recommendation": "Concrete follow-up action"
    }
  ],
  "recommendations": ["Concrete next action"],
  "limitations": [],
  "confidence": "medium",
  "metrics": {
    "task_success": "partial",
    "tool_errors": 2
  },
  "extra": {
    "reviewer_note": "Optional metadata for artifact consumers"
  }
}
```

Use valid JSON with no comments or trailing commas. Standard fields such as `summary`, `status`, `findings`, `recommendations`, `limitations`, and `confidence` are imported into the Trial analysis artifact. Other top-level fields are preserved under compiled `extra` and do not override peval-py-owned `subject`.

Markdown outline:

```md
# peval-py Session Analysis

## Context

- Session: `<session-id>`
- Trial key: `<trial-key>`
- Adapter: `<adapter>`
- Agent: `<agent-id>`
- Source: `<path-or-db>`

## Executive Summary

Summarize what happened, whether the session achieved its goal, and the main risk or opportunity.

## Findings

### Finding 1: `<title>`

- Severity: `high|medium|low|info`
- Evidence: `<step/tool/metric reference>`
- Impact: `<why it matters>`
- Recommendation: `<what to do next>`

## Metrics

Record relevant active duration, wall duration, turns, tool calls, tool errors, token usage, and cost when available.

## Limitations

State missing evidence, redactions, conversion warnings, unavailable timing, or assumptions.
```

## Import Analysis Reports

Use import only after the analysis report already exists and a Trial cell is known.

JSON report:

```sh
peval-py import analysis \
  -r <workspace> \
  --run-path <cell-path> \
  -p <analysis-report.json>
```

Markdown report:

```sh
peval-py import analysis \
  -r <workspace> \
  --run-path <cell-path> \
  -p <analysis-report.md>
```

Complementary JSON and Markdown reports:

```sh
peval-py import analysis \
  -r <workspace> \
  --run-path <cell-path> \
  -p <analysis-report.json> \
  -p <analysis-report.md>
```

`--run-path` may be absolute or relative to `<workspace>`, but it must resolve inside `<workspace>/runs/...` and name exactly one Trial cell.
