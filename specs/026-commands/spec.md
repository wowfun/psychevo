---
name: 026. Commands
psychevo_self_edit: deny
---

Define Psychevo's shared command contract across product command surfaces.

This topic builds on [025 CLI](../025-cli/spec.md) for process invocation and
[070 Experience](../070-experience/spec.md) for cross-cutting UX defaults. It
defines command discovery, naming, argument, alias, availability, and output
contract expectations that concrete product surfaces specialize.

## Scope

- shared command vocabulary and metadata expectations
- runtime-owned interface-neutral command catalog, parser, availability, and
  UI-independent execution effects
- command discovery and help behavior
- canonical names, hidden aliases, and command status semantics
- argument-shape and bounded-error conventions
- surface capability filtering across TUI, Web/Desktop, ACP, and future
  messaging adapters
- command output-kind and execution-effect conventions across projections

Out of scope:

- complete product command inventories
- process flags, clap schemas, keymaps, terminal layout, editor protocol
  payload shapes, or messaging-platform registration APIs
- custom command-template files or external plugin command loading
- runtime, provider, storage, session, skill, or tool semantics

## Command Contract

The shared command catalog lives in `psychevo-runtime` when multiple
entrypoints need to project the same command metadata. Runtime owns slash
command recognition, canonical identity, alias resolution, argument parsing,
capability requirements, active-turn availability, and UI-independent command
effects. Product surfaces own presentation, protocol payloads, terminal panels,
client-native attachment flows, process flags, and surface-specific state
application.

A command has one canonical name. Surfaces may accept hidden aliases for
compatibility or migration, but discovery surfaces show canonical names by
default. Built-in compatibility aliases must not appear as independent menu
rows. Help may mention aliases compactly on the canonical command's row.

Interactive surfaces may also allow user-configured aliases for existing
commands. User aliases are resolved as aliases of the command's canonical
metadata record, not as new command records. A configured alias must not
conflict with any canonical command name, built-in alias, dynamic command
prefix, or another configured alias; concrete products should reject such
configuration during startup or configuration loading. Interactive discovery
surfaces may render configured aliases as alias rows when that improves
completion affordance, but they must still execute through the canonical
command metadata and parser.

A command metadata record should identify:

- canonical name
- hidden aliases
- usage string
- short summary
- owning surface
- help group
- argument kind
- output kind
- status
- required surface capabilities
- whether the command is safe while an agent turn is active
- optional unsupported guidance for surfaces that lack a required capability
- optional expanded help detail for surfaces that have room to explain
  consequences, persistence, or provider/network behavior

Argument kinds are:

- none
- required value
- optional value
- fixed enum value
- free-form trailing text
- dynamic command suffix plus optional trailing text

Output kinds are:

- transcript/status block
- bottom selection pane
- bottom help pane
- immediate state change
- prompt submission
- process stdout/stderr result
- bounded feedback
- structured display artifact

Surface capabilities are descriptive gates, not permissions. They include
picker, clipboard, renderer toggle, process exit, side conversation, image
attachment, active-turn control, queue, session switch, local artifact write,
config write, policy write, skill-state write, and structured diff display. A
surface advertises a command only when it can satisfy the command's capability
requirements and the command is currently available. If a user types a known
command that is hidden only because the current surface lacks a capability, the
command returns bounded guidance instead of silently doing nothing.

Permission and approval policy remains separate from command capability
filtering. Capability filtering decides whether a command can be represented on
the current surface. Permission policy decides whether a selected command may
perform the requested local write, provider call, tool use, or state mutation.
Command-level approval is used for local artifact, config, policy, or
skill-state writes that do not naturally pass through an existing runtime tool
approval path.

Shared slash parsing returns a command invocation with canonical metadata,
resolved alias, raw argument text, parsed command arguments when available, and
the original submitted line. Unknown slash-looking input is represented as a
pass-through prompt so prompt-bearing user surfaces can send it to the model
with the original submitted text.

Gateway exposes the same command catalog to reconnectable clients through
typed `command/list` and `command/execute` methods. `command/list` returns
`CommandListResult { commands: CommandListItem[] }`, where each item carries
the command name, slash label, usage, summary, aliases, argument kind, source,
and optional presentation metadata for GUI-like surfaces. TUI, Web, Desktop,
ACP, and messaging surfaces must project the shared catalog rather than
inventing separate slash semantics.
Web and Desktop shells present the shared catalog as a command utility panel.
Executing `/help` or `/commands` opens that panel, `/agents` opens the agents
panel, `/status` opens status, and `/sessions` or `/history` opens history.
These panel switches are host display effects, not ordinary transcript facts.
The Web/Desktop catalog is capability-filtered by the runtime registry and may
include dynamic backend-provided commands when runtime exposes them. Client-side
presentation may hide only commands that the Gateway/runtime marks unavailable
for that host capability set; it must not drop unknown extension commands from
completion or execution merely because the frontend does not know their names.

GUI presentation metadata is derived from runtime command metadata rather than
from frontend command-name allowlists. `presentationKind` uses:

- `navigate` for commands that switch an existing surface, such as commands,
  sessions, agents, or status.
- `inspect` for observational structured views such as diff, context, usage,
  and status details.
- `control` for active-turn or local state controls.
- `submit` for commands that submit or transform prompt text.
- `export` for host download/share/artifact actions.
- `extension` for dynamic skill, bundle, or backend-provided slash commands.

`destination` names the preferred GUI landing area: `commands`, `history`,
`agents`, `status`, `preview`, `composer`, `download`, or `none`.
`feedbackAnchor` says where display-only feedback belongs: `trigger`,
`commandsPanel`, `composer`, or `status`. `alternateAction`, when present,
identifies a GUI-native replacement such as opening the model picker, attachment
control, or history panel. These fields are presentation hints, not permission
grants, and must not let clients execute commands hidden by the runtime surface
profile.

Shared execution returns an effect rather than directly manipulating a UI. The
effect vocabulary includes local text, pass-through prompt, prompt submission,
steer, queue, pending cancel, session switch, state patch, artifact result,
structured diff result, unsupported guidance, and approval required. Surfaces
apply these effects to their own transcript, panes, protocol updates, queues,
or approval UI.
Web/Desktop `command/execute` maps these shared effects to typed host actions
where a first-slice host action exists. Effects without a Web/Desktop action
return bounded unsupported guidance instead of falling through to arbitrary
frontend behavior.
Unknown slash-looking input on prompt-bearing surfaces remains a pass-through
prompt and reports `known=false` on typed Gateway execution results. A known
command hidden only because the current GUI surface cannot represent it reports
`known=true`, `accepted=false`, bounded guidance, and optional alternate action;
it must not be silently sent to the model.

Peer-agent ACP commands are dynamic catalog entries sourced from ACP
`available_commands_update`. They are exposed as namespaced commands of the
form `/agent:command`. Core Psychevo commands keep their canonical names and
are never shadowed by peer commands. When executing a peer command, Gateway
removes the namespace and sends the original peer slash command to the selected
peer thread.

`/diff` is an observational shared command. It requires a surface capable of
showing a structured diff result, is available during active turns, and must
not write runtime messages, affect model context, alter exports, or change
usage/accounting. Its concrete semantics are defined by
[214 pevo Diff Command](../214-pevo-diff-command/spec.md).
Web and Desktop surfaces satisfy this capability by opening a read-only inline
diff preview in the center workbench split. Right-panel changed-file rows may
reuse the same structured diff action without executing a slash command.

Interactive terminal surfaces may project local slash command feedback as
surface-local UI state. Such feedback is display-only: it must not become user
prompts, durable session messages, provider context, visible message counts, or
ordinary main transcript history. Commands whose output kind is a bottom pane
use that pane instead of adding transcript rows. Any future persistent command
result history requires an explicit domain sidecar spec rather than a generic
transcript sidecar.
This boundary follows the transcript state and projection ownership defined by
[030 Transcript State](../030-state-and-data-model/transcript-state.md) and
[213 pevo Display Model](../213-pevo-display-model/spec.md).

Statuses are:

- active
- upcoming

Removed or obsolete commands are not part of the command catalog. If entered,
they follow the same bounded unknown-command behavior as unsupported commands
unless a concrete product spec intentionally keeps a compatibility alias.

## Discovery

Interactive command discovery should be available from the command prefix used
by that surface. For slash commands, `/` opens a completion menu over canonical
command labels. The menu may include upcoming commands when the owning product
surface wants visible roadmap affordances, but upcoming commands must provide
bounded feedback instead of executing unfinished behavior.

Help output should be generated from command metadata rather than hand-written
duplicates. Help rows use:

```text
<usage> - <summary>
```

Expanded help surfaces may add one short continuation line after a row when the
command has important consequences, persistence behavior, provider/network
behavior, or sensitive-data handling to disclose. Compact discovery surfaces
such as slash menus should continue to use only canonical names and short
summaries.

When aliases are useful to disclose, the same row may append:

```text
aliases: <alias>, <alias>
```

If a concrete interactive surface supports command keybindings, expanded help
may also append compact shortcut text on the canonical command row. Shortcut
metadata is display-only and must not create extra command rows.

TUI slash help uses three user-facing groups:

- `General` for ordinary keyboard shortcuts and high-frequency built-in
  commands.
- `Commands` for the complete built-in slash command catalog.
- `Custom commands` for dynamic or user-provided command entries.

ACP and messaging slash command discovery should project the capability-filtered
command catalog, not the whole TUI inventory. Commands that require local TUI
state, a terminal-only panel, renderer toggles, process exit, clipboard access,
or client-native image attachment are not advertised to ACP unless the surface
declares that capability. Dynamic skill and bundle commands may be appended
after core commands with a surface-defined cap; hidden dynamic commands remain
valid when typed if they resolve at execution time.

Web/Desktop discovery groups visible commands by `presentationKind` rather than
by a frontend-owned command-name list. Dynamic extension commands remain visible
when returned by the runtime catalog even if the frontend has no built-in
knowledge of their names. Completion rows may show the command summary and a
compact destination label, but they must use the Gateway/runtime metadata rather
than hard-coded surface curation.

## Errors

Command errors are bounded user-visible text. They must not panic, hang, or
start provider network work unless the command explicitly submits a prompt.

No-argument commands reject arguments with:

```text
/<command> does not accept arguments
```

Commands with required arguments reject missing or malformed input with:

```text
usage: <usage>
```

Unsupported known commands reject with bounded guidance. Prompt-bearing user
surfaces pass unknown slash-looking input through as ordinary model input. This
fallback must apply only to unknown commands, not to known commands whose
arguments are malformed or whose required capability is missing.

Concrete surfaces may wrap these messages in their normal error presentation.

## Related Topics

- [025 CLI](../025-cli/spec.md) defines process-oriented CLI foundation
  semantics.
- [070 Experience](../070-experience/spec.md) defines cross-cutting UX and DX
  defaults.
- [200 pevo CLI](../200-pevo-cli/spec.md) defines the concrete `pevo` product
  command line.
- [212 pevo TUI Interaction](../212-pevo-tui-interaction/spec.md) defines the
  fullscreen interactive slash command surface.
- [027 ACP](../027-acp/spec.md) defines ACP slash-command projection.
- [214 pevo Diff Command](../214-pevo-diff-command/spec.md) defines the
  shared `/diff` command.
