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

Configured sandbox policy is the baseline. For direct model-visible file
mutation tools, an interactive approval may create a bounded, in-memory
sandbox write grant for the approved target paths when the effective sandbox
mode is `workspace-write` and the only sandbox violation is writing outside
the configured writer roots. `allow once` applies only to the current tool
call. `allow session` applies only to the current runtime session and the same
filesystem authorization key. `allow always` must not persist sandbox writer
roots; permanent sandbox widening requires explicitly editing
`[sandbox].writable_roots`.

Hard policy still fails closed without widening. Approval policy `never`,
granular filesystem approval disabled, `dontAsk`/`bypassPermissions`, protected
permission denies, read-only effective sandbox mode, and product plan-only
runtime constraints must not create sandbox write grants. `bypassPermissions`
bypasses permission prompts only; it does not bypass sandbox enforcement.

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

`mode = "workspace-write"` makes the canonical workdir writable for built-in
writers and shell children. `writable_roots` adds extra writable roots. Each
entry may be absolute or workdir-relative.

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

Effective write roots are canonicalized with deepest-existing-ancestor
resolution. This prevents `..`, symlink, and sibling-prefix escapes, while still
allowing missing file tails for create operations.

## Enforcement

Writer enforcement applies before mutation. `write`, `edit` replace mode, and
patch add/update/delete/move operations must validate every source and
destination path that will be modified. A denial must not create, delete, move,
or rewrite files. When a permission approval creates a sandbox write grant,
writer enforcement may allow exactly the approved canonical target paths for
the approved tool call, or for subsequent calls with the same session grant.
The grant also permits creating missing parent directories needed to materialize
that exact target, but it must not allow sibling paths or broaden global writer
roots.

Shell enforcement is selected by platform:

- macOS uses Seatbelt through `/usr/bin/sandbox-exec`
- Linux uses Landlock
- WSL2 uses the Linux Landlock path
- native Windows is unsupported in v1

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
- Effective policy canonicalizes workdir, writable roots, tmp roots, and cache
  roots without creating missing paths.
- Built-in writers allow writes inside effective writer roots and deny writes
  outside them, including parent escape, symlink escape, sibling prefix
  collision, missing target tail, and patch move source/destination cases.
- `read-only` mode denies writer mutations and gives shell children no writable
  roots.
- Sandboxed shell children include the `PSYCHEVO_SANDBOX*` markers.
- Sandbox-enabled `tty=true` is rejected before spawn.
- Backend-unavailable and native Windows cases fail closed.
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
- [220 pevo Gateway](../220-pevo-gateway/spec.md) owns Gateway command and
  `shell/start` projection.
