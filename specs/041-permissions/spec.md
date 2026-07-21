---
name: 041. Permissions
psychevo_self_edit: deny
---

# 041. Permissions

Define Psychevo's runtime permission policy for local resource and tool
operations. This topic is the source of truth for permission profiles,
approval policies, transparent approval routing, persistent grants, and
dangerous-action policy.

## Scope

- runtime permission policy before local resource operations and tool execution
- relationship between runtime mode, tool visibility, permission profiles,
  approval policy, and approval reviewer
- structured permission profiles for filesystem, network, and tool families
- canonical filesystem identity shared by policy, approval, and execution
- exec policy rule language and precedence
- hard/protected denies, protected reads, dangerous exec command policy, and
  fail-closed behavior
- approval choices, session grants, persistent grant adapters, and fallback
  behavior when no approval handler is available
- observable denial, timeout, and approval failure behavior
- acceptance criteria for deterministic permission validation

Out of scope:

- operating-system sandbox isolation, security guarantees, containerization,
  or process isolation, which are owned by [045 Sandbox](../045-sandbox/spec.md)
- concrete terminal rendering details beyond the approval-flow contract
- concrete tool result JSON fields beyond requiring permission outcomes to be
  observable through the owning tool contract
- Rust APIs and event names
- provider authentication, credential storage, secret redaction, or remote
  policy services

## Model

Psychevo permissions are a runtime policy gate before local resource operations
and tool execution inside the local environment defined by
[040 Environment](../040-environment/spec.md). They are not an
operating-system sandbox in this slice.
[045 Sandbox](../045-sandbox/spec.md) defines the separate enforcement layer
that may further constrain already-authorized writes or shell children.
Tool availability, runtime mode, permission mode, approval mode, and persistent
policy are separate concerns.

Runtime mode controls the hard ceiling for model-visible tools. `plan` remains
a read-only runtime mode, and `default` may expose normal editing tools when the
tool surface and caller entrypoint allow them. Permission policy must not expand
the current runtime-mode ceiling.

Permission profiles define the baseline capability boundary. Built-in profiles
are `:read-only`, `:workspace`, and `:danger-full-access`; project profiles may
extend another profile and add filesystem paths, network hosts/domains, and
tool-family grants. Profiles are policy gates, not OS sandboxes. `:read-only`
and `:workspace` may read any host path unless an explicit or protected read
deny wins. `:workspace` may write inside the canonical cwd and asks before
writing outside it. Filesystem profile entries may grant paths outside the
current cwd without changing the session cwd.

`approval_policy` controls whether an action that needs consent may ask:

- `on-request`: ask the configured reviewer when a profile, exec policy, MCP
  rule, skill action, or network policy requires approval.
- `untrusted`: currently equivalent to `on-request`; it is reserved for a
  future project-trust model.
- `never`: never ask. Profile-allowed actions may run, but actions that would
  prompt fail closed.
- `granular`: uses `[approval.granular]` booleans to enable or disable approval
  prompts per family. The table must explicitly set `filesystem`, `network`,
  `exec`, `mcp`, and `skill`.

`on-failure` is not supported because Psychevo does not provide a sandboxed
retry mechanism for it.

`approvals_reviewer` controls who reviews asks:

- `user`: route approval requests to the active UI/protocol handler.
- `smart`: route approval requests to a restricted reviewer model session. The
  reviewer may only approve the current action once, never persist grants.
  Timeout, provider failure, malformed output, or missing reviewer support all
  fail closed.

Agent frontmatter may use the legacy `permissionMode` alias, but an agent can
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
justification = "project workflow uses git pull before local sync"
match = ["git pull --ff-only"]
not_match = ["git push"]

[[exec_policy.rules]]
prefix = ["git", ["status", "diff", "show"]]
decision = "allow"

[[exec_policy.host_executables]]
name = "git"
paths = ["/usr/bin/git", "/opt/homebrew/bin/git"]
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
policy changes write through an explicit capability-specific configuration or
rule-management adapter:

- filesystem profile edits write the current project's `local` profile; if
  missing, Psychevo creates it and sets `default_permissions = "local"`.
- network approvals may persist through the current project's `local` profile.
- exec grants append de-duplicated `[[exec_policy.rules]]` entries using
  parsed command prefixes rather than whitespace fragments from the raw shell
  text.
- MCP grants write to the server/tool definition layer where the server was
  defined.
- skill grants write to the active profile tools section.

External capability tools may define additional rule families when the owning
capability spec requires them. MCP startup, MCP tool calls, MCP resource reads,
MCP prompt gets, MCP elicitation, and MCP sampling use MCP action metadata
rather than legacy `Tool(pattern)` strings. The first MCP implementation slice
uses the existing `mcp` granular approval family for all MCP sub-actions while
retaining the sub-action label for review prompts, evidence, and future policy
specialization.

## Policy

Hard/protected denies cannot be bypassed by `dontAsk`, `bypassPermissions`,
session grants, or configured allow rules. Filesystem hard denies are minimal:
they protect the active Psychevo permission configuration and other runtime
state whose mutation would let a model widen its own authority. Credential
files, SSH and cloud configuration, shell rc files, `.env`, and ordinary system
configuration are not blanket hard denies. They follow the same canonical
workspace, explicit rule, and external-write consent policy as other files.
System shutdown/reboot hard denies match executable command positions, including
common wrappers such as `sudo`, `env`, `exec`, `nohup`, and `setsid`; ordinary
arguments or quoted literals that merely contain words such as `shutdown`,
`reboot`, `poweroff`, or `halt` must not trigger the hard deny.

Protected reads are intentionally narrow. Internal Psychevo cache/index paths
that could inject stale or untrusted runtime material may be denied.

Filesystem reads, writes, and edits are evaluated against the active profile.
The current cwd is not a hard boundary for file tools; it is the default
auto-write root used by built-in profiles. External writes are capabilities the
user may grant at runtime, not security violations by definition.

Permission policy and tool execution use one canonical host path identity. For
an existing path the identity follows symlinks or junctions. For a missing
target it canonicalizes the deepest existing ancestor and appends the remaining
normalized path. The canonical cwd and configured roots use the same resolver.
Containment, protected-path checks, grant matching, approval display, sandbox
roots, and final tool access must not use conflicting lexical identities.

The runtime re-resolves a filesystem target immediately before mutation. If
the identity no longer equals the reviewed target or falls outside the granted
root, the operation fails with an observable `path_identity_changed` error and
must not mutate either location.

Relative filesystem profile entries are human path strings relative to the
current cwd. They use `/` separators after host normalization and must not
require file-URI percent encoding for ordinary path characters such as spaces.
Matching compares the canonical target expressed relative to the canonical
cwd, so an external sibling remains addressable as `../sibling/...` instead of
silently changing to an absolute rule identity. Targets on a host root that
cannot be expressed relative to cwd require an absolute entry.
Absolute filesystem entries use canonical host path identity for containment
matching. A lexical path inside cwd that resolves outside cwd is external; a
lexical path outside cwd that resolves inside it receives the inside-cwd policy.

Exec commands are evaluated in three layers:

1. hard/protected deny and background-process deny
2. configured `exec_policy.rules`
3. command safety classification for known-safe, dangerous, and unknown

Background-process deny is based on shell syntax, not raw substring scanning.
Foreground commands and heredocs whose quoted content merely contains `&` must
not be treated as background wrappers. True background execution with `&`, and
wrappers that detach work such as `nohup`, `setsid`, and `disown`, remain hard
denies because Psychevo cannot track their lifecycle.

`exec_policy.rules` are parsed token-prefix rules with decisions `allow`,
`prompt`, and `deny`. A prefix token may be either a string or a list of string
alternatives. Optional `justification` is user-facing rationale. Optional
`match` and `not_match` entries are load-time self-tests; they are tokenized
and validated when configuration loads, and any failed self-test rejects the
configuration. They are not runtime conditions.

`exec_policy.host_executables` may define executable basenames and allowed
absolute paths. When enabled for a name, an absolute executable path matches a
basename rule only if the path is listed for that name. If no host executable
entry exists for a basename, basename fallback may match an absolute path to
that basename.

Known-safe exec commands are read-only exploration commands and safe shell
compositions of those commands, including a bounded read-only subset such as
`pwd`, `ls`, `cat`, `wc`, `rg`, `sed -n`, and read-only `git` subcommands.
Known-safe exec is allowed by `:workspace` and `:read-only` profiles unless a
hard deny or explicit policy rule overrides it. Dangerous commands require
approval or denial according to the active approval policy. Unknown commands
inside `:workspace` may run only when they are not high-capability unknowns.

Inline interpreters such as `python -c`, `python3 -c`, `node -e`, `perl -e`,
and `ruby -e` are high-capability unknowns rather than blanket dangerous
commands. First-slice resource-aware auto-allow is limited to inline scripts
that can be statically recognized as literal file reads and whose literal paths
are already allowed by filesystem permissions. Inline scripts with dynamic
paths, writes, subprocess/process control, network access, `eval`, or otherwise
unrecognized behavior require approval. Shell network access is not
host-intercepted; network risk in shell commands is handled by exec approval.

Network permissions apply to built-in network-capable operations such as
`web_fetch` and managed MCP HTTP/SSE access. The built-in `:workspace` profile
allows `web_fetch` by default, matching the product stance that ordinary web
research is a read operation; shell network risk remains covered by exec
approval, and managed MCP/network services keep their own approval gates.
Explicit profile network rules are still evaluated first: `deny` blocks and
`prompt` asks even though the default for `web_fetch` is allow. Explicit
`allow` records trust for the host or domain. When a network action does
prompt, approval prompts default to the actual host. Configuration may express
broader domain or wildcard policy.

Permission policy applies after tool visibility and before or during execution.
A model-visible tool declaration says what the model may request, not what the
runtime must execute. Runtime and resource gates remain responsible for the
final allow, deny, or defer decision.

## Approval

Ordinary approval choices are allow once, allow session, allow always, and
deny. Filesystem mutation asks instead use a harness-owned scope contract:

- allow the exact operation once
- allow a selected canonical directory for the current turn
- allow a selected canonical directory for the current session
- deny

Filesystem directory approval is not persistent. Durable writable roots remain
an explicit profile or sandbox configuration change rather than an incidental
tool-call prompt.

The original tool call is suspended while approval is pending. The harness,
not the model, creates the approval request. Allow decisions resume the
original call; deny decisions return an explicit permission-denied error. No
model-visible `request_permissions` tool is part of this contract.

A filesystem approval request contains the operation, every requested path,
every canonical resolved path, and selectable canonical ancestor directories.
When requested and resolved paths differ, review surfaces show both. Scope
selection is collapsed by default so the common path is one-step exact
approval. Multi-target operations offer directory scope only through canonical
ancestors common to every mutation target. A client may submit only a scope
offered by the runtime. Its reason explains the policy boundary without
repeating target paths already carried by the structured filesystem payload.

For direct file-mutation tools, a permission approval is also the decision that
[045 Sandbox](../045-sandbox/spec.md) consumes to create an exact-operation,
turn, or session in-memory writable root. The same root applies to built-in
writers and later sandboxed shell children; it never auto-approves an exec
command and cannot bypass hard sandbox mode.

For a multi-target mutation, authorization is composed per canonical target.
Each target must be allowed either by the base permission profile or by a
matching in-memory filesystem scope; a cwd target must not invalidate a scope
that covers a different external target in the same operation. Any hard or
explicit deny on any target still denies the whole operation.

The runtime keeps an in-process FIFO of pending approval requests. Approval
request and response hooks may observe a request before it is shown and after
it resolves; hooks must not be required for the approval result and must not
write durable transcript events. Session cleanup, TUI exit, and abort paths
must release all pending approvals with deny/abort semantics and wake suspended
calls. [035 Event Stream](../035-event-stream/spec.md) defines the shared
blocking-action projection lifecycle used by public streams.
tool calls.

Turn grants are shared by permission runtimes participating in the active root
turn, including child agents, and are cleared when that turn completes or is
interrupted. Session grants are scoped to one runtime session and are cleared
on session cleanup. `allow always` persists through the relevant adapter only
for non-filesystem actions that support persistent grants.
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
- `granular` requires all current family booleans to be explicit and has no
  unused `request_permissions` family.
- Bash dangerous-command detection covers representative recursive delete,
  shell-pipe installer, destructive git, process kill, service, permission,
  and SQL destructive commands.
- System shutdown/reboot hard denies trigger for command-position invocations
  while avoiding quoted SQL/text false positives.
- Known-safe command classification covers representative read-only commands
  and safe shell compositions.
- Inline interpreter classification allows only literal already-authorized file
  reads without prompting; unrecognized inline behavior prompts.
- `exec_policy.rules` support token alternatives, `justification`,
  `match`/`not_match` self-tests, and host executable path resolution.
- `:read-only` and `:workspace` allow external reads unless an explicit or
  protected deny wins; `:workspace` asks before canonical external writes.
- Filesystem policy catches cwd-internal symlink or junction escapes and allows
  external aliases that canonically resolve inside an allowed root.
- Missing write targets use their deepest existing canonical ancestor, so a
  missing child below a symlink cannot bypass policy.
- Exact, turn-directory, and session-directory grants match canonical paths,
  expire at their documented lifecycle, and cannot authorize an unoffered root.
- A target whose canonical identity changes after review is not mutated.
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

`WebSearch(pattern)` matches the actual search query. Hosted web search may be
declared only when effective rules statically prove an unconditional allow;
query-specific ask or deny remains enforceable through the local lane. See
[111 Web Search](../111-web-search/spec.md).

- [040 Environment](../040-environment/spec.md) defines local host environment
  and authority boundaries that permission policy specializes.
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
- [210 TUI Interaction](../210-pevo-tui/interaction.md) owns interactive
  mode and permissions projection.
- [027 ACP](../027-acp/spec.md) owns ACP permission-request projection for
  runtime asks.
- [045 Sandbox](../045-sandbox/spec.md) defines filesystem write containment
  and native OS shell sandbox enforcement beneath permission policy.
