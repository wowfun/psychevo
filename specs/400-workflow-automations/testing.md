---
name: 400. Workflow Automations Testing
psychevo_self_edit: deny
---

Define deterministic validation for workflow automations. Tests for this topic
must use fake/local providers by default and must not require real credentials,
live provider calls, host-global config, or long-lived host state.

## Deterministic Defaults

Automation tests should isolate state with temporary homes, temporary databases,
fake clocks, fake providers, and local harnesses. Browser tests should use the
repo's deterministic Workbench fixture path and must not depend on the user's
normal `~/.psychevo`.

Live provider, API-key, or cloud-service validation is opt-in only. It is not
part of this topic's default gate.

## Required Narrow Validation

Schedule/runtime changes:

```sh
cargo test -p psychevo-runtime automations::tests
```

Gateway automation RPC, scheduler bridge, and model-tool changes:

```sh
cargo test -p psychevo-gateway automation_
```

Protocol schema or generated type changes:

```sh
cargo run -p psychevo-xtask -- gateway-protocol generate --check
```

Workbench automation component changes:

```sh
pnpm --filter @psychevo/workbench test -- appComposerAutomations.test.tsx
pnpm --filter @psychevo/workbench typecheck
```

Workbench automation visual/E2E changes:

```sh
pnpm --filter @psychevo/workbench build && pnpm exec playwright test apps/workbench/e2e/workbench.spec.ts --grep "Automations"
```

## Scenario Coverage

Runtime tests cover due and next-run behavior for `interval`, `daily`, `weekly`,
`delay`, and `once` schedules, including one-shot schedules reporting no next
scheduled run after success.

Gateway tests cover `automation/list`, `automation/write`,
`automation/pause`, `automation/resume`, `automation/delete`,
`automation/run`, scheduler claims, run records, and the model-facing
`automation` tool actions: `list`, `create`, `update`, `pause`, `resume`,
`run`, and `remove`.

Gateway tests should assert that `automation/write` preserves existing lifecycle
state and that pause/resume are the only RPCs that toggle `enabled`.

Gateway tests also cover the recursion boundary: automation draft turns and
automation-triggered turns must not expose the model-facing `automation` tool.

Workbench tests cover draft workspace/thread selection, title area with no
current workdir path, last-run display, pause/resume controls using
`automation/pause` and `automation/resume`, a single empty-state creation
surface with no duplicate template actions or empty draft placeholder, centered
editor layout, and no horizontal overflow on desktop or mobile.

E2E visual validation captures desktop and mobile Automations screenshots for
both the initial empty state and an open draft after the Workbench build. These
tests should assert the empty state has one visible template action group, no
orphan draft card, and no large right-side placeholder layout. Screenshot tests
should use fake/local data and should not exercise live providers.

## Broad Validation

When changes touch shared Rust runtime, Gateway protocol, persistence, or
permission/sandbox behavior beyond the automation feature boundary, run:

```sh
scripts/validate-rust.sh broad
```

Do not run multiple broad validation commands concurrently in the same worktree
unless the underlying harness explicitly supports isolation.

## Related Topics

- [400 Workflow Automations](./spec.md) defines the product behavior under
  test.
- [060 Automation](../060-automation/spec.md) defines shared automation
  evidence and validation principles.
- [240 Pevo Web Testing](../240-pevo-web/testing.md) owns broader Workbench
  validation expectations.
