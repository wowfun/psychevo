---
name: 130. Permissions
psychevo_self_edit: deny
---

# 130. Permissions

Define the first runtime permission system for Psychevo tool execution.

## Summary

Psychevo permissions are a runtime policy gate before local tool execution.
They are not an operating-system sandbox in this slice. The v1 behavior uses
non-interactive-safe defaults and keeps tool availability, permission mode,
approval mode, and persistent policy as separate concerns.

## Model

`RunMode` controls which tools are visible to the model. `plan` remains a
read-only runtime mode. `default` exposes the normal build tools.

`PermissionMode` controls approval behavior for visible tools:

- `default`: apply configured rules and default dangerous-action policy.
- `acceptEdits`: like `default`, but safe workdir file edit/write asks are
  allowed for the current session.
- `dontAsk`: fully non-interactive restricted mode. Any action that would
  otherwise prompt is denied instead of approved or shown to the user. Only
  explicit `permissions.allow` matches, read-only/safe defaults, and
  non-prompting actions may execute.
- `bypassPermissions`: dangerous explicit bypass mode; it skips prompt-level
  asks but must not bypass hard denies.

`ApprovalMode` controls who reviews asks:

- `manual`: user approval, default.
- `smart`: a configured reviewer model may approve or deny; failure falls back
  to manual approval when a manual approval channel is available.

## Configuration

Configuration lives under `permissions` in JSONC config:

```jsonc
{
  "permissions": {
    "approval_mode": "manual",
    "smart_model": "provider/model",
    "allow": ["Bash(npm test *)"],
    "ask": ["Bash(cargo publish *)"],
    "deny": ["Write(.env)"]
  }
}
```

Rules use strings of the form `Tool(pattern)`, with tool names `Bash`, `Read`,
`Write`, and `Edit`. Filesystem patterns may be workdir
relative globs or canonical absolute path globs. Generated persistent rules
prefer workdir-relative patterns. Bash rules match normalized command prefixes
and may use `*` and `?`.

Rule precedence is:

1. hardline/protected deny
2. configured deny
3. configured ask
4. configured allow
5. default policy

Project config overrides global config through the existing JSONC deep merge.
`allow always` writes only to the project-local `.psychevo/config.jsonc`.

## Policy

Hardline/protected denies cannot be bypassed by `dontAsk`,
`bypassPermissions`, `allow always`, session grants, or configured allow rules.
They cover sensitive write targets such as SSH, cloud credentials, shell rc
files, `.env`, system account/service files, and the project permissions
configuration surface.

Protected reads are intentionally narrow. Internal Psychevo cache/index paths
that could inject stale or untrusted runtime material may be denied.

Ordinary workdir file reads, writes, and edits are allowed by default unless a
rule or protected path says otherwise. Dangerous bash patterns use two tiers:
catastrophic commands are denied; other risky commands ask in interactive
contexts and are allowed in non-interactive contexts.

## Approval

Approval choices are:

- allow once
- allow session
- allow always
- deny

Session grants are scoped to one runtime session. `allow always` appends a
safe suggested rule to `permissions.allow` when the action supports persistent
rules. File asks are session-scoped by default; they are not automatically
persisted from approval prompts.

When no approval handler is available, Psychevo uses the non-interactive
fallback in `default` and `acceptEdits`: prompt-level asks are allowed, while
hardline/protected denies still fail closed. `dontAsk` is the exception:
prompt-level asks are denied so CI or restricted environments can predefine the
exact allow set.

Denied or timed-out approvals become structured tool-result errors so the
model can explain or choose a safer action.

## Related Topics

- [009 Resource Surface](../009-resource-surface/spec.md) defines resource
  decision semantics.
- [110 Coding Core Tools](../110-coding-core-tools/spec.md) defines tool
  result contracts that surface permission denial.
- [200 pevo CLI](../200-pevo-cli/spec.md) defines CLI command and flag surface.
- [212 TUI Interaction](../212-pevo-tui-interaction/slash-commands.md) defines
  slash command behavior.
