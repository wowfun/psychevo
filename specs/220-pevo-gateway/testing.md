---
name: 220. pevo Gateway Testing
psychevo_self_edit: deny
---

# 220. pevo Gateway Testing

Define acceptance expectations and validation scenarios for the managed
`pevo gateway` product lifecycle.

## Long-Term Acceptance Contract

- `pevo gateway` with no subcommand is equivalent to `pevo gateway open`, and
  `pevo web` is a top-level convenience alias for the same managed open flow.
- Lifecycle commands emit exactly one machine-readable JSON object to stdout.
- Managed state under `$PSYCHEVO_HOME/gateway/` separates non-secret server
  metadata from owner-readable bearer token material.
- Managed server reuse proves the instance lease, instance id, OS process
  ownership, authenticated Gateway identity, executable fingerprint,
  non-deleted executable state, static asset directory, asset mode, bind
  address, and active profile compatibility.
- Default managed binds prefer `127.0.0.1:58080` and may fall back through the
  managed range; explicit binds are strict.
- Launch URLs carry opaque one-time material only, never raw absolute cwds,
  and establish browser-session authorization before redirecting to the clean
  Web Shell URL.
- Direct visits to the managed base URL without a valid browser session show a
  launch-required diagnostic rather than mounting a broken Workbench.
- Fingerprinted `/assets/` responses are immutable-cacheable, while HTML, SPA
  fallbacks, and non-fingerprinted files are `no-store`; static reads preserve
  existing content-type and authorization behavior.
- `stop` requests authenticated shutdown, waits for managed cleanup, uses only
  the verified process group or Windows Job Object as fallback, and proves both
  the managed server and an exercised fake ACP Agent child have exited before
  reporting success.
- Windows Job inspection retains sufficient rights to wait for the verified
  tree. Managed Web lifecycle coverage asserts the first stop succeeds and
  clears state, while a second idempotent stop reports no running server.

## Current Implementation Slice

CI/CD vocabulary and generic validation boundaries follow
[065 CI/CD](../065-ci-cd/spec.md).

The deterministic managed-lifecycle validation path uses isolated
`PSYCHEVO_HOME`, managed state, config, SQLite state, and cwd. Workbench UI
behavior belongs to [240 pevo Web Testing](../240-pevo-web/testing.md).

Manual real-provider validation is not required for this lifecycle topic.

## Scenario Matrix

- `open`, `start`, `status`, `stop`, `restart`, default `gateway`, and
  `pevo web` preserve the JSON stdout contract.
- Managed state with missing, stale, mismatched, or deleted executable metadata
  is rotated or reported stale instead of reused as healthy.
- Invalid state, a missing instance id, a free instance lease, an exited
  process, process identity mismatch/unavailability, and Gateway identity
  mismatch/unavailability produce their specified machine-readable stale
  reasons. A held lease whose owner cannot be proven fails closed without
  rotating state/token, starting another server, or signaling the recorded pid.
- A stale Windows state whose pid was reused by an unrelated test process does
  not terminate that process. A proven managed Job is stopped as a tree and
  leaves neither the server nor its deterministic child alive.
- Two concurrent managed open calls serialize through the lifecycle lock and
  return the same pid, instance id, and base URL.
- Managed startup failure leaves stdout unpolluted and reports both the bounded
  current-attempt child output and the full `server.log` path on stderr without
  replaying earlier appended log entries.
- Default bind fallback, explicit nonzero bind strictness, and explicit port-0
  ephemeral assignment are observable in lifecycle responses.
- `-C/--cd`, `--default-workspace`, `--print-url`, and `--no-browser` produce a
  launch entry with clean browser recovery behavior.
- Managed Web and Gateway-open argument parsing reject the removed `--dir`
  spelling.
- Consumed or expired launch URLs recover cleanly with and without a valid
  browser-session cookie.
- Direct visits to the managed base URL without a valid browser session show a
  launch-required diagnostic rather than mounting a broken Workbench.
- A real managed `serve` subprocess starts a deterministic resident ACP Agent;
  `gateway stop` must leave neither process alive and must remove managed state
  only after the server exits.
- The final managed-stop fallback targets the exact process group/tree created
  for the managed server and kills a deterministic child even when both parent
  and child ignore graceful SIGTERM.
- Managed identity and shutdown require the owner token; shutdown with a
  mismatched instance id returns conflict without triggering cleanup, and the
  routes are absent from non-managed `pevo serve`.
- A recoverable launch failure performs at most one ownership recheck,
  replacement, and retry; a second recoverable failure is returned.

## Validation Boundaries

- Deterministic tests should use fake or test providers and isolated local
  state, not the user's normal config, browser profile, credentials, or global
  Gateway state.
- Tests should assert lifecycle JSON, managed state, bind, token, and launch
  invariants rather than private server implementation details.
- Windows Git Bash smoke covers dead-state recovery (including the former
  connection-refused/10061 case), PID reuse safety, and Job-tree cleanup on a
  modern Windows host. Host-specific tests may be target-gated, but their
  deterministic helpers remain covered on every platform.
- Browser-visible Workbench behavior is validated by `240`.
