# peval-py Inputs and Adapters

## Inputs

The command supports path and DB input sources:

- `-p, --path PATH` reads one source file. By default this accepts JSONL with
  one JSON object per line, an exported ATIF JSON trajectory object, or a Trial
  cell artifact directory containing `agent/trajectory.json` and
  `agent/trajectory_meta.json`. Adapters may parse other path formats directly.
  Trial cell artifact directory input is read-only and must not refresh source
  databases, mutate workspace state, or scan unrelated `runs/` directories.
- `-d, --db PATH` reads an adapter-owned SQLite persistence format. `view
  trajectory` may repeat `-d` to compare sessions across adapters. `-d
  @adapter` expands to that adapter's configured `default_db_path` and binds
  that DB input to the token adapter. Peval-py workspace source state is not a
  DB input; saved workspace snapshots are read through direct Trial cell paths
  or workspace/runs paths, while `-d` remains reserved for adapter-owned SQLite
  persistence.
- `-i, --input-table PATH` reads a structured input manifest and appends its
  rows after any direct `-p/--path` and `-d/--db` inputs. CSV and JSON manifests
  use only the Python standard library. `.xlsx` manifests are supported only
  when the optional `openpyxl` package is importable; `.xls` is unsupported and
  must fail with guidance to use `.xlsx` or CSV.

`view trajectory` may mix repeated `-p` and repeated `-d` inputs in one
invocation, and may also use repeated `-i/--input-table` manifests. Each loaded
path, DB session, or manifest row becomes one Trial in the report. `export
trajectory` remains single-session only and must fail clearly when the effective
input set contains more than one session.

CLI path resolution treats POSIX absolute paths, Windows drive paths, and UNC
paths as absolute-like inputs. On native Windows those paths resolve normally.
On POSIX or WSL, an accessible Windows drive path may resolve through the
existing `/mnt/<drive>/...` mapping; otherwise peval-py leaves it unresolved
instead of interpreting `C:\...` or `C:/...` as a current-directory relative
path. UNC paths remain absolute-like and are not prefixed with the workspace
root or current directory.

`-s, --session-id` is valid only when at least one `-d, --db` input is present.
With one DB input, bare `-s ID` remains valid and may be repeated to compare
multiple sessions from that DB in `view trajectory`. With multiple DB inputs,
session ids must use `-s dN=ID`, where `N` is the one-based DB input index.
Repeating `-s dN=ID` selects multiple sessions from that DB. `view trajectory`
also accepts session indexes for adapter-owned DBs: `-s #3` always selects the
third listed session, while `-s 3` first tries a real session id `3` and falls
back to index 3 only when that id is absent. With multiple DB inputs, index
selectors use the same DB prefix, such as `-s d1=#3`. A DB input without
explicit session ids lets its adapter choose its default or latest session.

The command surface follows a peval-style verb and scenario shape:

- `peval-py view trajectory ...` defaults to bounded inspect JSON for one or
  more sessions. `peval-py view trajectory -m raw ...` writes the complete
  peval-compatible JSON or HTML report.
- `peval-py export trajectory ...` writes a single ATIF trajectory object.
- `peval-py import analysis ...` imports JSON or Markdown analysis reports into
  a peval-py workspace Trial cell.
- `peval-py init ...` creates or repairs the minimal peval-py serve state.
- `peval-py serve ...` starts a local Report First saved-workspace web UI over
  trajectory sources.

`tr` is an accepted alias for `trajectory`, so `peval-py view tr ...` and
`peval-py export tr ...` are equivalent to the long scenario form. `init` and
`serve` are top-level commands and do not require a trajectory scenario
argument.

Common trajectory flags use both long and short forms:

- `-c, --config PATH`
- `-r, --root DIR`
- `-a, --adapter ADAPTER`
- `-p, --path PATH` for JSONL, report JSON, ATIF trajectory JSON, trajectory
  artifact files/directories, Trial cell directories, or descendants inside
  Trial cells; repeatable
- `-i, --input-table PATH`
- `-o, --output [PATH]`
- `-m, --mode inspect|raw` for `view trajectory`; `inspect` is the default and
  `raw` preserves full report rendering
- `-f, --format json|html` for `view trajectory`
- Inspect output for `view trajectory` is a fixed `inspect_schema_version: 2`
  JSON digest. Each source includes session, agent/model, token totals, active
  duration in seconds, tool-call and turn totals, compact step head/tail
  previews, step/tool duration distributions in seconds, top step durations,
  top step tokens, tool errors, and top tool durations. `status` is emitted
  only when it is non-empty and not `passed`; `score` is emitted only when it
  is non-empty.
- Inspect evidence controls for `view trajectory`: `--head N` and `--tail N`
  default to 2, `--top N` defaults to 5, `--source N` restricts output to
  one-based source indexes, and `--max-content-chars N` bounds inspect preview
  text. Inspect preview text defaults to 3000 characters when neither CLI nor
  config sets `max_content_chars`; raw report and export content bounding keeps
  the normal configured default. `--steps VALUE` adds `selected_steps` evidence
  for matching trajectory `step_id` values. `VALUE` may be repeated, may contain
  comma-separated selectors, and supports inclusive positive integer ranges
  with `start:end` syntax, such as `1,3:5,7:9`. Exact non-range selectors are
  preserved as strings so non-numeric `step_id` values remain selectable. When
  `--steps` is present, inspect output omits the default `steps` and `tools`
  digest sections and keeps source identity plus selected evidence.
  `--tool-call ID` independently adds `selected_tool_calls` evidence with the
  matching tool call and corresponding tool result when retained trajectory data
  provides one; using `--tool-call` alone does not suppress the default digest.
- `-n, --note N=TEXT` for `view trajectory`, where `0` is report-level and
  positive one-based indexes attach to the ordered input sessions
- `--source-alias N=TEXT` for `view trajectory` and `serve`, where positive
  one-based indexes attach display-only aliases to the ordered input sessions
- `--list, -l` for `view trajectory` with one or more `-d/--db` inputs, which
  prints DB sessions and exits
- `--list-interactive, -li` for `view trajectory` with exactly one `-d/--db`
  input, which prints DB sessions, prompts for a comma/range selection, and then
  renders the selected sessions

Raw report override flags for `view trajectory` are available only when
`-m raw` is selected: `--agent-name`, `--agent-version`, `--model`, and
`--no-redact`. Passing any of those flags to the default inspect mode must fail
with a clear diagnostic telling the user to use `-m raw`. `--max-content-chars`
remains a general trajectory option because it bounds large source content
before inspect or raw rendering.

`--trajectory-id` is not a supported CLI or config override. Generated ATIF
trajectory output still contains a `trajectory_id`; conversion uses the stable
default `session:t001` when the input does not already provide one, and report
or snapshot readers continue to preserve existing `trajectory["trajectory_id"]`
values.

`view trajectory` and `export trajectory` accept `-r, --root DIR` to select an
existing peval-py workspace root for config discovery. The selected root is
used to load top-level config such as `locale`, `analysis_eval_slug`, adapter
defaults, and adapter `default_db_path` values. `view trajectory --list` and
`--list-interactive` inherit that same root-selected config behavior for DB
listing and selection. Rendered `view trajectory` reports also use the root for
read-only cached `analysis.json` / `analysis.md` overlays. `export trajectory`
uses the root-selected config and adapter defaults but still writes only one
ATIF trajectory object and does not include report annotations. Passing
`-r/--root` to `view` or `export` must not initialize,
repair, or mutate the workspace. If `<root>/peval-py.toml` is missing or
invalid, the command fails clearly and tells the user to run
`peval-py init -r <root>`. Existing current-directory discovery remains valid
when `-r/--root` is omitted. When `view trajectory` or `export trajectory`
receives a path input shaped like
`<workspace>/runs/<eval>/<agent>/<session>/<cell>` and
`<workspace>/peval-py.toml` exists, it may infer `<workspace>` as the read-only
workspace root if `-r/--root` is omitted. The same inference applies when an
accessible Windows drive path maps to that local cell path. If explicit
`-r/--root` resolves to a different workspace than the inferred path workspace,
the command fails with a clear conflict diagnostic instead of silently using
either root.

For direct Trial cell artifact directory input, `view trajectory -p
W/runs/E/A/S/C` and `export trajectory -p W/runs/E/A/S/C` read
`agent/trajectory.json` and `agent/trajectory_meta.json` from that cell. If the
`-p` value is literal `<cell-dir>/**`, literal `<cell-dir>/**/*`, or a
shell-expanded descendant inside the cell such as `<cell-dir>/agent` or
`<cell-dir>/agent/trajectory.json`, peval-py canonicalizes it back to
`<cell-dir>`. If any `-p` value resolves to a Trial cell for `view trajectory`
or `export trajectory`, the command enters cell-path mode: it keeps the
recognized Trial cells, deduplicates repeated references in input order, ignores
non-cell `-p` values, and ignores `-r`, `-a`, `-d`, `-s`, `-i`, `--list`, and
`--list-interactive`. In cell-path mode no warning is emitted for ignored input
selectors because the Trial cell path is the stronger explicit source.

If the canonical cell path is under an inferred workspace and
`.peval/state.json` exists, peval-py uses that cell-local source state so source
aliases, active state, current cell-local notes, and cached analysis overlays
remain consistent with serve rendering. The state file is an overlay only:
artifact identity, source key, adapter/session/model fields, and Trial summary
fields are still derived from the cell path and agent artifacts. Missing
`.peval/` or `.peval/state.json` is allowed and renders the artifact snapshot
with default source state. A path under `runs/...` that looks like a Trial cell but lacks
either required agent artifact must fail with an actionable diagnostic naming
`agent/trajectory.json` and `agent/trajectory_meta.json`.

When the input was canonicalized to a Trial cell directory, the rendered report metadata
may preserve the original `data_ref` from the retained trajectory, such as the
source DB label. It must also add a separate `artifact_ref` object to the Trial
metadata in raw reports so the current artifact input remains visible without
changing the original provenance. Inspect v2 remains a minimal digest and does
not include `artifact_ref`. `artifact_ref.kind` is `trial-cell-artifact`;
`path` is the readable path relative to the current working directory when
possible; `workspace_relative_path` is present when the cell belongs to a
discovered workspace; and `source_key` is present when the cell has a
matching `.peval/state.json`. `export
trajectory -p <cell-dir>` still writes only the ATIF trajectory object and does
not include `artifact_ref`.

`import analysis` accepts `-r, --root DIR`, `--run-path PATH`, repeatable
`-p, --path PATH`, and `--json`. The root is required and must already contain
`peval-py.toml`; import must not initialize or repair workspaces. `--run-path`
is required, may be absolute or relative to the workspace root, must resolve
inside `<workspace>/runs/...`, and must contain exactly the Trial cell identity
segments `runs/<analysis_eval_slug>/<agent-id>/<session-id>/<cell_key>`.
`--path/-p` is required and repeatable. Suffixes select the input format:
`.json` imports one JSON analysis report and writes compiled `analysis.json`,
while `.md` or `.markdown` imports one Markdown analysis report and writes
`analysis.md`. At most one JSON input and one Markdown input may be imported
per command. A Markdown-only import writes only `analysis.md` and must not
create `analysis.json`. With
`--json`, import prints machine-readable selected run path, written artifact
paths, and diagnostic warnings; otherwise it prints concise text without
warnings. Import mutates only the selected Trial cell files.

`init` accepts `-r, --root DIR` and `--json`. Without `--root`, it initializes
the current directory. It creates `<workspace>/peval-py.toml` when missing and
creates `<workspace>/logs/` when needed. Existing valid `peval-py.toml` files are
preserved, including custom adapter defaults; legacy `state_db` keys are ignored
by peval-py workspace state. New config files include built-in adapter default
DB paths using `~`: Psychevo `~/.psychevo/state.db`, OpenCode
`~/.local/share/opencode/opencode.db`, and Hermes `~/.hermes/state.db`. It must
not create or edit `peval.toml`, `runs/`, `datasets/`, `scripts/`, workspace
templates, `$PSYCHEVO_HOME/peval-config.toml`, or `.gitignore`. `--json` emits
`schema_version`, `root`, `peval_py_config`, and `log_path`.

`serve` accepts `-r, --root DIR` to select a peval-py workspace root. Explicit
`--root` and `PEVAL_ROOT` roots are accepted directly and create missing
`peval-py.toml` defaults as needed. `PEVAL_ROOT` is only a shared environment
variable name for the root path override; it does not imply Rust `peval`
workspace discovery or validation. Without an explicit or environment root,
discovery walks the current directory and parents for `peval-py.toml`; if none
is found, `serve` fails clearly and names `peval-py init`, `--root/-r`, and
`PEVAL_ROOT`. `serve` discovery must not read or require Rust
`peval.toml` or `$PSYCHEVO_HOME/peval-config.toml`.

`serve` accepts `-c`, `-a`, `-p`, `-d`, `-s`, `-i`, `-n`, and
`--source-alias` with the same trajectory-source semantics as `view
trajectory`. Source flags are persistent: they create or update stable saved
sources by writing each Trial cell's minimal `.peval/state.json` overlay after conversion
succeeds and before the server starts. In serve state, one source is one Trial
cell: it is the user-facing management object for that cell and is not a
separate provenance row that can share artifacts with another source. Stable
source keys are derived from the canonical cell identity after conversion:
analysis eval slug, effective agent id, session id, and
`trajectory_meta.trial_key`. Repeated imports that resolve to the same cell
update the same source instead of appending duplicates. Source aliases are
display-only metadata and must not contribute to source keys.
Uploaded JSONL, ATIF JSON, and report JSON sources are stored as canonical
snapshots and are not refreshable.
In the web UI, DB sources may also be added through a session picker: the user
enters a DB path, inspects it, selects one or more listed sessions, and saves
each selected session as its own refreshable source. This picker must reuse the
same path-token adapter inference rules as direct `-d` inputs, but inspection
does not fall back to the default adapter when inference cannot choose one; the
user must provide an explicit adapter and retry.

`serve` binds only localhost addresses. By default it tries `127.0.0.1:58010`
and falls back sequentially through `58029` when the default port is busy.
Explicit `--port PORT` binds strictly and fails if unavailable. `serve` prints
the selected URL and does not open a browser.

`-a ADAPTER` sets the default adapter for all inputs. `-a pN=ADAPTER` overrides
the adapter for the one-based path input `N`; `-a dN=ADAPTER` overrides the
adapter for the one-based DB input `N`. `-a` may be repeated. The default
adapter starts from config, the last bare `-a ADAPTER` overrides that default,
and selector forms override only their matching input. Invalid selectors,
duplicate selectors, selectors that reference missing inputs, and unknown
adapter ids must fail clearly and list available adapters for unknown ids.
Selector forms apply only to directly supplied `-p/--path` and `-d/--db`
inputs. A DB token of `@adapter` expands through that adapter's configured
`default_db_path`; if the same DB index also has `-a dN=...`, it must match the
token adapter or fail clearly. Manifest rows use their own `adapter` or `a`
column for row-level adapter selection, falling back to the effective default
adapter when omitted.
For direct `-p` and `-d` inputs without a per-input selector and without a bare
CLI `-a ADAPTER`, `peval-py` may infer the adapter from the path before falling
back to the config/default adapter. Inference is generic over available adapter
ids: an adapter id must appear as a complete path component or filename token,
not as an arbitrary substring; hidden directory names such as `.hermes` and
`.psychevo` are tokenized the same way. If multiple adapter ids match one path,
inference fails clearly and asks for `-a`. ATIF and peval-py report JSON
passthrough inputs remain pseudo-adapter sources and are not overridden by
inference. `peval-py` must not hard-code built-in adapter schema probes for
adapter inference.

Input table manifests are input lists, not batch job runners: they do not
introduce per-row output paths or multiple command executions. CSV manifests use
the first row as a header, read with `utf-8-sig`, preserve cell newlines, skip
blank data rows, and resolve relative `path` or `db` values relative to the
manifest file's directory. JSON manifests may be a top-level array of row
objects or an object with `rows` and optional `report_notes`. `.xlsx` manifests
use the active worksheet with the first row as a header. Headers are normalized
by removing leading dashes, lowercasing, and converting hyphens and spaces to
underscores. Supported manifest columns are `path`/`p`, `db`/`d`,
`session_id`/`session`/`s`, `adapter`/`a`, `note`/`notes`/`n`,
`report_note`/`report_notes`, `alias`/`label`/`source_alias`, `agent_name`,
`agent_version`, and `model`.
Unknown or duplicate columns must fail clearly. Each non-blank row must provide
exactly one of `path` or `db`; `session_id` is valid only for `db` rows. A DB
with multiple selected sessions is represented by multiple manifest rows.
Existing raw-mode CLI `--agent-name`, `--agent-version`, and `--model` values
are defaults for every session; manifest row values override those defaults
only for that row's conversion. Manifest aliases are display-only and override
only that row's source alias.

When `-o/--output` is omitted, commands write to stdout. When `-o/--output` is
present without a path, `view trajectory` uses a timestamped default file name
and prints the saved path to stdout as `wrote report: <path>`. Inspect-mode
`view trajectory -o` writes `inspect-YYYYMMDD-HHMMSS-ffffff.json`.
Single-session raw-mode `view trajectory -m raw` writes
`report-<adapter>-<session>-YYYYMMDD-HHMMSS-ffffff.html`, or the same stem with
`.json` when `--format json` is set. Multi-session raw-mode `view trajectory -m
raw` writes `report-<adapter>-sessions-<count>-YYYYMMDD-HHMMSS-ffffff.<format>`
when every session uses the same adapter, or
`report-multi-adapter-sessions-<count>-YYYYMMDD-HHMMSS-ffffff.<format>` when
multiple adapters are present. If a generated default name already exists,
`-2`, `-3`, and so on are appended before the suffix. Explicit output paths are
used as provided and also print the saved path for `view trajectory`. Single
session `export trajectory -o` without a path remains
`trajectory-<adapter>-<session>.json`. Unsafe filename characters are replaced
with `-`, and missing session ids fall back to `session`.

`export trajectory` remains single-session only. Multiple path inputs, multiple
DB inputs, mixed path/DB inputs, or multiple selected DB sessions must fail
clearly for export.

TOML config uses `defaults.adapter` for the input adapter default. Older
`defaults.agent` config keys may be accepted for local compatibility, but the
public CLI and docs use `adapter`. `defaults.locale` selects the generated HTML
report UI locale from `-c/--config` files; top-level `locale = "zh-CN"` in a
discovered `peval-py.toml` provides the same locale default for all commands.
When both exist, `-c/--config` overlays the discovered `peval-py.toml` and only
keys present in the explicit config replace workspace values. There is no CLI
locale flag. Supported values are `en`, `en-US`, `zh-CN`, and `zh`; `en-US`
normalizes to `en`, and `zh` normalizes to `zh-CN`. Unsupported locale values
must fail with a clear config error. Adapter-specific options live under
`[adapters.<adapter-id>]`. The reserved adapter option `default_db_path =
"PATH"` is consumed by peval-py for `-d @adapter` and serve Source Manager
defaults; `~` is expanded and relative paths resolve against the TOML file that
defined the value. POSIX absolute paths, Windows drive paths, and UNC paths are
treated as absolute for parsing and must not be joined to the TOML directory.
When peval-py writes adapter defaults, paths under the current user's home are
stored with a leading `~` while runtime config continues to expose resolved
paths. `peval-py` passes the remaining effective adapter option table through
to that adapter and does not define adapter-specific CLI flags.
Top-level `analysis_eval_slug = "default"` selects the peval run subtree used
for read-only cached analysis enrichment. Explicit `-c/--config` files may
override this key while preserving other workspace TOML values.

JSONL accepts either direct message objects or wrapper objects containing
`message`, optional `usage`, optional `metadata`, optional `accounting`, and
optional `session_seq`. Exported ATIF JSON path input preserves the trajectory
object as the canonical data source, does not require a selected adapter, and
uses `atif` as the report metadata adapter id to mark passthrough input.
It rebuilds only minimal report sidecar step metadata; peval-only timing
metadata that is not present in ATIF is not reconstructed.
Psychevo observability trace JSONL is also accepted by the Psychevo adapter. It
is a redacted typed runtime trace, not an exported session transcript. Version 1
trace JSONL may be converted directly when it contains retained message
payloads. Version 2 compact trace JSONL does not contain transcript messages;
direct conversion must return a warning and avoid fabricating transcript
content.

SQLite `--db` input is interpreted by the effective adapter for that DB input.
Adapters may implement native database conversion for their own
retained-session persistence. If an adapter does not implement native DB
conversion but does
implement record conversion, `peval-py` may use the configured generic
`messages` table mapping and then call `convert(records, config)`. That generic
mapping reads `session_seq`, `message_json`, `usage_json`, `metadata_json`, and
accounting columns ordered by `session_seq`. Table and column names supplied by
config must be SQL identifiers, not raw SQL fragments.
For generic SQLite inputs, the selected `--session-id` is report metadata even
when it is not duplicated inside individual message rows. ATIF output must set
`session_id` from that selected id. Native DB adapters may define their own
session selection behavior. Psychevo defaults to the most recently updated
session from the `sessions` table, OpenCode defaults to the most recently
updated session, and Hermes defaults to the session with the most recent active
message, ending, or start time when `--session-id` is omitted. Psychevo,
OpenCode, and Hermes also expose read-only session lists for
`view trajectory --list`, ordered by their default-selection recency semantics.
The rendered list columns are `#`, `session_id`, and `name`; missing names
render as `-`.
When a Psychevo DB session has a sibling observability trace sidecar, the
Psychevo adapter prefers version 1 or version 2 trace timing for generation and
tool execution wall start/end timing, then falls back to message metadata, then
timestamp intervals. Generation spans come from `generation_start` /
`generation_end`; tool execution spans come from `tool_execution_start` /
`tool_execution_end`. Trace absence or parse failure must produce a warning at
most and must not block message-based conversion.

## Adapters

`-a, --adapter` selects the default adapter or a per-input adapter selector.
Built-in adapters are always available:

- `psychevo` supports current Psychevo retained messages with
  `role=user|assistant|tool_result`, user text blocks, assistant text,
  reasoning, tool-call blocks, and current Psychevo SQLite persistence with
  `sessions` and `messages` tables. It may enrich retained messages with
  sibling `sessions/<session_id>/events.jsonl` observability traces.
- `opencode` supports the common single-session message JSONL shape and current
  OpenCode SQLite persistence with `session`, `message`, and `part` tables. For
  DB inputs, it may enrich current-session timing from the same database's
  `event` table when `message.part.updated` events are available: tool
  execution uses the first `running` start and final `completed`/`error` end,
  while model timing is an OpenCode boundary estimate from assistant message
  creation to the first tool call, or to assistant completion for no-tool final
  responses.
- `hermes` supports the common single-session message JSONL shape and current
  Hermes SQLite persistence with `sessions` and `messages` tables. For DB
  inputs, it may enrich the current session with explicit timing from the
  sibling `logs/agent.log` when the log's session-scoped API/tool events
  strictly match the DB transcript.

Adapters may mark source timestamps as order-only persistence timestamps rather
than measured execution timing. For order-only sources such as Hermes DB
records, peval-py preserves `wall_duration_ms` from available timestamps but
must not infer active model or tool durations from adjacent message timestamp
deltas. Explicit elapsed/start/end metadata remains trusted when a source
provides it, including Hermes `agent.log` timing fused into DB records.

Third-party adapters register through installed Python package entry points in
the `peval_py.adapters` group. The entry point name is the adapter id after
lowercase normalization. Unknown adapters fail with a diagnostic that lists
available adapter ids, and duplicate ids fail clearly instead of shadowing an
existing adapter.

Adapters may implement the normal record conversion contract,
`convert(records, config)`, for JSONL and generic SQLite inputs. Adapters that
need to parse a source file directly may also implement `convert_path(path,
config)`. Adapters that own a SQLite persistence format may implement
`convert_db(path, session_id, config)`.
For `-p/--path`, `peval-py` first recognizes exported ATIF JSON trajectory
objects. Otherwise it calls `convert_path` when the effective adapter provides
it, then falls back to reading JSONL into `MessageRecord` values and calling
`convert`. For `-d/--db`, `peval-py` calls `convert_db` when the effective
adapter provides it; otherwise it reads the configured generic SQLite
`messages` rows and requires a record adapter. An adapter used with `--db` that
supports neither native DB input nor record conversion must fail with a clear
unsupported-input diagnostic.

Adapters may preserve source metadata in report sidecars, but ATIF output must
stay standard and must not include peval-only fields.
