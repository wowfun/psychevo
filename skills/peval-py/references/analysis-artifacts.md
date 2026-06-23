# Analysis Reports

Create JSON or Markdown analysis reports, or import existing reports into a Trial cell when requested. Analysis writing is independent from peval-py report rendering and workspace placement: a user may ask only for an analysis report, only for a peval-py report, or for both.

## Choose Format

For analysis, choose one report by default. Use both JSON and Markdown only when their contents are complementary:

- Use a JSON analysis report for fixed-format judgment: summary, status, findings, recommendations, limitations, and confidence.
- Use a Markdown analysis report for free-form narrative, review notes, reasoning, evidence discussion, or material meant to be read by humans or later agents.
- If both are used, avoid duplicating the same analysis twice. Keep JSON concise and structured; put narrative, caveats, and long evidence discussion in Markdown.

## Output Location

Respect an explicit user-provided output path. If the user provides a Trial cell path, analyze that cell's data and keep the same path available for a later import. Do not write final cell `analysis.json` or `analysis.md` files by hand.

For import, pass the cell path to `--run-path`:

```sh
peval-py import analysis \
  -r <workspace> \
  --run-path <cell-path> \
  -p <analysis-report.json-or-md>
```

Repeat `-p` once when importing complementary JSON and Markdown reports.

## Find Cell Identity

Skip this when the user already provided a cell path. If the cell path is missing, use a generated peval-py report instead of guessing session and agent identities:

```sh
python <skill-dir>/scripts/report_tools.py subjects <workspace>/report.json --workspace <workspace>
```

The helper prints one JSON object per Trial with raw report identities and a normalized `run_path`. If multiple Trials are present and the user did not identify one, ask which Trial to target before import.

## Import Analysis Reports

Purpose: import existing JSON or Markdown analysis reports into peval-py-owned `analysis.json` and/or `analysis.md` under the selected Trial cell.

JSON report only:

```sh
peval-py import analysis \
  -r <workspace> \
  --run-path <cell-path> \
  -p <analysis-report.json>
```

Markdown report only:

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

`--run-path` may be absolute or relative to `<workspace>`, but it must resolve inside `<workspace>/runs/...` and name exactly one Trial cell. `-p` format is inferred from suffix: `.json`, `.md`, or `.markdown`.

## JSON Analysis Report

Purpose: fixed-format, machine-readable analysis. `peval-py import analysis` compiles this input into cell-local `analysis.json`, adds `subject` from `--run-path`, defaults missing `status` to `analyzed`, and does not synthesize top-level `metrics` or `commands`.

Standard fields are compiled to top-level `analysis.json` fields. Additional fields, including imported `subject`, `metrics`, and `commands`, are preserved under compiled `extra` and do not override peval-py-owned compiled `subject`. If the input includes `extra`, it must be a JSON object; top-level non-standard fields win when they use the same key as `extra`.

Generated reports always add peval-py automatic metrics under
`annotations.analysis[].analysis_metrics.auto`. These automatic metrics are
derived analysis values and do not repeat direct facts already stored in
`trajectory.final_metrics` or `trajectory_meta[]`. Imported JSON `metrics`
remain flat keys in the same `analysis_metrics` object when the report is
rendered. Do not use `auto` as a custom metric key; it is reserved for
peval-py-owned computed metrics.

Template:

```json
{
  "summary": "One concise paragraph summarizing the session outcome, highest-impact findings, and recommended next action.",
  "status": "analyzed",
  "findings": [
    {
      "severity": "high",
      "title": "Short finding title",
      "evidence": ["Report section, step id, tool name, or metric reference"],
      "recommendation": "Concrete follow-up action"
    }
  ],
  "recommendations": ["Concrete next action"],
  "limitations": [],
  "confidence": "medium",
  "extra": {
    "reviewer_note": "Optional non-standard metadata for artifact consumers"
  }
}
```

Use valid JSON. Do not include comments or trailing commas. Standard fields:

- `summary`
- `status`
- `findings`
- `recommendations`
- `limitations`
- `confidence`

Any other top-level input field is moved to compiled `extra`. Report generation ignores compiled `extra`; it remains available to tools that read the artifact directly.

Current peval-py report generation recognizes these compiled `analysis.json` fields:

- `summary` -> `annotations.analysis[].summary`
- `status` -> `annotations.analysis[].analysis_status` (`annotations.analysis[].status` remains the cache source status)
- `subject`, `findings`, `recommendations`, `limitations`, `commands`, and `confidence` with the same field names
- `metrics` -> flat keys in `annotations.analysis[].analysis_metrics`; the
  reserved `auto` key is ignored so imported metrics cannot replace automatic
  metrics

Unknown top-level fields and recognized fields with the wrong type are ignored by reports, but remain in the compiled artifact for other tools.

When `--json` is used, `peval-py import analysis` includes diagnostic `warnings`
for top-level fields that look meaningful but are preserved under `extra`
instead of compiled, such as `subject`, `metrics`, `commands`,
`analysis_status`, `analysis_metrics`, and `auto`. It also warns when standard
fields such as `summary` or `findings` appear inside `extra`; place standard
fields at the top level when they should be compiled.

## Markdown Analysis Report

Purpose: free-form analysis readable by humans and agents. Its format and content are intentionally unconstrained by this skill. When imported into a Trial cell, peval-py renders it as `md_report`.

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
