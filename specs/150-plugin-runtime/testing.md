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
- Codex catalog and Apps tests use a deterministic fake app-server process and
  never depend on the user's `CODEX_HOME`, credentials, or network.

## Current Implementation Slice

The current slice covers local directory and local Git installs, JSON store
records, TOML policy overlay, static skill/agent/hook roots, source-scoped MCP
and toolset descriptors, stdio JSON-RPC worker tools, and worker hook handlers.
Provider and command descriptors remain inert. Interface metadata is display
only. Codex-owned Apps execute through the broker; portable components from an
exposed installed package root enter their native owners through a
Codex-authority selected root.

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
- Duplicate profile/project installations of the same package source require
  the scoped canonical selector; bare `name` and `name@source` work only when
  unique.
- Scoped policy keys keep duplicate installations independently configurable,
  while an unscoped policy key remains effective only for a unique record.
- Static skill roots, hook sources, and worker tool descriptors are loaded only
  when the plugin package is enabled, then routed through the owning runtime
  module.
- CLI `plugin view` human output shows typed package display metadata from
  Codex-compatible `interface` without exposing executable assumptions.
- Gateway `plugin/list`, `plugin/read`, and `plugin/doctor` return the same
  read-only runtime plugin values as CLI JSON output and never mutate plugin
  policy or install records.
- Plugin hook sources are listed but not executed when their normalized hook
  hashes are untrusted or modified.
- Worker-provided hook handlers are routed through 140 Hook Runtime after
  plugin enablement and hook trust select the handler.
- Prompt hook declarations from plugin hook sources contribute only turn-local
  context through 140 Hook Runtime.
- Agent hook declarations from plugin hook sources list and skip with
  adapter-unavailable diagnostics.
- Worker `contributions/list` receives the effective loaded environment so
  config-parent and project `.env` variables are available during discovery.
- Worker tool descriptors enter the shared tool surface as plugin-source
  bindings and are searchable/deferred by default when `tool_search` is
  enabled for the invocation.
- Worker startup failures and discovery timeouts are reported by
  `plugin doctor` without crashing runtime assembly.
- Worker tool execution succeeds through the public tool surface adapter.
- Worker tool execution failures and timeouts return tool errors and keep
  diagnostics source-qualified.
- Contribution projection facts from plugin declarations feed owning
  diagnostics such as `plugin doctor`; they do not create a generic
  contributions-inspection command.
- Codex authority preserves `<plugin>@<marketplace>` identity, does not create
  a Psychevo mirror record, and keeps same-name rows from different authorities
  distinct.
- An enabled installed Codex package root is resolved in place, keeps Codex
  authority in the selected-root record, and is frozen across later turns of
  that Psychevo thread. Archive/delete clears both the snapshot and ephemeral
  Codex thread.
- A Codex MCP whose package root is not exposed is delegated by effective
  server name and is never projected both natively and through the broker.
- Apps inventory, OAuth, standard/openai/URL elicitation, app-backed MCP calls,
  ephemeral thread cleanup, profile mismatch, and post-delivery disconnects
  are exercised through the broker interface.
- The pinned `openai/form` image-picker returns item ids, renders only safe
  HTTPS or image-data sources, and rejects unknown URL schemes in Workbench.

## CLI Coverage

- Singular `pevo plugin` parses and obsolete plural `pevo plugins` rejects.
- `plugin list`, `view`, and `doctor` support secret-free JSON output.
- `plugin view` human output includes display name, category, capabilities, and
  short description when typed `interface` metadata is present.
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
- The opt-in `codex-plugin-broker-live` check uses the current Codex executable
  and `CODEX_HOME` only for initialize and read-only `plugin/list` projection.
  It must not install, uninstall, enable, authenticate, or execute a plugin, and
  it does not require an LLM-provider credential.
