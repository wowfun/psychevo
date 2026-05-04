---
name: 110. Tool I/O Attachment
psychevo_self_edit: deny
---

Define the first implementation slice parameter and result contract for the
`coding-core` tools.

This attachment is part of [110 Coding Core Tools](spec.md). It is not an
independently numbered spec and does not define a stable public Rust API.

## Scope

- first-slice parameter names
- JSON result fields
- path containment behavior
- default bounds and timeout values
- edit and patch matching behavior

Out of scope:
- interactive approvals
- background processes or PTY behavior
- binary/image file reading
- search/list tools

## Common Rules

All four tools use strict required-field and type validation. Unknown input
fields are ignored.

Path inputs may be relative or absolute. Runtime resolves them against the
accepted working directory, canonicalizes filesystem access, and denies any
target that escapes the working directory, including symlink escape. Model-
visible paths in successful results are working-directory relative.

All failures return a JSON object with a non-empty top-level `error` field.
Tool failures become failed tool outcomes but do not automatically fail the
agent invocation.

## `read`

Parameters:

- `path`: string, required
- `offset`: integer, optional, 1-based start line
- `limit`: integer, optional, maximum number of lines

Defaults and bounds:

- reads are text-only
- output is head-truncated at 50KB or 2000 lines
- missing files include `similar_files`, initially an empty array

Successful result fields:

- `path`
- `content`
- `total_lines`
- `file_size`
- `truncated`
- `hint`
- `error`
- `similar_files`

Binary files and invalid UTF-8 return JSON errors.

## `write`

Parameters:

- `path`: string, required
- `content`: string, required

`write` creates missing parent directories when contained in the working
directory and completely replaces the target file.

Successful result fields:

- `path`
- `bytes_written`
- `dirs_created`
- `error`

## `edit`

Parameters:

- `mode`: string, optional, defaults to `replace`

For `replace`:

- `path`: string, required
- `edits`: non-empty array of `{ oldText, newText }`

All `oldText` values are matched against the original file, not incrementally.
Each `oldText` must occur exactly once and edits must not overlap. No-change,
ambiguous, missing, stale, or conflicting edits return JSON errors.

Replacement matching strips an initial BOM from matching material, normalizes
line endings to LF for matching, and writes back using the original file's
dominant line ending.

For `patch`:

- `patch`: string, required unified diff

Patch mode may update multiple existing files. It rejects file creation,
deletion, rename, `/dev/null`, and paths outside the working directory. Hunks
apply by matching old hunk content uniquely in the current file.

Successful result fields:

- `success`
- `diff`
- `files_modified`
- `error`

Diffs are unified diffs generated from original and updated text.

## `bash`

Parameters:

- `command`: string, required
- `timeout`: number of seconds, optional

Execution uses `bash -lc` in the accepted working directory. The default
timeout is 120 seconds; the maximum accepted timeout is 300 seconds.

stdout and stderr are merged into one tail-bounded `output` string. Output is
tail-truncated at 50KB or 2000 lines. On timeout, `exit_code` is `null`.

Successful or failed result fields:

- `output`
- `exit_code`
- `error`
- `exit_code_meaning`
- `truncated`

Exit code 0 is success unless another runtime boundary reports failure.
Non-zero exit codes are failed tool results. The first explanation table covers
exit code 1 for `grep`, `rg`, `ag`, `ack`, `diff`, `test`, and `[`.

## Related Topics

- [110 Coding Core Tools](spec.md) defines the semantic toolset contract.
- [100 Runtime Assembly](../100-coding-agent/runtime-assembly.md) defines how
  these tools are assembled for smoke.
