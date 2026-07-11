---
name: 210. pevo TUI
psychevo_self_edit: deny
---

# 210. pevo TUI Sessions

Define session lifecycle behavior owned by the interactive `pevo tui` surface.

## Session Behavior

Without `--session` or `--new`, TUI resumes the latest human-visible session
from the local state database, regardless of the directory where TUI was
opened. Internal sessions such as `tui-side-conversation` are excluded. If no matching
session exists, the first submitted prompt creates a new session with
`source = "tui"` for the opened working directory.

`--session` resumes the requested session. `--new` defers creation until the
first prompt is submitted, then creates a `source = "tui"` session.

When TUI starts with a current session, it loads that session's sanitized
history into the transcript before accepting input. If that session belongs to
a different stored cwd than the launch directory, TUI switches its active
cwd, Gateway source, project context, file/completion scope, sidebar state,
and subsequent turn options to the stored cwd. Switching sessions inside
fullscreen TUI follows the same rule, then replaces the displayed transcript
with the selected session's sanitized history. Folded reasoning remains hidden
or folded according to TUI rendering rules and must not leak provider replay
fields. TUI history reload may restore folded local reasoning into
`Thinking: <reasoning>` transcript evidence, but only from persisted message
material that is already marked as reasoning and never by replaying provider
wire fields as visible assistant text.

When history contains an assistant tool-call message without persisted tool
results, TUI may keep those tool rows live when the current TUI process owns
the matching running turn or when shared Gateway activity reports a non-stale
foreign owner for that session. After a process restart or any other
history-only reload without either local ownership or valid durable ownership,
those orphaned tool calls render as interrupted historical evidence with no
spinner or live elapsed timer. TUI does not automatically retry or resume those
tool calls.
If the orphaned call was an `Agent` call and persisted agent-edge metadata can
identify the child session, the row may still expose the child `Open` action,
but it remains an interrupted historical row rather than a running spinner.

Fullscreen TUI may switch sessions while a turn is still running. Streamed
events, status-line running state, queued input ownership, and `Esc`
interruption are scoped to the session that owns the running turn. Events for a
non-visible session must not append rows, thinking, tool output, errors, or
status notices to the currently displayed transcript. Returning to a still-live
session reloads its persisted history and replays only that session's buffered
live events. `/new` leaves no visible session selected, so later output from a
previous running session must not appear in the new-session transcript.
Typed Gateway events for a child thread are retained with the same bounded,
ordered semantics as raw scoped runtime events, including updates received
while that child is visible. Opening or reopening the child while its turn is
running replays the retained events through the ordinary Gateway transcript
reducer before future child updates continue from the detached parent turn.
Terminal completion clears the live backlog because persisted history becomes
authoritative.

When `/new` is entered while the current turn is still running, TUI starts a
new draft lane for the next prompt instead of queueing it behind or steering
the previous turn. The previous turn continues in the background, and its later
session id discovery, completion, permission, clarify, and live transcript
events must not steal the empty draft, change `current_session`, or bind the
new draft back to the previous session. The draft lane is internal routing
state only; it must not appear in session rows, search text, grouping, titles,
or persisted transcript content.

Opening, resuming, selecting, or viewing a session is read-only for session
recency. These operations must not update the session's latest-activity time,
ended state, archive state, messages, usage rows, or evidence. Persisting new
loop-visible transcript material is session activity: it updates the
latest-activity time and reopens the session by clearing ended/archive state.

Fullscreen composer history is seeded from the current session's persisted user
prompts in session order. Switching sessions replaces that persisted prompt
seed with the selected session's prompts while preserving slash commands and
user shell escapes submitted earlier in the current TUI process. History recall
still preserves the in-progress draft and restores it when the user moves past
the newest history entry.

After a prompt has run, later prompts in the same TUI process append to the
current session explicitly.

TUI sessions use the shared human-visible session title contract defined by the
Gateway spec. When a new top-level TUI session is created from a user prompt and
the session title is still empty, runtime attempts to generate a concise title
with `auxiliary.title_generation` when configured, falling back to the selected
provider/model, by using a non-persisted, no-tool title request. That title
request must not append messages, tool calls, usage rows, or evidence to the
session transcript. If the title request fails, returns empty text, or returns
unusable text, runtime falls back to a deterministic title derived from the first
user prompt. Titles are trimmed, internal whitespace is collapsed, and stored
titles are bounded to 100 characters.

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
global session views in the shared bottom selection pane. Active sessions are
the default view. Archived sessions are hidden from the default view, from
default TUI startup resume, and from latest-session resolution until restored.
Rows show compact project/cwd context and are searchable by session id,
title, project, cwd, provider, and model. Runtime `source` remains an
internal classification, not a user-facing search, grouping, or visibility
boundary. Non-terminal scripted `/sessions`, `/resume`, and `/continue`
continue to print only active sessions, using the same global human-visible
list.

The fullscreen session pane uses the shared session-browser defaults. For each
workspace it initially shows sessions updated within the last 7 days, capped to
20 rows, while keeping the current session and any running session visible even
outside that window. Older rows are represented by a selectable
workspace-scoped `older sessions` row; pressing Enter expands the next 20 rows
for that workspace. Expanding, searching, and selecting sessions are read-only
and must not update persisted recency.

When the fullscreen session pane opens and the current visible session appears
in the active view, the selected-row arrow defaults to that current session
instead of the first latest-activity row. If no current session appears in the
opened view, selection falls back to the first visible row.

In the fullscreen session pane, rows for sessions that still own background
work after the user switches away render a low-intensity live marker using the
shared `activity_spinner_frame(elapsed)` motion primitive. The marker is part
of the row's existing leading state area, so it must not add a second line,
change row height, change click targets, or rewrite persisted session recency.
The current-session marker remains reserved for the visible session; when a
visible session is also running, the fixed bottom status line remains the
primary running indicator.

Fullscreen TUI tool rows render elapsed duration on the right side of the row
header only once the active or persisted tool duration reaches 1 second.
Sub-second tool durations are still kept in row state and persistence metadata
but are not shown as a `0s` right-side label. The fixed bottom turn-status
timer may still show `0s` while a turn has been running for less than 1 second.

TUI publishes Gateway activity ownership while it runs turns and shell
activities. The ownership record includes an owner id, source key, turn id,
lease, generation, and active timestamps so other Gateway processes can show
running state, relay live transcript events, and route thread-scoped controls.
If another Gateway takes over a stale activity generation, late events from the
old TUI owner must be ignored rather than rendered into the current transcript
or clearing the new owner.

TUI also observes Gateway activity ownership published by other surfaces. When
the current visible session has a non-stale foreign activity, TUI treats that
activity as the session's live owner for history reload, bottom running status,
tool-row timers, and thread-scoped interrupt routing. TUI replays retained
foreign `gateway_live_events` for the visible session and polls later events by
monotonic sequence; events for unrelated sessions must not mutate the current
transcript or session recency. Foreign live events are display overlays only:
committed runtime messages remain the transcript source of truth and replace
live overlays when the turn completes.

Both active and archived session views are ordered latest-activity-first by the
persisted session latest-activity time. Restoring an archived session exposes it
in the active view at its existing activity position; it does not make the
session latest unless new transcript material is later appended.

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

- [Spec](spec.md) is the parent topic.
- [Testing](testing.md) defines deterministic acceptance coverage.
