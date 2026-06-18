# Analysis Artifacts

Create `analysis.json`, `analysis.md`, or both when the user asks for analysis artifacts. These artifacts are independent from report rendering: a user may ask only for artifacts, only for a report, or for both.

Choose one artifact by default. Write both only when their contents are complementary:

- Use `analysis.json` for fixed-format machine-readable status, subject identity, findings, metrics, evidence references, and recommendations.
- Use `analysis.md` for free-form narrative, review notes, reasoning, evidence discussion, or material meant to be read by humans or later agents.
- If both are written, avoid duplicating the same analysis twice. Keep JSON concise and structured; put narrative, caveats, and long evidence discussion in Markdown.

## Output Location

Respect an explicit user-provided output path. When the artifacts should be recognized by peval-py reports or `serve`, prefer this workspace path:

```text
<workspace>/runs/<analysis_eval_slug>/<agent-id>/<session-id>/<cell_key>/analysis.json
<workspace>/runs/<analysis_eval_slug>/<agent-id>/<session-id>/<cell_key>/analysis.md
```

`Cached analysis` is the peval/peval-py report concept for analysis loaded from a workspace. The skill does not require every analysis artifact to be cached or placed under `runs/...`; this path is simply the first-choice location when peval-py recognition is desired.

Rules:

- `<workspace>` is the directory containing `peval-py.toml`.
- `<analysis_eval_slug>` is top-level `analysis_eval_slug` from `peval-py.toml`, or `default`.
- `<agent-id>` is `--agent-name` from the report command when supplied, otherwise the effective adapter id.
- `<session-id>` is the displayed `trajectory.session_id` from the peval-py report.
- `<cell_key>` is `peval-py-analysis` when no matching analysis cell exists.
- If exactly one existing cell directory under the session directory contains `analysis.json` or `analysis.md`, update that cell.
- If multiple cell directories match, stop and ask the user which cell to use.

All path segments must be plain names. Do not use empty segments, `.`, `..`, `/`, or `\`.

## Find Report Identities

Use a generated report instead of guessing session and agent identities:

```sh
python <skill-dir>/scripts/report_tools.py subjects <workspace>/report.json
```

## `analysis.json`

Purpose: fixed-format, machine-readable analysis artifact. Current peval-py report generation renders only the top-level `summary` and path metadata, but downstream agents and tools can consume the full JSON object. Keep fields stable and concise.

Template:

```json
{
  "summary": "One concise paragraph summarizing the session outcome, highest-impact findings, and recommended next action.",
  "status": "analyzed",
  "subject": {
    "session_id": "<session-id>",
    "trial_key": "<trial-key>",
    "adapter": "<adapter>",
    "agent_id": "<agent-id>"
  },
  "findings": [
    {
      "severity": "high",
      "title": "Short finding title",
      "evidence": ["Report section, step id, tool name, or metric reference"],
      "recommendation": "Concrete follow-up action"
    }
  ],
  "metrics": {
    "turns": null,
    "tool_calls": null,
    "tool_errors": null,
    "duration_ms": null,
    "wall_duration_ms": null
  },
  "commands": [],
  "limitations": []
}
```

Use valid JSON. Do not include comments or trailing commas.

## `analysis.md`

Purpose: free-form analysis artifact rendered by peval-py as `md_report` when placed in a recognized workspace path. It is readable by humans and agents. Its format and content are intentionally unconstrained by this skill.

Optional outline when a narrative report is useful:

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

## Timeline

Describe the key turns, tool calls, long-running stages, retries, failures, and idle gaps. Reference step ids or report sections where useful.

## Findings

### Finding 1: `<title>`

- Severity: `high|medium|low|info`
- Evidence: `<step/tool/metric reference>`
- Impact: `<why it matters>`
- Recommendation: `<what to do next>`

## Metrics

Record relevant active duration, wall duration, turns, tool calls, tool errors, token usage, and cost when available.

## Commands Used

List the peval-py and inspection commands used to produce this analysis.

## Limitations

State missing evidence, redactions, conversion warnings, unavailable timing, or assumptions.
```

## Verify

Only verify peval-py recognition when the artifacts are intended to be read by reports or `serve`. After writing artifact files to the preferred workspace path, re-run `view tr -r <workspace>` or run `view tr` from the workspace root/descendant, then verify the relevant fields:

```sh
# If analysis.json was written:
python <skill-dir>/scripts/report_tools.py check <workspace>/report.json --require-summary

# If analysis.md was written:
python <skill-dir>/scripts/report_tools.py check <workspace>/report.json --require-md-report
```

- For `analysis.json`, `annotations.analysis[0].summary` equals the JSON summary.
- For `analysis.md`, `annotations.analysis[0].md_report` contains the Markdown body.
- `annotations.analysis[0].relative_paths` points to the analysis files.
