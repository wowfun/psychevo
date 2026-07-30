---
name: 045. Sandbox
psychevo_self_edit: deny
---

# 045. Sandbox

Define Psychevo's v1 local sandbox enforcement for model-visible coding
operations. Sandbox enforcement sits below permissions: permissions decide
whether an operation may be attempted, while sandbox policy constrains where
that operation can write when it runs inside the local environment defined by
[040 Environment](../040-environment/spec.md).

## Scope

- sandbox configuration and effective runtime policy
- filesystem write containment for built-in `write` and `edit`
- native OS shell containment for `exec_command`, yielded `write_stdin`
  sessions, user shell commands, and Gateway `shell/start`
- sandbox status reporting through a read-only `/sandbox` command
- observable fail-closed behavior when sandbox enforcement is unavailable
- acceptance criteria for deterministic local validation

Out of scope:

- whole-process sandboxing of the Psychevo runtime
- hiding filesystem reads, credentials, provider state, or environment from the
  agent process
- network sandboxing
- container, remote, or cloud sandbox providers
- sandboxing MCP stdio servers, LSP helpers, managed tool downloads, internal
  Git probes, provider calls, skill loading, agent loading, hooks, or other
  in-process/auxiliary runtime paths

## Model

Sandbox v1 is write containment. It does not make the whole agent process
untrusted-safe. The only v1 hard guarantees are:

- built-in writer tools refuse writes outside effective writer roots
- sandboxed shell children are launched under the selected native OS backend
- sandbox-enabled shell execution fails closed if the backend cannot enforce
  the requested policy

Configured sandbox policy is the baseline. A harness-owned filesystem approval
may create a bounded in-memory writable root when effective mode is
`workspace-write` and the only sandbox violation is writing outside configured
roots. Exact-operation approval applies only to the suspended tool call. A
directory approval applies to the active root turn or runtime session and uses
the same canonical root as permission policy. Filesystem prompts never persist
sandbox roots; permanent widening requires explicitly editing
`[sandbox].writable_roots`.

Hard policy still fails closed without widening. Approval policy `never`,
granular filesystem approval disabled, `dontAsk`/`bypassPermissions`, protected
permission denies, read-only effective sandbox mode, and product plan-only
runtime constraints must not create sandbox write grants. `bypassPermissions`
bypasses permission prompts only; it does not create implicit writable roots or
bypass sandbox enforcement.

`plan` remains a read-only runtime mode. It may still expose read-only shell
exploration through `exec_command`, but effective sandbox mode is read-only:
writer tools are unavailable or denied, and sandboxed shell children receive no
writable roots.

Sandbox v1 intentionally follows a terminal/file boundary rather than a
whole-process boundary. MCP servers, LSP servers, managed helper installers,
skill and agent loading, hooks, provider clients, and internal runtime probes
may still run outside this sandbox. Status output must call these paths
`not-confined` so operators do not mistake v1 for whole-process containment.

## Configuration

Configuration lives under `[sandbox]`:

```toml
[sandbox]
enabled = false
mode = "workspace-write" # workspace-write | read-only
writable_roots = []
include_tmp = true
include_common_caches = true
```

`enabled = false` is the default for non-Plan execution and preserves existing
behavior there. Effective-policy calculation always resolves and validates the
configured baseline before applying runtime narrowing. Plan forces sandbox
enabled and read-only even when the configured value is disabled, and clears
configured, approved, temporary, cache, and other writable roots.

`mode = "workspace-write"` makes the canonical cwd writable for built-in
writers and shell children. `writable_roots` adds extra writable roots. Each
entry may be absolute or cwd-relative.

`mode = "read-only"` makes writer tools fail with a sandbox denial and runs
shell children with no writable roots. It is a hard sandbox mode in Psychevo
v1; user approval does not convert it into workspace-write.

`include_tmp` and `include_common_caches` apply only to shell sandboxing. They
do not expand model-visible `write` or `edit`. When enabled, Psychevo adds only
roots that already exist. Missing cache directories must not be created for the
sandbox policy.

Common cache roots are best-effort development caches that reduce false
failures for build and test commands. They may include `XDG_CACHE_HOME` or
`~/.cache`, Cargo/Rustup, npm/pnpm/yarn, pip, Go, Gradle, and Maven caches
where those paths are discoverable from the inherited environment.

Shell children in `workspace-write` mode may also receive narrow write rules
for `/dev/null` and `/dev/zero` when those devices exist. These are shell-only
compatibility sinks for commands that open standard sink devices with write or
read-write flags; they must not add `/dev` as a writable root and must not
expand model-visible `write` or `edit`.

Effective write roots and targets use the shared filesystem identity from
[041 Permissions](../041-permissions/spec.md): existing targets follow
symlinks/junctions, while missing targets canonicalize the deepest existing
ancestor and append the normalized tail. This prevents `..`, symlink, junction,
and sibling-prefix escapes while allowing create operations.

## Enforcement

Writer enforcement applies before mutation. `write`, `edit` replace mode, and
patch add/update/delete/move operations must validate every source and
destination path that will be modified. A denial must not create, delete, move,
or rewrite files. When a writer target is under a shell-only extra root such as
a temporary or cache root, the denial should explain that the root is writable
only for sandboxed shell children and does not expand model-visible writers.
When a permission approval creates a sandbox write grant, writer enforcement
allows exactly the approved canonical targets for that call or descendants of
an approved turn/session directory. Turn and session roots also join the
effective writable roots used to build each later sandboxed shell-child policy;
the exec command still passes independent permission review. Grants permit
creating missing descendants but do not broaden global configured roots.

Shell enforcement is selected by platform:

- macOS uses Seatbelt through `/usr/bin/sandbox-exec`
- Linux uses Landlock
- WSL2 uses the Linux Landlock path
- native Windows uses a private pipe-only advisory restricted-token backend

The Linux Landlock crate is a Linux-only dependency. Native Windows builds must
not compile or link Landlock.
Landlock and Seatbelt shell-enforcement helper code must be compiled only on
the platforms that can use those backends.

The Windows backend creates a restricted token with
`DISABLE_MAX_PRIVILEGE | LUA_TOKEN | WRITE_RESTRICTED`, adds a process-specific
restricting SID that receives no write ACL, and retains the current user,
logon, and World SIDs as Git Bash/MSYS compatibility identities. The token
default DACL grants those compatibility SIDs `GENERIC_ALL` for private
pipes/IPC objects and excludes the process-specific identity SID.

This native Windows Plan backend is advisory defense in depth, not a filesystem
write boundary: retaining the token user SID is required for Git Bash to open
MSYS's existing per-user `CreateFileMapping` objects, and consequently the
restricted token can still write locations writable by the signed-in user.
Plan-mode tool selection, the immutable Plan permission ceiling, and the
Plan-mode agent instruction remain the primary controls. `/sandbox` and child
environment markers must identify this backend as
`windows-restricted-token-advisory` and report helper/filesystem/network
enforcement as `not-confined`; no UI or log may describe it as read-only
filesystem confinement.

The backend launches Git Bash with `CreateProcessAsUserW` and assigns it to a
kill-on-close Job before execution is exposed to the caller. It preserves pipe
streaming, yielding, stdin polling, and abort. It does not add a sandbox user,
service, filesystem ACL rewrite, WFP/network proxy, private desktop, ConPTY, or
command blacklist. Token, default-DACL, process creation, or Job assignment
failure fails closed; runtime never falls back to an unrestricted child.
Restricted process creation uses an extended startup attribute list whose
handle whitelist contains only that invocation's stdin, stdout, and stderr
child ends. Enabling handle inheritance must never expose pipe or IPC handles
owned by another concurrent invocation.

If `[sandbox].enabled = true` and the platform backend is unsupported, missing,
or reports that policy was not enforced, shell execution fails closed. It must
not silently run unconfined.

Sandboxed shell children receive these environment markers:

- `PSYCHEVO_SANDBOX=1`
- `PSYCHEVO_SANDBOX_MODE`
- `PSYCHEVO_SANDBOX_BACKEND`
- `PSYCHEVO_SANDBOX_HELPERS=not-confined`

`tty=true` is unsupported while sandbox is enabled in v1. Existing yielded
sessions preserve streaming, reader threads, abort handling, timeouts, session
IDs, and `write_stdin` polling semantics. An empty `write_stdin` request may
poll a session in every mode. A non-empty request is checked against the
current invocation policy before session lookup and is denied in effective
read-only mode even when the session was created by an earlier Execute Turn
with the same task identity. In writable modes, non-empty stdin remains allowed
only for stdin-capable sessions; v1 sandboxed sessions do not add new stdin
support.

Denials use the wording:

```text
denied by sandbox policy: <reason>
```

The reason should name the mode, backend, or violated root when useful. Denials
must not use redirect-style language that implies the model should retry the
same operation elsewhere.

## Status

`/sandbox` is a read-only command. It reports:

- configured enabled state and effective mode
- platform and backend
- shell enforcement: `confined`, `disabled`, `unsupported`, or `not-confined`
- writer enforcement: `confined` or `disabled`
- helper enforcement: `not-confined` for LSP, MCP, managed tools, skills,
  agents, hooks, provider calls, and internal probes
- writer roots and shell-only extra roots
- network status: `not-confined` in v1

Gateway and Workbench expose the same status through the normal command
surface; v1 does not add new RPC request fields.

## Acceptance Criteria

- Default config keeps sandbox disabled for non-Plan execution. Plan is always
  effectively enabled and read-only with no writable roots.
- Invalid sandbox modes fail config loading with a clear diagnostic.
- Effective policy canonicalizes cwd, writable roots, tmp roots, and cache
  roots without creating missing paths.
- Built-in writers allow writes inside effective writer roots and deny writes
  outside them, including parent escape, symlink escape, sibling prefix
  collision, missing target tail, and patch move source/destination cases.
- Exact-operation, turn-directory, and session-directory grants use the same
  canonical identity as permission review, expire at the correct lifecycle,
  and affect both built-in writers and subsequently launched shell children.
- Built-in writer denial for shell-only temp/cache roots clearly says the path
  is shell-only and does not imply that `bypassPermissions` can bypass sandbox
  enforcement.
- In `workspace-write` mode, sandboxed shell children may open `/dev/null` and
  `/dev/zero` for writing when those devices exist, while built-in writer roots
  remain unchanged.
- `read-only` mode denies built-in writer mutations and gives enforced
  macOS/Linux/WSL shell children no writable roots. Native Windows shell
  children retain the explicitly advisory boundary above.
- Sandboxed shell children include the `PSYCHEVO_SANDBOX*` markers.
- Sandbox-enabled `tty=true` is rejected before spawn.
- Linux requests Landlock ABI V3 with hard compatibility and accepts only a
  fully enforced ruleset. Partial or unavailable enforcement fails closed.
  Linux-only Landlock dependencies and unused Unix helpers are not compiled
  into native Windows builds.
- macOS and Linux/WSL smoke tests verify inside-root write allowed and
  outside-root write denied when the backend is available.
- native Windows Git Bash smoke verifies startup, reads, yielding, abort, Plan
  instruction presence, and truthful advisory markers; it does not assert
  filesystem write denial.
- Native Windows token construction proves the process-specific identity SID is
  absent from the token default DACL while the user, logon, and World
  compatibility identities allow Git Bash/MSYS IPC initialization.
- User shell and Gateway `shell/start` use the same effective sandbox policy as
  model `exec_command`.
- `/sandbox` reports helper paths and network as `not-confined`.
- Validation uses deterministic local harnesses. Real provider, live network,
  or host-global state validation is opt-in only.

## Related Topics

- [040 Environment](../040-environment/spec.md) defines the local host
  environment and authority boundary that sandbox enforcement constrains.
- [041 Permissions](../041-permissions/spec.md) defines policy gates that run
  before sandbox enforcement.
- [110 Coding Core Tools](../110-coding-core-tools/spec.md) defines the
  model-visible tools whose write and shell behavior this topic constrains.
- [200 pevo CLI](../200-pevo-cli/spec.md) owns CLI invocation flags and slash
  command projection.
- [240 pevo Web](../240-pevo-web/spec.md) owns Workbench command and
  `shell/start` projection.
