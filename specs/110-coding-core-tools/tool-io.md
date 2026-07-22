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
- in-process exec session lifecycle events used by local UI surfaces

Out of scope:
- interactive approvals
- durable background processes across runtime restart
- binary/image file reading
- dedicated search/list function tools

## Common Rules

All coding-core tools use strict required-field and type validation. Unknown input
fields are ignored.

Path inputs may be relative or absolute. Runtime resolves relative inputs
against the accepted working directory and uses the canonical filesystem
identity defined by [041 Permissions](../041-permissions/spec.md). Existing
targets follow symlinks/junctions; missing targets canonicalize the deepest
existing ancestor. Reads may address any permitted host path. Writes outside
the canonical workspace are suspended for harness approval rather than treated
as malformed. Model-visible result paths are working-directory relative when
the canonical target is inside cwd and absolute otherwise.

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
- `offset` values below 1 are normalized to 1
- `limit` values below 1 are normalized to 1
- `limit` values above 2000 are normalized to 2000
- missing files include a bounded `similar_files` array when nearby candidates
  can be found inside the working directory

Successful result fields:

- `path`
- `content`
- `total_lines`
- `file_size`
- `truncated`
- `hint`
- `error`
- `similar_files`
- `shown_start_line`
- `shown_end_line`
- `next_offset`
- `output_lines`
- `output_bytes`
- `truncated_by`
- `first_line_exceeds_limit`

Binary files and invalid UTF-8 return JSON errors.

## `write`

Parameters:

- `path`: string, required
- `content`: string, required

`write` creates missing parent directories when authorized by the active
workspace, profile, or temporary directory grant and completely replaces the
target UTF-8 text file. It should be used instead of shell redirection for
complete-file writes.

When the target does not exist, `write` commits through no-clobber creation and
fails if the target appears before commit. When the target exists, the current
task must have completed an untruncated `read` of that version or successfully
written that version earlier in the same runtime process. Runtime compares the
recorded modification time and in-process writer sequence immediately before
commit. Missing, partial, stale, or sibling-invalidated evidence is a failed
tool outcome and does not modify the file.

Existing targets retain their UTF-8 BOM, dominant line-ending style, and file
permissions. File replacement is atomically visible through a prepared
same-directory temporary file but is not required to synchronize file or
directory data for power-loss durability. External changes that preserve mtime
and do not participate in the writer sequence are outside this conflict check.

Successful result fields:

- `path`
- `bytes_written`
- `dirs_created`
- `lint`
- `lsp_diagnostics`
- `error`

`lsp_diagnostics` is `null` when no introduced diagnostics are available. A
missing, installing, broken, or timed-out language server is reported only
through internal runtime status events, not through additional model-visible
result fields.

## `edit`

Parameters:

- `mode`: string, optional, defaults to `replace`

For `replace`:

- `path`: string, required
- `old_string`: string, required
- `new_string`: string, required
- `replace_all`: boolean, optional, defaults to `false`

`old_string` must be non-empty and must differ from `new_string`. With
`replace_all=false`, matching must resolve to exactly one location; ambiguous,
missing, stale, partial-read, or conflicting edits return failed JSON results.
With `replace_all=true`, every accepted matched occurrence is replaced.

Replacement matching strips an initial BOM from matching material, normalizes
line endings to LF for matching, and writes back using the original file's
dominant line ending. Matching tries exact text first, then bounded fuzzy
strategies for trimmed lines, whitespace, indentation, escaped newlines/tabs,
trimmed boundaries, Unicode quote/dash/space normalization, anchored blocks,
and context-aware line similarity. Exact matching and the first six fuzzy
strategies may commit after preserving target indentation and Unicode and after
rejecting transport escape drift. Anchored-block and context-aware matches are
candidate-only: they return a failed result stating that no mutation occurred,
the strategy used, and a bounded set of 1-based candidate line ranges. The task
must read and retry with more precise `old_string` material.

Unicode preservation applies only to complete normalized character expansions.
For example, an equal segment that covers both hyphens normalized from one em
dash may restore that dash, while a segment retaining only one hyphen must emit
one hyphen. The same rule applies to shortening the three-dot expansion of an
ellipsis.

Replace mode snapshots the target modification time before matching and checks
it again immediately before atomically visible replacement. A changed mtime is
a hard conflict. A same-mtime external content change remains outside the
contract unless it participates in the in-process writer sequence.

For `patch`:

- `patch`: string, required V4A patch text

Patch mode accepts V4A patch text with `*** Begin Patch`, `*** Update File`,
`*** Add File`, `*** Delete File`, and `*** End Patch` markers. Move markers are
rejected. It may update, create, or delete multiple files. Runtime validates all
operations and rejects duplicate or overlapping canonical targets before
applying them. Update hunks apply through the same matching tiers used by
replace mode. Delete requires complete current-task read evidence. Canonical
external mutation targets require the same harness approval as `write` and
replace mode.

Validated operations apply sequentially without rollback. Add uses no-clobber
creation; Update and Delete compare their validated mtime immediately before
commit. When a later operation fails, earlier committed operations remain and
the failed result contains their `diff` and file lists plus
`failed_operation: { index, kind, path }`, where `index` is 1-based. The
model-facing result uses a compact failure summary naming the failed operation
and the paths that remain committed. That summary also includes a bounded form
of the concrete `error`, including any safe retry or no-clobber guidance.

Successful result fields:

- `success`
- `diff`
- `files_modified`
- `files_created`
- `files_deleted`
- `lint`
- `lsp_diagnostics`
- `error`

Diffs are Git patch blocks. Update, add, and delete operations include
`diff --git`, file headers, and unified hunks.

`lsp_diagnostics` follows the same best-effort rule as `write`: it is populated
only when the runtime has introduced diagnostics ready for the edited files,
and language-server status is not exposed by adding model-visible fields.

## LSP Runtime Behavior

The default LSP config is enabled with `wait_mode = "document"`,
`wait_timeout = 5.0`, and `install_strategy = "auto"`.

`install_strategy = "manual"` resolves only existing language-server binaries.
`install_strategy = "off"` skips managed language-server installation and
diagnostics. `install_strategy = "auto"` never shells out through ephemeral
package runners from the `write` or `edit` hot path. If an npm-backed language
server is missing, runtime may start one background managed install under
`$PSYCHEVO_HOME/lsp/node` and return the current tool result without waiting for
that install.

Supported npm-backed managed language servers in this slice are:

- Python: npm package `pyright`, executable `pyright-langserver --stdio`
- JavaScript/TypeScript: npm package `typescript-language-server`, executable
  `typescript-language-server --stdio`
- YAML: npm package `yaml-language-server`, executable
  `yaml-language-server --stdio`

Language-server status events are internal `RunStreamEvent::Event` payloads
with `type = "lsp_status"`. They are intended for local UI and diagnostic
surfaces and are not included in provider-visible tool result JSON.

## `exec_command`

Parameters:

- `cmd`: string, required
- `cwd`: string, optional; relative paths resolve against the accepted
  cwd, absolute paths must pass permission/resource gates
- `shell`: string, optional; defaults to the user's shell
- `tty`: boolean, optional, default `false`
- `yield_time_ms`: integer, optional, default `10000`, clamped to
  `250..30000`
- `max_output_tokens`: integer, optional, default `10000`
- `login`: boolean, optional, default `false`; requires explicit permission
  config when true

The tool runs bounded shell commands. Models should use `read`, `write`, and
`edit` for file I/O instead of shell `cat`, redirection, or patching commands.
For text search, models should prefer `rg`; for project file listing, models
should prefer `rg --files`. For JSON or JSONL inspection, models may use `jq`
when available, but runtime does not guarantee or install `jq`. Runtime ensures
`rg` is available before an agent invocation that exposes `exec_command` starts,
using the managed
`$PSYCHEVO_HOME/tools/rg[.exe]` binary first, then system `PATH`, then a managed
latest-release ripgrep install. If the managed binary is used, its tools
directory is prepended to the subprocess `PATH`.
If a command is still running after the yield window, the result includes a
`session_id` and can be continued with `write_stdin`.

Runtime passes the effective run environment to `exec_command` subprocesses.
Managed tool path prefixes are prepended to that environment's `PATH` before
launch. On native Windows Git Bash, runtime sets UTF-8 child-process defaults
only when unset, including `PYTHONUTF8=1`, `PYTHONIOENCODING=utf-8`,
`LANG=C.UTF-8`, `LC_ALL=C.UTF-8`, and `LC_CTYPE=C.UTF-8`. Model-visible
stdout/stderr text is decoded as UTF-8 with Windows legacy locale fallback for
non-UTF-8 output.

`tty=false` uses pipes and closes stdin. `tty=true` uses a PTY and keeps stdin
writable. If the PTY backend is unavailable, execution falls back to pipe mode,
keeps stdin writable, and prefixes the first output chunk with a short fallback
notice.

## `write_stdin`

Parameters:

- `session_id`: integer, required
- `chars`: string, optional, default `""`; empty means poll
- `yield_time_ms`: integer, optional, default `250`; non-empty input clamps to
  `250..30000`, empty poll clamps to `5000..300000`
- `max_output_tokens`: integer, optional, default `10000`

Non-empty `chars` writes to the session stdin. Sessions started without TTY and
without PTY fallback reject non-empty stdin writes.

Both exec tools return strict result fields:

- `chunk_id`
- `wall_time_seconds`
- `exit_code`
- `session_id`
- `original_token_count`
- `output`

Normal non-zero process exits are successful tool results with `exit_code` set.
Invocation failures are failed tool results. Output is bounded by
`max_output_tokens`; no full-output temp path is returned.

Runtime may also emit internal TUI-only lifecycle events for yielded exec
sessions. These events never add fields to the model-visible result object:

- `exec_session_yielded`: emitted when `exec_command` returns a non-null
  `session_id` and null `exit_code`.
- `exec_session_output_delta`: emitted from background readers as new output is
  appended; carries `session_id`, root `tool_call_id`, `seq`, and `output`.
- `exec_session_stdin`: emitted for non-empty stdin writes; carries
  `session_id`, the `write_stdin` tool call id, and bounded `chars`.
- `exec_session_finished`: emitted when the process exits or is interrupted;
  carries `session_id`, root `tool_call_id`, `exit_code`, `elapsed_ms`, and
  `interrupted`.

Empty `write_stdin` polls remain model-visible tool calls but should not be
rendered as separate primary transcript rows in fullscreen TUI when they can be
associated with an existing exec session. This includes suppressing provisional
rows created from streamed tool-call arguments before the `write_stdin` call
executes. The associated `exec_command` row owns the visible running state and
output.

## `web_fetch`

Parameters:

- `url`: string, required, must start with `http://` or `https://`
- `format`: string, optional, one of `markdown`, `text`, or `html`, default
  `markdown`
- `timeout`: number, optional seconds, default `30`, clamped to `1..120`

The tool uses the runtime HTTP client, follows bounded redirects, and returns a
JSON object. Text and HTML responses are fetched up to 5MB. Model-visible
`content` is bounded after conversion and includes `truncated`, `original_bytes`,
and `output_bytes`.

Successful text result fields:

- `url`
- `final_url`
- `status`
- `content_type`
- `format`
- `content`
- `truncated`
- `original_bytes`
- `output_bytes`
- `error`

HTML responses convert to markdown for `format=markdown`, plain text for
`format=text`, and raw HTML for `format=html`. Non-HTML text is returned as-is
for `markdown` and `text`.

Image responses return metadata and an `attachments` array with data URL image
content. Unsupported binary responses return JSON errors instead of base64 text.

## Related Topics

- [110 Coding Core Tools](spec.md) defines the semantic toolset contract.
- [100 Runtime Assembly](../100-coding-agent/runtime-assembly.md) defines how
  these tools are assembled for smoke.
