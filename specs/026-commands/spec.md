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
- command discovery and help behavior
- canonical names, hidden aliases, and command status semantics
- argument-shape and bounded-error conventions
- command output-kind conventions across CLI and TUI projections

Out of scope:

- complete product command inventories
- concrete command handlers, flags, clap schemas, keymaps, or terminal layout
- custom command-template files or external plugin command loading
- runtime, provider, storage, session, skill, or tool semantics

## Command Contract

A command has one canonical name. Surfaces may accept hidden aliases for
compatibility or migration, but discovery surfaces show canonical names by
default. Aliases must not appear as independent menu rows. Help may mention
aliases compactly on the canonical command's row.

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

Interactive terminal surfaces may project local slash command feedback that is
written to the transcript as a distinct command-result transcript row. Such
rows are display-only: they do not become user prompts, durable session
messages, provider context, or visible message counts. Commands whose output
kind is a bottom pane use that pane instead of adding command-result transcript
rows.

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

When aliases are useful to disclose, the same row may append:

```text
aliases: <alias>, <alias>
```

TUI slash help uses three user-facing groups:

- `General` for ordinary keyboard shortcuts and high-frequency built-in
  commands.
- `Commands` for the complete built-in slash command catalog.
- `Custom commands` for dynamic or user-provided command entries.

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

Unsupported commands reject with:

```text
unknown slash command: /<command>
```

Concrete surfaces may wrap these messages in their normal error presentation.

## Related Topics

- [025 CLI](../025-cli/spec.md) defines process-oriented CLI foundation
  semantics.
- [070 Experience](../070-experience/spec.md) defines cross-cutting UX and DX
  defaults.
- [200 pevo CLI](../200-pevo-cli/spec.md) defines the concrete `pevo` product
  command line.
- [210 pevo TUI](../210-pevo-tui/spec.md) defines the fullscreen interactive
  slash command surface.
