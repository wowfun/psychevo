---
name: 210. pevo TUI
psychevo_self_edit: deny
---

# 210. pevo TUI Sessions

Define session lifecycle behavior owned by the interactive `pevo tui` surface.

## Session Behavior

Without `--session` or `--new`, TUI resumes the latest `run` or `tui` session
for the canonical working directory. If no matching session exists, the first
submitted prompt creates a new session with `source = "tui"`.

`--session` resumes the requested session. `--new` defers creation until the
first prompt is submitted, then creates a `source = "tui"` session.

When TUI starts with a current session, it loads that session's sanitized
history into the transcript before accepting input. Switching sessions inside
fullscreen TUI replaces the displayed transcript with the selected session's
sanitized history. Folded reasoning remains hidden or folded according to TUI
rendering rules and must not leak provider replay fields. TUI history reload
may restore folded local reasoning into `Thinking: <reasoning>` transcript
evidence, but only from persisted message material that is already marked as
reasoning and never by replaying provider wire fields as visible assistant
text.

Fullscreen composer history is seeded from the current session's persisted user
prompts in session order. Switching sessions replaces that persisted prompt
seed with the selected session's prompts while preserving slash commands and
user shell escapes submitted earlier in the current TUI process. History recall
still preserves the in-progress draft and restores it when the user moves past
the newest history entry.

After a prompt has run, later prompts in the same TUI process append to the
current session explicitly.

TUI sessions have an optional display title. When a new TUI session is created
from a user prompt and the session title is still empty, TUI attempts to
generate a concise title with the selected provider/model by using a
non-persisted, no-tool title request. That title request must not append
messages, tool calls, usage rows, or evidence to the session transcript. If the
title request fails, returns empty text, or returns unusable text, TUI falls
back to a deterministic title derived from the first user prompt. Titles are
trimmed, internal whitespace is collapsed, and stored titles are bounded to 100
characters.

When the first prompt explicitly selects skills with `$skill-name` markers or
`--skill`, title generation receives compact selected-skill context
containing skill names and descriptions, not full skill bodies. Deterministic
fallback titles remove resolved skill markers from the prompt; if no prompt
text remains, fallback uses the selected skill name or names instead of the raw
`$skill-name` marker.

Fullscreen TUI must treat the streamed `agent_end` event as the end of the
interactive turn. Auxiliary work that may happen after `agent_end`, including
new-session title generation, must not keep the composer blocked or cause later
prompts to fail with `a turn is already running`.
After the detached auxiliary run task completes, fullscreen TUI refreshes the
current session title and sidebar from SQLite without adding transcript rows or
re-blocking the composer.

`/rename <title>` updates the current session title. It is available in
fullscreen and non-terminal scripted TUI. Empty titles and rename attempts
without a current session fail with bounded user-visible errors.

Fullscreen `/sessions`, `/resume`, and `/continue` expose active and archived
session views in the shared bottom selection pane. Active sessions are the
default view. Archived sessions are hidden from the default view, from default
TUI startup resume, and from latest-session resolution until restored.
Non-terminal scripted `/sessions`, `/resume`, and `/continue` continue to print
only active sessions.

`Tab` toggles the fullscreen session pane between active and archived views.
Typing still edits the search query. `Ctrl+K` arms a one-shot action mode so
the next key is interpreted as a session-management action instead of search
text. In the active view, action mode accepts `A` to archive the selected
session and `D` to delete it. In the archived view, `Enter` restores and
switches to the selected session, action mode accepts `R` to restore without
changing the action model, and `D` deletes it. Action mode clears after one
action key, `Esc`, selection movement, search edits, or view toggles.

Deleting a session from the fullscreen pane requires confirmation in the pane.
The first delete action for a selected row shows a bounded notice; repeating
the same delete action for the same row performs a hard delete of the session
and retained messages. `Esc`, selection movement, search edits, or view toggles
cancel the pending delete confirmation.

Archiving or deleting the current session clears the displayed transcript,
current-session prompt history seed, current session title, and sidebar session
state, then leaves the TUI in the same new-session pending state as `/new`.
This prevents later prompts from appending to an archived or deleted session.
When a turn is running, archiving or deleting the current session is rejected
with bounded pane feedback instead of interrupting the turn.

## Related Topics

- [210 pevo TUI](spec.md) is the parent topic.
- [210 pevo TUI Testing](testing.md) defines deterministic acceptance coverage.
