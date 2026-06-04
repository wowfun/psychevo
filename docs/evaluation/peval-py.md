# peval-py Lightweight Trajectory Reports

`peval-py` is a lightweight Python edition of peval for retained agent
trajectories. It reads JSONL or SQLite `messages` rows and writes derived ATIF
trajectories or static peval-style reports. It does not run agents, score
tasks, or mutate peval workspaces.

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

Adapter-specific settings go in TOML, not CLI flags. `peval-py` passes the
selected adapter's table to `config.adapter_options`:

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
SQLite `-d/--db` input requires `convert(records, config)` because `peval-py`
loads `messages` rows before conversion.

## Report From A Psychevo State DB

Use `view trajectory` for the peval-compatible JSON or offline HTML report.
`tr` works here too. The output suffix chooses the format, so `-f` is usually
unnecessary:

```bash
peval-py view tr \
  -d ~/.psychevo/state.db \
  -s <session-id> \
  -o
```

For JSON:

```bash
peval-py view tr \
  -d ~/.psychevo/state.db \
  -s <session-id> \
  -f json \
  -o

python -m json.tool report-psychevo-<session-id>.json >/dev/null
```

The SQLite reader only reads the selected `messages` rows. It preserves the
selected session id in the trajectory and report header.

## Compare Sessions

`view tr` can compare retained sessions without requiring a peval workspace.
Each input session becomes one trial in a session-first report. The report
shows report notes, a metric-switchable Visible Heatmap, a Leaderboard, then
the selected Trial trajectory. The comparison tables intentionally omit
benchmark and task columns. In the heatmap, each session occupies one row with
a session/trial label on the left and a metric cell on the right, so large
comparisons grow vertically instead of forming one long horizontal strip.

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

## Useful Flags

- `-p, --path PATH`: read a session path. Built-in adapters treat it as JSONL;
  custom path adapters may parse their own file format.
- `-d, --db PATH`: read a SQLite state database.
- `-s, --session-id ID`: select a DB session. Repeat it with `view tr` to
  compare sessions from the same DB.
- `-a, --adapter ADAPTER`: select a built-in adapter or an installed adapter
  entry point.
- `-f, --format json|html`: force report format.
- `-o, --output [PATH]`: write to a file instead of stdout. Bare `-o` writes
  `trajectory-<adapter>-<session>.json` for export,
  `report-<adapter>-<session>.html` for HTML view, or
  `report-<adapter>-<session>.json` with `-f json`. Multi-session views use
  `report-<adapter>-sessions-<count>.<format>`.
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

If a tool result has no matching tool call, `peval-py` keeps it visible as a
standalone observation step and records a conversion warning in the report.

`export tr` is intentionally single-session only. Use repeated `-p` or repeated
`-s` only with `view tr`.
