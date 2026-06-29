---
name: 250. UI Display Model Testing
psychevo_self_edit: deny
---

Define acceptance expectations and validation scenarios for Psychevo's shared
transcript projection contract.

## Long-Term Acceptance Contract

- Committed transcript entries are rebuilt from durable runtime messages ordered
  by `session_seq`.
- Each committed entry preserves message role, stable entry/block identity,
  message sequence, turn identity when known, content order, status, visible
  text, typed metadata, and terminal-answer footer metadata.
- Assistant text, visible reasoning, tool calls, and later tool results keep
  semantic block order without depending on viewport, terminal layout, or
  client-specific row storage.
- Tool-call blocks preserve call metadata and merge later tool-result facts by
  `tool_call_id` without losing arguments, content index, call index, assistant
  message sequence, or result message sequence.
- `write_stdin` observations targeting yielded `exec_command` sessions are
  displayed through the owning exec block, not as primary transcript blocks.
- Live overlay entries are presentation-only. Committed turn entries replace
  optimistic prompt rows and same-turn live overlay material without duplicate
  prompt, reasoning, assistant text, or tool rows.
- Authoritative assistant-segment live updates replace provisional segment
  blocks while preserving earlier non-empty reasoning when final assistant
  content omits reasoning.
- Display-only artifacts, including command feedback, bottom panes, completion
  popovers, `/diff`, diagnostics, launch messages, and local artifacts, do not
  become model-visible messages, session exports, usage/cost accounting, or
  ordinary transcript entries.
- Parsed UI projections from stable tool-result fields, such as `edit.diff`,
  do not replace or mutate the model-visible tool result, and parse failures
  fall back to the original display text.

## Current Implementation Slice

CI/CD vocabulary and generic validation boundaries follow
[065 CI/CD](../065-ci-cd/spec.md).

This topic is implemented across Gateway projection, Workbench transcript
rendering, TUI rendering, and ACP/Web adapters. The default validation path
should use deterministic local harnesses and fake or test providers. Product
surface tests may exercise the shared projection through their own rendering
adapters, but semantic assertions should stay at the shared display-model
boundary when possible.

Manual real-provider smoke validation is allowed only as live opt-in
validation.

## Scenario Matrix

- History reload rebuilds prompt, answer, reasoning, tool call, tool result,
  failure, and terminal-answer metadata from persisted messages.
- Tool execution observations that arrive before final assistant message
  content are reconciled into the same ordered assistant segment once final
  content is known.
- Reasoning deltas before `message_end` remain visible and are completed when
  the assistant segment closes.
- Hidden assistant messages, including `write_stdin` polls, close the current
  assistant segment and do not absorb later reasoning into the wrong segment.
- Snapshot refresh during an active turn removes covered live overlay rows while
  retaining only uncovered active material.
- Reconnect or resume replaces optimistic prompt rows with the committed user
  message for the turn.
- Empty reasoning-completion observations close existing reasoning blocks
  without creating empty Thinking rows.
- Ordinary assistant prose that says a tool will be used does not create a tool
  row unless a typed tool call, execution observation, or message-derived
  tool-result relationship exists.
- Display-only `/diff` or command feedback does not change transcript history,
  model context, export content, or usage statistics.
- Parsed diff projection from a tool result remains display-only: successful
  parsing may add render metadata, while malformed diff content still exposes
  the original result text and does not change transcript facts.
- Child thread creation controls are hidden for draft/no-session states while
  explicit commands return bounded guidance, and opened side/child threads
  render ordinary transcript projection for their own session ids.

## Validation Boundaries

- Tests should compare semantic transcript entries and display invariants, not
  private storage layouts, provider payload shapes, DOM structure, or ratatui
  line buffers.
- Surface-specific visual tests may prove rendered behavior, but they should
  not redefine transcript fact ownership.
- Child thread view tests should assert session ids, parent/child routing, and
  display-only navigation effects rather than client-specific tab DOM or
  terminal row storage.
- Snapshot tests should avoid brittle full-prompt or full-provider-payload
  comparisons unless the tested surface owns that exact text.
