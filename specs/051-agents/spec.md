---
name: 051. Agents
psychevo_self_edit: deny
---

Define reusable agent definitions and selected-agent runtime semantics for
Psychevo. Agent definitions are reusable identities; main sessions, forked
agents, and parent-owned child agents are all runtime agent invocations over
the same definition model.

## Scope

- reusable agent definition semantics
- agent definition discovery, precedence, validation, and diagnostics
- main-session selected-agent behavior
- runtime tool-policy resolution for selected agents
- model preference, skill, hook, and MCP-scope contributions
- compatibility input formats for local agent definitions
- external peer-agent backend references and generated agent identities

Out of scope:

- parent/child and forked agent run control, defined by [Subagents](subagents.md)
- concrete CLI command spelling, terminal rendering, process behavior, or exit
  codes
- byte-for-byte compatibility with any external product
- persistent memory execution
- git worktree isolation execution
- public agent registries or marketplaces
- remote provider, MCP, or API-key validation without explicit opt-in
- stable storage schemas, wire payloads, or TUI layout details

## Architecture Boundary

An agent definition is an orchestration artifact, not an agent-loop primitive.
`psychevo-agent-core` executes an already assembled agent invocation. Runtime
or a future orchestration layer owns agent definition discovery, policy
assembly, selected-agent specialization, child-agent control, and compatibility
input handling. First-class agent run metadata remains runtime-owned; it must
not move agent definition, lineage, or control state into `psychevo-agent-core`.

In the first implementation slice, `psychevo-runtime` owns that orchestration
and lowers selected agent definitions into the `AgentLoopRequest` and tool
bindings consumed by `psychevo-agent-core`.

## Agent Definitions

An agent definition is a reusable instruction package that may run as the main
session identity or as a child agent. The same definition shape applies to all
roles; the runtime records whether a specific invocation is `main`, `child`,
`fork`, or `system`. "Subagent" is a parent/child relationship term, not a
lesser agent type.

Agent definition files use Markdown with optional YAML frontmatter followed by
the agent instruction body. Runtime accepts compatibility fields including
`name`, `description`, `model`, `tools`, `disallowedTools`, `permission`,
`permissions`, `permissionMode`, `mcpServers`, `skills`, `hooks`,
`background`, `initialPrompt`, `maxTurns`, `maxSpawnDepth`,
`projectInstructions`, `effort`, `backend`, `entrypoints`, and `enabled`.
`backend.ref` references a configured external agent backend when this
definition delegates execution to a peer agent; command-bearing backend details
are never declared directly in Markdown agent files. `entrypoints` declares
where the definition may be used, with supported values `peer` and `subagent`.
Local Markdown definitions default to `subagent`; Markdown definitions with
`backend.ref` default to `peer` and `subagent` unless they explicitly narrow
the list. `enabled` defaults to `true`. When `enabled: false`, the definition
remains visible to management clients but is excluded from active runtime
catalogs, prompt catalogs, completion targets, selected-agent validation, and
model-visible spawn targets.
`maxSpawnDepth` is a
Psychevo extension that defaults to `0`; it controls how many additional
descendant spawn levels a child created from this definition may use, as
defined by [Subagents](subagents.md). `memory` and `isolation: worktree` are
parsed for compatibility but are unsupported in the first implementation slice
and must produce diagnostics rather than executing.

Runtime validates names using lowercase letters, digits, and hyphens. Missing
or empty descriptions are diagnostics and prevent model-index loading. Unknown
frontmatter fields may be preserved as diagnostics but must not prevent loading.

Discovery is deterministic. Precedence is:

1. explicit CLI or session-selected agent path
2. recursively discovered project `.psychevo/agents/**/*.md`
3. recursively discovered nearest-to-root ancestor compatible agent directories
4. recursively discovered global `$PSYCHEVO_HOME/agents/**/*.md`
5. recursively discovered global compatible agent directories
6. built-in agents

The first definition for a name wins. Later duplicates are omitted from the
model-visible active catalog with a diagnostic, but interactive clients may
surface them as shadowed definitions so users can see which source is active.
Disabled definitions do not participate in the active precedence table; a
disabled higher-precedence definition therefore allows a lower-precedence
enabled definition with the same name to become active. Management clients may
surface disabled definitions alongside active and shadowed definitions, with
their source and target path preserved for re-enable, edit, or delete actions.
Supported definition files that fail to parse or validate must surface a
diagnostic to interactive clients instead of being silently discarded. The TUI
may render such files as disabled/error entries in Available, while model
prompt catalogs must exclude invalid definitions.

External `--agents` JSON, settings-provided agents, and plugin-provided agents
are future compatibility targets and are not loaded in the first implementation
slice.

Configured external backends may generate default agent definitions. Generated
definitions follow normal catalog precedence and are shadowed by Markdown
definitions with the same name. Generated definitions are runnable when the
referenced backend is enabled. Their description uses the configured backend
description when present, otherwise the backend label, otherwise the backend id,
so descriptive metadata is useful but not required to launch a configured ACP
backend.

## Management Interfaces

GUI management writes target either the current project or the active profile.
Project agents are stored as `<cwd>/.psychevo/agents/<name>.md`. Profile agents
are stored as `<active-profile-config-dir>/agents/<name>.md`, where the active
profile config directory follows the same `PSYCHEVO_CONFIG`-aware resolution
used by profile-scoped Workbench settings. Management writes must not mutate
Claude-compatible agent directories, built-in agents, generated backend agents,
or plugin-provided agents.

Gateway exposes target-aware agent management RPCs:

- `agent/list` returns active, shadowed, and disabled definitions with source,
  target, mutability, path, enablement, entrypoints, backend reference, and
  diagnostics.
- `agent/read` can address a Project or Profile definition directly so a
  shadowed Profile agent is still editable.
- `agent/write` supports structured form writes and raw Markdown writes.
- `agent/setEnabled` updates only the `enabled` frontmatter field for a
  mutable Project or Profile definition.
- `agent/delete` deletes only the requested Project or Profile definition file.

Structured `agent/write` updates only fields owned by the form (`name`,
`description`, `enabled`, `backend`, `entrypoints`, `tools`, `mcpServers`, and
the Markdown body). It must preserve frontmatter keys that the GUI does not
display, such as `model`, `permissionMode`, `skills`, hooks, and future
compatibility fields. Raw Markdown writes must parse and validate the requested
definition before saving; invalid raw content is rejected and the previous file
is left untouched.

Workbench creates and edits Project/Profile Markdown definitions from the
right-side Capabilities detail panel. Existing definitions keep name and target
read-only during edit; rename remains create-new plus delete-old. The detail
preview renders the raw Markdown file source, including YAML frontmatter, with
the shared Markdown renderer.

## Selected-Agent Behavior

When a caller selects an agent definition for the main session, runtime starts
the invocation with that definition's selected-agent instruction block, model
preference, tool policy, selected skills, hooks, MCP scope, and diagnostics.
The selected-agent instruction block includes the selected identity, the
definition description as model-visible purpose guidance, and the instruction
body when present. This block is ordered after the runtime-mode instruction and
before agent catalogs, skill catalogs, and contextual-user context, as defined
by [006 Prompt Assembly](../006-context-assembly/prompt-assembly.md). It is a
developer-policy specialization layer. Its description and body take precedence
over generic coding-agent behavior unless runtime mode, tool policy, safety
constraints, resource gates, or direct user constraints are stricter. Session
metadata records the selected definition and source.

Child and fork invocations use the same selected-agent identity, description,
and instruction-body construction, with additional child-run control guidance
owned by the subagent runtime. Their persisted child threads record a
child-invocation prefix snapshot and prompt-scoped evidence for export and
last-provider-request reconstruction.
Foreground child invocations are part of the parent turn's active control
boundary: aborting or interrupting the parent turn must abort any foreground
native child loop that the parent is awaiting. Runtime must mark the child run
as interrupted/aborted and close its persisted parent-child agent edge when
the child exits. Completed, failed, or interrupted foreground and background
child invocations must not leave durable `agent_edges` rows in an open state.

When a child invocation targets an agent definition with `backend.ref` and a
`subagent` entrypoint, runtime treats the `spawn_agent` tool call as an
external delegation boundary instead of synthesizing a native child loop.
Runtime owns target validation, effective catalog visibility, unsupported
background/fork checks, and the model-visible `spawn_agent` tool result shape.
The concrete peer transport is injected as an optional runtime-to-Gateway
delegate because Gateway owns ACP client sessions and peer observation. If that
delegate is not available, the `spawn_agent` tool must return a structured
unavailable result rather than falling back to a native child thread that only
has the peer's name.
Ordinary local Markdown/built-in agents continue to use the existing native
child-thread path.

Interactive clients may treat the selected agent as a session-scoped setting:
changing it affects only future invocations in that session, not previous
messages. A missing session setting falls back to the process or CLI selected
agent, while an explicit default setting clears the selected agent for that
session. Runtime projections for each invocation should still record the
resolved selected agent, when any, so replay can identify which main-session
agent produced a turn.
Interactive clients persist concrete main-agent selections and explicit
`Default Agent` selections in the current session metadata only. They must not
write project-local Agent defaults from selector changes. A draft or new session
without session metadata starts from the process or CLI selected agent, then no
Agent.
Refreshing the selected-agent prompt prefix for an existing session must not
require provider credentials or a provider-ready home config; it may read only
the lightweight project context policy needed for prompt assembly.

Selected agents specialize an invocation; they do not bypass capability,
resource, context, session, or runtime-mode constraints. The selected agent may
narrow tools, add context candidates, or prefer a model, but runtime remains
responsible for assembling one valid invocation boundary.

`projectInstructions` is a Psychevo extension for selected main agents. When
omitted, `null`, or `true`, runtime injects AGENTS/project instructions for the
invocation. When `false`, runtime does not inject AGENTS/project instructions
for that selected agent. Non-boolean values are diagnostics and default to
injection. Project instructions are policy context, not task input; when
injected, they use the developer-policy prompt surface with provider-role
fallback to `system` for models that do not support `developer`.

## Tool Policy

Agent invocation policy is an invocation-scoped constraint, not direct execution
authority. Runtime computes effective tools as the intersection of:

- runtime-available tools
- the current run-mode hard ceiling
- the selected agent's allow and deny policy
- scoped MCP availability
- parent invocation safety policy

`tools` is an allowlist for the selected agent. When omitted, `null`, or an
empty string, the selected agent inherits the runtime-available tool surface
subject to the other constraints in this section. A YAML empty array,
`tools: []`, is an explicit empty allowlist and exposes no tools.
`disallowedTools` is a denylist.
When both `tools` and `disallowedTools` are set, runtime removes denied tools
first and then resolves the allowlist against the remaining pool; a tool listed
in both is removed.

`RunMode::Default` may expose mutating coding tools. `RunMode::Plan` is a hard
read-only ceiling; agent definitions cannot expand it into mutating tools.
`permissionMode: plan` narrows the invocation to the same read-only ceiling.
`permissionMode: default` and `permissionMode: acceptEdits` operate only inside
the active runtime ceiling. Dangerous or bypass permission modes are
diagnosed as unsupported and do not grant broader access.

Accepted tool-name spellings normalize to Psychevo tool names:

- `Read` -> `read`
- `ExecCommand` -> `exec_command`
- `WriteStdin` -> `write_stdin`
- `Edit` -> `edit`
- `Write` -> `write`
- `Agent` and `Task` -> agent-spawn/control tools
- `Skill` -> read-only skill access, including `list_skills`, `view_skill`,
  and model-visible skill catalog entries

Removed coding tools such as `list`, `search`, `Grep`, and `Glob` are not
compatibility aliases. If an agent definition names them, they remain unknown
tool names and do not grant access to any built-in tool.

Named restrictions such as `Agent(review,explore)` or `Task(review,explore)`
are runtime-enforced and affect model-visible agent catalog projection. The
`Agent` spawn entrypoint may only target allowed names after the active runtime
mode, selected-agent policy, and parent safety policy are applied. If `Agent` is
not in the effective tool surface, runtime must not inject the agent catalog
prompt slot for that invocation. If `Agent(review,explore)` is effective,
runtime must show only those allowed agent definitions, minus denied names.
Backend-backed agents are advertised in the `Agent` catalog for subagent use
only when their definition has a `backend.ref`, the referenced backend is
enabled, and `entrypoints` includes `subagent`. A model call to such a target
is an explicit delegated task, not a request to change the parent session's
runtime.

The `skills` frontmatter field preloads selected skill content when supported;
it is not a callable capability grant. Runtime exposes the skill catalog prompt
slot only when the effective tool surface includes the read-only `Skill`
surface.

When an invocation's effective tool surface is empty, runtime must use a
minimal no-tools base prompt that states no callable tools are available. It
must not claim read, write, edit, shell, agent, or skill capability for that
invocation.

Invocation and export metadata records the effective tool names, visible agent
catalog state, visible skill catalog state, and project-instruction visibility
and provider role used for that run. `last-provider-request` reconstruction
must use the recorded effective tool names rather than mode defaults; when the
effective list is empty, the reconstructed provider body omits `tools` just as
the actual provider request would.

MCP tools use canonical MCP tool identifiers. MCP scope may narrow available
MCP tools but must not bypass runtime capability selection or resource policy.
For peer-agent backends, the backend's client-capability list is a hard ceiling
and the selected agent's `tools` policy may only narrow that ceiling.

## Hooks

Runtime owns hook execution. Agent definitions may declare hook sources, but
those declarations enter the same runtime hook module as profile, project,
plugin, and managed hooks. Hooks do not enter `psychevo-agent-core`.

Agent-declared hooks use the event catalog, trust model, handler families, and
event-scoped effects defined by [053 Hooks](../053-hooks/spec.md) and
[140 Hook Runtime](../140-hook-runtime/spec.md). Selecting an agent may add
hook declarations to invocation assembly, but it must not bypass hook trust,
permission, resource, context, tool, or registry boundaries.

## Attachments

- [Subagents](subagents.md) defines child and forked agent run semantics.
- [Peer Agents](peer-agents.md) defines configured external backend references,
  generated agent identities, ACP client behavior, and unified surface
  projection.

## Related Topics

- [001 Architecture](../001-architecture/spec.md) defines crate boundaries and
  dependency direction.
- [004 Runtime Contract](../004-runtime-contract/spec.md) defines runtime-owned
  agent-invocation assembly.
- [006 Prompt Assembly](../006-context-assembly/prompt-assembly.md) defines
  selected main-agent prompt slot ordering, cache behavior, and provider-role
  fallback.
- [007 Tool Surface](../007-tool-surface/spec.md) defines tool declaration and
  execution binding semantics.
- [053 Hooks](../053-hooks/spec.md) defines runtime hook authority.
- [140 Hook Runtime](../140-hook-runtime/spec.md) defines hook execution.
- [055 Skills](../055-skills/spec.md) defines skill package semantics that an
  agent definition may reference.
- [100 Coding Agent](../100-coding-agent/spec.md) defines the built-in coding
  capability that may be specialized by a selected agent.
- [200 pevo CLI](../200-pevo-cli/spec.md) owns command spelling.
- [210 pevo TUI Interaction](../210-pevo-tui/interaction.md) owns
  interactive projection.
