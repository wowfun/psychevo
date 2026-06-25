---
name: 060. Automation
psychevo_self_edit: deny
---

Define the cross-cutting foundation for Psychevo automation. Automation in this
spec covers two related boundaries:

- local, CI, harness, and coding-agent validation of Psychevo behavior
- local product workflow automations that schedule Psychevo turns for a project
  or for an existing thread

Automation remains local-first in this slice. It does not define hosted job
services, external orchestration APIs, remote control APIs, provider catalog
automation, or long-running OS-level background schedulers.

## Scope

- shared validation vocabulary used by downstream testing specs
- automation evidence principles for human, CI, and coding-agent consumers
- relationship between structured facts, durable evidence, rendered output, and
  projection artifacts
- boundaries for deterministic, live opt-in, snapshot, harness, integration,
  and E2E validation categories
- local-first product automation principles shared by concrete workflow
  surfaces
- product automation evidence, isolation, and run-policy boundaries

Out of scope:
- concrete validation commands, CI workflow files, artifact paths, storage
  schemas, JSON schemas, snapshot file formats, or screenshot formats
- product-specific acceptance matrices, provider-specific live checks, TUI
  rendering test mechanics, or CLI output contracts
- hosted automation services, remote scheduling APIs, OS cron/systemd launch
  agents, cloud workers, plugin automation, or provider catalog automation
- guaranteed execution while Gateway is closed
- whole-process isolation, containerized execution, or per-automation worktree
  creation

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
Broad validation entrypoints may hide successful command output when that keeps
routine validation readable, but they must preserve the failing command, exit
status, and captured output when a step fails.

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

## Repo-Local Development Home

The repository-local development home is `.local/.psychevo-dev/` under the repo
root. It is an opt-in development and automation convention, not a product
default profile and not a replacement for the user's normal `~/.psychevo`.

This directory may be used for local development, tests, and validation across
surfaces, including deterministic helper state, GUI/TUI visual artifacts, peval
development workspaces, and explicit live opt-in provider validation. It may
contain isolated `config.toml`, `.env`, SQLite state such as `state.db`, logs,
sessions, snapshots, TUI/Web artifacts, and live-validation workdirs.

Commands and scripts that rely on this home must set `PSYCHEVO_HOME`
explicitly. When a fixed config file should be used, they should set
`PSYCHEVO_CONFIG=.local/.psychevo-dev/config.toml`; when validation state should
be isolated from the dev home's normal state, they should set `PSYCHEVO_DB`
explicitly. Relative examples are relative to the repository root.

Automation scripts should keep substantial structured-output parsing in
repo-local helper programs rather than embedding long inline programs in shell
heredocs. Shell entrypoints may still own orchestration, environment selection,
and process wiring, while helper programs own reusable evidence parsing and
assertions.

Using this dev home does not make live validation deterministic. Provider,
API-key, or live-service validation remains live opt-in and must not enter the
default deterministic validation path. Scripts must not automatically copy
credentials from the user's normal home or external auth stores into this
directory; developers must prepare any live credentials explicitly.

## Product Workflow Automations

Concrete workflow automation behavior, UI, model-tool actions, schedule
grammar, and tests are defined by
[400 Workflow Automations](../400-workflow-automations/spec.md). This section
only keeps the shared product-automation boundaries that apply across future
workflow, channel, and surface-specific automation topics.

Psychevo product automations are local scheduled prompts executed through the
same Gateway turn path as ordinary user turns. The scheduler runs inside the
local Gateway/Web process. If the process is closed, missed work is not executed
until Gateway starts again. On restart, a task may run at most once for the
latest missed occurrence; the scheduler must not replay every missed interval as
a backlog.

Automation definitions and run records are local semantic state. The transcript
remains the durable evidence for model-visible messages and tool results;
automation run records are coordination and inspection facts, not a second
transcript.

Schedulers must avoid overlapping runs for the same task. If a task is already
running, another scheduled or manual request should report or record a
skipped/busy run instead of starting a second turn.

The default execution policy is `Auto in sandbox`. It maps to prompt-approval
auto-allow behavior while still preserving hard permission denies and sandbox
enforcement. In the current runtime this is implemented by running automation
turns with `bypassPermissions` plus an automation-only sandbox override:
`enabled=true`, `mode=workspace-write`, and the usual temporary/cache roots for
shell children, including the shell-only `/dev/null` and `/dev/zero`
compatibility sinks defined by Sandbox v1. This must not mutate user
configuration. Sandbox v1 does not
confine network access; network remains governed by the current permission and
shell-risk model from [041 Permissions](../041-permissions/spec.md) and
[045 Sandbox](../045-sandbox/spec.md).

An alternate Ask-first policy may run with ordinary permission prompts. If a
scheduled run reaches a user approval or clarify prompt, it becomes an
ordinary pending interaction in the owning thread/source; the scheduler must
not invent a second approval channel.

## Validation Boundaries

Default validation should avoid hidden dependencies on real user config,
credentials, persistent host state, global mocks, wall-clock timing, open
sockets, live services, and provider availability unless the owning topic
explicitly defines those dependencies as opt-in or isolated.

Structural refactors should run the closest deterministic narrow validation for
the touched subsystem. When a refactor spans multiple crates or product
surfaces, it should also run the broad deterministic validation gate when one
exists.

Large-file refactors should include a structural inventory pass over the touched
workspace roots. The canonical inventory excludes build and runtime artifact
directories such as `target/`, `dist/`, `node_modules/`, `coverage/`,
`test-results/`, and `.local/`, then reports oversized files by category:
ordinary production/specification, test, and generated artifact. The inventory
is evidence for the refactor; it should not become a brittle default test over
volatile generated lists.

Inventory success is not sufficient by itself. Review should also confirm that
new module boundaries are semantic and that extracted files are not mechanical
ordinal slices such as `part_001.rs` or `part_a.ts`.

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

Repo-local dev/test dependency helpers are automation support, not product
installers. They should default to check-only behavior, print exact missing
dependency remediation, and require an explicit install flag before changing
host packages, browser caches, or other machine-level state. Product install
scripts should remain focused on installing the product surface and should not
silently absorb validation-only tools such as browser, terminal-capture, or
database inspection dependencies.

## Related Topics

- [000 Foundation](../000-foundation/spec.md) defines the upstream principle
  that execution leaves evidence.
- [005 Durable Evidence](../005-durable-evidence/spec.md) defines durable
  evidence semantics for inspectable agent-invocation facts.
- [020 Interfaces](../020-interfaces/spec.md) defines caller-facing observation
  boundaries that automation may consume.
- [031 Storage and Persistence](../031-storage-and-persistence/spec.md) defines
  persistence boundaries for evidence-backed material.
- [050 Capability Extensions](../050-capability-extensions/spec.md) defines the
  preceding foundation layer for extension contributions.
- [070 Experience](../070-experience/spec.md) defines the UX and DX defaults
  that automation evidence and diagnostics should support.
- [400 Workflow Automations](../400-workflow-automations/spec.md) defines the
  concrete product workflow automation implementation and testing source of
  truth.
