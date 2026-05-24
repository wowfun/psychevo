---
name: Specs Guide
psychevo_self_edit: deny
---

Specs define the long-lived contracts in numbered topic directories. `000-099` are foundation specs. `100+` specs may define capabilities or product surfaces (prefer long-lived domain names). Numbering should leave space for future insertion.

Topic directories use this structure:
- `spec.md`: stable specification, required; must include at least an opening purpose paragraph, `## Scope` (including explicit out-of-scope items when useful), `## Related Topics`
- `testing.md`: allowed and required for `100+` specs only
- `plan.md`: phased implementation plan for `100+` specs only; do not create unless necessary
- `tasks.md`: implementation checklist and status tracking for `100+` specs only; do not create unless necessary
- Additional supplementary files; must have entry links in `spec.md`, `testing.md` or `plan.md`

In `spec.md`, same-directory `.md` supplements belong under `## Attachments`.
Same-topic attachment labels omit the topic number; cross-topic attachment
labels keep the target topic number. Apply the same label rule to inline
references.

Specs that directly drive implementation should include functional requirements and acceptance criteria when behavior is non-trivial.

When a topic grows beyond a maintainable single file, split supplementary files by durable topic responsibility.

Historical specs should be moved under `_archive/` when superseded, and are read-only by default.

## Source of Truth

- Each stable rule should have exactly one best-fit `spec.md` as its source of truth.
- Other specs should link to that source or quote only the minimum needed context.
- Updates should prioritize modifying that source rather than editing multiple downstream topics separately.
