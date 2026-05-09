---
name: Specs Guide
psychevo_self_edit: deny
---

Specs define the long-lived contracts, using topic directories with this structure:
- `spec.md`: stable specification, required; must include at least an opening purpose paragraph, `## Scope` (including explicit out-of-scope items when useful), `## Related Topics`
- `plan.md`: phased implementation plan; do not create unless necessary
- `tasks.md`: implementation checklist and status tracking; do not create unless necessary.
- Additional supplementary files; must have entry links in `spec.md` or `plan.md`

`000-099` are foundation specs. `100+` may be capability or product surface specs
(prefer long-lived domain names). Numbering should leave space for future
insertion.

Specs that directly drive implementation should include functional requirements and acceptance criteria when behavior is non-trivial.

When a topic grows beyond a maintainable single file, split supplementary files by durable topic responsibility.

Historical specs should be moved under `_archive/` when superseded, and are read-only by default.

## Source of Truth

- Each stable rule should have exactly one best-fit `spec.md` as its source of truth.
- Other specs should link to that source or quote only the minimum needed context.
- Updates should prioritize modifying that source rather than editing multiple downstream topics separately.
