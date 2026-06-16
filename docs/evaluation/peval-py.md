# peval-py Lightweight Trajectory Reports

Language: English | [简体中文](../i18n/zh-CN/evaluation/peval-py.md)

`peval-py` is a lightweight Python edition of peval for retained agent
trajectories. It reads JSONL sessions or adapter-owned SQLite databases and
writes derived ATIF trajectories or static peval-style reports. It can
initialize a local peval workspace for `serve`, but it does not run agents or
score tasks.

For install, source-tree usage, and local binary packaging, see
[tools/peval-py/README.md](../../tools/peval-py/README.md).

## Convert A JSONL Session

Use `export trajectory` when you want the raw ATIF JSON trajectory. `tr` is
the short form for `trajectory`:

```bash
peval-py export tr -p session.jsonl -o
python -m json.tool trajectory-psychevo-<session>.json >/dev/null
```

JSONL input accepts one object per line. Each line may be a direct message
object, or a wrapper with `message`, `usage`, `metadata`, `accounting`, and
`session_seq`.

`-p/--path` also accepts an exported ATIF JSON trajectory. ATIF JSON input does
not require `-a/--adapter`; it is treated as a passthrough source and appears
with adapter `atif` in report metadata:

```bash
peval-py view tr -p trajectory-opencode-<session>.json -o
```

Use `-a` when the source is not the default Psychevo adapter:

```bash
peval-py export tr -a opencode -p session.jsonl -o
peval-py export tr -a hermes -p session.jsonl -o
```

## Custom Agent Adapters

`peval-py` can load installed adapter packages through Python entry points.
Use this when a custom agent writes a transcript format that the built-in
Psychevo, OpenCode, and Hermes adapters do not parse.

In the adapter package, register an entry point in the `peval_py.adapters`
group. The entry point name is the adapter id users pass to `--adapter`:

```toml
[project.entry-points."peval_py.adapters"]
custom = "custom_peval_adapter:CustomAdapter"
```

Adapters may expose either `convert(records, config)` or `convert_path(path,
config)`. Use `convert` when the source can use the normal JSONL or SQLite
`messages` loaders. Use `convert_path` when the adapter needs to parse the
input file itself.

```python
from peval_py.adapters.base import ConversionResult, StepMeta


class CustomAdapter:
    agent_id = "custom"

    def convert_path(self, path, config):
        return ConversionResult(
            trajectory={
                "schema_version": "ATIF-v1.7",
                "trajectory_id": "custom:t001",
                "agent": {
                    "name": config.agent_name or "custom",
                    "version": config.agent_version,
                },
                "steps": [
                    {
                        "step_id": 1,
                        "source": "user",
                        "message": "converted custom transcript",
                    }
                ],
                "final_metrics": {
                    "total_steps": 1,
                    "total_turns": 1,
                    "total_tool_calls": 0,
                    "total_tool_errors": 0,
                },
            },
            steps_meta=[StepMeta(step_id=1, source="user")],
            warnings=[],
            total_events=1,
            unmapped_events=0,
            started_at_ms=None,
            finished_at_ms=None,
        )
```

Adapter-specific settings go in TOML, not CLI flags. `peval-py` passes each
effective adapter's table to `config.adapter_options`:

```toml
[defaults]
adapter = "custom"

[adapters.custom]
input_mode = "transcript"
```

Run the adapter like any built-in adapter:

```bash
peval-py view tr -c custom.toml -p custom-session.log -o
peval-py export tr -a custom -p custom-session.log -o
```

When an input has no explicit `-a`, `pN=`, `dN=`, or manifest adapter, peval-py
can infer the adapter from the path. The adapter id must appear as a full path
component or filename token, so paths under `.hermes/` and `.psychevo/` infer
`hermes` and `psychevo`. Ambiguous matches fail and ask for `-a`.

If a custom adapter only implements `convert_path`, use it with `-p/--path`.
For SQLite `-d/--db` input, implement `convert_db(path, session_id, config)` to
own the database parsing. Adapters without `convert_db` can still use
`convert(records, config)` with the generic configured `messages` table shape.

## Report From An OpenCode DB

The `opencode` adapter can read the current OpenCode SQLite persistence format
directly. Pass the OpenCode database path with `--db`. If `--session-id` is
omitted, the adapter selects the most recently updated session:

```bash
peval-py view tr \
  -a opencode \
  -d ~/.local/share/opencode/opencode.db \
  -o
```

Select a specific session when needed:

```bash
peval-py view tr \
  -a opencode \
  -d ~/.local/share/opencode/opencode.db \
  -s <session-id> \
  -o
```

List available sessions before choosing:

```bash
peval-py view tr -a opencode -d ~/.local/share/opencode/opencode.db --list
peval-py view tr -a opencode -d ~/.local/share/opencode/opencode.db -s #2 -o
peval-py view tr -a opencode -d ~/.local/share/opencode/opencode.db -li -o
```

For current OpenCode databases that include the `event` table, peval-py uses
the event stream to recover tool execution duration from first `running` start
to final `completed` or `error` end. Model timing is shown as an OpenCode
assistant/tool boundary estimate, not as provider API latency. Older databases
without matching events keep the existing part timestamp fallback.

## Report From A Hermes DB

The `hermes` adapter can read the current Hermes SQLite persistence format
directly. Pass the Hermes database path with `--db`. If `--session-id` is
omitted, the adapter selects the most recently active session. Stored
`sessions.system_prompt` content is included as the first system step when it
is present.

Hermes DB message timestamps are treated as persistence/order timestamps. The
report preserves wall duration from those timestamps, but active model and tool
durations stay unknown unless Hermes records include explicit elapsed/start/end
timing metadata. For current Hermes DB inputs, peval-py also checks the sibling
`logs/agent.log` file and uses its strictly matched API/tool timing as explicit
model and tool duration. If the log is missing or does not match the DB
transcript, active timing remains unknown.

```bash
peval-py view tr \
  -a hermes \
  -d ~/.hermes/state.db \
  -o
```

Select a specific session when needed:

```bash
peval-py view tr \
  -a hermes \
  -d ~/.hermes/state.db \
  -s <session-id> \
  -o
```

Use `--list`/`-l` to print session indexes, ids, and names. Use `-s #N` to
select by list index, or `--list-interactive`/`-li` to enter selections such as
`1,3-4` or `all`.

## Report From A Psychevo State DB

Use `view trajectory` for the peval-compatible JSON or offline HTML report.
`tr` works here too. The output suffix chooses the format, so `-f` is usually
unnecessary. If `--session-id` is omitted, the adapter selects the most
recently updated session from the Psychevo `sessions` table:

```bash
peval-py view tr \
  -d ~/.psychevo/state.db \
  -o
```

For JSON:

```bash
peval-py view tr \
  -d ~/.psychevo/state.db \
  -f json \
  -o

python -m json.tool report-psychevo-<session-id>.json >/dev/null
```

Select a specific session when needed:

```bash
peval-py view tr \
  -d ~/.psychevo/state.db \
  -s <session-id> \
  -o
```

`peval-py view tr -d ~/.psychevo/state.db --list` prints `#`, `session_id`, and
`name`. `-s 3` first tries a real session id `3`; if absent, it selects list
index 3. `-s #3` always means index 3.

The Psychevo DB reader selects a session first, then reads that session's
`messages` rows. It preserves the selected session id in the trajectory and
report header.

## Compare Sessions

`view tr` can compare retained sessions without requiring a peval workspace.
Each input session becomes one trial in a session-first report. The report
shows report notes, a filterable Leaderboard, a Trajectory Overview, then the
selected Trial trajectory. The comparison JSON stores one canonical
`leaderboard.entries` row list and intentionally omits benchmark/task matrix
fields plus older duplicate heatmap/table row lists.
Leaderboard `duration_ms` is active agent/tool work time and excludes retained
session idle gaps longer than 10 minutes. The original first-to-last event span
is preserved as `wall_duration_ms` in Trial metadata and leaderboard rows.

Compare JSONL sessions:

```bash
peval-py view tr -a opencode \
  -p session-a.jsonl \
  -p session-b.jsonl \
  -o
```

Compare Psychevo DB sessions:

```bash
peval-py view tr \
  -d ~/.psychevo/state.db \
  -s <session-a> \
  -s <session-b> \
  -o
```

Compare sessions from different adapter-owned DBs:

```bash
peval-py view tr \
  -d ~/.hermes/state.db \
  -d ~/.local/share/opencode/opencode.db \
  -a d1=hermes \
  -a d2=opencode \
  -o
```

Use `-a ADAPTER` as the default adapter for every input. Use `-a pN=ADAPTER`
or `-a dN=ADAPTER` when one path or DB input needs a different adapter. Path
and DB indexes are one-based and counted independently.

When multiple DB inputs need explicit sessions, bind each session id to its DB
with `-s dN=<session-id>`:

```bash
peval-py view tr \
  -d ~/.hermes/state.db \
  -d ~/.local/share/opencode/opencode.db \
  -a d1=hermes \
  -a d2=opencode \
  -s d1=<hermes-session-id> \
  -s d2=<opencode-session-id> \
  -o
```

`view tr` can also mix path and DB inputs. `export tr` remains single-session
only.

Add lightweight notes with peval-style indexes:

```bash
peval-py view tr \
  -d ~/.psychevo/state.db \
  -s <session-a> \
  -s <session-b> \
  -n 0="Report context" \
  -n 2="Session B follow-up" \
  -o
```

`-n/--note 0=...` is report-level. Positive indexes attach to the one-based
session order in the command. Repeating `-n/--note` appends notes in CLI order.
HTML report notes, Leaderboard note snippets, and selected Trial notes follow
the same display style as `peval view`.

## Serve UI Layout

Use `init` once when you want a local saved workspace for `serve`:

```bash
peval-py init --root .local/peval-py
peval-py serve --root .local/peval-py
```

`peval-py init` creates only the files needed by `peval-py serve`:
`peval-py.toml` and `state.db`. It preserves existing valid `peval-py.toml`
state DB paths and does not create `peval.toml`, `runs/`, `datasets/`,
`scripts/`, eval templates, `$PSYCHEVO_HOME/peval-config.toml`, or `.gitignore`.
Use `--json` for machine-readable output. `serve` uses an explicit `--root`,
`PEVAL_ROOT`, or current-or-parent `peval-py.toml`; the environment variable is
only a shared root-override name and does not make `serve` require a Rust
`peval` workspace.

The static HTML report remains the canonical offline report. `peval-py serve`
uses the same report body instead of a separate dashboard layout: Report Notes,
Leaderboard, Trajectory Overview, and the selected Trial trajectory keep the
static report order and styling.

Serve UI mode only adds web-only controls around that shared body. It shows a
compact source/status toolbar and opens source management in a modal for
Session/ATIF paths, SQLite DBs, input tables, JSONL uploads, ATIF JSON uploads,
and report JSON uploads. The path and DB fields accept multiple pasted paths;
quote paths that contain spaces. Windows drive paths such as `C:\...`, `D:\...`,
and UNC paths are preserved instead of being resolved under the workspace root;
when `serve` runs on POSIX and an existing `/mnt/<drive>/...` path matches, that
WSL-style path is used. Adapter controls are compact dropdowns in each form's
action row next to the add/upload action and default to `auto`, which uses the
same inference/default adapter rules as the CLI. Failed imports show the server
error and are not saved as sources. Sources can be archived for later restore or
deleted from peval-py state without deleting the original file or database. For
refreshable sources, the selected Trial Notes section can edit the matching
peval cell `notes.md`; snapshot uploads remain read-only. Source import forms
and Timeline diagnostic sections use transparent report-integrated shells,
while inputs and menus keep solid readable surfaces.

In the Leaderboard, web UI mode may add row checkboxes for export selection and
one `Export` menu with Table, JSON Report, and HTML Report choices. Row clicks
still select the Trial; checkbox clicks only control export scope. Exports use
visible checked rows when any currently visible row is checked, otherwise they
use the current filtered and sorted visible rows. JSON and HTML exports follow
the same row scope as table exports. Export and table filter menus close when
clicking outside them. Long Trajectory Overview rows wrap nodes onto additional
lines, and timed nodes use very low-contrast ten-level background shade depth
relative to the slowest step in that Trial so slow steps stand out without
adding text labels. Timeline Waterfall and Timeline Detail Table sections are
collapsible, and clicking user/system markers or timed rows opens the
corresponding Step details drawer.

For SQLite DB sources, the modal includes an Inspect flow. Enter or paste one
DB path, optionally choose an adapter, and click Inspect DB. Without an explicit
adapter, `serve` uses the same path-token adapter inference as `view tr -d`;
paths under `.hermes/`, `.psychevo/`, or `.opencode/` infer those adapters. If
the path cannot be inferred or matches multiple adapters, choose the adapter and
inspect again. Selected sessions are saved as independent refreshable sources,
so each can be archived, deleted, or refreshed on its own.

## Cached Analysis And Cell Notes

When a peval-py workspace root is known, `view tr` and `serve` refresh can read
cached peval cell analysis without modifying the source trajectory. The lookup
is read-only and uses:

```text
<workspace>/runs/<analysis_eval_slug>/<agent-id>/<session-id>/<cell_key>/analysis.json
<workspace>/runs/<analysis_eval_slug>/<agent-id>/<session-id>/<cell_key>/analysis.md
```

`analysis_eval_slug` defaults to `default`. `<session-id>` is the rendered
session id. `<agent-id>` is the input `agent_name` when available, otherwise
the effective adapter id. peval-py only uses cached analysis when exactly one
cell directory under the matching session directory contains `analysis.json` or
`analysis.md`. If both files exist in that one cell directory, JSON summary and
Markdown report are merged. Missing files, malformed JSON, unreadable Markdown,
or ambiguous cell matches are silently ignored.

The JSON report stores matching analysis under `annotations.analysis[]` with
compatible `relative_path`, optional top-level JSON `summary`, optional Markdown
`md_report`, and per-format `relative_paths`. The HTML selected Trial area
shows an Analysis section only when cached analysis exists. `serve` persists
the enriched report snapshot during refresh, so changes to `analysis.json` or
`analysis.md` need Refresh before the browser view updates.

peval-py also reads peval cell manual notes from the same task tree:

```text
<workspace>/runs/<analysis_eval_slug>/<agent-id>/<session-id>/<cell_key>/notes.md
```

`notes.md` is a Trial note, not analysis. It is accepted only when exactly one
cell directory with `notes.md` matches the task, then appears in
`annotations.notes[]` with `source = "cell"`, label `notes.md`, Markdown text,
and a relative `source_ref`. Cell notes render before CLI or input-table notes.

In `serve`, `Edit notes` or `Add notes` writes that cell-local `notes.md` for a
refreshable source and immediately refreshes the source snapshot. If no note
cell exists, peval-py writes beside a unique analysis cell, or creates
`peval-py-notes/notes.md` when no cell exists. Ambiguous note or analysis cells
fail without writing.

## Localized HTML Reports

English is the default report UI language. To localize the report title and
comparison chrome to Simplified Chinese, set the locale in `-c` config:

```toml
[defaults]
locale = "zh-CN"
```

For workspace-local defaults, put top-level locale in `peval-py.toml`:

```toml
state_db = "state.db"
locale = "zh-CN"
analysis_eval_slug = "default"
```

An explicit `-c` file overlays `peval-py.toml`; keys omitted from `-c` keep the
workspace value.

`zh` is accepted as an alias for `zh-CN`, and `en-US` normalizes to `en`.
Locale is config-only; there is no CLI flag. In Simplified Chinese reports,
domain terms such as Run, Result, Notes, Evidence, Steps/events, Session,
variant, evaluator, reasoning, selected trial trajectory, Turns, Tool Calls,
tool success / total, cache read, and cache write remain English. The final
selected Trial Steps detail section also remains English.

## Useful Flags

- `-p, --path PATH`: read a session path. Built-in adapters treat it as JSONL;
  exported ATIF JSON is accepted without an adapter, and custom path adapters
  may parse their own file format.
- `-d, --db PATH`: read an adapter-owned SQLite database. Repeat it with
  `view tr` for cross-DB comparison.
- `-s, --session-id ID`: select a DB session. With one DB, bare `-s ID`
  remains valid and repeatable. Use `-s #N` for list indexes; with multiple
  DBs, use `-s dN=ID` or `-s dN=#M`.
- `--list, -l`: print DB session indexes, ids, and names, then exit.
- `--list-interactive, -li`: prompt for session indexes such as `1,3-4` or
  `all`, then render the selected sessions.
- `-a, --adapter ADAPTER`: select the default built-in adapter or installed
  adapter entry point. Repeat it with `pN=ADAPTER` or `dN=ADAPTER` for
  per-input overrides.
- `-f, --format json|html`: force report format.
- `-o, --output [PATH]`: write to a file instead of stdout. Bare `-o` writes
  `trajectory-<adapter>-<session>.json` for export,
  `report-<adapter>-<session>.html` for HTML view, or
  `report-<adapter>-<session>.json` with `-f json`. Multi-session views use
  `report-<adapter>-sessions-<count>.<format>` when all inputs share an
  adapter, or `report-multi-adapter-sessions-<count>.<format>` when they do
  not.
- `-n, --note N=TEXT`: add a report note (`0`) or session note (`1..N`) for
  `view tr`.
- `--max-content-chars N`: bound large message/tool payloads.
- `--no-redact`: disable default secret redaction.

By default, reports redact obvious secret-bearing keys, authorization headers,
bearer tokens, and common `token=...` text. Numeric token and accounting totals
remain visible.

## What To Look For

The HTML report shows the selected Trial/session, Run and Result summaries,
optional Notes and Usage Breakdown evidence, and the visible trajectory steps.
Matched tool observations appear inside the Agent step that issued the tool
call. Failed tool calls use a red tool chip and still remain attached to the
same Agent step.

Step token chips use real per-step metrics when the source provides them. If a
step has visible text but no per-step token metrics, the HTML report shows an
estimated chip with an `≈` prefix and tooltip. If `tiktoken` is installed in
the runtime environment, `peval-py` uses it for that HTML estimate; otherwise
it falls back to a deterministic byte-length estimate. These estimates are
visual only and are not written into ATIF or report JSON.

Steps timing chips use a subtle proportional fill when timing metadata is
available. Step duration, elapsed time, and tool execution time each scale
against comparable timings in the selected Trial; elapsed time scales against
the retained wall duration when available. The fill is a visual cue rather than
a new report metric.

If a tool result has no matching tool call, `peval-py` keeps it visible as a
standalone observation step and records a conversion warning in the report.

`export tr` is intentionally single-session only. Use repeated `-p`, repeated
`-d`, repeated `-s`, or mixed path/DB inputs only with `view tr`.
