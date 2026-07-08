---
name: 051. Missions
psychevo_self_edit: deny
---

Define mission orchestration for Psychevo.

## Scope

- `/mission [--team <name>] <goal>` entrypoint semantics
- mission-run metadata and status projection
- prompt-led lead orchestration using existing child-agent primitives
- Workbench and TUI mission visibility

Out of scope:

- first-class editable task graphs
- automatic PRD confirmation loops
- multi-team missions in a single session
- real-provider validation requirements

## Mission Entry Point

`/mission [--team <name>] <goal>` is the main user entrypoint for coordinated
multi-agent work. The command creates a mission run, optionally activates a
team template, and submits a lead prompt that tells the lead to decompose,
spawn, wait, verify, and summarize.

Mission v1 is agent-decided. Runtime records durable mission and team metadata
but does not own an editable task board. Derived task views may be projected
from child runs, summaries, and mailbox events, but those views are read-only
diagnostics.

When `--team` is omitted, runtime uses the default lead agent and generic
subagent catalog. When `--team <name>` is present, runtime validates the team,
uses the template's leader as the lead identity, injects the template body as
coordination policy, and constrains `spawn_agent.team_member` values to the
template members.

The generated lead prompt must be explicit about the product boundary:

- the lead coordinates and summarizes user-facing progress;
- child agents do implementation, research, or verification work;
- the lead should avoid flooding the parent transcript with raw child output;
- the lead should respect the team concurrency cap and spawn-depth cap;
- verification should use deterministic local harnesses unless the user opts
  into live providers.

## Mission Metadata

Starting a mission creates an `agent_mission_runs` record containing:

- mission run id
- parent session id
- optional team run id
- optional team name
- goal
- lead agent name
- started/ended timestamps
- status
- final summary

Child `agent_edges.metadata` includes `missionRunId` whenever a child is spawned
under an active mission. This lets status, export, and replay surfaces group
children by mission without moving execution semantics into the mission layer.

## Status And UX

`team/status` includes the active mission summary when the team run is mission
backed. `agent/status` includes mission labels on rows so existing running
agent panels can remain useful without a separate task board.

Workbench's right `team` tab renders the mission goal, status, cap labels,
member rows, child summaries, and final summary. It should not create an
always-visible horizontal set of teammate chat panes; opening a member uses the
existing child-session navigation.

TUI adds `/mission` parsing/submission and shows mission labels in the running
agents panel. The command does not require a live provider at parse time.

## Persistence And Recovery

Mission and team run records are durable runtime metadata. If the process exits
while a mission is running, existing child sessions and open edges remain
recoverable. A resumed parent session can inspect status and continue with
normal agent-control operations, but v1 does not automatically restart a lead
loop without user action.
