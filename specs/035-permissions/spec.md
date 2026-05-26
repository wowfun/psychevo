---
name: 035. Permissions
psychevo_self_edit: deny
---

# 035. Permissions

Define Psychevo's runtime permission policy for local resource and tool
operations. This topic is the source of truth for permission profiles,
approval policies, transparent approval routing, persistent grants, and
dangerous-action policy.

## Scope

- runtime permission policy before local resource operations and tool execution
- relationship between runtime mode, tool visibility, permission profiles,
  approval policy, and approval reviewer
- structured permission profiles for filesystem, network, and tool families
- exec policy rule language and precedence
- hard/protected denies, protected reads, dangerous exec command policy, and
  fail-closed behavior
- approval choices, session grants, persistent grant adapters, and fallback
  behavior when no approval handler is available
- observable denial, timeout, and approval failure behavior
- acceptance criteria for deterministic permission validation

Out of scope:

- operating-system sandbox isolation, security guarantees, containerization,
  or process isolation
- concrete terminal rendering details beyond the approval-flow contract
- concrete tool result JSON fields beyond requiring permission outcomes to be
  observable through the owning tool contract
- Rust APIs and event names
- provider authentication, credential storage, secret redaction, or remote
  policy services

## Model

Psychevo permissions are a runtime policy gate before local resource operations
and tool execution. They are not an operating-system sandbox in this slice.
Tool availability, runtime mode, permission mode, approval mode, and persistent
policy are separate concerns.

Runtime mode controls the hard ceiling for model-visible tools. `plan` remains
a read-only runtime mode, and `default` may expose normal editing tools when the
tool surface and caller entrypoint allow them. Permission policy must not expand
the current runtime-mode ceiling.

Permission profiles define the baseline capability boundary. Built-in profiles
are `:read-only`, `:workspace`, and `:danger-full-access`; project profiles may
extend another profile and add filesystem paths, network hosts/domains, and
tool-family grants. Profiles are policy gates, not OS sandboxes. Filesystem
profile entries may grant paths outside the current workdir, so a cross-project
read can be approved without changing the session workdir.

`approval_policy` controls whether an action that needs consent may ask:

- `on-request`: ask the configured reviewer when a profile, exec policy, MCP
  rule, skill action, or network policy requires approval.
- `untrusted`: currently equivalent to `on-request`; it is reserved for a
  future project-trust model.
- `never`: never ask. Profile-allowed actions may run, but actions that would
  prompt fail closed.
- `granular`: uses `[approval.granular]` booleans to enable or disable approval
  prompts per family. The table must explicitly set `filesystem`, `network`,
  `exec`, `mcp`, `skill`, and `request_permissions`.

`on-failure` is not supported because Psychevo does not provide a sandboxed
retry mechanism for it.

`approvals_reviewer` controls who reviews asks:

- `user`: route approval requests to the active UI/protocol handler.
- `smart`: route approval requests to a restricted reviewer model session. The
  reviewer may only approve the current action once, never persist grants.
  Timeout, provider failure, malformed output, or missing reviewer support all
  fail closed.

Agent frontmatter may use Claude-compatible `permissionMode`, but an agent can
only narrow its parent permission context. Unsupported or widening values must
not expand access.

## Configuration

Configuration lives in top-level TOML keys and structured tables:

```toml
approval_policy = "on-request"
approvals_reviewer = "user"
default_permissions = "local"

[approval.granular]
filesystem = true
network = true
exec = true
mcp = true
skill = true
request_permissions = false

[auto_review]
model = "provider/model"
timeout_secs = 90
policy = "Additional reviewer policy."

[permissions.local]
extends = ":workspace"

[permissions.local.filesystem]
"/home/user/other-project/docs/README.md" = "read"

[permissions.local.network.domains]
"api.example.com" = "allow"

[permissions.local.tools.skills]
"skill_manage/install" = "allow"

[[exec_policy.rules]]
prefix = ["git", "pull"]
decision = "allow"
```

Legacy global configuration fields `permission_mode`, `approval_mode`,
`smart_model`, and `permissions.allow`/`permissions.ask`/`permissions.deny`
are invalid and must produce a clear migration error. Product-unreleased
compatibility is not preserved except for agent frontmatter `permissionMode`.

Profile and policy precedence is:

1. hard/protected deny
2. explicit structured deny
3. active profile allow
4. session/turn grant
5. approval policy and reviewer
6. default policy

Project config overrides global config through TOML deep merge. Persistent
user approval writes through a capability-specific adapter:

- filesystem and network grants write the current project's `local` profile;
  if missing, Psychevo creates it and sets `default_permissions = "local"`.
- exec grants append de-duplicated `[[exec_policy.rules]]` entries.
- MCP grants write to the server/tool definition layer where the server was
  defined.
- skill grants write to the active profile tools section.

External capability tools may define additional rule families when the owning
capability spec requires them. MCP startup and MCP tool calls use server/tool
approval metadata rather than legacy `Tool(pattern)` strings.

## Policy

Hard/protected denies cannot be bypassed by `dontAsk`, `bypassPermissions`,
`allow always`, session grants, or configured allow rules. They cover sensitive
write targets such as SSH, cloud credentials, shell rc files, `.env`, system
account/service files, and the project permissions configuration surface.

Protected reads are intentionally narrow. Internal Psychevo cache/index paths
that could inject stale or untrusted runtime material may be denied.

Filesystem reads, writes, and edits are evaluated against the active profile.
The current workdir is no longer the hard boundary for file tools; it is the
default workspace root used by built-in profiles. A profile grant may authorize
an absolute path outside the workdir, while protected denies still win.

Exec commands are evaluated by `exec_policy.rules`. Rules are ordered
de-duplicated token-prefix matches with decisions `allow`, `prompt`, and
`deny`. Shell network access is not host-intercepted; network risk in shell
commands is handled by exec approval.

Network permissions apply to built-in network-capable operations such as
`web_fetch` and managed MCP HTTP/SSE access. Approval prompts default to the
actual host. Configuration may express broader domain or wildcard policy.

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

The original tool call is suspended while approval is pending. Allow decisions
resume the original call; deny decisions return an explicit permission-denied
error instructing the model not to retry the same operation.

Session grants are scoped to one runtime session. `allow always` persists
through the relevant adapter only when the action supports persistent grants.
Ordinary deny is one-shot for filesystem, exec, MCP, and skill. Network
prompts may also offer a persistent host/domain deny.

When no approval handler is available, actions that would prompt fail closed.
There is no silent allow fallback.

Denied or timed-out approvals become structured tool-result errors or
before-agent-start rejection through the owning tool, resource, or runtime
contract, so the model or caller can explain the denial or choose a safer
action.

`approvals_reviewer = "smart"` runs a restricted reviewer with a strict JSON
contract. The reviewer receives recent session context, the exact action, and
optional `[auto_review].policy` text. The reviewer must answer allow or deny
with a rationale. Review failure or timeout is a denial. Repeated denials in
one turn may interrupt the turn. The user may explicitly override the most
recent smart denial with `/approve once|session|always`.

## Acceptance Criteria

- Hard/protected denies win over configured allow, profile grants, session
  grants, and approval reviewer outcomes.
- Legacy global permission fields are rejected with migration diagnostics.
- `granular` requires all current family booleans to be explicit.
- Bash dangerous-command detection covers representative recursive delete,
  shell-pipe installer, interpreter inline execution, destructive git, process
  kill, service, permission, and SQL destructive commands.
- Filesystem grants match canonical paths inside or outside the workdir.
- No-handler approval paths fail closed.
- `approval_policy = "never"` denies prompt-level actions without showing UI.
- `allow always` writes project-local TOML through the correct adapter and
  skips exact duplicate grants.
- Smart reviewer uses fake/test providers in validation, fails closed on
  timeout or malformed output, and never persists grants automatically.
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
- [027 ACP](../027-acp/spec.md) owns ACP permission-request projection for
  runtime asks.
