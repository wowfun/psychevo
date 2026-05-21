---
name: 110. Coding Core Tools Testing
psychevo_self_edit: deny
---

Define acceptance expectations and validation scenarios for the `coding-core` toolset.

## Long-Term Acceptance Contract

- `coding-core` expands to exactly `read`, `edit`, `write`, `exec_command`,
  and `write_stdin`.
- Each exposed tool declaration has a matching execution binding for the same agent invocation and generation-request tool declaration snapshot.
- Each tool operates only through the runtime-resolved working context and resource surface accepted for the coding-agent invocation.
- Each tool returns model-visible JSON results with the stable fields defined by [110 Coding Core Tools](spec.md).
- Failures are observable through top-level `error` and any tool-specific outcome fields.
- Truncation, timeout, abort, resource denial, ambiguity, not-found, and conflict behavior are observable.
- Permission denial is observable as a normal failed tool result or
  before-agent-start rejection through the owning runtime boundary.

## Current Implementation Slice

Automation vocabulary and generic validation boundaries follow
[060 Automation](../060-automation/spec.md).

There is currently no required validation command for this toolset because this
repository slice is specification-only. When implementation exists, this topic's
default validation path should use deterministic local harnesses and fake or
test providers.

Real provider, real shell, and live service validation remain live opt-in unless
a later implementation provides an isolated deterministic harness for those
resources.

## Scenario Matrix

- Toolset assembly succeeds when all declarations and bindings are available.
- Toolset assembly fails or degrades observably when any required core tool is unavailable.
- Tool declaration snapshot exposure follows [007 Tool Surface](../007-tool-surface/spec.md) while preserving the `110` JSON result contract.
- `read` returns text `content`, `total_lines`, `file_size`, and `truncated` information.
- `read` reports missing targets with `error` and optional `similar_files`.
- `read` refuses binary and image content without returning binary or base64 payloads.
- `write` creates missing parent directories when allowed and returns `path`, `bytes_written`, and `dirs_created`.
- `write` reports resource denial or write failure through `error`.
- `edit.replace` modifies existing content and returns `success`, `diff`, and `files_modified`.
- `edit.patch` may update, create, delete, or move files when every target is
  permitted.
- `edit` reports not-found, ambiguous match, no-change, stale-content conflict, or resource denial through JSON.
- `exec_command` returns strict `chunk_id`, `wall_time_seconds`, `exit_code`,
  `session_id`, `original_token_count`, and `output` fields.
- Non-zero process exits are successful tool results with `exit_code` set.
- `exec_command` yields long-running commands with `session_id`; `write_stdin`
  can poll with empty `chars`.
- `write_stdin` can send stdin to TTY or PTY-fallback sessions and rejects
  non-empty stdin for non-TTY pipe sessions.
- `max_output_tokens` truncates model-visible output while preserving
  `original_token_count`.
- Shell-level background wrappers are rejected; foreground long-running
  commands are allowed to yield.
- PTY fallback prefixes the first output chunk with a short notice and keeps
  stdin writable.
- Start failure, resource denial, permission denial, abort, unknown session,
  unsupported stdin, and output truncation are observable.
- Resource denial, permission denial, boundary failure, or missing working
  context becomes JSON `error` or before-agent-start rejection according to the
  owning boundary.

## Validation Boundaries

- Tests should assert stable behavior and stable result fields, not concrete parameter names unless a later API spec freezes them.
- File and process resources used by tool tests should be scoped to explicit
  fixtures.
- Snapshot or golden-output tests should be limited to intentional stable JSON
  result fields and should not include volatile command output.
