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
  precedence, per-workdir mode persistence, global thinking persistence, global
  sidebar visibility persistence, and recent-model bounding
- slash command parsing, model/variant/mode validation, `/rename`,
  `/sessions`/`/resume`/`/continue`, and ambiguous session prefix handling
- composer behavior for submit, newline, current-session persisted user-prompt
  history seeding, history recall with draft restoration, history search, and
  fixed keymap help text
- slash menu default selection: the first visible completion row is selected
  and pressing `Enter` executes it
- evidence-ledger projection for unlabeled prompt blocks without left rails,
  inline `Thinking: <reasoning>` blocks, `Explored`/`Ran`/`Changed` tool groups,
  failures inside their original group, unlabeled answer body text without left
  rails, tool blocks without left rails, `Ran <actual first command line>`
  titles that survive start-to-end tool updates even when end events omit tool
  arguments, metadata left rails, unlabeled turn meta only after visible answers
  or failure summaries, and debug meta; only the
  `Thinking:` prefix uses the paper color role, reasoning content uses the
  normal thinking body role, and explicit reasoning paragraphs do not receive
  label-width indentation
- plain non-terminal renderer output for `Prompt`, `Thinking`, `Explored`,
  `Ran`, `Changed`, `Answer`, and `Meta` blocks, including `--debug`
- narrow and wide layout rendering, sidebar hidden by default for fresh state,
  persisted optional sidebar visible state, thinking visible/hidden,
  expanded/collapsed tool output, minimal bottom state line, and composer
  surface without a left accent rail; user prompt blocks and the composer must
  share the same `RGB(38,38,38)` full-width surface with a leading `›` prompt
  marker, the empty composer must occupy two input rows with surface background
  on both rows, wrapped historical prompt rows including CJK/wide-character
  content must keep full-width prompt background on each physical row, and
  sidebar headings must be bold without colored left rails
- streaming runtime projection that never leaks folded reasoning into sanitized
  message events while still delivering dedicated TUI thinking events
- runtime metrics projection that can expose usage and allowlisted metadata to
  TUI without putting them in sanitized transcript messages
- context-percent display in the sidebar only when a model context limit is
  known
- `/show-thinking` toggle behavior: default visible, explicit on/off, global
  persistence, visible reasoning rendered only in TUI output and never in
  sanitized transcript views, fullscreen visibility changes immediately refresh
  existing Thinking blocks, and hidden thinking does not render a
  `Thinking: hidden` marker or append a status row; removed `/thinking`
  returns a bounded error that points to `/show-thinking`
- `/mode` behavior: default `default`, persisted `plan`/`default` per workdir,
  `Shift+Tab` cycling in the fullscreen event loop, no transcript status row
  for mode cycling, and next-turn application while a turn is running
- slash command completion from `Tab`
- runtime Plan mode toolset: exposes `read`, `list`, and `search`; does not
  expose `bash`, `write`, or `edit`
- mode instruction is sent to providers for the current turn and is not
  persisted in `messages`
- default TUI session selection of latest `run` or `tui` session by canonical
  workdir
- session title persistence, `/rename <title>`, title display in session
  picker/sidebar, automatic title generation with model success, and
  deterministic first-prompt fallback when title generation fails or returns an
  unusable title
- fullscreen startup history loading for the selected/latest session and
  session-picker transcript replacement without synthetic status rows, while
  restoring persisted folded reasoning as local `Thinking: <reasoning>` evidence
  and persisted elapsed time in turn metadata when available
- `--new` session creation behavior
- fullscreen `/new` clearing behavior: no transcript status row and a forced
  terminal clear before repaint so stale glyphs cannot survive the empty state
- explicit `--session` behavior
- non-terminal scripted input with prompt lines and slash commands
- fullscreen `/model` bottom-pane selection sourced from local configured
  models only, including search, current/default markers, model-to-variant
  transition, `Config default` clearing the variant override, explicit variant
  persistence, and `Esc` close/back behavior
- `/variant set` persistence and fullscreen bottom state rendering of the
  current effective variant instead of falling back to `default`
- scripted `/model` output from configured provider/model entries without live
  provider catalog calls
- `/sessions` scripted fallback list output
- fullscreen session selection through the shared bottom pane: search, grouped
  row rendering, visible-message counts matching the sidebar, selection, and
  transcript/history replacement; rows with CJK/wide-character titles must keep
  the updated time right-aligned on the same physical row
- `/models`, `/model set`, `/session list`, `/session show`, and
  `/session switch` rejected as removed commands and absent from the slash menu
- `/undo` and `/redo` parsing, menu rows, fullscreen behavior, scripted output,
  Git snapshot restore, repeated undo/redo boundary movement, composer prompt
  restoration after undo, cleanup before the next prompt, and bounded no-op or
  error paths for no session, no user message, missing snapshot, non-Git
  workdir, and unsettled running turns
- `Esc` interrupts a running turn through runtime control in fullscreen mode
- slash menu prefix filtering, disabled `/compact` and `/export` entries, and
  bounded `upcoming` feedback
- transcript focus and expansion behavior: `Ctrl+T`, selected block movement,
  `Enter`/`Space` expand-collapse, `Esc` returning to composer, and keyboard
  transcript scrolling
- slash menu row selection with Up/Down/Home/End and mouse click, including
  `/mo` navigation to `/mode` before `Enter`
- transcript auto-follow behavior: new prompts reset to bottom-following,
  streaming assistant deltas and long generated answers remain visible while at
  bottom, manual scrolling opts out, and returning to the bottom resumes
  auto-follow
- long-output resilience for model-generated long answers and read-tool results:
  wrapped content must not overwrite composer/sidebar, collapsed read output
  must retain expandable full content, and expanded long reads must scroll
  coherently
- fullscreen TUI captures mouse events in alternate screen mode, disables mouse
  capture on exit, routes mouse wheel to transcript or active bottom pane
  scrolling, supports left-click selection for slash and bottom-pane rows, and
  supports app-native mouse drag text selection with `Ctrl+C`/mouse-up copying
  through test-injected clipboard sinks; selection extraction must use final
  rendered-buffer transcript/sidebar rows, preserve wrapped and wide-character
  visible text, lock copying/highlighting to the rendered region where dragging
  started so same-row transcript/sidebar text is not mixed, trim only right-side
  terminal padding, visibly highlight active selections, and clear highlight on
  `Esc`, mouse-up success, and mouse-up failure; slash menu and bottom-pane
  clicks must keep priority over text selection; WSL detection must include
  kernel version/release markers when WSL
  environment variables are absent, WSL clipboard command candidates must
  prefer `powershell.exe` then `clip.exe`, Linux Wayland sessions must try
  `wl-copy`, and all clipboard backend failures must report bounded errors
  without quitting fullscreen TUI
- sidebar starts with the bold current session title, omits source, mode, model,
  variant, and thinking visibility, and carries Context and Modified Files
  sections;
  Context carries token usage and context percentage when known; Modified Files
  must cap visible entries and tail-compact long paths

## Visual Regression

The primary TUI visual regression path is a `ratatui`
`TestBackend` or `Buffer` snapshot. These checked-in goldens render stable text
plus stable style-role markers so tests can assert layout, emphasis, and
color-role discipline without storing raw ANSI escape sequences as the default
golden format.

Snapshot changes must use an explicit snapshot review flow. The developer
or agent should inspect pending diffs before accepting intentional changes.
These stable buffer/style snapshots are part of default broad validation.

Required visual fixtures cover at least 80-column and 120-column widths with a
realistic coding-agent turn. The fixture set should include idle composer,
running thinking, tool evidence, collapsed and expanded output, slash menu,
bottom selection panes for models, variants, and sessions, debug meta, sidebar
visible/hidden, failure/tool-error meta, and narrow compact layout. The default
composer fixture should verify the stripped bottom chrome: no composer mode
title, no shortcut footer, no `mode=`/`model=`/`variant=` prefixes, stable
model/variant positions, and non-default mode last.

When practical, snapshot tests should write untracked Agent-readable diagnostic
material under `target/pevo-tui-snapshots/<fixture>/` on failure or review:
plain rendered text, style-role projection, combined projection, and fixture
metadata. These diagnostics are not the checked-in source of truth.

VHS capture is required validation for changes that affect fullscreen TUI
visual display. This includes layout, color, visible transcript text,
composer, sidebar, slash menu, and screenshot-visible interaction states. The
diagnostic script uses a deterministic local mock provider, an isolated
repo-local `PSYCHEVO_HOME`, and the current workspace `pevo` binary. It writes
PNG screenshots and companion material under
`.local/.psychevo-dev/tui-shots/<timestamp>/`.

The demo workdir must be isolated from the parent repository's git state so
Modified Files does not reflect unrelated uncommitted work. The tape should pin
terminal color environment, clear inherited `NO_COLOR`, and avoid theme choices
that squash TUI color-role contrast across repeated runs.

VHS capture remains outside default broad validation and is not a pixel golden.
Screenshots stay untracked. A person or visually capable agent must inspect the
generated PNGs and report the screenshot directory and visual judgment in the
handoff for a fullscreen TUI visual change.

The VHS path is `scripts/pevo-tui-capture.sh demo`. Its required tools are
`vhs`, `ttyd`, `ffmpeg`, and `python3`. If dependencies are missing, the
implementer may skip the VHS run only with an explicit note that lists the
missing dependency blocker and the install command printed by the script.
Dependency installation must be opt-in because it mutates the host system.

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
