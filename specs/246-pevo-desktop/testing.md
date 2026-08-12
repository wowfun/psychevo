---
name: pevo Desktop Testing
psychevo_self_edit: deny
---

# pevo Desktop Testing

The normative acceptance inventory and platform exemptions remain in the
[Desktop validation section](spec.md#validation). Use the `desktop-rust`,
`visual`, and `package` CI profiles for deterministic Linux validation. Run
native/provider checks through `cargo xtask live run --suite desktop`; a host
capability failure is evidence and must remain a structured blocked or skipped
result rather than a silent omission.
