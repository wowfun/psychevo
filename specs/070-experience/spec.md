---
name: 070. Experience
psychevo_self_edit: deny
---

Define Psychevo's cross-cutting baseline for user experience and developer
experience. Experience in this spec means how people and coding agents
understand, control, change, validate, and diagnose Psychevo behavior across
product surfaces and implementation work.

## Scope

- shared UX and DX vocabulary for downstream specs
- user-facing operation clarity, state visibility, consequence clarity,
  recoverability, interruption, and sensitive-data handling
- developer-facing changeability, validation, diagnostics, reviewability, and
  human/coding-agent collaboration
- ownership boundaries between foundation experience defaults and concrete
  product or subsystem behavior
- lightweight acceptance criteria for experience-impacting changes

Out of scope:
- command syntax, flags, shortcuts, UI layout, visual style, exact copy, or
  product-specific flows
- Rust traits, structs, functions, modules, crate structure, storage schemas,
  transport payloads, event schemas, or API shapes
- validation commands, CI workflow files, snapshot formats, artifact paths, or
  test-harness mechanics
- concrete security policy, permission rules, sandbox policy, provider
  behavior, approval UX, billing UX, or credential storage formats
- product-specific acceptance matrices for CLI, TUI, provider registry,
  skills, coding tools, or automation

## Experience Model

User experience covers people who invoke Psychevo, inspect its work, control an
active operation, recover from a failure, or configure a product surface.

Developer experience covers human maintainers, coding agents, test harnesses,
and automation consumers that read, modify, validate, or diagnose Psychevo
behavior.

UX and DX are equal experience axes. A change should not improve one by making
the other opaque unless the owning downstream spec names the tradeoff and keeps
the source of truth discoverable.

This spec defines defaults. Downstream specs may tighten these defaults for a
product surface or subsystem, but they should not duplicate this baseline as a
second source of truth.

## UX Baseline

User-facing operations should preserve user intent across setup, confirmation,
execution, interruption, completion, and failure. If a product surface accepts a
request, it should make the accepted intent and active scope visible enough for
the user to tell what Psychevo is doing.

Interfaces should show meaningful state for pending, active, completed,
stopped, aborted, and failed work when the surface exposes those states.
Progress presentation should favor facts the system knows over decorative
activity signals.

User-visible actions should make their consequences clear before they perform
destructive, persistent, expensive, or credential-affecting work. When a surface
cannot provide confirmation, the owning spec should define the safe default.

Failures should leave the user with the next useful action when the system can
know it. Retriable setup failures, validation errors, provider errors,
permission denials, and interrupted work should not collapse into the same
undifferentiated message.

Interruption and control behavior should match the user's active context.
Prompt editing, menu navigation, shell escapes, foreground work, background
work, and provider waits may need different control handling; downstream specs
own the concrete key, signal, or command behavior.

Sensitive values must not appear in human-facing output, persisted
configuration, tests, logs, snapshots, status messages, or diagnostics unless a
downstream spec explicitly defines a safe redaction or opt-in inspection
boundary.

## DX Baseline

Implementation work should be local enough that a contributor can identify the
owning spec, source module, test surface, and evidence boundary for a change
without reading unrelated product surfaces.

Source-of-truth ownership should stay discoverable. When a change crosses UX,
DX, runtime, persistence, automation, or product behavior, the implementation
should update the best-fit spec instead of repeating rules across downstream
documents.

Validation should be deterministic by default and close to the changed
behavior. A change that affects experience should provide structured facts,
snapshots, rendered projections, or other reviewable evidence at the layer that
owns the behavior.

Diagnostics should help humans and coding agents repair failures. Error output,
test failures, traces, snapshots, and artifacts should preserve enough context
to identify the broken contract without depending on real credentials, global
host state, live services, or hidden timing.

Reviewable artifacts should separate semantic facts from projections when both
exist. Rendered output, screenshots, and terminal captures are useful, but
structured evidence should carry the correctness claim when the implementation
can expose it.

Coding-agent collaboration should assume partial worktrees and parallel
changes. Specs, tests, and diagnostics should make the intended ownership and
validation path clear enough for an agent to make a scoped change without
reverting unrelated work.

## Tradeoffs and Ownership

Concrete product specs own command syntax, UI interaction, wording, layout,
output format, and product-specific acceptance tests. This spec owns the shared
experience defaults those product specs should preserve.

Runtime, execution, provider, storage, resource, memory, and extension specs
own their semantic contracts. Experience requirements may require those facts
to be visible or diagnosable, but this spec does not redefine the underlying
semantics.

Automation specs own validation categories and artifact handling. This spec
requires experience-impacting changes to remain verifiable, but it does not
choose commands, harnesses, or CI policy.

When UX and DX goals conflict, the implementation should prefer the option that
keeps user intent safe and system behavior inspectable. If the tradeoff affects
a public surface or contributor workflow, the owning spec should record the
decision.

## Acceptance Criteria

An experience-impacting change should be accepted only when:

- the affected user or developer intent is clear
- the owning source-of-truth spec is updated or the change fits an existing
  spec without amendment
- visible state, outcomes, and failure modes remain distinguishable at the
  owning surface
- sensitive data stays out of durable and diagnostic material unless a safe
  boundary owns the exception
- a deterministic validation or review path exists for the changed behavior
- evidence or diagnostics are useful enough for a human or coding agent to
  inspect a failure and choose the next repair step

## Related Topics

- [000 Foundation](../000-foundation/spec.md) defines the upstream project
  foundation and implementation-neutral principles.
- [020 Interfaces](../020-interfaces/spec.md) defines caller-facing invocation,
  observation, completion, and control-signal semantics.
- [025 CLI](../025-cli/spec.md) defines command-line interface foundation
  semantics.
- [060 Automation](../060-automation/spec.md) defines validation vocabulary,
  evidence principles, and deterministic validation boundaries.
- [200 pevo CLI](../200-pevo-cli/spec.md) defines the concrete `pevo` product
  CLI.
- [210 pevo TUI](../210-pevo-tui/spec.md) defines the concrete fullscreen TUI
  product surface.
