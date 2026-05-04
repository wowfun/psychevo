---
name: 200. pevo CLI Implementation Plan
psychevo_self_edit: deny
---

Plan the OpenCode-style `pevo` CLI slice.

## Phase 1: Specs

- Create [025 CLI](../025-cli/spec.md).
- Create [200 pevo CLI](spec.md), [pevo init](pevo-init.md), [pevo run](pevo-run.md),
  and [200 Testing](testing.md).
- Move concrete `pevo run` source-of-truth from `020 Interfaces` to this topic.
- Update provider registry and runtime assembly links.

## Phase 2: Runtime Config and State

- Replace `PSYCHEVO_CONFIG_DIR` discovery with `PSYCHEVO_HOME`.
- Add `PSYCHEVO_CONFIG` single-file config replacement.
- Add `PSYCHEVO_DB` path override at the CLI layer.
- Validate reasoning variants across CLI and JSONC.
- Add latest run-session lookup by canonical workdir.

## Phase 3: CLI Commands

- Add `pevo init` with non-overwrite home initialization.
- Convert `pevo run` to positional prompt, stdin append, `--dir`, `-m`, `--variant`,
  `-s`, `-c`, and `--format`.
- Remove old conflicting `pevo run` flags.
- Preserve `pevo smoke` as the deterministic harness.

## Phase 4: Validation

- Update deterministic CLI and runtime tests.
- Keep real-provider tests ignored by default.
- Run `scripts/validate.sh broad`.

## Related Topics

- [200 pevo CLI](spec.md) defines the product CLI surface.
- [200 Testing](testing.md) defines acceptance coverage.
- [120 Provider Registry](../120-provider-registry/spec.md) defines provider
  config resolution.
