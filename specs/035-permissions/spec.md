---
name: 035. Permissions
psychevo_self_edit: deny
---

# 035. Permissions

Define Psychevo's runtime permission policy for local resource and tool
operations. This topic is the source of truth for permission modes, approval
semantics, persistent permission rules, and the first dangerous-action policy.

## Scope

- runtime permission policy before local resource operations and tool execution
- relationship between runtime mode, tool visibility, permission mode, and
  approval mode
- `Tool(pattern)` permission rule language and precedence
- hard/protected denies, protected reads, and dangerous exec command policy
- approval choices, session grants, persistent allow rules, and fallback
  behavior when no approval handler is available
- observable denial, timeout, and approval failure behavior
- acceptance criteria for deterministic permission validation

Out of scope:

- operating-system sandbox isolation, security guarantees, containerization,
  or process isolation
- concrete CLI flags, slash commands, terminal rendering, or approval UI layout
- concrete tool result JSON fields beyond requiring permission outcomes to be
  observable through the owning tool contract
- storage schemas, config file mutation APIs, Rust APIs, payload schemas, or
  event names
- provider authentication, credential storage, secret redaction, or remote
  policy services

## Model

Psychevo permissions are a runtime policy gate before local resource operations
and tool execution. They are not an operating-system sandbox in this slice.
Tool availability, runtime mode, permission mode, approval mode, and persistent
policy are separate concerns.

Runtime mode controls the hard ceiling for model-visible tools. `plan` remains
a read-only runtime mode, and `default` may expose normal build tools when the
tool surface and caller entrypoint allow them. Permission policy must not expand
the current runtime-mode ceiling.

`PermissionMode` controls approval behavior for visible tools and resource
operations:

- `default`: apply configured rules and default dangerous-action policy.
- `acceptEdits`: like `default`, but safe workdir file edit/write asks are
  allowed for the current session.
- `dontAsk`: fully non-interactive restricted mode. Any action that would
  otherwise prompt is denied instead of approved or shown to the user. Only
  explicit `permissions.allow` matches, read-only/safe defaults, and
  non-prompting actions may execute.
- `bypassPermissions`: dangerous explicit bypass mode; it skips prompt-level
  asks but must not bypass hard/protected denies.

Product surfaces may expose `plan` beside permission modes as a convenience,
but `plan` selects the read-only runtime mode rather than a separate
permission-mode rule set.

`ApprovalMode` controls who reviews asks:

- `manual`: user approval, default.
- `smart`: a configured reviewer model may approve or deny; failure falls back
  to manual approval when a manual approval channel is available.

## Configuration

Configuration lives under `permissions` in TOML config:

```toml
[permissions]
approval_mode = "manual"
smart_model = "provider/model"
allow_login_shell = false
allow = ["ExecCommand(npm test *)"]
ask = ["ExecCommand(cargo publish *)"]
deny = ["Write(.env)"]
```

Rules use strings of the form `Tool(pattern)`, with tool names `ExecCommand`,
`Read`, `Write`, and `Edit`. Filesystem patterns may be workdir-relative globs
or canonical absolute path globs. Generated persistent rules prefer
workdir-relative patterns. Exec command rules match normalized command prefixes
and may use `*` and `?`. Legacy `Bash(...)` rules are not interpreted by the
provider-visible execution tool.

Rule precedence is:

1. hard/protected deny
2. configured deny
3. configured ask
4. configured allow
5. default policy

Project config overrides global config through the existing TOML deep merge.
`allow always` writes only to the project-local `.psychevo/config.toml`.

## Policy

Hard/protected denies cannot be bypassed by `dontAsk`, `bypassPermissions`,
`allow always`, session grants, or configured allow rules. They cover sensitive
write targets such as SSH, cloud credentials, shell rc files, `.env`, system
account/service files, and the project permissions configuration surface.

Protected reads are intentionally narrow. Internal Psychevo cache/index paths
that could inject stale or untrusted runtime material may be denied.

Ordinary workdir file reads, writes, and edits are allowed by default unless a
rule or protected path says otherwise. Dangerous exec command patterns use two
tiers: catastrophic commands are denied; other risky commands ask in
interactive contexts and are allowed in non-interactive contexts except under
`dontAsk`. Shell-level background wrappers that escape session tracking are
rejected before execution.

Permission policy applies after tool visibility and before or during execution.
A model-visible tool declaration says what the model may request, not what the
runtime must execute. Runtime and resource gates remain responsible for the
final allow, deny, or defer decision.

## Approval

Approval choices are:

- allow once
- allow session
- allow always
- deny

Session grants are scoped to one runtime session. `allow always` appends a safe
suggested rule to `permissions.allow` when the action supports persistent
rules. File asks are session-scoped by default; they are not automatically
persisted from approval prompts.

When no approval handler is available, Psychevo uses the non-interactive
fallback in `default` and `acceptEdits`: prompt-level asks are allowed, while
hard/protected denies still fail closed. `dontAsk` is the exception:
prompt-level asks are denied so CI or restricted environments can predefine the
exact allow set.

Denied or timed-out approvals become structured tool-result errors or
before-agent-start rejection through the owning tool, resource, or runtime
contract, so the model or caller can explain the denial or choose a safer
action.

## Acceptance Criteria

- Hard/protected denies win over configured allow, session grants, and bypass
  modes.
- Configured precedence is deny, then ask, then allow, then default policy
  after hard/protected denies.
- Bash dangerous-command detection covers representative recursive delete,
  shell-pipe installer, interpreter inline execution, destructive git, process
  kill, service, permission, and SQL destructive commands.
- File path rules match both workdir-relative and canonical absolute patterns.
- `acceptEdits` auto-allows safe workdir write/edit asks for the current
  session only.
- Non-interactive runs allow prompt-level asks but deny hard/protected actions,
  except `dontAsk`, which denies actions that would otherwise prompt.
- `allow always` writes project-local TOML and skips exact duplicate rules.
- Permission validation uses deterministic local harnesses and fake or test
  providers. It must not use live providers, real API keys, network services,
  or host-global state.

## Related Topics

- [004 Runtime Contract](../004-runtime-contract/spec.md) defines runtime
  assembly and permission wiring for an invocation.
- [007 Tool Surface](../007-tool-surface/spec.md) defines tool visibility and
  the boundary between model requests and execution.
- [009 Resource Surface](../009-resource-surface/spec.md) defines resource
  gates and allow, deny, and defer decisions that permissions specialize.
- [110 Coding Core Tools](../110-coding-core-tools/spec.md) defines tool result
  contracts that surface permission denial.
- [115 Interactive Clarify](../115-interactive-clarify/spec.md) defines a user
  input tool that must not substitute for permission approval.
- [200 pevo CLI](../200-pevo-cli/spec.md) owns concrete CLI permission flags
  and rule-management commands.
- [212 TUI Interaction](../212-pevo-tui-interaction/spec.md) owns interactive
  mode and permissions projection.
