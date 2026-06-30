---
name: 150. Plugin Runtime Testing
psychevo_self_edit: deny
---

Define acceptance expectations and validation scenarios for plugin runtime.

## Long-Term Acceptance Contract

- Installing a plugin materializes a package into the selected profile or
  project cache without copying content outside the package root.
- Installing does not enable a plugin; profile and project policy decide
  package enablement for each invocation.
- Project policy can override profile-installed plugin enablement without
  requiring a duplicate project install record.
- Runtime loads enabled plugin declarations before agent and skill discovery,
  then routes each declaration through the existing owning boundary.
- Plugin hook declarations load only when the plugin package is enabled, then
  remain subject to the hook system's normalized-hash trust review before
  execution.
- Worker contribution discovery and tool execution receive the same effective
  environment and bounded startup context.
- Worker startup, response, timeout, and tool errors degrade only the affected
  plugin contribution and produce secret-free diagnostics.
- Plugin tests use isolated stores, config, data roots, state paths, and fake or
  local providers by default.

## Current Implementation Slice

The current slice covers local directory and local Git installs, JSON store
records, TOML policy overlay, static skill/agent/hook roots, and stdio
JSON-RPC worker tools. MCP, provider, command, and toolset descriptors may be
recognized as declared resources, but executable routing remains owned by
their future boundaries.

Manual broad validation for code changes is still the Rust workspace gate
defined by [065 CI/CD](../065-ci-cd/spec.md), but this topic's
acceptance coverage should come from focused plugin runtime and CLI smoke tests.

## Scenario Matrix

- Local directory install writes an install record under the selected scope and
  materializes the package in the cache root.
- Local install rejects package symlinks and does not follow a symlink to copy
  package-external content into the cache.
- Local Git install works from a deterministic temporary Git repository.
- Profile-scope install, enable, and disable default to the active profile
  store/config.
- `--local` install writes the current cwd plugin store.
- `--local` enable and disable can resolve a profile-installed plugin selector
  while writing only current cwd `.psychevo/config.toml` policy.
- `--global` and `--local` conflict for plugin write commands.
- Project policy overlays profile policy for package enablement.
- Selector conflicts require `name@source`.
- Static skill roots, hook sources, and worker tool descriptors are loaded only
  when the plugin package is enabled, then routed through the owning runtime
  module.
- Plugin hook sources are listed but not executed when their normalized hook
  hashes are untrusted or modified.
- Worker-provided hook handlers are either routed through 140 Hook Runtime or
  reported as unsupported diagnostics until the worker hook adapter exists.
- Worker `contributions/list` receives the effective loaded environment so
  config-parent and project `.env` variables are available during discovery.
- Worker startup failures and discovery timeouts are reported by
  `plugin doctor` without crashing runtime assembly.
- Worker tool execution succeeds through the public tool surface adapter.
- Worker tool execution failures and timeouts return tool errors and keep
  diagnostics source-qualified.

## CLI Coverage

- Singular `pevo plugin` parses and obsolete plural `pevo plugins` rejects.
- `plugin list`, `view`, and `doctor` support secret-free JSON output.
- `plugin install`, `uninstall`, `enable`, and `disable` honor default,
  `--local`, and `--global` scope semantics.
- `plugin marketplace list/add/remove` manages local/Git source catalogs
  separately from plugin enablement policy.
- CLI smoke tests cover a profile-installed plugin enabled from project-local
  policy and visible through `pevo run` without contacting a live provider.

## Validation Boundaries

- Tests must use isolated `PSYCHEVO_HOME`, cwd, config, database, and
  plugin data paths.
- No default test contacts a network or live provider.
- Worker tests should use tiny local fixture workers with bounded stdout/stderr
  and no host-global config access.
- Timeout tests must use bounded local workers and must not rely on wall-clock
  sleeps longer than the runtime timeout under test.
