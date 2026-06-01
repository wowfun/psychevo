---
name: 006. Prompt Assembly Attachment
psychevo_self_edit: deny
---

Define the runtime-owned prompt assembly contract for model-visible instruction
slots, contextual-user placement, stable prefix snapshots, and provider-role
fallback.

This attachment is part of [006 Context Assembly](spec.md). It is not an
independently numbered spec and does not define exact prompt wording.

## Scope

- typed prompt slots and their semantic order
- session-stable prefix snapshot boundaries
- selected main-agent prompt specialization
- provider-role fallback for developer-policy slots
- reload and invalidation triggers for prompt prefixes
- context usage category accounting for assembled prompt material
- redaction boundaries for prefix export/share metadata

Out of scope:

- byte-for-byte prompt text
- provider-specific wire payload fields beyond semantic role fallback
- tool schema contents
- prompt-cache billing policy
- legacy compatibility with external products

## Typed Slots

Runtime assembles model-visible prompt material as typed slots before lowering
them into provider messages. A slot records:

- `slot`: stable slot name
- `tier`: `base`, `prefix`, or `turn`
- semantic role: runtime meaning such as base policy, developer policy,
  contextual-user project context, history, turn context, or current prompt
- provider role: the role emitted for the selected provider/model after
  capability fallback
- order: total order inside its tier
- content hash
- source metadata: source kind, source name, and source path when available

Exact Rust type names and storage payloads are implementation details unless a
public API explicitly exposes them.

## Assembly Order

For one accepted main-session turn, runtime assembles model context in this
normative order:

1. base/mode
2. runtime environment
3. selected main agent
4. agent catalog
5. skill index
6. AGENTS/project context
7. history
8. turn-scoped hints and selected skill body
9. current prompt

`base/mode`, runtime environment, selected main agent, agent catalog, skill
index, and AGENTS/project context are instruction slots. Runtime environment
context identifies the canonical workdir and path-resolution boundary for model
planning; it is not a permission grant. AGENTS/project context is policy context
rather than user task input and is placed before retained history to keep the
prefix stable. Selected skill bodies and required `@agent` call hints are
turn-scoped and appear after history and before the current prompt.

Provider projection must preserve the semantic separation between instruction
slots, contextual-user project context, retained transcript history,
turn-scoped contextual-user input, and the user's current prompt even when a
provider requires coalesced message content.

## Selected Main Agent

A selected main agent is a developer-policy specialization layer. Its slot
contains:

- selected identity
- definition source and path when known
- purpose derived from the agent `description`
- instruction body derived from the agent file body
- an explicit reminder that mode, tool policy, safety rules, resource gates,
  and direct user constraints still take precedence when stricter

The selected-agent slot specializes the invocation; it does not replace the
runtime mode slot and does not bypass tool, safety, session, context, or user
constraints. Child agents use the same identity/purpose/body construction with
child-run control guidance owned by the subagent runtime. Child-agent sessions
persist their own prefix snapshot for the actual child invocation, including
the selected child-agent identity/body and child-run control slot.

When the effective tool surface for an invocation is empty, the base/mode slot
uses a minimal no-tools instruction instead of the normal coding-mode wording.
It must not claim read, write, edit, shell, agent, or skill capability.

## Prefix Snapshot

Runtime persists the latest session prefix snapshot keyed by session id. The
snapshot stores the complete slot content for the session-stable prefix plus:

- prefix hash
- tool declaration hash
- provider and model
- snapshot version
- created time
- invalidation reason

Only the latest full prefix snapshot is retained. If a prior turn used an
older prefix and the full old snapshot is no longer available, export/share
surfaces must mark that prefix as approximate or unavailable instead of
silently applying the latest full text.

Runtime lazily creates a missing prefix snapshot on the next accepted turn
using the current config and files. Current sessions do not automatically
refresh when workdir context, AGENTS, agent, or skill files change. The stable
prefix is rebuilt only by:

- TUI `/refresh`
- non-TUI `/reload-context`
- `pevo session reload-context <id|latest>`
- starting a new session
- switching the selected main agent

Reload and selected-main-agent switching are rejected while a turn is running.
Switching the selected main agent in an existing session rebuilds the prefix,
records cache invalidation, and injects a one-turn developer notice before the
next prompt.

## Provider Role Fallback

Developer-policy slots use semantic role `developer`. Runtime emits provider
role `developer` only when the resolved model metadata declares
`capabilities.developer_role = true`.

When `developer_role` is false or unknown, developer-policy slots fall back to
provider role `system`. This fallback changes only the provider role; the
semantic role and slot accounting remain developer-policy material.

AGENTS/project context uses the same provider-role fallback as
developer-policy slots. Selected skill bodies and required `@agent` call hints
remain turn-scoped and are not part of the session-stable prefix.

Runtime environment uses the same developer-policy provider-role fallback as
other runtime-owned instruction slots.

## Usage Categories

Context usage projections use these top-level categories:

- `base_policy`
- `developer_prompt`
- `project_context`
- `history`
- `turn_context`
- `current_prompt`
- `system_tools`
- `free_space`

Selected-agent text is counted as `developer_prompt`, not as skills. Skill
index entries and AGENTS/project instructions are counted as
`developer_prompt`; runtime environment is counted as `base_policy`; selected
skill body text is counted as `turn_context`.

## Export And Share

Default export/share header output exposes prefix slot names, hashes, source
metadata, prompt-prefix metadata, provider-role fallback, and stale or
approximate markers only. Full hidden prefix text is included only by explicit
full-input or last-provider-request options that already warn about
hidden prompt disclosure. `last-provider-request` reconstructs hidden prefix
messages and tools from the persisted snapshot only when the corresponding user
prompt's recorded prefix hash matches the latest retained snapshot.
Assistant-turn prefix metadata is only a fallback for older records. Otherwise
the export must mark the request as approximate or unavailable instead of
silently applying a newer prefix.
