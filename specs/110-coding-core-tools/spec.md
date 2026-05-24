---
name: 110. Coding Core Tools
psychevo_self_edit: deny
---

Define the required `coding-core` toolset for the built-in coding-agent capability.

## Scope

- `coding-core` toolset semantics
- core coding tools: `read`, `edit`, `write`, `exec_command`, and `write_stdin`
- adjacent core-managed web toolset containing `web_fetch`
- model-visible JSON result contracts for those tools
- working-context and resource-boundary expectations for core tools
- observable failure, truncation, timeout, abort, and conflict behavior

Out of scope:
- tool declaration schemas, parameter names, JSON Schema shapes, Rust APIs, or handler signatures
- provider-specific tool-call fields or wire formats
- CLI commands, terminal rendering, or durable process registry behavior outside
  the in-process exec session model
- approval UX, sandbox behavior, deny lists, dangerous-command policy, or concrete resource policy, except for surfacing permission denial as normal tool-result failure
- binary/image file reading, append/delete/rename tools, dedicated search/list
  tools, memory tools, skill adjunct tools, web search/provider extraction
  tools, or self-evolution tools
- storage schemas, evidence record shapes, or replay formats

## Toolset Contract

`coding-core` is the default toolset required by [100 Coding Agent](../100-coding-agent/spec.md). It directly contains exactly these tools:
- `read`
- `edit`
- `write`
- `exec_command`
- `write_stdin`

`coding-core` does not include dedicated search, list, grep, find, memory,
skill, or project-discovery function tools. A model may use `exec_command` for
command-line search or listing when the runtime resource boundary allows it, but
must prefer dedicated file tools for reads and writes. Optional skill tools are
adjacent runtime tools defined by [055 Skills](../055-skills/spec.md), not
members of `coding-core`.

The built-in `web` toolset is adjacent to `coding-core` and contains the
read-only `web_fetch` tool. Default coding-agent invocations enable both
`coding-core` and `web` unless configuration disables `web`. `web_fetch` is not
a web search or provider-backed extraction tool; it reads a known `http(s)` URL
through the runtime HTTP client. Runtime mode system prompts do not include
dedicated `web_fetch` guidance; its availability and parameter contract are
communicated through the model-visible tool declaration.

When the runtime exposes a Plan-mode coding surface, its core tools are the
non-mutating subset `read`, `exec_command`, and `write_stdin`. Plan mode does
not expose `write` or `edit`; its read-only shell behavior is governed by mode
instructions and the normal permission/resource boundary, not by a separate
runtime command allowlist.

Each `coding-core` tool operates through the runtime-resolved working context accepted for the coding-agent invocation. Tools must not independently choose a different project, filesystem, process environment, or resource boundary.

Runtime must expose a model-visible tool declaration only when the matching execution binding is available for the same agent invocation and generation-request tool declaration snapshot. [007 Tool Surface](../007-tool-surface/spec.md) owns snapshot visibility semantics.

## Toolset Management

Runtime maintains a built-in tool registry and a toolset registry. A toolset may
list tools and include other toolsets. Per-mode configuration selects enabled
toolsets and disabled toolsets; disabled toolsets subtract from enabled
expansion. Plan mode remains a hard read-only ceiling after expansion, so
mutating tools are filtered even when a configured toolset includes them.
Unknown tools, unknown includes, and cycles must be observable during assembly.

## JSON Result Contract

Each `coding-core` tool result is a model-visible JSON object. This is a `110`
contract for these tools only; [007 Tool Surface](../007-tool-surface/spec.md)
does not require all tools to use this result shape.

Each tool uses a top-level `error` field for failure explanation. A missing, null, or empty `error` means the result does not report a failure through that field. Tool-specific failure rules may also use other fields such as `exit_code` or `success`.

Resource denial, resource deferral, permission denial, timeout, size bound, truncation failure, abort, ambiguity, not-found, and conflict conditions must be observable in the JSON result or as before-agent-start rejection. They must not be silently hidden.

Output that may grow large must be bounded. When material is truncated, the result must make truncation observable through a stable field or a clear adjacent result field. This spec does not define concrete byte, line, or time limits.

## `read`

`read` reads text from the working context. It is text-only in this slice.

Successful text reads return a JSON object with stable fields:
- `content`: model-visible text content
- `total_lines`: total line count when known
- `file_size`: file size when known
- `truncated`: whether the returned content is incomplete
- `hint`: optional guidance for reading the next or narrower range
- `error`: failure explanation when the read fails
- `similar_files`: optional candidate paths when the target cannot be found
- `shown_start_line`: first returned line number when known
- `shown_end_line`: last returned line number when known
- `next_offset`: next line offset to continue reading when known
- `output_lines`: number of complete lines returned
- `output_bytes`: byte length of returned content
- `truncated_by`: optional reason the returned content is incomplete
- `first_line_exceeds_limit`: whether the first selected line exceeded the
  byte bound and could not be returned

Binary files and images are not inlined by `read`. They must return an error or clear hint instead of binary or base64 content.

Read ranges, pagination, and truncation are stable behaviors, but this spec
does not freeze parameter names or numeric limits. Runtime may normalize
out-of-range pagination values before reading, but type errors remain failures.

## `write`

`write` creates or completely replaces one target's text content in the working context.

`write` is not append, delete, rename, or patch. It writes the complete intended content for its target.

`write` creates missing parent directories when the resource boundary allows it.

Successful writes return a JSON object with stable fields:
- `path`: target path or equivalent target identifier
- `bytes_written`: number of bytes written when known
- `dirs_created`: whether parent directories were created
- `lint`: optional post-write syntax/lint summary
- `lsp_diagnostics`: optional diagnostics introduced by the write
- `warning`: optional stale-content or partial-read warning
- `error`: failure explanation when the write fails

## `edit`

`edit` modifies existing text content in the working context.

`edit` supports two semantic modes:
- `replace`: replace target text in an existing file or resource
- `patch`: apply a patch to files or resources

The `patch` mode may update, create, delete, or move files or equivalent
resources when the resource boundary and permission policy allow every target.
Patch application must validate all planned operations before writing so
validation failures do not leave partial changes behind.

Successful edits return a JSON object with stable fields:
- `success`: true when the edit was applied
- `diff`: unified diff or equivalent change text
- `files_modified`: modified paths or equivalent target identifiers
- `files_created`: created paths or equivalent target identifiers
- `files_deleted`: deleted paths or equivalent target identifiers
- `files_moved`: moved path pairs or equivalent target identifiers
- `lint`: optional post-edit syntax/lint summary
- `lsp_diagnostics`: optional diagnostics introduced by the edit
- `warning`: optional stale-content or partial-read warning
- `error`: failure explanation when the edit fails

Ambiguous matches, not-found targets, no-change edits, stale content conflicts,
partial-read overwrite risk, sibling-agent write conflicts, and resource-denied
writes must be observable in the JSON result. Same-resource mutations must be
ordered or conflicts must be visible.

This spec defines semantic modes and result material. The first implementation
slice's concrete parameter names and patch syntax are defined by [Tool I/O](tool-io.md).

## `exec_command` and `write_stdin`

`exec_command` runs a bounded shell command through the runtime-bound process
resource for the working context. It supports foreground completion, yielded
long-running sessions, optional PTY execution, stdin-capable sessions, and
bounded model-visible output.

Before starting an agent invocation that exposes `exec_command`, runtime must
ensure an `rg` command is available to that invocation. Resolution checks
`$PSYCHEVO_HOME/tools/rg[.exe]` first, then the inherited system `PATH`. If both
are missing, runtime downloads the latest GitHub ripgrep release for the current
platform and installs it at `$PSYCHEVO_HOME/tools/rg[.exe]`. If this guarantee
cannot be satisfied, the invocation fails before `agent_start` with a clear
observable error. When the managed binary is selected or installed, runtime
prepends `$PSYCHEVO_HOME/tools` to the `PATH` used by `exec_command` subprocesses.
Prompts may guide models to use `jq` for JSON or JSONL inspection when it is
available, but runtime does not guarantee or install `jq`.

`write_stdin` polls an existing yielded session or writes text to its stdin.
Empty `chars` is a poll. Non-empty `chars` requires a stdin-capable session.

Execution sessions are in-process runtime state. They are not durable across
runtime restarts, and the runtime must bound the number of active sessions.
Shell-level background wrappers that escape session tracking, such as trailing
`&`, `nohup`, `disown`, or `setsid`, are rejected with guidance to run the
foreground command and let the runtime yield a session.

Yielded sessions have two observable surfaces:
- provider-visible `exec_command`/`write_stdin` results, which remain the only
  model-visible command output channel
- internal TUI lifecycle events, which may continue after a tool invocation
  returns so the terminal can render true process state

Internal lifecycle events use `exec_session_*` names. When an `exec_command`
returns a non-null `session_id` with `exit_code: null`, runtime emits
`exec_session_yielded`. While the process runs, output readers may emit
`exec_session_output_delta` with the yielded session id, root tool call id,
monotonic sequence, and output text. Non-empty stdin writes emit
`exec_session_stdin`. Process completion or interruption emits
`exec_session_finished` with the yielded session id, root tool call id, exit
code when known, elapsed time, and interruption state. These events are not
part of the provider-visible result schema.

Internal lifecycle events are best-effort UI events for the current runtime
process. They do not make sessions durable across runtime restarts and do not
inject results into model context. If the owning runtime/TUI connection is
detached, an active session may remain alive briefly for reconnect; after a
short timeout it must be cleaned up or terminated.

Successful command and poll results return a JSON object with the strict stable
fields:
- `chunk_id`: monotonically increasing output chunk number for the session
- `wall_time_seconds`: elapsed time spent by the current tool invocation
- `exit_code`: process exit code when the session has completed, otherwise null
- `session_id`: active session id when more polling is possible, otherwise null
- `original_token_count`: token count before output truncation
- `output`: model-visible bounded command output

Normal process exit, including a non-zero exit code, is a successful tool result
and is represented by `exit_code`. Invocation-level failures such as invalid
arguments, command-start failure, unknown session, permission denial, resource
denial, timeout/abort before a yielded session can be returned, or unsupported
stdin are failed tool results.

`max_output_tokens` bounds the model-visible output. When output is truncated,
`original_token_count` remains the count before truncation. The result does not
include a full-output path.

## `web_fetch`

`web_fetch` fetches one fully formed `http://` or `https://` URL. Parameters are
`url`, optional `format` (`markdown`, `text`, or `html`, default `markdown`),
and optional `timeout` in seconds. It follows bounded redirects and reports the
final URL.

The tool is read-only and default-allowed by permission policy, while explicit
`WebFetch(pattern)` allow/ask/deny rules may override it. It does not hard-block
localhost or private addresses in this slice.

Successful text results return stable fields including `url`, `final_url`,
`status`, `content_type`, `format`, `content`, `truncated`, `original_bytes`,
`output_bytes`, and `error`. HTML content is converted to markdown or text when
requested. Output is bounded; downloads larger than the fixed fetch limit fail
or stop before unbounded memory growth.

Image responses return JSON metadata and an image attachment. Providers whose
tool-result channel only accepts text receive the normal text tool result plus a
runtime-originated image context message when the selected model supports image
input; otherwise the tool result contains a visible warning. PDF, archive, and
other unsupported binary responses return structured errors rather than base64
text.

## Attachments

- [Tool I/O](tool-io.md) defines the first implementation slice parameter
  and JSON result contract.
- [Testing](testing.md) defines acceptance scenarios and validation expectations.

## Related Topics

- [100 Coding Agent](../100-coding-agent/spec.md) requires the `coding-core` toolset for default coding-agent invocations.
- [004 Runtime Contract](../004-runtime-contract/spec.md) defines agent-invocation assembly and tool surface wiring.
- [007 Tool Surface](../007-tool-surface/spec.md) defines agent-invocation scoped tool declarations, generation-request tool declaration snapshots, execution bindings, and toolset expansion.
- [009 Resource Surface](../009-resource-surface/spec.md) defines resource decisions that may affect tool execution.
- [035 Permissions](../035-permissions/spec.md) defines the concrete runtime
  permission policy that may deny or defer core tool execution.
- [005 Durable Evidence](../005-durable-evidence/spec.md) defines durable linkage for tool requests, outcomes, and result artifacts.
- [055 Skills](../055-skills/spec.md) defines optional skill adjunct tools.
