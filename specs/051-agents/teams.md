---
name: 051. Agent Teams
psychevo_self_edit: deny
---

Define durable agent team templates and runtime team context for Psychevo.

## Scope

- Markdown team template discovery, precedence, validation, and diagnostics
- Gateway management RPC semantics for Project/Profile team definitions
- runtime team-run metadata and status projection
- `spawn_agent.team_member` behavior inside an active team context
- Workbench, TUI, and CLI visibility for team runs

Out of scope:

- isolated git worktree execution for teammates
- editable task-board semantics
- multiple active teams in one session
- remote agent-team registries
- byte-for-byte compatibility with external products

## Team Templates

Team templates are Markdown files with YAML frontmatter and a body. The
frontmatter declares a reusable team; the body is lead coordination policy that
is injected only when a mission or team run activates that template.

Supported frontmatter fields are:

- `name`: required machine name using lowercase letters, digits, and hyphens.
- `description`: required user-facing purpose.
- `enabled`: optional boolean, default `true`.
- `leader`: required agent definition name.
- `members`: required non-empty list of member entries. A member entry has
  `id`, `agent`, optional `role`, optional `description`, optional `maxTurns`,
  optional `runtimeRef`, optional `runtimeOptions`, and an optional
  `runtimeProfileRevision` captured by management or Team activation. Gateway
  JSON uses an unsigned decimal string for this u64-derived value; Team
  Markdown uses the equivalent YAML integer.
- `maxParallelAgents`: optional integer. Runtime clamps the effective value to
  the implementation cap; the default is `4`.

Unknown frontmatter fields are preserved as diagnostics and do not participate
in runtime behavior. Unsupported fields such as `isolation: worktree` remain
diagnostic-only until an explicit worktree slice exists.

Discovery mirrors agent definitions:

1. recursively discovered project `.psychevo/teams/**/*.md`
2. recursively discovered profile `<active-profile-config-dir>/teams/**/*.md`

The first enabled definition for a name wins. Later enabled duplicates are
shadowed with diagnostics. Disabled definitions remain visible to management
clients but do not participate in active runtime catalogs. A disabled
higher-precedence definition allows a lower-precedence enabled definition with
the same name to become active.

Template validation must resolve `leader` and every member `agent` against the
active agent catalog. Backend-backed agents are valid team participants only
when their generated or Markdown definition is enabled and supports the
required entrypoint. Unavailable external backends surface diagnostics; runtime
must not silently replace them with local agents.

## Management Interfaces

Gateway exposes target-aware team management RPCs:

- `team/list` returns active, shadowed, and disabled definitions with source,
  target, mutability, path, enablement, members, leader, cap, and diagnostics.
- `team/read` can address a Project or Profile definition directly.
- `team/write` supports structured form writes and raw Markdown writes.
- `team/setEnabled` updates only the `enabled` frontmatter field for a mutable
  Project or Profile definition.
- `team/delete` deletes only the requested Project or Profile definition file.

Project teams are stored as `<cwd>/.psychevo/teams/<name>.md`. Profile teams
are stored as `<active-profile-config-dir>/teams/<name>.md`. Management writes
must not mutate built-in, generated, or compatibility-source team definitions.

Workbench manages teams inside `Capabilities > Agents` as a compact
subsection. It must favor scan-first rows and a single detail/editor panel over
an always-open multi-pane team canvas. The product surface should expose only
fields that clarify intent or enable action: name, description, enabled state,
leader, members, cap, body policy, source, target, diagnostics, and raw
preview.

## Runtime Team Context

A session may have at most one active team run in v1. Activating a team creates
a durable `agent_team_runs` record containing:

- team run id
- parent session id
- team name, description, and source path
- leader agent name
- member definitions
- effective concurrency cap
- started/ended timestamps
- status and final summary

Child runs created while the context is active add team fields to the
`agent_edges.metadata` object:

- `teamRunId`
- optional `missionRunId`
- `teamMemberId`
- `agentPath`

`spawn_agent` accepts optional `team_member` only when a team context is
active. The value must match a member id from the active team. Outside an
active team context the argument is invalid because unknown fields remain
strict. Inside the context, runtime validates that `agent_type` resolves to the
member's configured agent definition. It must not infer a member from
`task_name`.

Runtime enforces the effective per-team/session concurrent child-turn cap. The
default cap is `4`, chosen to preserve the existing low-noise child-agent
posture. The cap applies to active team child turns and
does not require worktree isolation.

## Status And Control

`agent/status` includes team and mission labels on child rows when available.
`team/status` returns the active or requested team run with grouped member rows,
child status, final summaries, depth cap, concurrency cap, and pause state.

`agent/control` owns interactive controls shared by team and non-team child
runs:

- `stop`: stop a child or subtree.
- `resume`: continue a closed or completed child with a new message when
  supported.
- `send`: send a message to a child.
- `pauseSpawning` and `resumeSpawning`: toggle new-spawn admission for the
  active session.

Workbench's right workspace may render a `team` tab for active team/mission
state. The tab must be compact by default: overview, grouped member rows,
open-child-session actions, stop/resume/send controls, and mission summary.
Detailed transcript inspection stays in child session tabs rather than
duplicating every child message in the parent transcript.

TUI enriches the running agents panel with team, mission, member, and
concurrency labels. Keyboard controls remain simple: Enter opens, `S` stops,
and `P` pauses or resumes new spawning.

## Runtime-Backed Members

Each Team member may select a runtimeRef and Advanced runtimeOptions separately
from its Agent Definition. Profile defaults are inherited. Structured Gateway
and Workbench writes preserve the execution fields in Team Markdown. Team write
and activation fail closed for an unknown or disabled Profile, an incompatible
Agent Definition pairing, an unsupported override, or an explicitly stale
runtimeProfileRevision. Activation captures a missing revision into the durable
Team run. Immediately before a member starts, Gateway resolves the current
Profile again, requires the captured Profile revision, and lets the adapter
revalidate model, mode, catalog-backed per-turn, and safety values against its
current capability contract. ACP model/effort/mode and versioned capability-pack
options require exact selectable choices from the cached Thread Context for the
effective model.
Reasoning summary, arbitrary feature keys, and output schemas are not Team
options because the stable catalog does not enumerate them. Unsupported values
are never ignored.

The stable bridge is leader-first: a Native Psychevo leader may dispatch
Runtime Profile-backed managed members. An ACP leader receives Psychevo Team
tools only when the effective capability contract explicitly grants them. The
Team surface groups controllable Psychevo-managed members separately from
capability-gated Agent-native activity.
Only managed members use agent/control.
