# peval-py

Language: English | [简体中文](README.zh-CN.md)

`peval-py` is the lightweight Python edition of `peval` for retained agent
trajectories. It reads JSONL sessions or adapter-owned SQLite databases and
writes ATIF JSON or static peval-style reports.

## Install From A Checkout

Install the local Python tool once with `uv`:

```bash
uv tool install --editable ./tools/peval-py
```

Then use the shorter command directly:

```bash
peval-py --help
peval-py view tr --help
```

Run it from the source tree without installing:

```bash
uv run --project tools/peval-py peval-py --help
```

## Build A Local Binary

`peval-py` has no default runtime dependencies outside the Python standard
library, so you can package it as a local executable. Reading `.xlsx` input
manifests is optional and requires `openpyxl` at runtime. Build on the same
operating system and CPU architecture where you plan to run the file. Keep
generated artifacts under `.local/`; the repository ignores that directory.

PyInstaller is the simplest single-file path:

```bash
cd /path/to/psychevo

uvx pyinstaller \
  --onefile \
  --name peval-py \
  --paths tools/peval-py/src \
  --distpath .local/peval-py-build/dist \
  --workpath .local/peval-py-build/work \
  --specpath .local/peval-py-build/spec \
  tools/peval-py/src/peval_py/cli.py
```

Run the packaged command and check a fixture-backed report:

```bash
.local/peval-py-build/dist/peval-py --help

.local/peval-py-build/dist/peval-py view tr \
  -a opencode \
  -p tools/peval-py/tests/fixtures/common_session.jsonl \
  -o .local/peval-py-build/report.json

python3 -m json.tool .local/peval-py-build/report.json >/dev/null
```

Nuitka is another option if you want a compiled-Python build and have a native
C compiler, but check its output size and startup behavior on your target
platform before choosing it.

## Usage Guide

Use `-a ADAPTER` to set the default adapter for all inputs. For comparison
reports, repeat `-a` with `pN=ADAPTER` or `dN=ADAPTER` to parse individual
path or DB inputs with different adapters.

Use `-i, --input-table PATH` when the inputs are easier to maintain as a CSV,
JSON, or `.xlsx` manifest. Each table row becomes one session in the same
report. Direct `-p/--path` and `-d/--db` inputs are loaded first, then table
rows are appended in file order. Relative `path` and `db` values resolve from
the manifest directory. `.xlsx` works only when `openpyxl` is installed; save
as CSV when you want the standard-library-only path.

In comparison reports, the Leaderboard Duration column and JSON `duration_ms`
fields show active agent/tool work time. Long retained-session idle gaps are
kept separately as `wall_duration_ms`.

When a peval-py workspace root is available, reports also try to read cached
peval cell analysis from
`runs/<analysis_eval_slug>/<agent-id>/<session-id>/<cell_key>/analysis.json`
and `analysis.md`. The default slug is `default`; matching summaries and
Markdown reports appear in the selected Trial Analysis section and in JSON
`annotations.analysis[]`.

The same task tree can also provide manual Trial notes at
`runs/<analysis_eval_slug>/<agent-id>/<session-id>/<cell_key>/notes.md`.
These appear in JSON `annotations.notes[]` before CLI/table notes. In
`peval-py serve`, refreshable sources can edit or add that cell-local
`notes.md`; snapshot uploads remain read-only.

CSV example:

```csv
path,db,session_id,adapter,n,report_note,agent_name,agent_version,model
runs/hermes.jsonl,,,,Hermes row note,Cross-agent comparison,Hermes,,deepseek-v4-flash
,state.db,ses_123,opencode,OpenCode row note,,,,
```

Then render one multi-session HTML report:

```bash
peval-py view tr \
  -a psychevo \
  -i inputs.csv \
  -f html \
  -o report.html
```

JSON manifests may be a top-level array or an object with `rows` and
`report_notes`:

```json
{
  "report_notes": ["Local cross-agent comparison."],
  "rows": [
    {"path": "runs/hermes.jsonl", "adapter": "hermes", "note": "Hermes row"},
    {"db": "opencode.db", "session_id": "ses_123", "adapter": "opencode"}
  ]
}
```

`export tr -i` is still single-session only after expansion. Use `view tr -i`
for a manifest with multiple rows.

For reporting, comparison, and custom adapter examples, read
[peval-py Lightweight Trajectory Reports](../../docs/evaluation/peval-py.md).
