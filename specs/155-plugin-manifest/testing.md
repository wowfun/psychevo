---
name: 155. Plugin Manifest Testing
psychevo_self_edit: deny
---

Define acceptance expectations and validation scenarios for plugin manifests.

## Long-Term Acceptance Contract

- Manifest selection is deterministic across native and compatibility manifest
  paths.
- Native manifests require `name`, `version`, and `description`.
- Compatibility manifests may load for local development without native-only
  required fields, but installable compatibility packages must have resolvable
  name and version.
- Supported shared fields or Psychevo extension fields with invalid shapes skip
  only the affected declaration and return diagnostics.
- Unknown fields are ignored with diagnostics.
- Every manifest local path is explicit package-relative input and cannot
  escape the plugin root through absolutes, `..`, symlinks, or canonicalization.

## Current Implementation Slice

The current slice validates native Psychevo manifests plus selected Codex and
Claude compatibility manifests. Compatibility loading is a field-subset bridge,
not ABI compatibility with external plugin runtimes.

Manual broad validation for code changes is still the Rust workspace gate
defined by [065 CI/CD](../065-ci-cd/spec.md), but this topic's
acceptance coverage should come from focused manifest loader tests.

## Scenario Matrix

- Native `.psychevo-plugin/plugin.json` is selected before compatibility
  manifests.
- Additional recognized manifests are reported as ignored diagnostics.
- A malformed preferred manifest fails on that manifest and does not fall
  through to lower-priority manifest paths.
- Missing native `name`, `version`, or `description` produces invalid manifest
  diagnostics.
- Compatibility manifests may load for local development with compatibility
  diagnostics.
- Compatibility manifest install rejects packages without resolvable name or
  version.
- Compatibility manifest install accepts packages with name and version but no
  description, storing an empty description.
- Unknown fields produce ignored-field diagnostics.
- Shared fields and Psychevo extension fields with invalid shape skip the
  affected declaration.
- Local path values must start with `./`.
- Absolute paths are rejected.
- Paths containing `..` are rejected.
- Symlink or canonicalization escapes outside the plugin root are rejected.
- `psychevo.runtime.worker.command` paths use the same local path safety rules.
- Top-level `runtime.worker` is ignored for new worker loading and reported as an
  ignored field.

## Validation Boundaries

- Tests should use structured manifest loader results, not string-grep output.
- Path-safety tests must create real directories and symlinks inside isolated
  temp dirs.
- Fixture manifests should stay small and explicit so diagnostics remain
  reviewable.
