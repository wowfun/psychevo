# Trial Cell Artifacts

Create `notes.md`, `analysis.json`, `analysis.md`, or a useful subset when the user asks for Trial notes or analysis artifacts. These artifacts are independent from report rendering: a user may ask only for artifacts, only for a report, or for both.

## Contents

- [Output Location](#output-location)
- [Find Report Identities](#find-report-identities)
- [`notes.md`](#notesmd)
- [`analysis.json`](#analysisjson)
- [`analysis.md`](#analysismd)
- [Verify](#verify)

For analysis, choose one artifact by default. Write both analysis files only when their contents are complementary:

- Use `notes.md` for manual Trial notes, observations, or lightweight commentary that should render as `annotations.notes[]`.
- Use `analysis.json` for fixed-format machine-readable status, subject identity, findings, metrics, evidence references, and recommendations.
- Use `analysis.md` for free-form narrative, review notes, reasoning, evidence discussion, or material meant to be read by humans or later agents.
- If both are written, avoid duplicating the same analysis twice. Keep JSON concise and structured; put narrative, caveats, and long evidence discussion in Markdown.

## Output Location

Respect an explicit user-provided output path. When the artifacts should be recognized by peval-py reports or `serve`, prefer this workspace cell path:

```text
<workspace>/runs/<analysis_eval_slug>/<agent-id>/<session-id>/<cell_key>/notes.md
<workspace>/runs/<analysis_eval_slug>/<agent-id>/<session-id>/<cell_key>/analysis.json
<workspace>/runs/<analysis_eval_slug>/<agent-id>/<session-id>/<cell_key>/analysis.md
```

`Cached analysis` is the peval/peval-py report concept for analysis loaded from a workspace. The skill does not require every analysis artifact to be cached or placed under `runs/...`; this path is simply the first-choice location when peval-py recognition is desired.

Rules:

- `<workspace>` is the directory containing `peval-py.toml`.
- `<analysis_eval_slug>` is top-level `analysis_eval_slug` from `peval-py.toml`, or `default`.
- `<agent-id>` is `--agent-name` from the report command when supplied, otherwise the effective adapter id.
- `<session-id>` is the displayed `trajectory.session_id` from the peval-py report when present. If it is missing, use the rendered Trial key normalized as a path segment.
- `<cell_key>` is the displayed Trial key from `trajectory_meta[].trial_key`, normalized as a path segment by replacing unsafe characters with `_`.
- Session-root `analysis.json`, `analysis.md`, and `notes.md` are session-level artifacts and are not read into Trial reports in this version.

All path segments must be plain names. Do not use empty segments, `.`, `..`, `/`, or `\`.

## Find Report Identities

Use a generated report instead of guessing session and agent identities:

```sh
python <skill-dir>/scripts/report_tools.py subjects <workspace>/report.json --workspace <workspace>
```

The helper prints one JSON object per Trial with raw report identities, normalized `agent_segment`, `session_segment`, and `cell_segment`, and full `notes_path`, `analysis_json_path`, and `analysis_md_path` values when `--workspace` is provided. If multiple Trials are present and the user did not identify one, ask which Trial to target before writing recognized cell artifacts.

## `notes.md`

Purpose: cell-local manual Trial note rendered by peval-py as `annotations.notes[]` when placed in a recognized workspace path. Use it for concise observations or commentary that should stay separate from analysis.

Template:

```md
# Trial Notes

- Session: `<session-id>`
- Trial key: `<trial-key>`
- Adapter: `<adapter>`
- Agent: `<agent-id>`

<notes>
```

Keep `notes.md` human-readable Markdown. It is not the place for fixed schema fields; use `analysis.json` for structured analysis.

## `analysis.json`

Purpose: fixed-format, machine-readable analysis artifact. Current peval-py report generation renders `summary` plus a typed whitelist of incremental fields. Downstream agents and tools can consume the full JSON object. Keep fields stable and concise.

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
  "recommendations": ["Concrete next action"],
  "metrics": {
    "turns": null,
    "tool_calls": null,
    "tool_errors": null,
    "duration_ms": null,
    "wall_duration_ms": null
  },
  "commands": [],
  "limitations": [],
  "confidence": "medium"
}
```

Use valid JSON. Do not include comments or trailing commas. peval-py report generation recognizes these typed fields:

- `summary` -> `annotations.analysis[].summary`
- `status` -> `annotations.analysis[].analysis_status` (`annotations.analysis[].status` remains the cache source status)
- `subject`, `findings`, `recommendations`, `limitations`, `commands`, and `confidence` with the same field names
- `metrics` -> `annotations.analysis[].analysis_metrics`

Unknown top-level fields and recognized fields with the wrong type are ignored by reports, but remain in the artifact for other tools.

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

Only verify peval-py recognition when the artifacts are intended to be read by reports or `serve`. After writing artifact files to the preferred workspace path, re-run `view tr -r <workspace>` or run `view tr` from the workspace root/descendant, then verify the relevant fields for the targeted Trial:

```sh
# If notes.md was written:
python <skill-dir>/scripts/report_tools.py check <workspace>/report.json --trial-key <trial-key> --require-notes

# If analysis.json was written:
python <skill-dir>/scripts/report_tools.py check <workspace>/report.json --trial-key <trial-key> --require-summary

# If findings were written:
python <skill-dir>/scripts/report_tools.py check <workspace>/report.json --trial-key <trial-key> --require-findings

# If analysis.md was written:
python <skill-dir>/scripts/report_tools.py check <workspace>/report.json --trial-key <trial-key> --require-md-report
```

- For `notes.md`, a matching `annotations.notes[]` item has `source = "cell"`, `label = "notes.md"`, Markdown content, and a note `source_ref.relative_path`.
- For `analysis.json`, the matching `annotations.analysis[]` item has a `summary` equal to the JSON summary and any valid typed whitelist fields.
- For `analysis.md`, the matching `annotations.analysis[]` item has `md_report` containing the Markdown body.
- The matching `annotations.analysis[]` item has `relative_paths` pointing to the analysis files.
