---
name: 210. pevo TUI Testing
psychevo_self_edit: deny
---

Define deterministic acceptance coverage for the first `pevo tui` slice.

Automation vocabulary and generic validation boundaries follow
[060 Automation](../060-automation/spec.md).

## Deterministic Tests

Required coverage:

- TUI state read/write, version tolerance, per-workdir model and variant
  precedence, per-workdir mode persistence, global thinking persistence, and
  recent-model bounding
- slash command parsing, model/variant/mode validation, and ambiguous session
  prefix handling
- composer behavior for submit, newline, history recall, history search, and
  fixed keymap help text
- evidence-ledger projection for turn rails, prompt blocks, thinking blocks,
  `Explored`/`Ran`/`Changed` tool groups, failures inside their original group,
  unlabeled answer body text, turn meta, and debug meta
- plain non-terminal renderer output for `Prompt`, `Thinking`, `Explored`,
  `Ran`, `Changed`, `Answer`, and `Meta` blocks, including `--debug`
- narrow and wide layout rendering, sidebar visible/hidden, thinking
  visible/hidden, expanded/collapsed tool output, footer, bottom bar, and
  composer surface
- streaming runtime projection that never leaks folded reasoning into sanitized
  message events while still delivering dedicated TUI thinking events
- runtime metrics projection that can expose usage and allowlisted metadata to
  TUI without putting them in sanitized transcript messages
- context-percent display only when a model context limit is known
- `/thinking` toggle behavior: default visible, explicit on/off, global
  persistence, visible reasoning rendered only in TUI output and never in
  sanitized transcript views
- `/mode` behavior: default `build`, persisted `plan`/`build` per workdir, `Tab`
  cycling in the fullscreen event loop, and next-turn application while a turn
  is running
- runtime Plan mode toolset: exposes `read`, `list`, and `search`; does not
  expose `bash`, `write`, or `edit`
- mode instruction is sent to providers for the current turn and is not
  persisted in `messages`
- default TUI session selection of latest `run` or `tui` session by canonical
  workdir
- `--new` session creation behavior
- explicit `--session` behavior
- non-terminal scripted input with prompt lines and slash commands
- `/model set` persistence to `$PSYCHEVO_HOME/tui-state.json`
- `/models` from configured provider/model entries without live provider
  catalog calls
- `/session show` sanitized transcript output
- `Esc` interrupts a running turn through runtime control in fullscreen mode
- slash menu prefix filtering, disabled `/undo`, `/compact`, and `/export`
  entries, and bounded `upcoming` feedback
- transcript focus and expansion behavior: `Ctrl+T`, selected block movement,
  `Enter`/`Space` expand-collapse, `Esc` returning to composer, and mouse click
  expansion for expandable evidence blocks
- sidebar sections for Session, Context, Modified Files, and Footer; Modified
  Files must cap visible entries and tail-compact long paths

## Visual Regression

The primary TUI visual regression path is a Codex-style `ratatui`
`TestBackend` or `Buffer` snapshot. These snapshots should render stable text
plus stable style markers so tests can assert layout, emphasis, and color-role
discipline without storing raw ANSI escape sequences as the default golden
format.

Snapshot changes must use an explicit review flow. The developer or agent
should inspect the snapshot diff before accepting intentional changes, following
an `insta`-style pending/show/accept workflow when the implementation uses
`insta`.

Required visual fixtures cover at least 80-column and 120-column widths with a
realistic coding-agent turn. The fixture set should include idle composer,
running thinking, tool evidence, collapsed and expanded output, slash menu,
debug meta, sidebar visible/hidden, and narrow compact layout.

An optional ANSI test mode may generate diagnostic artifacts for local
inspection. ANSI artifacts are not the default checked-in golden source and
should not be required by the broad validation gate.

Optional tmux or PTY capture may be used for E2E diagnostic artifacts and
scripted interaction evidence. It is not part of the default broad validation
gate unless a later testing spec explicitly promotes it.

## Validation

Relevant narrow validation:

- `cargo test -p psychevo-ai`
- `cargo test -p psychevo-agent-core`
- `cargo test -p psychevo-runtime`
- `cargo test -p psychevo-cli`

Broad validation remains:

- `scripts/validate.sh broad`
