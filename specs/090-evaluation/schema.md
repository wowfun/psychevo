---
name: 090. Evaluation Schema Attachment
psychevo_self_edit: deny
---

Define common schema and versioning rules for evaluation manifests, results,
trajectories, and reports.

This attachment is part of [090 Evaluation](spec.md).

## Scope

- schema-version field requirements
- persisted evaluation document classes
- format ownership between manifests and artifacts
- compatibility and unsupported-version behavior

Out of scope:

- concrete `psychevo-eval` Rust type names
- CLI flag spelling
- benchmark-specific dataset schemas

## Versioning

Every persisted public evaluation document must contain a `schema_version`
field. The field identifies the document contract, not the Psychevo application
version. Lower specs may define exact version strings, but must use a stable
format that can be compared by readers.

Readers must reject unsupported schema versions with a clear diagnostic. The
first implementation slice does not require automatic migration or best-effort
loading of older evaluation artifacts.

Schema readers must not silently reinterpret one document class as another. A
manifest, run summary, task result, trajectory, report index, or sidecar bridge
payload must be distinguishable by document kind or by the surrounding file
contract.

## Formats

Human-authored manifests should use TOML unless a lower spec explicitly
requires a different source format for an external benchmark adapter.

Machine-authored artifacts should use JSON or NDJSON:

- run summaries and task results use JSON
- event trajectories use NDJSON when streaming or append behavior matters
- tabular report exports may use CSV as a derived format

Lower specs may define HTML, Markdown, or external harness outputs as derived
artifacts, but those formats must not replace the canonical structured result
documents.

## Stability

Stable fields should be additive when possible. A lower spec may allow unknown
fields for forward compatibility only when the reader can ignore them without
changing scoring, artifact retention, or safety behavior.

Required fields must be validated before a run starts when they influence
environment setup, agent execution, provider access, or scorer behavior.

## Related Topics

- [095 Manifest](../095-evaluation-framework/manifest.md)
- [095 Crate API](../095-evaluation-framework/crate-api.md)
- [090 Artifacts](artifacts.md)
