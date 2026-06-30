---
name: 140. Hook Runtime Testing
psychevo_self_edit: deny
---

Define acceptance expectations and validation scenarios for runtime-owned hook
execution.

## Long-Term Acceptance Contract

- Agent-declared and plugin-declared hook sources enter the same runtime hook
  execution path.
- Hook declaration parsing never panics on unsupported shapes; malformed
  declarations are skipped with source-qualified diagnostics.
- Canonical Codex-style event matcher groups normalize from profile, project,
  agent, plugin, and managed hook declarations.
- Profile and selected-agent hooks run as trusted configuration; project and
  plugin hooks are skipped until source policy and normalized-hash trust allow
  them.
- Matching trusted handlers for one event occurrence launch concurrently, while
  summaries remain ordered by declaration/display order.
- `PreToolUse` runs before permission/resource checks; rewritten current-call
  input is what permission policy evaluates.
- `PermissionRequest` decisions apply only to the current approval request and
  never persist permission grants.
- `PostLLMCall` preserves raw provider output while allowing display/projected
  reasoning or typed feedback.
- `Notification` payloads expose only the minimum actionable redacted message.
- Hook diagnostics identify event name, source identity, handler/display index,
  status, exit code when available, and bounded stdout/stderr snippets.
- Hook command execution remains deterministic in tests and isolated from real
  user credentials, global config, and host-persistent state.

## Current Implementation Slice

The current implementation slice covers command hooks for tool events. Tests
exercise local command fixtures through the shared hook runtime rather than
testing agent-only wrappers.

Until worker, prompt, and agent hook handlers execute, tests should assert that
those handler types normalize and skip with structured diagnostics rather than
silently running through ad hoc paths.

Manual broad validation for code changes is still the Rust workspace gate
defined by [065 CI/CD](../065-ci-cd/spec.md), but this topic's
acceptance coverage should come from focused hook/runtime tests.

## Scenario Matrix

- Agent-declared `PreToolUse` hooks still run before tool execution.
- Plugin hook source descriptors and agent hook source descriptors produce the
  same hook runtime outcomes for equivalent hook declarations.
- Codex-style `hooks.<Event>[]` matcher groups normalize to stable internal
  records from every accepted hook source.
- Project/plugin hook sources with untrusted or modified hashes are listed but
  skipped before handler execution.
- Multiple matching handlers all start for the same event even when one later
  blocks.
- `PreToolUse` input rewrites affect only the current call and are checked by
  permission policy before execution.
- `PermissionRequest` allow/deny/no-decision results do not change future
  permission state.
- `PreToolUse` exit code `2` blocks the current tool call using stderr, then
  stdout, then the default block reason fallback.
- `PreToolUse` non-zero exit codes other than `2` do not block the tool call
  and emit diagnostics.
- `PostToolUse` hook failures do not alter completed tool output and emit
  diagnostics.
- `PostLLMCall` tests preserve raw provider output while exercising any
  projected/display output path through typed hook results.
- `Notification` fixtures assert redaction instead of snapshotting full
  approval or memory payloads.
- Malformed event maps, command arrays, and command object forms are skipped
  with diagnostics rather than panics.
- Hook payload fixtures include event name, tool name, tool input, cwd, and
  source identity so regressions in hook-facing JSON are visible without
  snapshotting full prompts.
- Hook run summaries are available for diagnostics but are not projected as
  ordinary transcript rows.

## Validation Boundaries

- Tests should assert observable tool-call outcomes and structured hook
  diagnostics, not private process-spawning internals.
- Tests must use local command fixtures and isolated temp directories.
- Real shell hooks are allowed only in deterministic local harnesses and must
  not read user credentials or host-global config.
