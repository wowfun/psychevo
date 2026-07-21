---
name: 270. UI Interaction
psychevo_self_edit: deny
---

# 270. UI Interaction

Define cross-surface interaction semantics for Psychevo composers, command
routing, permission/clarify requests, active-turn controls, and display-only
feedback.

## Scope

- composer-first interaction semantics shared by TUI and Web/Workbench
- prompt submission, steer, queue, shell mode, attachment, and restored-draft
  behavior at the UI contract level
- shared command discovery/execution result routing into panels, overlays,
  previews, status, composer state, downloads, and display-only feedback
- permission and clarify request presentation, response routing, stale-request
  handling, and interruption cleanup
- undo/redo and interrupt interaction invariants shared across product
  surfaces
- display-only mutation receipts and destructive-action confirmation semantics

Out of scope:

- exact TUI key chords, mouse routing, alternate-screen panes, terminal
  selection, slash aliases, and terminal clipboard behavior; these belong to
  [210 pevo TUI](../210-pevo-tui/spec.md)
- exact Workbench DOM layout, menus, popovers, responsive panels, and browser
  host actions; these belong to [240 pevo Web](../240-pevo-web/spec.md)
- command catalog semantics independent of UI projection; these belong to
  [026 Commands](../026-commands/spec.md)
- permission policy and sandbox grant semantics; these belong to
  [041 Permissions](../041-permissions/spec.md) and
  [045 Sandbox](../045-sandbox/spec.md)

## Composer And Active Turns

Product surfaces are composer-first: ordinary user input starts from the
composer, and panels or overlays stay anchored to that flow unless a concrete
product spec owns a different layout.

Plain prompt submission during an active agent turn is a steer request when the
surface/runtime supports steering. Queueing is an explicit next-turn action and
must remain distinguishable from steer. If submitted input is pending but not
yet committed to transcript history, the UI may show a pending preview, but
that preview is display-only and must be replaced by committed transcript
entries when they arrive.

`/steer <message>` is the explicit active-turn steer form. When idle, inside
non-agent work, or inside an unavailable target, it reports bounded feedback
and does not queue implicitly. `/queue <message>` appends a prompt to the
caller-owned next-turn FIFO queue. Pending preview controls may edit or cancel
not-yet-started queued prompts and not-yet-committed steer inputs, but they
must not rewrite already committed transcript history.

Shell mode is an explicit composer state, not a model tool request. Web and TUI
may expose different entry gestures, but the visible shell command and bounded
result follow the shared runtime shell-context contract and remain distinct
from model-initiated `exec_command` tool calls.

Attachments are controlled composer state. Image and file attachment entry may
vary by host, but the submitted prompt text remains the user-visible source and
attachment metadata must not become ordinary transcript content unless a domain
spec defines that projection.

Undo/redo interactions must keep transcript state, file/workspace state,
context/observability, and composer continuity aligned. When undo restores
prompt text, the surface returns that text to the active composer rather than
leaving it only in command feedback.

## Commands And Feedback

UI command discovery is backed by the shared command catalog. Concrete surfaces
filter or route commands according to their capabilities, but should not carry
separate hard-coded inventories when a shared catalog and host action can
describe the command.

Command results are applied by destination:

- transcript commands update the current thread through normal turn flow
- panels and overlays reveal their destination without adding ordinary
  transcript rows
- status, context, and usage commands focus observability surfaces
- diff and preview commands open display artifacts
- export and share commands use host download/share paths
- unsupported or stale results become bounded display-only feedback

Display-only command feedback is scoped to the current session/cwd and is
cleared on session switches, new input, or product-specific dismissal. Command
feedback must not count as user prompt text, visible assistant message,
durable session message, or provider-context input.

User-initiated committed mutations may publish a transient ledger receipt.
Navigation, selection, copying, uncommitted draft edits, and ordinary turn
submission do not. A receipt may offer Undo only when the surface has a reliable
inverse operation; invoking it keeps the receipt pending until the inverse
settles. Receipt state is display-only and follows the same persistence,
export, accounting, and provider-context exclusions as command feedback.
Diagnostics, health checks, import inspection, and connection-start operations
are command feedback rather than committed mutations and must not publish a
default mutation receipt.

Reversible organization actions such as archive, restore, pin, unpin, or
locally reconstructable mapping removal execute immediately with Undo.
Irreversible deletes and remote configuration removal require a product-owned
confirmation dialog. Native host confirmation dialogs are not part of the UI
contract. Confirmation begins on Cancel, never treats Enter as implicit
approval, and cannot be dismissed while its mutation is pending. The
confirmation surface owns that asynchronous mutation lifecycle: it remains
visible, marks the confirm action pending, and disables every dismissal path
until the operation settles. Product modal dialogs do not dismiss from backdrop
presses; cancellation is always an explicit control or an enabled Escape path.

Shared application roots own the interaction providers needed to implement
confirmation and mutation receipts. Browser, Desktop, tests, and other hosts
that render the exported application root must receive the same interaction
semantics without independently wrapping host entrypoints.

`/diff` is backed by the shared command catalog and
[214 pevo Diff Command](../214-pevo-diff-command/spec.md). Executing it opens a
structured display artifact rather than appending ordinary transcript rows.

`/btw [prompt]` follows the shared `Side chat` behavior in
[250 Thread Navigation](../250-ui-display-model/thread-navigation.md). Surfaces
may open it as a child tab, split view, or entered view, but it must not add a
command transcript row to the parent. If an inline prompt is supplied, the
surface opens the side thread before submitting the prompt through the ordinary
thread composer/reconciliation path.

## Permission, Clarify, And Interrupt

Permission and clarify requests are scoped interaction requests, not global
chrome. Surfaces route responses through Gateway/runtime APIs with enough
thread/source context to reject stale or wrong-thread responses. Backend
permission and sandbox layers remain responsible for translating allow
decisions into bounded runtime grants.

Permission decisions use explicit scope language. `Allow once` is the primary
visual action without becoming an Enter default, `Allow for session` is
secondary, `Always allow` is cautionary and explains persistence, and `Deny` is
a neutral exit. Submitting one decision makes the whole decision group pending
until the runtime response settles.

Across TUI and GUI surfaces, filesystem approval requests use one compact
information hierarchy: the heading identifies the tool and source, the policy
reason appears once, and each requested path plus any distinct canonical
resolved path appears once as a path-identity rail. Generic action and
suggested-grant rows are omitted when they repeat the same filesystem target.
Non-filesystem approvals retain action, matched-rule, and persistent-grant
context when those details add information needed for the decision. Supporting
authorization-lifetime text appears only when it distinguishes session or
persistent scope; it does not restate the visible allow-once action.

When a running turn is interrupted or a surface exits while requests are
pending, the UI releases pending permission or clarify state so suspended work
can settle observably instead of leaving orphaned live rows or background work.

Interrupt controls target the active foreground work first. If foreground work
has pending steer or queued inputs that have not committed, interruption
restores those inputs to the composer or pending preview according to the
surface's local editing model.

## Related Topics

- [022 UI](../022-ui/spec.md) defines the shared UI source-of-truth map.
- [026 Commands](../026-commands/spec.md) defines shared command semantics.
- [041 Permissions](../041-permissions/spec.md) defines approval policy.
- [250 UI Display Model](../250-ui-display-model/spec.md) defines committed
  transcript and display-only boundaries.
- [260 UI Rendering](../260-ui-rendering/spec.md) defines how interaction
  state appears in transcript/status surfaces.
- [210 pevo TUI](../210-pevo-tui/spec.md) defines concrete terminal controls.
- [240 pevo Web](../240-pevo-web/spec.md) defines concrete Web/Workbench
  controls.
