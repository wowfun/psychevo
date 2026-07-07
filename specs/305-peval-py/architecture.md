# peval-py Architecture

## Module Shape

`peval-py` keeps adapters isolated under `peval_py.adapters`. Non-adapter code
is organized around deep modules whose public interfaces match user-visible
workflows:

- command dispatch parses CLI arguments and delegates to trajectory, import,
  init, and serve workflows.
- input loading turns CLI or serve source selections into loaded session
  descriptors without owning conversion, report building, or workspace state.
- workspace state owns peval-py workspace discovery, cell-local source overlays,
  Trial cell artifacts, snapshot discovery, source lifecycle mutations, and
  report composition over persisted artifacts.
- report building owns report JSON v19 assembly, timing metadata, annotations,
  automatic analysis metrics, and input data references.
- analysis owns cached analysis and notes reads, analysis import compilation,
  note writes, and path safety for Trial cell annotation artifacts.
- HTML rendering owns package asset loading, safe payload injection, serve shell
  markup, and token estimates.
- serve owns local HTTP protocol handling, startup background loading, route
  controllers, request payload validation, source mutation response envelopes,
  and ECharts cache serving.

Adapters must not import the refactored internals directly. The adapter-facing
modules `peval_py.config`, `peval_py.sources`, and `peval_py.redaction` remain
stable import surfaces for built-in and third-party adapters.

## Dependency Direction

Shared dataclasses for loaded inputs, adapter assignments, report sessions, and
notes live in a neutral model module. Workflow modules may depend on these
models, but storage and report modules must not import the CLI parser or the
input loader.

The intended dependency flow is:

```text
cli/serve workflows -> inputs, workspace, report, html
inputs              -> adapters, input tables, workspace snapshot reader
workspace           -> repository, artifacts, report, analysis overlays
report              -> analysis schema/cache interfaces, redaction
html                -> assets and i18n
```

This prevents cycles where workspace state imports input loading while input
loading imports workspace snapshot state.

## Package Facades

Public imports may continue to use the package-level facades
`peval_py.analysis`, `peval_py.cli`, `peval_py.html`, `peval_py.inputs`,
`peval_py.report`, `peval_py.serve`, and `peval_py.state`. Their implementations
are split into focused internal modules for parsing, payload validation,
artifact IO, report assembly, serve handlers, and state mutations. New code
should import the deepest stable module it owns when working inside the package,
but external callers should prefer the facade unless a lower-level module is
documented as an extension point.

## Serve Response Envelope

Serve source mutations return a source list and may also return the selected
report:

```json
{
  "sources": [],
  "report": {},
  "report_source_key": "source-key"
}
```

`report` and `report_source_key` are omitted only when no readable source can be
selected. The browser must treat absent report data as an empty report and must
not keep stale report content after all sources become unreadable.

During startup, the serve runtime may return `loading = true` with an empty
report while the background initial load imports explicit source flags and scans
workspace Trial cells. Handlers use the runtime snapshot for `GET /` and
`GET /api/sources`; they should not synchronously scan the workspace on the
first page request.

## Assets

Report assets are source files, not generated artifacts. CSS and JavaScript may
be split into ordered package assets and concatenated by the Python asset loader
without a build step. The ordering must keep bootstrap/state helpers before
renderers and keep the final startup call last.

Asset refactors must preserve static report mode and serve mode over the same
report body. Serve-only code may add source management and export controls, but
static report output must not show serve controls.
