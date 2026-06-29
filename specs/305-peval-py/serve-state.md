# peval-py Serve Workspace State

## Serve Workspace State

`peval-py serve` is backed by a selected peval-py workspace. Python-owned
configuration lives at `<workspace>/peval-py.toml`. The first version stores
`state_db = "state.db"`, optional top-level `locale`, built-in adapter default
DB paths, and serve defaults only. Serve may also create an ECharts cache at
`<workspace>/.cache/echarts/6.0.0/echarts.min.js`.
Runtime state lives in
`<workspace>/state.db`, which may become a shared state database later; this
version creates and updates only these `peval_py_*` tables:

- `peval_py_sources` stores stable source keys, source kind, adapter, original
  path or DB/session metadata, optional display alias, active/archived state,
  refreshability, latest status/error summary, and the latest Trial cell
  artifact directory/update time for that source.
- `peval_py_refresh_log` stores refresh attempts with time, status, source key,
  warning count, and error summary.

Active sources compose the default served report. Archived sources remain in the
state database and can be restored, but they do not contribute Trial rows. The
state layer keeps only the latest canonical Trial artifacts for each source plus
a bounded refresh log; it does not preserve every historical report blob.

Canonical Trial artifacts live under the peval run tree, not inside SQLite. The
minimum persisted unit is the Trial cell:

```text
<workspace>/runs/<analysis_eval_slug>/<agent-id>/<session-id>/<cell-key>/
  agent/trajectory.json
  agent/trajectory_meta.json
  notes.md
  analysis.json
  analysis.md
```

`<cell-key>` is `trajectory_meta.trial_key` after safe path-segment
normalization. `peval_py_sources.artifact_dir` points at this cell directory.
`trajectory.json` is the ATIF-like agent trajectory. `trajectory_meta.json` is
the viewer/report sidecar for timing, status, warnings, and step metadata.
Cell-local `analysis.json`, `analysis.md`, and `notes.md` are the persisted
annotation truth for that Trial. Session-root `analysis.json`, `analysis.md`,
and `notes.md` belong to the whole session and are reserved but not read by this
version. These files are general peval run artifacts; they must not be treated
as private `serve` cache. The served report JSON is computed from active source
rows plus these artifacts and is not persisted as a complete blob.

Uploaded JSONL files are converted through the selected adapter. Uploaded ATIF
JSON trajectory objects and uploaded peval-py report JSON are accepted without
requiring a message adapter. Uploaded source payloads are limited to 20 MiB,
converted immediately, persisted only as canonical Trial artifacts plus source
rows, and discarded after ingestion; raw uploaded files are not written to disk
or stored as blobs. When the uploaded source is a peval-py report JSON, matching
Trial `annotations.notes[]` are materialized into that Trial cell's `notes.md`,
matching `annotations.analysis[]` entries are materialized into `analysis.json`
and `analysis.md`, and report-level notes are ignored until a session/report
artifact model exists.

`peval-py init` writes only the Python-owned serve state described above.
Existing unrelated workspace files are left untouched, but they are neither
created nor required.
