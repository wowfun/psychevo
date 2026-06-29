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
- Managed server reuse proves pid liveness, executable fingerprint,
  non-deleted executable state, static asset directory, asset mode, bind
  address, and active profile compatibility.
- Default managed binds prefer `127.0.0.1:58080` and may fall back through the
  managed range; explicit binds are strict.
- Launch URLs carry opaque one-time material only, never raw absolute cwds,
  and establish browser-session authorization before redirecting to the clean
  Web Shell URL.
- Direct visits to the managed base URL without a valid browser session show a
  launch-required diagnostic rather than mounting a broken Workbench.

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
- Default bind fallback and explicit bind strictness are observable in
  lifecycle responses.
- `--dir`, `--default-workspace`, `--print-url`, and `--no-browser` produce a
  launch entry with clean browser recovery behavior.
- Consumed or expired launch URLs recover cleanly with and without a valid
  browser-session cookie.
- Direct visits to the managed base URL without a valid browser session show a
  launch-required diagnostic rather than mounting a broken Workbench.

## Validation Boundaries

- Deterministic tests should use fake or test providers and isolated local
  state, not the user's normal config, browser profile, credentials, or global
  Gateway state.
- Tests should assert lifecycle JSON, managed state, bind, token, and launch
  invariants rather than private server implementation details.
- Browser-visible Workbench behavior is validated by `240`.
