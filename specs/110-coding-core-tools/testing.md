---
name: 110. Coding Core Tools Testing
psychevo_self_edit: deny
---

Define acceptance expectations and validation scenarios for the `coding-core` toolset.

## Long-Term Acceptance Contract

- `coding-core` expands to exactly `read`, `edit`, `write`, and `bash`.
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

- Toolset assembly succeeds when all four declarations and bindings are available.
- Toolset assembly fails or degrades observably when any required core tool is unavailable.
- Tool declaration snapshot exposure follows [007 Tool Surface](../007-tool-surface/spec.md) while preserving the `110` JSON result contract.
- `read` returns text `content`, `total_lines`, `file_size`, and `truncated` information.
- `read` reports missing targets with `error` and optional `similar_files`.
- `read` refuses binary and image content without returning binary or base64 payloads.
- `write` creates missing parent directories when allowed and returns `path`, `bytes_written`, and `dirs_created`.
- `write` reports resource denial or write failure through `error`.
- `edit.replace` modifies existing content and returns `success`, `diff`, and `files_modified`.
- `edit.patch` updates existing targets only and rejects creation or deletion in this slice.
- `edit` reports not-found, ambiguous match, no-change, stale-content conflict, or resource denial through JSON.
- `bash` returns bounded `output`, `exit_code`, and `error`.
- `bash` treats non-zero exit codes as failed tool results by default.
- `bash` reports `exit_code_meaning` for the minimum explanation table: grep/rg/ag/ack `1`, diff `1`, test/[ `1`.
- `bash` reports timeout, abort, start failure, resource denial, and output truncation observably.
- `bash` abort and timeout terminate same-process-group children created by
  foreground shell commands, and output collection does not hang on inherited
  pipes left open by descendants.
- `bash` closes command stdin so prompt-style commands observe EOF instead of
  reading from the interactive TUI.
- Resource denial, permission denial, boundary failure, or missing working
  context becomes JSON `error` or before-agent-start rejection according to the
  owning boundary.

## Validation Boundaries

- Tests should assert stable behavior and stable result fields, not concrete parameter names unless a later API spec freezes them.
- File and process resources used by tool tests should be scoped to explicit
  fixtures.
- Snapshot or golden-output tests should be limited to intentional stable JSON
  result fields and should not include volatile command output.
