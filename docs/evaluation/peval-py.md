# peval-py Lightweight Trajectory Reports

Language: English | [简体中文](../i18n/zh-CN/evaluation/peval-py.md)

`peval-py` is a lightweight Python edition of peval for retained agent
trajectories. It reads JSONL sessions or adapter-owned SQLite databases and
writes derived ATIF trajectories or static peval-style reports. It does not run
agents, score tasks, or mutate peval workspaces.

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

## Report From A Hermes DB

The `hermes` adapter can read the current Hermes SQLite persistence format
directly. Pass the Hermes database path with `--db`. If `--session-id` is
omitted, the adapter selects the most recently active session. Stored
`sessions.system_prompt` content is included as the first system step when it
is present.

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

The static HTML report remains the canonical offline report. A future
`peval-py serve` web UI uses the same report body instead of a separate
dashboard layout: Report Notes, Leaderboard, Trajectory Overview, and the
selected Trial trajectory keep the static report order and styling.

Serve UI mode only adds web-only controls around that shared body. Its import
area is collapsed by default above the report title. In the Leaderboard, web UI
mode may add row checkboxes for export selection and a split export control in
the section header. Row clicks still select the Trial; checkbox clicks only
control export scope. Exports use visible checked rows when any currently
visible row is checked, otherwise they use the current filtered and sorted
visible rows. JSON and HTML exports follow the same row scope as CSV table
exports.

## Localized HTML Reports

English is the default report UI language. To localize the report title and
comparison chrome to Simplified Chinese, set the locale in config:

```toml
[defaults]
locale = "zh-CN"
```

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
  remains valid and repeatable. With multiple DBs, use `-s dN=ID`.
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
