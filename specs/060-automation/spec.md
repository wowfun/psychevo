---
name: 060. Automation
psychevo_self_edit: deny
---

Define the product-automation foundation for Psychevo. Automation in this spec
covers local scheduled turns that run through Psychevo product surfaces for a
project or for an existing thread.

Automation remains local-first in this slice. It does not define CI/CD
workflows, validation runners, hosted job services, external orchestration APIs,
remote control APIs, provider catalog automation, or long-running OS-level
background schedulers.

## Scope

- local-first product automation principles shared by concrete workflow
  surfaces
- product automation evidence, isolation, and run-policy boundaries

Out of scope:
- CI/CD workflows, validation vocabulary, CI artifacts, local validation
  profiles, release packaging, or hosted CI provider adapters
- hosted automation services, remote scheduling APIs, OS cron/systemd launch
  agents, cloud workers, plugin automation, or provider catalog automation
- guaranteed execution while Gateway is closed
- whole-process isolation, containerized execution, or per-automation worktree
  creation

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

## Related Topics

- [000 Foundation](../000-foundation/spec.md) defines the upstream principle
  that execution leaves evidence.
- [005 Durable Evidence](../005-durable-evidence/spec.md) defines durable
  evidence semantics for inspectable agent-invocation facts.
- [031 Storage and Persistence](../031-storage-and-persistence/spec.md) defines
  persistence boundaries for evidence-backed material.
- [065 CI/CD](../065-ci-cd/spec.md) defines validation, workflow runner, and
  artifact evidence semantics.
- [070 Experience](../070-experience/spec.md) defines the UX and DX defaults
  that product automation diagnostics should support.
- [400 Workflow Automations](../400-workflow-automations/spec.md) defines the
  concrete product workflow automation implementation and testing source of
  truth.
