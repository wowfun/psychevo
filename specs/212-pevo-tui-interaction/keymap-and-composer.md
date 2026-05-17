---
name: 212. pevo TUI Interaction Keymap and Composer
psychevo_self_edit: deny
---

# 212. pevo TUI Interaction Keymap and Composer

Define fullscreen key handling, composer behavior, paste handling, mouse routing, and selection basics.

## Keymap

The first fullscreen keymap is fixed and composer-first. V1 keeps the key
handling organized for future user keymap configuration, but it does not expose
custom keymaps.

- `Enter` submits the composer. When slash completion suggestions are visible,
  the first suggestion is selected by default and `Enter` executes that
  suggestion directly.
- `Shift+Enter`, `Ctrl+Enter`, `Alt+Enter`, and `Ctrl+J` insert a newline.
- Shell mode is an explicit composer state, not literal `!` text in the
  textarea. Pressing `Shift+1` from an empty composer enters shell mode and
  leaves the textarea empty. While shell mode is active, the composer prompt
  marker is `! ` instead of the normal prompt marker. Empty shell mode exits on
  either `Esc` or `Backspace`. Pasted text, history recall, scripted input, or
  raw submit text that begins with `!` after leading whitespace imports as
  shell mode with the leading `!` stripped. Submitting shell mode records
  `!<command>` in composer history, but the shell execution layer receives only
  `<command>`. Bare shell mode submission shows bounded shell-help text and
  does not execute.
- `!<command>` runs the command locally in the selected workdir through the
  bounded runtime shell executor. The command is not a provider-callable `bash`
  tool, but its bounded result is persisted as model-visible user shell context
  according to the runtime shell-context contract. Live and reloaded user shell
  transcript rows render like user prompt rows: the command line uses the same
  full-width prompt background, starts with `! ` followed by the user's command,
  uses the same marker color as the shell-mode composer `!`, and omits the
  normal tool-evidence bullet and `Ran` label. The command output
  remains below that prompt-styled command line as bounded evidence detail. This
  distinguishes user-submitted shell commands from model-requested bash tool
  calls.
- `Up` and `Down` recall submitted composer history when the current composer
  position is at the first or last logical line respectively. History recall
  preserves the in-progress draft and restores it when the user moves past the
  newest history entry. Within multi-line input away from those boundaries,
  `Up` and `Down` keep their normal textarea cursor movement.
- `Tab` completes slash commands in the composer when the current input starts
  with `/` and shell mode is not active.
- Typing an `@` token in the fullscreen composer opens a file path completion
  popup for the selected workdir. Valid tokens start at the beginning of the
  current line or after whitespace, so `@`, `@src`, and `hello @src` trigger
  completion while `foo@bar` does not. The popup is hidden while a bottom
  selection pane is open.
- While the `@` file popup is visible, `Up`/`Down` wrap selection,
  `Home`/`End` jump to the first/last result, `Tab` and `Enter` insert the
  selected relative path, and `Esc` closes the popup without editing text.
  Inserting a path replaces only the active `@` token, appends one trailing
  space, and quotes paths containing whitespace when they do not already
  contain a double quote.
- Shell mode reuses the same `@` file path completion popup, so
  `cat @src<Tab>` inserts a workdir-relative path such as `src/main.rs ` using
  the existing whitespace quoting rules. Image paths selected this way remain
  plain text paths and do not create attachments. Naked shell words such as
  `cat src<Tab>` do not trigger shell-native completion.
- `Shift+Tab` cycles `default -> plan -> default`.
- `Esc` clears active UI state before it can interrupt work: text selection,
  file and skill popups, slash menu, bottom selection panes, history search,
  and an empty shell-mode composer all take priority. If none of those states
  is active and foreground work is running, `Esc` requests
  interruption through runtime control. Runtime-controlled provider generation
  and foreground shell waits must wake on that signal instead of waiting for the
  next provider chunk, shell polling interval, or title-generation follow-up.
  When idle, it performs no destructive action.
- `Ctrl+T` focuses the transcript. In transcript focus, `Up`/`Down` move the
  focused transcript row, `PageUp`/`PageDown` scroll, `Enter`/`Space` toggles
  folded evidence rows or opens clickable `Agent` rows, and `Esc` returns to
  composer focus.
- `?` opens contextual shortcut help when the current surface supports it.
- When a TUI text selection is active, `Ctrl+C` copies and clears it. Otherwise
  `Ctrl+C` requests quit. `Ctrl+D` quits.
- `Ctrl+O` copies the latest visible assistant answer as raw Markdown source,
  equivalent to `/copy`.
- `Ctrl+B` toggles the local context sidebar.
- `Ctrl+R` enters history search.
- `PageUp`/`PageDown` scroll the transcript or active bottom selection pane.
  Mouse wheel events route by the current pointer row: transcript hover scrolls
  the transcript, bottom-pane hover scrolls the pane, and composer/status/other
  non-scrollable hover does not trigger composer history recall.
- In composer focus, plain `Up` and `Down` are input/history boundary keys, not
  transcript scrolling keys. `Up` recalls the previous submitted prompt only
  when the composer cursor is on the first logical line; an empty composer at
  that boundary recalls the latest prompt. `Down` recalls the next submitted
  prompt only while a history entry is already active and the cursor is on the
  last logical line, restoring the saved draft after the newest history entry.
  Otherwise `Up`/`Down` remain textarea navigation or no-op behavior.

Transcript rows are a lightweight keyboard focus target, not a modal editor.
Bounded Thinking, command, and tool evidence details expand inline by mouse
clicking the folded row or by focusing the transcript with `Ctrl+T` and pressing
`Enter`/`Space` on the selected row. Clickable `Agent` rows use the same row
focus path to enter the child session. Composer focus remains the default after
turn completion and after `Esc`.

Fullscreen TUI enables terminal mouse capture while the alternate screen is
active and enables xterm alternate-scroll mode so terminal wheel input stays
inside the fullscreen application instead of scrolling host terminal scrollback.
Wheel input reported as mouse events with coordinates uses hover-based routing;
terminals that synthesize plain `Up`/`Down` cursor keys are indistinguishable
from real keyboard input once delivered to the app and are handled by the
composer boundary rules above. Leaving fullscreen disables mouse capture and
leaves alternate-scroll disabled. Fullscreen TUI also enables bracketed paste
while active and disables it during terminal restoration.
Bracketed paste events are inserted into the composer as a single paste
operation after normalizing `\r\n` and bare `\r` to `\n`; pasted text must not be
reinterpreted as key-by-key input or lose bytes from local filesystem paths.
Pasting a standalone image source adds it to the pending image attachments
only when the pasted text resolves to a readable image source, and inserts a
plain-text attachment placeholder into the composer using the fixed
`[Image #N]` format. Pasting a standalone image-looking path that is missing,
unreadable, or unsupported inserts the normalized text unchanged and does not
show an image error. Pasting ordinary text, including prompts with embedded
local paths, relative paths, `http(s)://` URLs, or `data:image/*` URLs, inserts
the normalized text unchanged and must not auto-extract attachments. Pasting
updates file, skill, and slash completion popup state the same way as ordinary
composer edits. Left-click selection is
supported for slash menu rows and bottom selection pane rows, and those
interactive row hits take precedence over starting text selection.
Mouse drag selection over rendered transcript and sidebar text is also
supported. The active selection is highlighted while dragging, uses text from
the final rendered buffer rather than pre-wrapped logical lines, locks to the
rendered region where the drag started, and trims only right-side terminal
padding when copying. A drag that starts in the transcript must not copy same-row
sidebar text, and a drag that starts in the sidebar must not copy same-row
transcript text. On mouse release, selected text is copied through the
application clipboard backend and then the selection is cleared. On WSL,
detection must work even when
`WSL_INTEROP` and `WSL_DISTRO_NAME` are absent by inspecting Linux kernel
release/version text for WSL markers. WSL copy prefers `powershell.exe`
`Set-Clipboard` with UTF-8 stdin, then `clip.exe`, then terminal-mediated
OSC52/local Linux fallbacks. Copy failures are bounded visible errors and must
not exit fullscreen TUI. `Esc` clears an active selection before applying normal
idle behavior.
