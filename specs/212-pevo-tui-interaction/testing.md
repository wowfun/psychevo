---
name: 212. pevo TUI Interaction Testing
psychevo_self_edit: deny
---

# 212. pevo TUI Interaction Testing

Define deterministic acceptance coverage for fullscreen TUI input handling,
commands, popups, panels, mouse selection, clipboard behavior, and agent
controls. Functional interaction requirements live in
[212 pevo TUI Interaction](spec.md) and its linked attachments.

Automation vocabulary and generic validation boundaries follow
[060 Automation](../060-automation/spec.md).

## Scope

- composer key handling, prompt submission, shell mode, and history recall
- slash command parsing, registry-backed discovery, menu navigation, command
  feedback, and scripted command behavior
- file/image/skill/agent popups, bottom panes, model/session/variant pickers,
  and agent panels
- mouse wheel routing, row clicks, transcript focus, local selection, and
  clipboard fallbacks
- interruption, queued inputs, user shell escapes, undo/redo, and child-agent
  navigation control

Out of scope:

- visual shape of transcript rows, status line, sidebar, Markdown, and visual
  snapshots; see [211 testing](../211-pevo-tui-rendering/testing.md)
- parent state/session/model ownership; see
  [210 testing](../210-pevo-tui/testing.md)

## Deterministic Tests

Required interaction coverage:

- Registry-backed slash command parsing and compact menu rows, aliases, model,
  variant, mode validation, `/copy`, `Ctrl+O`, `/show-raw`, invalid
  `/show-raw` arguments, `/rename`, `/sessions`/`/resume`/`/continue`, and
  ambiguous session prefix handling.
- Configured slash aliases and shortcuts: effective config loading, project
  overriding global values, alias parsing for concrete slash lines with flags,
  trailing alias arguments appended before parsing, configured alias rows in
  the slash menu with Tab/Enter behavior, help alias/shortcut text, shortcut
  dispatch only from an empty composer, no history write for shortcut dispatch,
  `none` clearing a binding, leader-key timeout, and startup rejection for
  unknown commands, malformed aliases, duplicate aliases, duplicate shortcuts,
  and fixed-key conflicts.
- `/help` fullscreen bottom pane and scripted output with `General`,
  `Commands`, and `Custom commands` groups, compact aliases, configured slash
  targets in `Custom commands`, no CLI command appendix, bottom help
  tabs/navigation, no transcript row, no accepted arguments, and scripted
  output without command-row wrapping.
- Composer behavior for prompt submit, newline insertion, `Ctrl+A` full-text
  selection with visible highlighting, composer mouse drag selection,
  input-local selection clearing via `Backspace` and `Delete`, selection
  replacement via typing and bracketed paste, current-session persisted
  user-prompt history seeding, history recall with draft restoration,
  multi-line boundary behavior, and history search.
- Slash menu behavior for exact, prefix, and subsequence fuzzy matching,
  default first selection, Up/Down/Home/End navigation, mouse click, prefix-only
  Tab completion, argument placeholder hints in descriptions only, active
  `/compact [instructions]` parsing, and menu hiding when argument text or
  bottom panes take over.
- User shell escape behavior for fullscreen and scripted TUI: `!` detection
  after leading whitespace, explicit shell-mode state, `Shift+1`, shell marker,
  empty shell `Esc`/`Backspace`, pasted/raw/history `!<command>` import,
  command history recording as `!<command>`, execution receiving only
  `<command>`, bounded empty-shell help, provider/model resolution before
  execution, marker-file commands not running when config is missing, bounded
  persisted user shell context, aborting active shell work, auxiliary shell
  execution during foreground agent turns, pending auxiliary commands waiting
  for `run_start`, and next-turn persistence if the foreground turn has ended.
- Fullscreen composer `@` file completion: token detection for empty,
  path-like, Unicode, second-`@`, whitespace-boundary, and multi-line tokens;
  mid-word rejection; workdir-relative search; directory marking; gitignore
  handling; stale-result rejection; keyboard and mouse insertion; `Esc`
  dismissal until token changes; slash-menu and bottom-pane interop; shell-mode
  reuse for `cat @src<Tab>`; preserved quoting for paths with spaces; no naked
  shell completion; and image paths inserted as text rather than attachments.
- Image attachment UX: ordinary prompt text with image-looking paths remains
  text; leading absolute non-image paths and unknown slash-looking inputs submit
  as prompt text instead of command errors; standalone readable image-source
  paste creates a `[Image #N]` placeholder and pending attachment;
  unreadable/missing image-looking paste inserts text without error; `/image
  missing.png` renders a bounded error; `/image image.png describe` inserts
  placeholder plus prompt text; deleting a placeholder unbinds the image; `/new`
  clears pending images and placeholders; submitted image prompts preserve
  composer text.
- `/status`, `/usage`/`/stats`, `/context`, `/refresh`, `/btw`,
  `/show-thinking`, `/show-raw`, `/mode`, `/variant`, `/skills`,
  `/<skill-or-bundle>`, `/diff`, `/export`, and `/share`
  behavior, including parser errors, fullscreen/scripted parity, transcript
  command-row versus bottom-pane ownership, `-f` format aliases for export,
  share rejecting `-f`, and sensitive include handling.
- `/diff` interaction: registry/menu/help visibility, availability during an
  active turn, argument rejection, fullscreen overlay opening, computing state,
  static snapshot behavior, empty-diff message, scrolling with arrow/page
  keys, `Esc` close priority before transcript interruption, scripted bounded
  fallback, and no durable transcript/model-message insertion.
- `/context` command interaction: argument rejection, fullscreen command row,
  scripted output without the bar, and output limited to implemented context
  categories.
- `/refresh` command interaction: TUI-visible replacement for
  `/reload-context`, prompt-prefix reload result, background orphan side-session
  cleanup status, whole-command rejection while the active thread is running,
  rejection inside side conversations, and `/reload-context` direct-input
  feedback pointing to `/refresh` while remaining absent from help and menu
  discovery.
- `/btw` side conversation interaction: parsing for `/btw`, `/btw <prompt>`,
  and hidden `/side <prompt>`; help/menu visibility for `/btw` only; hidden
  temporary side-session creation; inherited snapshot plus boundary
  instructions; auto-submission of the initial prompt; parent running detachment
  without cancellation; parent live-event buffering and replay on return;
  parent status projection while inside the side; side-local model and
  permission changes restoring parent settings on return; limited side slash
  command whitelist; side transcript deletion on idle `Ctrl+C`; running side
  turn interruption before return; and real workspace mutation persistence.
- `/usage` and `/stats` alias behavior without provider calls.
- `/show-thinking` and `/show-raw` toggles: default values, explicit on/off,
  persistence, immediate refresh of existing blocks, no extra status row, and
  obsolete `/thinking`/`/raw` unsupported.
- `/mode <plan|default>` and `/variant <value>`: default values, persistence,
  bare-command rejection, obsolete `set` forms unsupported, fullscreen bottom
  state update, `Shift+Tab` cycling, no transcript status row for cycling, and
  next-turn application while a turn is running.
- `/model` bottom pane: local configured models, explicit fetched catalogs,
  default `Models` tab, `Info` tab switching preserving query/selection/scroll,
  provider fetch rows, footer changes, current-model initial focus, search,
  local precedence over fetched duplicates, stale fetched-row removal, variant
  transition, `Config default`, explicit variant persistence, `Esc`
  close/back/cancel, no live provider catalog calls from scripted output, and
  one non-blocking `models.dev` warmup only when needed.
- `/model` fetch and metadata refresh: explicit Enter-triggered fetch only,
  concurrent all-provider fetch, single-provider retry, skipped missing
  credentials with env-var hints, no-auth loopback requests without
  Authorization, five-second timeout, provider success counts, empty results,
  partial failure status, failure preserving old cache, cancellation preserving
  completed results, duplicate Enter feedback, and `Ctrl+R` metadata refresh.
- `/sessions` scripted fallback and fullscreen session bottom pane: search,
  grouped rows, visible-message counts, CJK/wide-character title alignment,
  active/archived view switching, action mode isolated from search, archive,
  restore, delete confirmation/cancellation, current-session clearing, and
  running-turn rejection for current-session archive/delete.
- Shared bottom selection pane Up/Down navigation wraps between first and last
  visible rows for sessions, model selection, and variant selection; Home/End
  remain direct first/last jumps.
- Permission approval bottom panel coverage: mouse clicks on allow once,
  session, always, and deny resolve the clicked option; long wrapped
  tool/action/grant details grow the panel until all options are visible when
  space permits; over-height details preserve content through internal
  scrolling instead of hiding options.
- `/agents Running` coverage for cap-state rendering, Pause/Resume spawning,
  and Stop subtree semantics. `/agents Available` coverage for disabled/error
  definition rows, `Use as main`, `Default main agent`, active/shadowed rows,
  local `.psychevo` create/update/delete, run-prompt panels, and rejection of
  main-agent switching while a turn is running.
- Main-agent switching coverage: selection scoped to current session, restored
  across session switching, reflected in `/status` and the
  transcript/composer separator rather than the bottom status line, used for
  subsequent `RunOptions.agent`, and not altering `@agent-name` delegation
  semantics. Child-session coverage must show persisted `agent.name` as the
  default separator identity and `Default main agent` restoring that identity.
- Agent row interaction coverage: row-click toggles details, `Open` title
  action enters the child session, overlapping click regions route to `Open`
  only inside the visible action region, transcript-focus `Space` toggles
  details, and `Enter`/`O` opens Agent rows.
- Running child interaction coverage: opening a child before parent completion,
  replaying scoped child backlog, receiving future child stream events,
  returning to the parent with `Alt+P`/`Alt+Left`, and `Esc` interrupting the
  detached child from either child or parent view.
- `/undo` and `/redo`: parsing, menu rows, fullscreen/scripted behavior, Git
  snapshot restore, repeated boundary movement, composer prompt restoration,
  cleanup before next prompt, and bounded no-op/error paths for no session, no
  user message, missing snapshot, non-Git workdir, and unsettled turns.
- `Esc` priority: `/diff` overlay, transcript/sidebar selection, file popup,
  skill popup, slash menu, composer selection, bottom pane, history search, and empty shell
  composer clear before interruption; provider transport and foreground shell
  waits wake promptly; post-abort title generation is skipped; idle `Esc` is
  non-destructive.
- Transcript focus behavior: `Ctrl+T`, `Esc`, Up/Down focused row movement,
  Enter/Space toggles/opening, PageUp/PageDown scrolling from composer and
  transcript focus, and composer-focus Up/Down remaining input/history boundary
  keys.
- Fullscreen mouse behavior: alternate-screen mouse capture, alternate-scroll,
  no any-motion tracking, hover-routed wheel scrolling, composer/status wheel
  ignored, slash/bottom-pane click priority, composer input drag selection with
  CJK-aware cursor mapping and edit-only release behavior, app-native
  transcript/sidebar drag selection, final rendered-buffer extraction,
  wide-character preservation, transcript/sidebar region locking,
  right-padding trimming, visible highlight, non-blocking clipboard copy,
  `Ctrl+C` selection copy, and bounded clipboard failures.
- Clipboard backend coverage for WSL kernel marker detection, WSL
  `powershell.exe` then `clip.exe`, Linux Wayland `wl-copy`, X11 fallbacks,
  SSH terminal-mediated copy, tmux clipboard forwarding fallback, OSC52 payload
  bounds, UTF-8/CJK OSC52 payloads, and no fullscreen exit on copy failure.
- Interruption and queueing: interrupted foreground turns restore queued prompt
  and shell inputs to the composer, normal completion drains queued inputs FIFO,
  running prompt `Enter` steers the active agent turn without immediate
  durable transcript insertion, pending steer and queued prompt content is
  visible in the fixed preview above the composer while unsent, committed steer
  inputs clear the preview entry and appear as ordinary user transcript content,
  `/queue` drains FIFO after normal completion, `/steer` rejects idle and
  non-agent contexts, pending preview `edit` confirms or cancels drafts with
  `Enter`/`Esc`, pending preview `undo` cancels steer or removes queued
  prompts, `/pending cancel` clears unsent steer and queue items, interrupted
  foreground turns restore unsent pending inputs, and settled interrupted
  transcript evidence remains distinct from ordinary failure styling.
- Obsolete inputs such as `/models`, `/model set`, `/variant set`, `/mode set`,
  `/thinking`, `/session list`, `/session show`, and `/session switch` are
  unsupported and absent from the slash menu.

## Validation

Relevant narrow validation:

- `cargo test -p psychevo-cli`

Broad validation remains:

- `scripts/validate.sh`
