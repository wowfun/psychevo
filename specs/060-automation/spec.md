---
name: 060. Automation
psychevo_self_edit: deny
---

Define the cross-cutting foundation for Psychevo test and validation
automation. Automation in this spec means local, CI, harness, and coding-agent
validation of Psychevo behavior; it does not define external runtime
orchestration, remote control APIs, product workflow automation, or provider
catalog automation.

## Scope

- shared validation vocabulary used by downstream testing specs
- automation evidence principles for human, CI, and coding-agent consumers
- relationship between structured facts, durable evidence, rendered output, and
  projection artifacts
- boundaries for deterministic, live opt-in, snapshot, harness, integration,
  and E2E validation categories

Out of scope:
- concrete validation commands, CI workflow files, artifact paths, storage
  schemas, JSON schemas, snapshot file formats, or screenshot formats
- product-specific acceptance matrices, provider-specific live checks, TUI
  rendering test mechanics, or CLI output contracts
- external orchestration APIs, job schedulers, hosted automation services,
  plugin automation, or model-driven operational workflows

## Validation Vocabulary

Topic testing specs own their concrete commands and acceptance scenarios. When
they use the terms below, they should use these meanings unless they explicitly
define a narrower topic-specific meaning.

Narrow validation is a focused check for a changed subsystem or behavior. It
should minimize runtime and scope while exercising the closest meaningful
contract for the change.

Broad validation is a wider deterministic gate for repository confidence. A
topic may name a broad command, but this spec does not require a global command
or decide which packages, crates, fixtures, or scripts are included.

A snapshot or golden check compares a stable projection against an intentional
baseline. The owning topic decides which projections are stable enough to
version, how review and acceptance work, and which volatile fields must be
normalized or excluded.

A harness or integration test runs multiple components together through a
controlled local boundary. It should prefer fake providers, local deterministic
servers, temporary state, and explicit fixtures over host-global state.

An E2E test drives a product or user-facing entrypoint through realistic I/O.
It should still isolate credentials, state, terminals, files, network services,
and clocks where practical.

Live opt-in validation uses real providers, real credentials, live network
services, or other host/user-specific resources. Live validation should not be
part of a topic's deterministic default gate unless that topic explicitly
changes the boundary and provides isolation strong enough for default use.

## Evidence Principles

Automation should treat coding agents as first-class consumers. Failure output,
artifacts, and diffs should be practical for an agent to inspect and use for a
repair loop when the implementation can provide that without making tests more
brittle.

Structured truth comes first. When determining state, outcome, or semantic
correctness, automation should prefer structured events, durable evidence,
database facts, result objects, machine-readable diffs, and explicit exit
status over terminal prose or rendered text.

Rendered terminal output, screenshots, pane captures, ANSI captures, visual
snapshots, and product screenshots are projection evidence. They are valuable
for regression testing user-facing layout and presentation, but they should not
be the only source of truth when structured facts are available.

For terminal UIs, in-process render buffers and style-marker snapshots are
usually better default goldens than raw ANSI or tmux pane captures because they
are less dependent on terminal emulator behavior. ANSI, PTY, or tmux captures
are still useful diagnostic and E2E artifacts when the owning topic defines
their scope explicitly.

Automation artifacts may reuse runtime durable evidence as their primary source
of facts. Reusing evidence is preferable to duplicating execution records when
the artifact can preserve enough relationship information for inspection.

Failure evidence should preserve enough context to make the next action clear,
but this spec does not define a universal diagnostic schema. Topic specs may
define stricter machine-readable formats when a product or harness needs them.

## Validation Boundaries

Default validation should avoid hidden dependencies on real user config,
credentials, persistent host state, global mocks, wall-clock timing, open
sockets, live services, and provider availability unless the owning topic
explicitly defines those dependencies as opt-in or isolated.

Structural refactors should run the closest deterministic narrow validation for
the touched subsystem. When a refactor spans multiple crates or product
surfaces, it should also run the broad deterministic validation gate when one
exists.

Tests should assert behavior and stable invariants before volatile inventories,
generated prose, provider catalogs, incidental terminal formatting, or
implementation-private storage layouts.

Snapshot, golden, baseline, generated inventory, and expected-failure updates
should happen only for intentional behavior or artifact-boundary changes, and
should be treated as review material by the owning topic. This spec defines the
category; the topic testing spec owns the exact review workflow, checked-in
artifact policy, and acceptance commands.

Automation that mutates files, state, sessions, processes, terminals, or
environment variables should keep those resources isolated and clean up after
itself. If cleanup is impossible or intentionally skipped for diagnostics, the
owning harness should make the artifact boundary explicit.

## Related Topics

- [000 Foundation](../000-foundation/spec.md) defines the upstream principle
  that execution leaves evidence.
- [005 Durable Evidence](../005-durable-evidence/spec.md) defines durable
  evidence semantics for inspectable agent-invocation facts.
- [020 Interfaces](../020-interfaces/spec.md) defines caller-facing observation
  boundaries that automation may consume.
- [040 Storage and Persistence](../040-storage-and-persistence/spec.md) defines
  persistence boundaries for evidence-backed material.
- [050 Capability Extensions](../050-capability-extensions/spec.md) defines the
  preceding foundation layer for extension contributions.
- [070 Experience](../070-experience/spec.md) defines the UX and DX defaults
  that automation evidence and diagnostics should support.
