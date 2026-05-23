---
name: 110. Coding Core Tools Testing
psychevo_self_edit: deny
---

Define acceptance expectations and validation scenarios for the `coding-core` toolset.

## Long-Term Acceptance Contract

- `coding-core` expands to exactly `read`, `edit`, `write`, `exec_command`,
  and `write_stdin`.
- Plan-mode core tools expand to `read`, `exec_command`, and `write_stdin`;
  dedicated `list` and `search` tools are not registered, normalized, or
  specially handled.
- The adjacent `web` toolset expands to `web_fetch`; default Plan and Default
  surfaces include it unless disabled by toolset configuration.
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

Default validation should use deterministic local harnesses and fake or test
providers. The broad validation entrypoint is `scripts/validate.sh`; narrower
implementation validation should cover `psychevo-runtime` tool assembly and
exec-session behavior.

Real provider and live service validation remain opt-in. Managed ripgrep tests
must not perform real GitHub downloads; download behavior should be covered with
an injectable fake resolver/client or an equivalent deterministic harness.

## Scenario Matrix

- Toolset assembly succeeds when all declarations and bindings are available.
- Toolset assembly fails or degrades observably when any required core tool is unavailable.
- Plan-mode tool assembly exposes `read`, `exec_command`, and `write_stdin`,
  plus `web_fetch`, while Default-mode tool assembly exposes `read`, `write`,
  `edit`, `exec_command`, `write_stdin`, and `web_fetch`.
- Dedicated `list` and `search` tools do not appear in Plan or Default tool
  declarations, agent tool-name normalization, or TUI tool-evidence special
  cases.
- Built-in and custom toolsets expand `tools` and `includes`, subtract disabled
  toolsets, and report unknown or cyclic definitions.
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
- `rg` resolution prefers a managed `$PSYCHEVO_HOME/tools/rg[.exe]` over a
  system `PATH` binary.
- A system `PATH` `rg` is accepted without attempting download when no managed
  binary exists.
- When managed `rg` is selected, `$PSYCHEVO_HOME/tools` is prepended to the
  `PATH` inherited by `exec_command` subprocesses.
- Missing `rg` plus a failed managed download returns a clear before-agent-start
  error.
- PTY fallback prefixes the first output chunk with a short notice and keeps
  stdin writable.
- Start failure, resource denial, permission denial, abort, unknown session,
  unsupported stdin, and output truncation are observable.
- `web_fetch` fetches a local deterministic HTTP fixture, follows bounded
  redirects, converts HTML to markdown/text/html, truncates bounded output, and
  reports oversized, timeout, and unsupported binary responses.
- `web_fetch` image responses produce metadata plus a tool attachment, and
  provider translation preserves text tool-result ordering while exposing the
  image as model-visible image input when supported.
- `WebFetch(pattern)` permission rules may deny, ask, or allow matching URLs;
  with no matching rule, `web_fetch` is allowed by default.
- Resource denial, permission denial, boundary failure, or missing working
  context becomes JSON `error` or before-agent-start rejection according to the
  owning boundary.

## Validation Boundaries

- Tests should assert stable behavior and stable result fields, not concrete parameter names unless a later API spec freezes them.
- File and process resources used by tool tests should be scoped to explicit
  fixtures.
- Snapshot or golden-output tests should be limited to intentional stable JSON
  result fields and should not include volatile command output.
