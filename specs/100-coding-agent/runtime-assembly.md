---
name: 100. Runtime Assembly Attachment
psychevo_self_edit: deny
---

Define the first implementation slice contract for assembling the built-in
`coding-agent` capability in `psychevo-runtime`.

This attachment is part of [100 Coding Agent](spec.md). It is not an
independently numbered spec and does not define a stable public Rust API.

## Scope

- first-slice runtime assembly for `coding-agent`
- session create/resume behavior used by the smoke entrypoint
- session create/resume behavior used by the live run entrypoint
- context history selection
- FakeProvider smoke scenario selection
- Chat-compatible live provider assembly
- control signal wiring
- project instruction discovery for AGENTS-named files

Out of scope:
- interactive terminal UI
- non-Chat provider transports, OAuth, or provider login flows
- memory, approvals, third-party assistant instruction globs, legacy
  assistant rules/imports, and implicit legacy memory-file loading

## Assembly Contract

Runtime assembles one accepted coding-agent invocation from:

- a resolved session boundary
- a resolved working directory
- a deterministic `FakeProvider` for smoke or a resolved Chat-compatible
  provider for live run
- the `coding-core` toolset
- an event sink that persists `message_end` events to SQLite
- optional context message limit
- optional stop or abort control mode for smoke validation

If the session cannot be created or resumed, runtime rejects before
`agent_start`. If the working directory or required toolset cannot be assembled,
runtime rejects before `agent_start`.

## Session Selection

The smoke entrypoint supports:

- create when no session id is supplied
- resume by explicit session id

The live run entrypoint supports create, resume by explicit session id, and
product-CLI continue selection of the latest run session for a workdir.

Session ids are UUID v7 text values. Reopening an existing session appends new
messages to the same durable session instead of creating a per-invocation root.

## Context Selection

By default, runtime sends all retained loop-visible session messages to the
provider.

When `max_context_messages` is supplied, runtime keeps the most recent N
loop-visible messages. If this would cut a tool-call/tool-result pair, runtime
expands the retained prefix backward until the visible transcript has complete
tool-call/tool-result relationships.

## Project Instructions

Runtime discovers project instructions from the canonical workdir before each
live coding-agent invocation. The first slice uses `.git` as the project-root
marker. When no `.git` ancestor exists, only the canonical workdir is searched.
When a root exists, runtime searches directories from root to workdir.

For each searched directory, runtime appends non-empty regular files in this
order:

- `AGENTS.md`
- `.psychevo/AGENTS.md`
- `AGENTS.local.md`

Project instructions are injected as typed hidden contextual-user input after
instruction prefix slots and before retained history, selected skill context,
and the current prompt, matching
[006 Prompt Assembly](../006-context-assembly/prompt-assembly.md). They are not
persisted as ordinary transcript messages. Runtime groups all
project-instruction fragments for the prompt into one contextual-user message
with one text block per source fragment. Each block uses the AGENTS context
marker:

```text
# AGENTS.md instructions for <directory>

<INSTRUCTIONS>
...
</INSTRUCTIONS>
```

Provider projection must preserve source message boundaries between the grouped
project-instruction context, selected skill context, and the accepted user
prompt. It may coalesce text blocks within one contextual-user message when a
provider shape requires string-only content, but it must not merge hidden
context with the user's prompt. Runtime persists model-visible project
instruction injections as context evidence with source paths, truncation
metadata, and provider reconstruction group/block ordering.

Runtime does not load legacy assistant memory files, sidecar rule/import
directories, third-party instruction globs, or remote instruction URLs. When a
legacy assistant memory file is present and the corresponding AGENTS file for
that directory is absent, runtime emits a non-fatal warning that suggests
creating an AGENTS-named symlink.

Project instruction content is bounded by a total runtime budget. Content that
exceeds the budget is truncated and marked in the model-visible context.

## Smoke Scenario

The smoke prompt is deterministic:

- omitted prompt becomes `smoke`
- prompt text with no tool names produces a text-only assistant answer
- prompt text containing `read`, `write`, `edit`, or `bash` selects those tools
  in prompt occurrence order
- `read` selection emits two read calls to exercise parallel tool execution

The smoke harness only touches `.psychevo-smoke/` under the workdir. `--reset`
cleans files listed in `.psychevo-smoke/manifest.json` before the run and does
not remove unknown files.

## Live Run Provider

The live run entrypoint uses provider/model resolution from
[120 Provider Registry](../120-provider-registry/spec.md). Runtime receives a
concrete provider configuration and assembles the same coding-agent toolset and
SQLite persistence sink used by smoke.

Live run does not use deterministic fake scripts. It sends caller prompt input
and retained session history to the resolved Chat-compatible provider.

## Control Signals

The smoke entrypoint exposes:

- `none`
- `stop-after-turn`
- `abort-on-agent-start`

`stop-after-turn` finishes the current assistant response and tool batch, then
ends with outcome `stopped` before another model generation. `abort-on-agent-start`
requests abort after `agent_start`; the invocation must end with outcome
`aborted`.

## Related Topics

- [100 Coding Agent](spec.md) defines the built-in capability semantics.
- [006 Prompt Assembly](../006-context-assembly/prompt-assembly.md) defines
  typed prompt slot ordering and stable prefix-cache behavior.
- [120 Provider Registry](../120-provider-registry/spec.md) defines the live
  provider/model resolution contract.
- [200 pevo run](../200-pevo-cli/pevo-run.md) defines the concrete live CLI
  command behavior.
- [110 Tool I/O](../110-coding-core-tools/tool-io.md) defines the first-slice
  coding tool parameter and result contracts.
- [040 SQLite Persistence](../040-storage-and-persistence/sqlite-persistence.md)
  defines the durable session/message shape.
