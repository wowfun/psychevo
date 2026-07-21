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
- native Windows enforcement
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

`enabled = false` is the default and preserves existing behavior.

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
- native Windows is unsupported in v1

The Linux Landlock crate is a Linux-only dependency. Native Windows builds must
not compile or link Landlock; they report the sandbox backend as `unsupported`
and fail closed when sandbox enforcement is enabled.
Landlock and Seatbelt shell-enforcement helper code must be compiled only on
the platforms that can use those backends.

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
IDs, and `write_stdin` polling semantics. Non-empty stdin remains allowed only
for stdin-capable sessions; v1 sandboxed sessions do not add new stdin support.

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

- Default config keeps sandbox disabled and preserves existing behavior.
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
- `read-only` mode denies writer mutations and gives shell children no writable
  roots.
- Sandboxed shell children include the `PSYCHEVO_SANDBOX*` markers.
- Sandbox-enabled `tty=true` is rejected before spawn.
- Backend-unavailable and native Windows cases fail closed without compiling
  Linux-only Landlock dependencies or unused native shell-enforcement helpers
  into native Windows builds.
- macOS and Linux/WSL smoke tests verify inside-root write allowed and
  outside-root write denied when the backend is available.
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
