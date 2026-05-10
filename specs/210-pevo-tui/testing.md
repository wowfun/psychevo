---
name: 210. pevo TUI Testing
psychevo_self_edit: deny
---

Define deterministic acceptance coverage for the first `pevo tui` slice.

Automation vocabulary and generic validation boundaries follow
[060 Automation](../060-automation/spec.md).
The functional source material is split across the parent
[TUI spec](spec.md), [sessions](sessions.md), [state and models](state-and-models.md),
[input and commands](input-and-commands.md), and
[layout and rendering](layout-and-rendering.md).

## Deterministic Tests

Required coverage:

- TUI state read/write, version tolerance, per-workdir model and variant
  precedence, per-workdir mode persistence, global thinking persistence, global
  sidebar visibility persistence, and recent-model bounding
- slash command parsing, model/variant/mode validation, `/rename`,
  `/sessions`/`/resume`/`/continue`, and ambiguous session prefix handling
- composer behavior for submit, newline, current-session persisted user-prompt
  history seeding, history recall with draft restoration, and history search
- user shell escape behavior for fullscreen and scripted TUI: `!` detection
  after leading whitespace, empty `!` bounded help, local shell execution
  without provider credentials, `Ran <first command line>` evidence, failure
  projection, `Esc` clearing empty shell-mode input, aborting active shell work,
  FIFO queueing with prompts and shell escapes while active work is running,
  and process-local history that survives session switches without seeding
  future provider context
- fullscreen composer `@` file completion: token detection for empty, path-like,
  Unicode, second-`@`, whitespace-boundary, and multi-line current-line tokens;
  rejection of mid-word `foo@bar`; workdir-relative search; directory marking;
  gitignore handling; stale-result rejection; keyboard and mouse insertion;
  `Esc` dismissal until the token changes; and interop with slash menus and
  bottom selection panes
- slash menu default selection: the first visible completion row is selected
  and pressing `Enter` executes it
- evidence-ledger projection for unlabeled prompt blocks without left rails,
  expandable flat `Thinking` rows without left rails, flat
  `Explored`/`Ran`/`Changed` tool rows without `Tool calls` section headers,
  failures inside their original group, unlabeled answer body text without left
  rails, tool blocks without left rails, `Ran <actual first command line>`
  titles that skip leading blank/comment-only shell lines and survive
  start-to-end tool updates even when end events omit tool arguments, transient
  active `Exploring`/`Running`/`Changing` rows created from streaming assistant
  tool-call blocks before tool execution starts, temporary message-scoped
  `content_index:call_index` key migration to `tool_call_id`, no duplicate row
  when `tool_execution_start` follows a pending row, no overwrite when later
  assistant messages reuse the same `content_index:call_index` pair,
  interrupted pending rows stopping their timer as failed `interrupted`
  evidence, metadata left rails, unlabeled turn meta only after visible answers
  or failure summaries, visible-text-plus-tool-call assistant messages keeping
  active tool evidence below the visible text and above metadata, intermediate
  `finish_reason=tool_calls` text-plus-tool-call messages not rendering turn
  metadata, active tool rows suppressing turn metadata until they settle, and
  no extra red `turn ended: normal` row when the final turn outcome is normal
  but one or more tool calls failed, and debug meta; only the
  active Thinking uses shared activity motion, completed Thinking uses a stable
  bullet marker, reasoning content uses the normal thinking body role, and
  explicit reasoning paragraphs do not receive label-width indentation
- compact UI duration formatting for model metadata, tool evidence, plain
  renderer output, and bottom running status: whole seconds below one minute,
  `XmYYs` at one minute or more, zero-padded minute seconds, and floor rounding
  from persisted millisecond precision; active fullscreen tool-evidence cache
  keys must track the current elapsed label and spinner frame so `Running` /
  `Exploring` / `Changing` rows refresh while live
- plain non-terminal renderer output for `Prompt`, `Thinking`, active
  `Exploring`/`Running`/`Changing` preparation notices, completed `Explored`,
  `Ran`, `Changed`, `Answer`, and `Meta` blocks, including `--debug`, without
  printing repeated preparation lines for every argument delta
- narrow and wide layout rendering, sidebar hidden by default for fresh state,
  persisted optional sidebar visible state, thinking visible/hidden,
  expanded/collapsed tool output, minimal bottom state line, and composer
  surface without a left accent rail; user prompt blocks and the composer must
  share the same adaptive full-width surface with a leading `›` prompt marker,
  falling back to `RGB(38,38,38)` when no terminal background is known; the
  empty composer must occupy two input rows with surface background on both
  rows, wrapped historical prompt rows including CJK/wide-character content
  must keep full-width prompt background on each physical row, and sidebar
  headings must be bold without colored left rails; running-state snapshots must
  show spinner/elapsed/`Esc` appended to the stable bottom state line and must
  not contain `Working` or active phase words that belong in ledger rows
- active tool evidence snapshots and unit tests must preserve ledger-only
  `Exploring`/`Running`/`Changing` state while a tool timer is live, suppress
  redundant `running`/`preparing` body-only rows, keep completed
  `Explored`/`Ran`/`Changed` rows stable, and retain the first actual bash
  command line in active and completed titles after skipping leading blank and
  comment-only shell lines. A targeted snapshot must cover the provider shape
  where an intermediate `finish_reason=tool_calls` assistant message contains
  visible text followed by a `write` tool call, keeping the active `Changing`
  row visible below that text without premature turn metadata
- completed live tool evidence tests must cover `Exploring`/`Running`/
  `Changing` rows settling into `Explored`/`Ran`/`Changed` while preserving the
  visible active duration when runtime `elapsed_ms` is shorter, including 0ms
  instant completions
- fullscreen active tool visibility tests must cover reasoning-only assistant
  `finish_reason=tool_calls` messages with a `write` tool call, proving
  `Changing <path>` appears below `Thinking` with no premature metadata; they
  must also prove visible Thinking text with an explicit write/run intent can
  create a generic provisional `Changing files`/`Running command` row, hidden
  Thinking text cannot, runtime pending tool-call input events for `write`
  create a visible `Changing files` row before complete arguments or local
  execution arrive, and later concrete tool-call signals adopt the provisional
  row instead of duplicating it. Same-tick `message_end(write)` /
  `tool_execution_start(write)` / `tool_execution_end(write)` batches are
  deferred so `Changing` renders before completion
- runtime stream projection must expose named pending tool-call input events to
  fullscreen TUI without leaking folded reasoning into sanitized message events;
  provider streams that emit tool names before complete JSON arguments must
  produce active ledger rows during argument generation instead of waiting for
  local tool execution
- fullscreen history reload coverage must include a persisted assistant
  `finish_reason=tool_calls` message with a `write` tool call and no matching
  tool result yet; it must render an active `Changing <path>` row without turn
  metadata and must update that same row to `Changed <path>` when the matching
  persisted or streamed `tool_result` arrives. It must also include a persisted
  aborted assistant message whose tool calls have no matching tool results;
  those rows must render as static failed `interrupted` evidence, use completed
  tool titles such as `Ran <command>`, and must not keep live
  `Exploring`/`Running`/`Changing` timers after TUI restart. A targeted visual
  snapshot must lock the pending history `Changing <path>` row shape.
- visible assistant preamble fallback coverage must prove that text such as
  `Let me write the complete report` can create a provisional `Changing files`
  row during a still-open assistant message, that visible Thinking text follows
  the same explicit-intent provisional/adoption/removal rules while hidden
  Thinking does not, and that the provisional row is removed if the assistant
  message finishes without a matching tool call. Repeated visible preamble
  updates after a concrete write signal must not leave duplicate
  `Changing files` rows after `Changed` appears. A targeted visual snapshot
  must prove an active `Changing` row suppresses a prior failure
  turn-metadata block such as `0s 1 failure`
- streaming reasoning regression coverage must prove that a prior failure
  turn-metadata block is removed once `Thinking` continues, and that an aborted
  reasoning-only assistant message does not recreate metadata below `Thinking`
- expandable tool output snapshots must show right-side text hints such as
  `▸ N more lines`, `▸ more output`, and `▾ collapse`, with no bare `[+]` or
  `[-]` tokens
- expandable transcript row coverage must include shared collapse thresholds
  for long Thinking and long tool output, active Thinking elapsed, short
  Thinking detail collapse, completed and active tool row detail collapse, long
  `Ran`/`Running` command-title expansion, long JSON/HTML-like single-line tool
  output collapse, keyboard `Enter`/`Space` row toggles, mouse row toggles, and
  drag text selection not toggling anything. Snapshot coverage must prove
  transcript focus uses a single-line `›` marker instead of repeating `>` on
  every wrapped Markdown/table line
- terminal-adaptive TUI theme derivation for dark, light, and unknown terminal
  backgrounds; prompt/composer shared surfaces, popup/menu surfaces, selected
  row contrast, accent styles, and static motion fallback are covered by
  deterministic unit tests without relying on a live terminal palette
- lightweight TUI Markdown projection for assistant answers: headings, lists,
  emphasis, inline code, fenced code blocks, links, and workdir-relative local
  file links render in fullscreen snapshots without altering persisted message
  content or non-terminal renderer output
- transcript scroll regression coverage must include long Markdown/table
  answers with metadata, terminal reasoning-only Thinking tables with metadata,
  manual PageDown or mouse-wheel scrolling to the bottom, empty-composer
  `Down` scrolling, and transcript-focus `Up`/`Down` movement keeping the
  selected row visible
- streaming runtime projection that never leaks folded reasoning into sanitized
  message events while still delivering dedicated TUI thinking events
- runtime metrics projection that can expose usage and allowlisted metadata to
  TUI without putting them in sanitized transcript messages
- context-percent display in the sidebar only when a model context limit is
  known, sidebar token usage using `usage.input_tokens` as the last known
  context-window count rather than `total_tokens`, and staying visible while a
  model is answering events that do not include usage
- sidebar redraw clearing must be covered by a regression test that renders
  over a polluted previous terminal frame and proves labels such as `tokens`
  and blank sidebar rows do not retain stale glyphs
- sidebar estimated session cost display from persisted accounting, including
  unknown-priced messages and known free messages
- `/show-thinking` toggle behavior: default visible, explicit on/off, global
  persistence, visible reasoning rendered only in TUI output and never in
  sanitized transcript views, fullscreen visibility changes immediately refresh
  existing Thinking blocks, and hidden thinking does not render a
  `Thinking: hidden` marker or append a status row; removed `/thinking`
  returns a bounded error that points to `/show-thinking`
- `/mode <plan|default>` behavior: default `default`, persisted
  `plan`/`default` per workdir, bare `/mode` rejected, removed
  `/mode set <value>` rejected with guidance, `Shift+Tab` cycling in the
  fullscreen event loop, no transcript status row for mode cycling, and
  next-turn application while a turn is running
- `/status` behavior: fullscreen and scripted TUI project the same local state
  fields as one multi-line status block, excluding thinking visibility
- `/stats` behavior: fullscreen and scripted TUI project deterministic local
  usage/cost summaries without provider calls
- slash command completion from `Tab`, keeping argument placeholders in slash
  menu descriptions only and out of completed composer text
- `/skills` and `/skill:<name>` behavior: deterministic listing, skill prompt
  expansion, unknown-skill errors, and dynamic slash menu entries
- runtime Plan mode toolset: exposes `read`, `list`, and `search`; does not
  expose `bash`, `write`, or `edit`
- mode instruction is sent to providers for the current turn and is not
  persisted in `messages`
- default TUI session selection of latest `run` or `tui` session by canonical
  workdir
- session title persistence, `/rename <title>`, title display in session
  picker/sidebar, automatic title generation with model success, and
  deterministic first-prompt fallback when title generation fails or returns an
  unusable title; skill-marker first prompts include selected skill context in
  the non-persisted title request, fallback to selected skill names when the
  prompt contains only resolved skill markers, and fullscreen sidebars refresh
  after detached post-`agent_end` title generation completes
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
  models plus explicit current-process fetched catalogs, including no subtitle
  row, top-level `All providers` status row, selectable provider fetch rows,
  dynamic `Enter fetch`/`Enter select` footer text, initial focus on current
  model or first local model before fetch rows, search that keeps `All
  providers` visible and preserves provider rows for model matches, current
  query preservation after fetch, local-model precedence over duplicate fetched
  rows, stale fetched-row removal except for the current model, model-to-variant
  transition, `Config default` clearing the variant override or using provider
  default for fetched-only rows, explicit variant persistence, and `Esc`
  close/back/cancel behavior
- model rows render known limits, capability tags, and compact pricing metadata
  from config, cached `models.dev`, or explicit catalog fetches
- fullscreen `/model` fetch behavior: explicit Enter-triggered fetch only,
  concurrent all-provider fetch, single-provider retry, skipped missing
  credentials with env-var hints, loopback/no-auth catalog requests without
  Authorization, five-second provider timeout reported as `failed: timeout`,
  provider success counts, `no models` empty results, partial failure status,
  failure preserving old fetched cache, cancellation preserving completed
  provider results, and in-progress duplicate Enter bounded feedback
- `/variant <value>` persistence, bare `/variant` rejection, removed
  `/variant set <value>` rejection with guidance, and fullscreen bottom state
  rendering of the current effective variant instead of falling back to
  `default`
- scripted `/model` output from configured provider/model entries without live
  provider catalog calls
- `/sessions` scripted fallback list output
- fullscreen session selection through the shared bottom pane: search, grouped
  row rendering, visible-message counts matching the sidebar, selection, and
  transcript/history replacement; rows with CJK/wide-character titles must keep
  the updated time right-aligned on the same physical row
- fullscreen session management through the shared bottom pane: active and
  archived view switching, action mode that does not pollute search, archive,
  restore, hard-delete confirmation/cancellation, current-session clearing
  after archive/delete, and running-turn rejection for current-session
  archive/delete
- shared bottom selection pane Up/Down navigation wraps between first and last
  visible rows for sessions, model selection, and variant selection, while
  Home/End remain direct first/last jumps
- `/help`, `/models`, `/model set`, `/session list`, `/session show`, and
  `/session switch` rejected as removed commands and absent from the slash menu
- `/undo` and `/redo` parsing, menu rows, fullscreen behavior, scripted output,
  Git snapshot restore, repeated undo/redo boundary movement, composer prompt
  restoration after undo, cleanup before the next prompt, and bounded no-op or
  error paths for no session, no user message, missing snapshot, non-Git
  workdir, and unsettled running turns
- `Esc` interrupts a running turn through runtime control in fullscreen mode,
  shows the transient interrupting state without adding an immediate transcript
  row, gives selection/file popup/skill popup/slash menu/bottom panel/history
  search/empty shell composer priority over interruption, wakes provider
  transport and foreground shell waits promptly, skips post-abort
  title-generation follow-up, and has no destructive effect while idle
- interrupted fullscreen turns restore queued prompt and shell inputs to the
  composer instead of starting the next queue item automatically, while normal
  completion still drains queued inputs FIFO
- slash menu exact, prefix, and subsequence fuzzy matching over command labels,
  argument placeholder hints in description text, `/model` described as
  `select/fetch model`, prefix-only Tab completion that does not complete
  fuzzy-only matches, disabled `/compact` and `/export` entries, and bounded
  `upcoming` feedback
- transcript focus and expansion behavior: `Ctrl+T`, selected row movement,
  `Enter`/`Space` row expand-collapse, `Esc` returning to composer, and
  keyboard transcript scrolling
- slash menu row selection with Up/Down/Home/End and mouse click, including
  Up/Down wraparound and `/mo` navigation to `/mode` before `Enter`
- transcript auto-follow behavior: new prompts reset to bottom-following,
  streaming assistant deltas and long generated answers remain visible while at
  bottom, transcript scroll height excludes decorative border rows so the final
  line is not hidden behind the composer, bottom-scroll requests made before
  real viewport dimensions are known are applied on the next render, redraws
  after shorter rows do not retain stale glyphs from earlier longer rows,
  bottom-scroll limits use the same word-wrapped rendered line count as the
  paragraph widget for long mixed Markdown/CJK text, repeated scroll redraws
  reuse cached row heights without re-wrapping unchanged rows while invalidating
  stale row-height caches when row content or state changes, bursty mouse wheel
  redraws are bounded by input coalescing, manual scrolling opts out, and
  returning to the bottom resumes auto-follow
- long-output resilience for model-generated long answers and read-tool results:
  wrapped content must not overwrite composer/sidebar, collapsed read output
  must retain expandable full content, and expanded long reads must scroll
  coherently
- multi-tool turns that emit visible assistant text before and after tool calls
  preserve each visible assistant message as a separate answer block; later
  streaming updates must not replace earlier answer text from the same turn
- fullscreen TUI captures mouse events in alternate screen mode, disables mouse
  capture on exit, avoids any-motion mouse tracking, routes mouse wheel to
  transcript or active bottom pane scrolling, supports left-click selection for
  slash and bottom-pane rows, and supports app-native mouse drag text selection
  with `Ctrl+C`/mouse-up copying through test-injected clipboard sinks; mouse
  drag copy must not synchronously block the input loop; selection extraction
  must use final
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
composer, sidebar, slash menu, long Markdown/table transcript scrolling, and
screenshot-visible interaction states. The diagnostic script uses a
deterministic local mock provider, an isolated repo-local `PSYCHEVO_HOME`, and
the current workspace `pevo` binary. It writes PNG screenshots and companion
material under `.local/.psychevo-dev/tui-shots/<timestamp>/`.

The demo workdir must be isolated from the parent repository's git state so
Modified Files does not reflect unrelated uncommitted work. The tape should pin
terminal color environment, clear inherited `NO_COLOR`, and avoid theme choices
that squash TUI color-role contrast across repeated runs. The tape must include
a long Markdown/table answer and a terminal reasoning-only Thinking table with
turn metadata, scroll the transcript away from the bottom and then back down,
capture the default collapsed Thinking/table state, then expand the Thinking
row and capture a screenshot proving the bottom marker and metadata row are
visible.

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
