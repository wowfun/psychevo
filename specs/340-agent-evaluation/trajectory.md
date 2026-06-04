---
name: 340. Agent Trajectory Semantics
psychevo_self_edit: deny
---

# Trajectory Semantics and Visualization

Define shared agent trajectory timing, tool, observation, and display
semantics for agent evaluation reports.

This attachment is part of [340 Agent Evaluation](spec.md).

## Scope

- normalized transcript step timing
- tool-call and observation grouping
- report-side timing metadata
- trajectory row visualization rules shared by `peval view` and standalone
  trajectory conversion tools

Out of scope:

- agent execution, scoring, benchmark comparison, or report layout outside the
  selected trajectory panel
- non-trajectory artifact rendering
- producer-specific raw event schemas

## Timing Semantics

`timestamp_ms` is the start timestamp for a visible transcript step. The first
step with a timestamp is the trajectory start for per-step elapsed display.

`elapsed_ms` is the offset from trajectory start to the current step start. It
is not a duration and must not include tool execution time unless the step
itself starts after that execution.

`execution_duration_ms` is the execution duration of one tool call. It belongs
to the tool-call metadata, not to the transcript step as a whole. When a
producer exposes runtime tool duration, reports should prefer that value. When
runtime duration is unavailable, reports may use the interval from tool
execution start to matching observation completion.

`duration_ms` is the observed wall-clock span of the visible transcript step.
It starts at the step timestamp and should end at the latest known completion
time for content grouped into that step. For Agent steps with matched
observations, the step span must cover those observations. Multiple tool calls
inside one step are measured as one wall-clock span; their execution durations
are not summed.

If no reliable step end is available, report builders may fall back to the next
step start for Agent steps. A fallback must not display a step duration that is
obviously shorter than grouped tool or observation timing. In that case, the
report should derive the span from grouped observation/tool timing or render
the step duration as unknown.

## Tool And Observation Semantics

Tool calls are part of the Agent step that issued them. Tool observations with
a known matching call id are grouped into that same Agent step by tool-call id,
not by adjacency alone.

Unmatched observations are the exception. They may render as standalone Agent
observation steps only when no matching tool call is available, and converters
must record a warning naming the unmatched call id when one exists.

Tool status is derived from the matching observation when available. A failed
tool call marks both the tool metadata and observation block as `error` and
contributes to tool-error metrics. A later successful tool call in the same
trajectory remains independent and must not inherit the earlier failure state.

Step counts represent visible transcript rows: user rows, explicit system
rows, and Agent rows. Tool-call, observation, and tool-error counts remain
separate metrics.

## Visualization Rules

Collapsed trajectory rows use a compact two-line rail when specific tool chips
are available. The first line contains summary chips in this order when fields
are available:

1. tool ratio
2. token count
3. step duration
4. elapsed offset

The second line contains the specific tool chips, preserving tool call order:

1. tool name and execution duration

The tool ratio is `successful/total tools`. Failed tool calls reduce the
successful count but remain included in the total.

Step rail token chips use compact kilo-token display for large values:
`M.Nk tok` with one decimal place, such as `21.5k tok` for `21,460` tokens.
The exact token count should remain available through hover/title text or
expanded usage evidence. Detailed Run/Result/Usage sections may keep full
localized numeric formatting.

The tool chip uses `tool_name duration` when execution duration is available.
Its tooltip names the measurement as `tool exec <duration>`. Tool execution
timing is displayed separately from step duration. Failed tool-call chips use
the report failure treatment in both collapsed rails and expanded Tool Calls,
so mixed-success multi-tool steps can show the failed tool without coloring
successful tools.

Step duration and elapsed offset render as separate chips labeled
`step <duration>` and `elapsed <duration>`. Unknown durations render `-`. Report
surfaces should use one-decimal second precision for human duration labels.

Expanded rows show ordinary, non-nested blocks for Reasoning, Message or
System Prompt, Tool Calls, and Observations when those fields exist. Tool Calls
show call id, status, generation timing when available, and tool execution
timing near the tool name. Observations show the source call id and status.

Matched observations must appear inside the issuing Agent step's expanded
content. They must not create separate rows. Standalone observation-only rows
are reserved for unmatched observations and should be visually identifiable as
conversion fallbacks.

## Related Topics

- [300 Reporting](../300-peval-cli/reporting.md)
- [305 peval-py](../305-peval-py/spec.md)
