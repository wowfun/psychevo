---
name: pevo Floating Testing
psychevo_self_edit: deny
---

# pevo Floating Testing

The normative acceptance inventory and native-host opt-in boundary remain in
the [Floating validation section](spec.md#validation). Run deterministic
package checks through the repository Web and visual CI profiles. Real provider
and native Floating checks run through `cargo xtask live run --suite desktop`;
the live manifest must distinguish passed, blocked, and skipped checks.
