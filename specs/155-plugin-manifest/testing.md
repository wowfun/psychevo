---
name: 155. Plugin Manifest Testing
psychevo_self_edit: deny
---

Define acceptance expectations and validation scenarios for plugin manifests.

## Long-Term Acceptance Contract

- Manifest selection is deterministic across native and compatibility manifest
  paths.
- The pinned Codex base preserves optional version/description fields and uses
  active version `local` for local development packages without a version.
- `psychevo.plugin.json` is an overlay and cannot replace or repeat shared
  Codex components.
- Supported shared base fields with invalid shapes skip only the affected
  declaration and return diagnostics. An overlay that repeats a shared field or
  contains an unknown top-level field fails closed as one overlay unit.
- Unknown fields are ignored with diagnostics.
- Every manifest local path is explicit package-relative input and cannot
  escape the plugin root through absolutes, `..`, symlinks, or canonicalization.

## Current Implementation Slice

The current slice validates the raw and normalized
`codex-plugin/8604689e` models plus the optional Psychevo overlay. Compatibility
is measured per component through behavioral conformance, not field acceptance.

Manual broad validation for code changes is still the Rust workspace gate
defined by [065 CI/CD](../065-ci-cd/spec.md), but this topic's
acceptance coverage should come from focused manifest loader tests.

## Scenario Matrix

- `.codex-plugin/plugin.json` is selected before `.claude-plugin/plugin.json`.
- `psychevo.plugin.json` overlays the selected base without changing it.
- Additional recognized manifests are reported as ignored diagnostics.
- A malformed preferred manifest fails on that manifest and does not fall
  through to lower-priority manifest paths.
- Missing or blank base `name` falls back to the package-directory basename;
  missing local version resolves to `local`.
- Unknown fields remain in the raw manifest and produce a newer-contract
  diagnostic instead of being destroyed.
- Shared base fields with invalid shape skip the affected declaration; an
  invalid companion overlay contributes none of its runtime/agents/toolsets
  projection.
- Codex-compatible `hooks` declarations load as candidate hook declarations and
  do not imply trust or execution.
- Codex-compatible `mcpServers` object and package-relative path declarations
  parse valid siblings while reporting malformed siblings as diagnostics.
- Default `hooks/hooks.json` and `.mcp.json` files are recognized only when the
  manifest omits explicit fields for that family.
- Overlay `toolsets` uses configured custom-toolset shape and leaves expansion
  to the tool surface.
- Codex-compatible `interface` metadata is parsed into typed display fields,
  including media fields that use package-local path safety.
- Invalid `interface` display fields emit diagnostics, skip only the malformed
  display field, and keep the rest of the manifest loadable.
- Hermes `plugin.yaml` is diagnostic/descriptive input only and never causes
  Psychevo to import or execute Hermes dynamic `register(ctx)` behavior.
- `apps` reports native-unavailable or Codex-delegated readiness; overlay
  `commands` and `providers` are unsupported.
- Static `psychevo.tools` is unsupported; executable plugin tools must come
  from worker discovery, MCP listing, or a future owning static-tool path.
- Local path values must start with `./`.
- Absolute paths are rejected.
- Paths containing `..` are rejected.
- Symlink or canonicalization escapes outside the plugin root are rejected.
- Overlay `runtime.worker.command` paths use the same local path safety rules.

## Validation Boundaries

- Tests should use structured manifest loader results, not string-grep output.
- Path-safety tests must create real directories and symlinks inside isolated
  temp dirs.
- Fixture manifests should stay small and explicit so diagnostics remain
  reviewable.
