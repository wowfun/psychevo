---
name: 290. History Editing and Thread Fork Testing
psychevo_self_edit: deny
---

# History Editing and Thread Fork Testing

## Deterministic Coverage

Runtime/store tests must cover exact editable-envelope ordering, best-effort
legacy recovery, workspace-undo compatibility, mutually exclusive revert kinds,
restart recovery, restore, unchanged-draft no-op, and cleanup after admitted
replacement turns.

Gateway tests must cover Native eligibility, stable message-boundary resolution,
full fork, point fork, empty-prefix fork, durable evidence remapping, source
immutability, title and scalar lineage projection, root-history visibility, and
bounded rejection for running, staged, child, side, channel, automation, and ACP
point-fork requests.

Workbench tests must cover inline editor state, exact and best-effort drafts,
Text/Image changes, unchanged no-op, staged admission failure, Restore, point
fork navigation/prefill, full fork, and unavailable provenance.

TUI tests must cover mouse and transcript-focus keyboard activation, message
actions, image-capable editing, staged recovery, child navigation/prefill, and
sessions-panel full fork.

## Validation

Run changed focused tests first, then:

1. `cargo xtask gateway-protocol generate --check`
2. `pnpm -r check`
3. `pnpm -r test`
4. `cargo xtask ci run --profile rust-broad`
5. `cargo xtask ci run --profile visual`
6. `git diff --check`

All tests use local stores, deterministic fake/test providers, and isolated
temporary state. Live providers and live services are not part of this topic's
default validation.

Generated protocol and intentional visual diffs are review material and must be
inspected rather than accepted solely because a command completed.

## Related Topics

- [Spec](spec.md) defines behavior and acceptance criteria.
- [210 TUI Testing](../210-pevo-tui/testing.md) defines TUI harness rules.
- [240 Web](../240-pevo-web/spec.md) defines Workbench validation ownership.
