---
name: 220. pevo Gateway Testing
psychevo_self_edit: deny
---

Define acceptance expectations and validation scenarios for the managed Gateway
product surface and Workbench Web Shell.

## Long-Term Acceptance Contract

- `pevo gateway` with no subcommand is equivalent to `pevo gateway open`, and
  `pevo web` is a top-level convenience alias for the same managed open flow.
- Lifecycle commands emit exactly one machine-readable JSON object to stdout.
- Managed state under `$PSYCHEVO_HOME/gateway/` separates non-secret server
  metadata from owner-readable bearer token material.
- Managed server reuse proves pid liveness, executable fingerprint, non-deleted
  executable state, static asset directory, asset mode, bind address, and active
  profile compatibility.
- Default managed binds prefer `127.0.0.1:58080` and may fall back through the
  managed range; explicit binds are strict.
- Launch URLs carry opaque one-time material only, never raw absolute workdirs,
  and establish browser-session authorization before redirecting to the clean
  Web Shell URL.
- Browser-session authorization is limited to workdirs granted by launch/open
  flows, browser workspace management, or visible stored session adoption.
- Direct Bearer API clients use explicit request scope and do not depend on
  browser launch cookies.
- Workbench entrypoints preserve source/thread binding, draft replacement,
  transcript projection, settings/runtime controls, files, review, terminal,
  status, and debug surfaces without changing Gateway wire contracts.
- Generated protocol schemas and clients preserve public `gatewaySchemas`,
  method names, event names, and wire shape compatibility.

## Current Implementation Slice

Automation vocabulary and generic validation boundaries follow
[060 Automation](../060-automation/spec.md).

The deterministic browser validation path uses isolated config, SQLite state,
and workdir. It should exercise the built Workbench through the managed launch
path, normally via `pevo gateway open --no-browser --print-url` or the e2e
harness that wraps that command.

Live model, live skill, and ACP peer validation are opt-in. They must use
isolated `PSYCHEVO_CONFIG`, `PSYCHEVO_DB`, workdir, and test artifacts, and
must not print tokens or secrets.

## Scenario Matrix

- `open`, `start`, `status`, `stop`, `restart`, default `gateway`, and
  `pevo web` preserve the JSON stdout contract.
- Managed state with missing, stale, mismatched, or deleted executable metadata
  is rotated or reported stale instead of being reused as healthy.
- Default bind fallback and explicit bind strictness are both observable in
  lifecycle responses.
- `--dir`, `--default-workspace`, `--print-url`, and `--no-browser` produce a
  launch entry with clean browser recovery behavior.
- Consumed or expired launch URLs recover cleanly with and without a valid
  browser-session cookie.
- Direct visits to the managed base URL without a valid browser session show a
  launch-required diagnostic rather than mounting a broken Workbench.
- Workbench starts and resumes threads for the authorized scope, reconciles
  draft sessions, and keeps history switching from stealing background turns.
- Composer submit, permission, clarify, command feedback, runtime controls,
  settings, files, review, terminal, status, downloads, and debug panels remain
  functional after reconnect.
- Desktop and narrow viewports preserve usable navigation and non-overlapping
  primary controls.
- Protocol generation or schema layout changes do not require application-level
  call sites to change public schema imports.

## Validation Boundaries

- Deterministic tests should use fake or test providers and isolated local
  state, not the user's normal config, browser profile, credentials, or global
  Gateway state.
- Browser tests should assert user-visible behavior and stable protocol
  invariants rather than private DOM structure when possible.
- Screenshots, traces, and live samples are required evidence for visual/live
  changes, but live provider failures must be reported separately from code
  regressions when caused by credentials, provider state, or environment.
- Live peer streaming checks must tolerate providers that complete before the
  first post-visibility growth sample. Tests may require visible text growth
  while a response is still incomplete, but a completed sentinel or final
  invariant already present in the first assistant message is also valid.
