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
`TestBackend` or `Buffer` snapshot. These checked-in goldens render stable text
plus stable style-role markers so tests can assert layout, emphasis, and
color-role discipline without storing raw ANSI escape sequences as the default
golden format.

Snapshot changes must use an explicit `insta`-style review flow. The developer
or agent should inspect pending diffs before accepting intentional changes.
These stable buffer/style snapshots are part of default broad validation.

Required visual fixtures cover at least 80-column and 120-column widths with a
realistic coding-agent turn. The fixture set should include idle composer,
running thinking, tool evidence, collapsed and expanded output, slash menu,
debug meta, sidebar visible/hidden, failure/tool-error meta, and narrow compact
layout.

When practical, snapshot tests should write untracked Agent-readable diagnostic
material under `target/pevo-tui-snapshots/<fixture>/` on failure or review:
plain rendered text, style-role projection, combined projection, and fixture
metadata. These diagnostics are not the checked-in source of truth.

Optional VHS capture is the first real-terminal PNG diagnostic path. The
diagnostic script uses a deterministic local mock provider, an isolated
repo-local `PSYCHEVO_HOME`, and the current workspace `pevo` binary. It writes
PNG screenshots and companion material under
`.local/.psychevo-dev/tui-shots/<timestamp>/`.

The demo workdir must be isolated from the parent repository's git state so
Modified Files does not reflect unrelated uncommitted work. The tape should pin
terminal color environment, clear inherited `NO_COLOR`, and avoid theme choices
that squash TUI color-role contrast across repeated runs.

VHS capture is intentionally outside default broad validation and is not a
pixel golden. Its required tools are `vhs`, `ttyd`, `ffmpeg`, and `python3`.
The diagnostic script should fail clearly when dependencies are missing and
provide an explicit dependency-install command. Dependency installation must be
opt-in because it mutates the host system.

The VHS diagnostic script must clean up its local mock provider on success,
failure, and interrupt. A successful artifact write must exit successfully
instead of failing during cleanup, and repeated runs must not leave background
mock-provider processes behind.

## Validation

Relevant narrow validation:

- `cargo test -p psychevo-ai`
- `cargo test -p psychevo-agent-core`
- `cargo test -p psychevo-runtime`
- `cargo test -p psychevo-cli`

Broad validation remains:

- `scripts/validate.sh broad`
