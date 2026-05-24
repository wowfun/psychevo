---
name: 211. pevo TUI Rendering Testing
psychevo_self_edit: deny
---

# 211. pevo TUI Rendering Testing

Define deterministic acceptance coverage for fullscreen TUI rendering,
evidence projection, visual regression, and diagnostics. Functional rendering
requirements live in [211 pevo TUI Rendering](spec.md) and its linked
attachments.

Automation vocabulary and generic validation boundaries follow
[060 Automation](../060-automation/spec.md).

## Scope

- transcript ledger projection and evidence row rendering
- active/completed Thinking, tool, and Agent row behavior
- fixed composer/status-line rendering, context usage, sidebar, and local path
  display
- terminal Markdown projection, raw answer display, and non-terminal plain
  renderer shape
- deterministic visual snapshots and optional VHS screenshots

Out of scope:

- key bindings, slash command parsing, panel navigation, popup selection, and
  clipboard control flow; see
  [212 pevo TUI Interaction Testing](../212-pevo-tui-interaction/testing.md)
- session/model persistence and runtime ownership tests; see
  [210 pevo TUI Testing](../210-pevo-tui/testing.md)

## Deterministic Tests

Required rendering coverage:

- Evidence-ledger projection for unlabeled prompt blocks without left rails,
  flat expandable Thinking rows, flat tool-name-first evidence rows without
  `Tool calls` section headers, failures inside their original group, unlabeled
  answer body text, compact metadata rails, and turn metadata only after visible
  answers, terminal reasoning-only messages, or failure summaries.
- Tool evidence titles must start with the actual tool invocation name. `exec_command`
  titles must use the first actual command line, skip leading blank/comment-only
  shell lines, and survive start-to-end updates even when end events omit
  arguments.
- Active tool-name-first rows must appear from streaming
  assistant tool-call blocks, runtime pending tool-call input events, and local
  tool-execution starts; they must migrate temporary content keys to
  `tool_call_id`, avoid duplicate rows, suppress premature turn metadata during
  intermediate `tool_execution_end` events and while the foreground running
  marker remains visible after terminal `message_end`, and settle into
  completed rows while preserving the visible active duration.
- Interrupted pending rows render as muted `interrupted` evidence, aborted exec command
  results render `interrupted` instead of `(no output)`, timeout failures render
  an explicit timeout line before partial output, and user-confirmed interrupts
  do not create `1 failure` metadata.
- Active Thinking uses shared activity motion, completed Thinking uses a stable
  bullet marker, reasoning body text uses the ordinary thinking body role, and
  explicit reasoning paragraphs do not receive label-width indentation. A
  visible assistant answer that begins before an explicit `reasoning_end` event
  must complete the active Thinking row so the answer never streams beneath a
  live Thinking spinner.
- Compact duration formatting covers model metadata, tool evidence, plain
  renderer output, turn metadata, and bottom running status: whole seconds
  below one minute, `XmYYs` at one minute or more, zero-padded minute seconds,
  and floor rounding from persisted millisecond precision.
- Active fullscreen evidence cache keys must track elapsed labels and spinner
  frames so active tool-name-first rows refresh while live.
- Running-state status-line snapshots must show the same shared activity marker
  plus elapsed/`Esc` appended to the stable bottom status line, use
  deterministic elapsed values, and omit `Working` or active phase labels that
  belong in ledger rows.
- Child-session status-line rendering must cover context usage, child elapsed
  restored from child user prompt timestamp, parent elapsed restored after
  navigating back to a still-running parent turn, fallback to parent
  context-limit metadata, compact parent navigation, and no running marker for
  post-completion auxiliary cleanup tasks.
- Foreground Agent rendering tests must cover parent-row live tails, scoped
  child stream buffering/replay, coalesced child Thinking previews, completed
  token usage, hidden completion notification suppression, no duplicate
  placeholder rows, reloaded running parent rows enriched from durable edges,
  and the full title plus `Open` affordance.
- Shared evidence-row tests must cover Thinking/tool/Agent expand-collapse
  semantics, active elapsed labels, live `2+4` head/tail previews, short
  Thinking collapse, long Thinking preview/full/title-only transitions, long
  command-title expansion, long JSON/HTML single-line output collapse,
  unbroken table separator collapse, and display-token bounds.
- Layout snapshots must cover narrow and wide widths, sidebar visible/hidden,
  idle composer, running thinking, active and completed tool evidence,
  collapsed/expanded output, debug metadata, failure metadata, bottom panes as
  rendered surfaces, and narrow compact layout.
- Prompt blocks and composer must share the same adaptive full-width surface
  with a leading `›` prompt marker, fall back to `RGB(38,38,38)` when no
  terminal background is known, keep an empty composer to one visible row, and
  preserve full-width prompt backgrounds for wrapped/CJK rows.
- Fullscreen composer cursor anchoring tests must assert the terminal cursor
  position for empty input, normal text, shell mode input, CJK/wide text, and a
  popup rendered above the composer. A focused redraw-cadence test should cover
  any helper that throttles timeout-only passive running redraws.
- Composer, transcript, and sidebar active text selection must share a
  reverse-video/bold highlight that remains visible on full-width user prompt
  surfaces without depending on the prompt surface background color.
- Terminal-adaptive theme derivation for dark, light, and unknown backgrounds
  must cover prompt/composer surfaces, popup/menu surfaces, selected row
  contrast, accent styles, and shared activity motion without relying on a live
  terminal palette.
- Lightweight Markdown projection must cover headings, lists, emphasis, inline
  code, fenced code blocks, box table rendering, narrow pipe-table fallback,
  fenced Markdown table unwrapping, code-block folding/highlighting, links with
  visible URLs, and workdir-relative local file links without altering
  persisted content or non-terminal output.
- Raw transcript display snapshots must cover rich and raw assistant answer
  rendering at narrow and wide widths while preserving the ledger shell; visible
  Thinking content follows raw mode.
- Transcript scroll regression coverage must include long Markdown/table
  answers with metadata, terminal reasoning-only Thinking tables with metadata,
  manual PageDown or mouse-wheel bottom scrolling, auto-follow during streamed
  deltas, stale-cell clearing after shorter redraws, row-height cache reuse,
  and bottom-scroll limits based on rendered wrapped line counts.
- Sidebar rendering must prove title/session and `Modified Files` remain while
  the old Context section is absent, headings are bold without colored left
  rails, content is cleared before redraw, and removed labels such as `workdir`,
  `branch`, `messages`, `tool calls`, `tokens`, `context`, and `cost` leave no
  stale glyphs.
- Bottom context/path display must cover context formatter parity with
  `/context`, omission when context limit is unknown, refresh after `/context`,
  stability during unrelated model events, home-relative `~`, non-home absolute
  paths, long center truncation, CJK/wide-character width, and branch omission.
- Plain non-terminal renderer output must cover `Prompt`, `Thinking`, active
  tool-name-first preparation notices, completed tool-name-first evidence,
  `Answer`, and `Meta` blocks, including `--debug`, without repeated
  preparation lines for every argument delta.

## Visual Regression

The primary TUI visual regression path is a `ratatui` `TestBackend` or
`Buffer` snapshot. These checked-in goldens render stable text plus stable
style-role markers so tests can assert layout, emphasis, and color-role
discipline without storing raw ANSI escape sequences as the default golden
format.

Snapshot changes must use an explicit snapshot review flow. The developer or
agent should inspect pending diffs before accepting intentional changes. These
stable buffer/style snapshots are part of default broad validation.

Required visual fixtures cover at least 80-column and 120-column widths with a
realistic coding-agent turn. The fixture set should include idle composer,
running thinking, tool evidence, collapsed and expanded output, slash menu,
bottom selection panes for models, variants, and sessions, debug meta,
sidebar visible/hidden, failure/tool-error meta, and narrow compact layout.
The default composer fixture should verify stripped bottom chrome: no composer
mode title, no shortcut footer, no `mode=`/`model=`/`variant=` prefixes, stable
model/variant positions, and non-default mode last.

When practical, snapshot tests should write untracked Agent-readable diagnostic
material under `target/pevo-tui-snapshots/<fixture>/` on failure or review:
plain rendered text, style-role projection, combined projection, and fixture
metadata. These diagnostics are not the checked-in source of truth.

VHS capture is required validation for changes that affect fullscreen TUI
visual display. This includes layout, color, visible transcript text, composer,
sidebar, slash menu, long Markdown/table transcript scrolling, and
screenshot-visible interaction states. The diagnostic script uses a
deterministic local mock provider, an isolated repo-local `PSYCHEVO_HOME`, and
the current workspace `pevo` binary. It writes PNG screenshots and companion
material under `.local/.psychevo-dev/tui-shots/<timestamp>/`.

Large deterministic VHS inputs, including the mock provider, tape template,
and stable workdir/home fixture files, should live as checked-in assets next to
the capture script instead of being embedded as shell heredocs. The script may
still generate a per-run tape, config, request log, and screenshot directory
for values that necessarily vary between runs, such as ports, database paths,
and output artifact locations.

The demo workdir must be isolated from the parent repository's git state so
Modified Files does not reflect unrelated uncommitted work. The tape should pin
terminal color environment, clear inherited `NO_COLOR`, and avoid theme choices
that squash TUI color-role contrast across repeated runs. The tape must include
a long Markdown/table answer and a terminal reasoning-only Thinking table with
turn metadata, scroll the transcript away from the bottom and then back down,
capture the default collapsed Thinking/table state, then expand the Thinking
row and capture a screenshot proving the bottom marker and metadata row are
visible. It must also capture an interrupted foreground exec command row so the settled
`interrupted` marker can be visually inspected.

Agent-observability VHS coverage must include a foreground clickable `Agent`
tool row plus `/agents` Running, Available, definition action, and run-prompt
panels using deterministic local agent definitions; the parent running Agent
row with `Open`, live tail, and tokens; a running child session showing live
Thinking/tool activity; and the child navigation hint while preserving the
regular session layout.

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

- `cargo test -p psychevo-cli`

Broad validation remains:

- `scripts/validate.sh`
