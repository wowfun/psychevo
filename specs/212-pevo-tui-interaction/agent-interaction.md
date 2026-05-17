---
name: 212. pevo TUI Interaction Agent Controls
psychevo_self_edit: deny
---

# 212. pevo TUI Interaction Agent Controls

Define `/agents`, `@agent`, `/fork`, selected-main-agent, child-session navigation, and Agent row controls.

## Agent Interaction

`/agents`, `@agent-name`, and `/fork` are the interactive projections for agent
definition discovery and first-class child-agent runs. Bare `@word` completion
prefers agent names; path-shaped tokens continue to use file completion.
`/fork` creates a background forked child agent.

`/agents` opens a two-tab console. `Running` lists live child agents for the
current session tree, shows the current depth/concurrency cap state, and offers
`Open`, `Pause/Resume spawning`, and `Stop subtree`. `Available` lists callable
agent definitions from supported discovery sources, marks active and shadowed
duplicates, surfaces supported definition parse failures as disabled
diagnostics, and exposes a session-scoped `Use as main` action for active
definitions, a `Default main agent` row for clearing the current session's main
agent, local `.psychevo` create/update/delete, plus read-only view/run actions
for other sources. Completed, errored, interrupted, and closed child agents are
not listed in `/agents`; they remain reachable from `Agent` rows in the parent
transcript. `Stop subtree` first requests cooperative shutdown for the selected
child and descendants, waits a short grace window, then force-interrupts and
closes any still-running child edge.

`Use as main` changes the selected main-session agent for future turns in the
current session only. It does not rewrite history, does not start a child run,
and is unavailable for shadowed or diagnostic definitions. The selected main
agent is restored when reopening the session; if no session setting is present,
the TUI falls back to the startup `--agent` value, then to the default
unselected identity. Successful `Use as main` and `Default main agent` actions
close the `/agents` panel. The bottom status line does not show main-agent
text; instead, the transcript/composer separator embeds the effective session
identity when it is non-default.

The session identity separator applies to every TUI session view. Root sessions
show a label only when a non-default main agent is active. Child and forked
agent sessions use their persisted `agent.name` as the default identity, so
opening a `translate` child shows `translate` in the separator. If the user
selects another main agent inside a child session, the separator shows that
effective main agent; selecting `Default main agent` restores the child
session's own agent identity. The label is just the agent name, without `main`
or `Agent` prefixes.

Opening an agent enters the original child session and preserves its identity
and policy. The active composer follows the displayed session. Returning to the
parent/root session uses explicit TUI navigation; child completions still notify
the original parent while the edge remains open. Pressing `Esc` in an active
running child session, or in its parent/main session while that child is still
running, requests interrupt for the still-running child work, even when the
parent turn has been detached from the main running slot for inspection.
Parent navigation is available through `Alt+Left` and the mnemonic `Alt+P`.

Transcript row clicks follow the shared evidence rule: clicking an Agent row
body toggles details, and only clicking the visible right-side `Open` title
action enters the child session. If the title action overlaps the row title
line's general row hit area, `Open` wins only inside that visible action region;
clicks elsewhere on the row still toggle details. In transcript focus, `Space`
toggles details while `Enter` or `O` opens an Agent row.

Running an available definition from `/agents` prompts for a task, starts a
background fresh-context child agent, writes a concise clickable parent status
row, and leaves the user in the parent session. Local `.psychevo` definition
create/update forms include `name`, `description`, instruction body, `model`,
`tools`, `permission mode`, `background`, and `max_spawn_depth` with a default
of `0`. Compatible imported and built-in definitions are read-only in this
slice. Additional legacy directory schemas are not scanned in this slice.
