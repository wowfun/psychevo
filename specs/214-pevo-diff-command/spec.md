---
name: 214. pevo Diff Command
psychevo_self_edit: deny
---

# 214. pevo Diff Command

Define the `/diff` slash command and shared workspace diff model.

## Scope

- local git worktree diff acquisition
- TUI overlay projection
- ACP structured diff projection
- truncation, binary, and unreadable-file behavior

Out of scope:

- staged-only diff display
- session/turn-local diff tracking
- patch approval or edit application

## Diff Semantics

`/diff` is observational UI state. It must not append runtime messages, affect
model context, change exports, or affect usage/cost statistics.

The first implementation matches Codex's worktree behavior:

- verify the cwd is inside a git worktree with
  `git rev-parse --is-inside-work-tree`
- collect tracked changes with `git diff`
- collect untracked files with `git ls-files --others --exclude-standard`
- for each untracked file, use `git diff --no-index -- /dev/null <file>`
- treat git diff exit code `1` as a successful "differences found" result

Staged-only tracked changes are intentionally out of scope for this slice.

Diff output is capped at `256 KiB` or `3000` lines, whichever comes first. A
truncated result must include explicit truncation metadata and a visible
truncation notice. Binary or unreadable files produce a structured placeholder
with path and reason; raw binary bytes must not be embedded.

## TUI

Fullscreen TUI `/diff` opens a Codex-style read-only static overlay pager
titled `D I F F`. The overlay shows no changes for empty diffs, a non-git
message outside git worktrees, and semantic unified diff rendering otherwise.
Esc closes the overlay. Scroll and page keys move through the static snapshot.

The renderer should show file headers, hunk headers, add/delete/context lines,
line numbers, truncation notice, and lightweight syntax highlighting using the
existing terminal highlighter rather than a heavy new dependency.

The deterministic VHS demo must include a screenshot of the `/diff` overlay
against an isolated fixture worktree with a visible changed file, so visual
validation covers overlay framing, title, line numbering, and diff colors.

## ACP

ACP advertises `/diff` when it can project structured diff updates. ACP returns
a synthetic tool-call update whose content uses ACP `ToolCallContent::Diff`.
It must not fall back to a plain assistant text chunk. Summary, truncation, and
binary placeholder metadata may be included in raw output.

## Related Topics

- [026 Commands](../026-commands/spec.md)
- [027 ACP](../027-acp/spec.md)
- [213 pevo Display Model](../213-pevo-display-model/spec.md)
