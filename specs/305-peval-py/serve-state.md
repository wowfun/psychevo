# peval-py Serve Workspace State

## Serve Workspace State

`peval-py serve` is backed by a selected peval-py workspace. Python-owned
configuration lives at `<workspace>/peval-py.toml`. The workspace config stores
optional top-level `locale`, built-in adapter default DB paths, and serve
defaults only. Peval-py does not create, read, or write a workspace
`state.db`; existing files with that name are ignored by peval-py workspace
state.

Serve may create an ECharts cache at
`<workspace>/.cache/echarts/6.0.0/echarts.min.js` and a structured append-only
log at `<workspace>/logs/peval-py-serve.jsonl`.

Runtime source state lives beside each Trial cell in
`<cell>/.peval/state.json`. Missing `.peval/` or missing
`.peval/state.json` is not an error: a complete Trial cell without local source
state is treated as a readable active, non-refreshable artifact source with
default metadata. The state file is a minimal overlay with `schema_version = 2`,
`created_at_ms`, `updated_at_ms`, optional `source_alias`, optional
`source_tags` as an ordered string array, optional archived state
(`active = false`), optional latest status/error fields, optional
`last_refreshed_at_ms`, and an optional compact `source` object only when
provenance cannot be reconstructed from the Trial artifacts. Derived fields
such as source key, artifact path, adapter/session/model display fields,
refreshability, snapshot state, and Trial summary fields are computed from the
cell path plus `agent/trajectory.json` and `agent/trajectory_meta.json`.
Older non-v2 state files are ignored as overlays and are overwritten with v2
shape on the next source mutation.

Refresh and import attempts append JSONL records to
`<workspace>/logs/peval-py-serve.jsonl` with time, status, source key, warning
count, and error summary. The log is evidence only; it is not a source index
and is not required to compose reports.

Active sources with readable artifacts compose the default served report.
Archived sources remain in their cell-local state files and can be restored, but
they do not contribute Trial rows. Sources whose artifact directory is missing
or unreadable remain listed in source management with `last_status = "missing"`
or `last_status = "error"` when their cell-local state can still be found, but
they are skipped when composing multi-source serve reports. The state layer
keeps only the canonical Trial artifacts plus per-cell source state; it does not
preserve every historical report blob.

Canonical Trial artifacts live under the peval run tree. The minimum persisted
unit is the Trial cell:

```text
<workspace>/runs/<analysis_eval_slug>/<agent-id>/<session-id>/<cell-key>/
  agent/trajectory.json
  agent/trajectory_meta.json
  .peval/state.json
  notes.md
  analysis.json
  analysis.md
```

`<cell-key>` is `trajectory_meta.trial_key` after safe path-segment
normalization. A complete cell directory is a discoverable artifact fact even
when it has no `.peval/state.json`. `trajectory.json` is the ATIF-like agent
trajectory. `trajectory_meta.json` is the viewer/report sidecar for timing,
status, warnings, and step metadata. Cell-local `analysis.json`, `analysis.md`,
and `notes.md` are the persisted annotation truth for that Trial. Session-root
`analysis.json`, `analysis.md`, and `notes.md` belong to the whole session and
are reserved but not read by this version.

`serve` startup and explicit source reload scan
`<workspace>/runs/<analysis_eval_slug>/*/*/*` for complete Trial cells and
derive source rows from their artifacts plus optional `.peval/state.json`
overlays. The
Path source form may also import a local external workspace root, `runs/`,
`runs/<eval>`, or a directory above Trial cells; that import recursively finds
complete Trial cells, copies each cell into the current workspace run tree, and
writes only the minimal overlay needed for user state. External run trees are
read-only provenance; deleting a source deletes only the current workspace copy.
The served report JSON is computed from active readable source overlays plus
these artifacts and is not persisted as a complete blob.

Uploaded JSONL files are converted through the selected adapter. Uploaded ATIF
JSON trajectory objects and uploaded peval-py report JSON are accepted without
requiring a message adapter. Uploaded source payloads are limited to 20 MiB,
converted immediately, persisted only as canonical Trial artifacts plus a
cell-local overlay, and discarded after ingestion; raw uploaded files are not
written to disk or stored as blobs. When the uploaded source is a peval-py
report JSON, matching Trial `annotations.notes[]` are materialized into that
Trial cell's `notes.md`, matching `annotations.analysis[]` entries are
materialized into `analysis.json` and `analysis.md`, and report-level notes are
ignored until a session/report artifact model exists.

The Source Manager Path form accepts line-delimited batch input. Blank lines are
ignored. Each non-blank line is parsed and imported independently, so one bad
path does not block later paths. Multi-line requests return the normal source
mutation envelope plus `import_results[]` entries with per-line `status`,
`source_keys`, or `error`. Single-line failures keep the existing HTTP error
response behavior.

Serve source mutation endpoints return a shared JSON envelope with `sources`
and, when a readable source exists, `report` plus `report_source_key`. Reload,
add, upload, archive, activate, refresh, alias, notes, and delete actions use
this envelope so the browser can clear stale report state consistently when no
source is readable.

`peval-py init` writes only the Python-owned serve state described above.
Existing unrelated workspace files, including any old workspace `state.db`, are
left untouched, but they are neither created nor required.
