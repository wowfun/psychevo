# peval-py

`peval-py` is the lightweight Python edition of `peval` for retained agent
trajectories. It reads JSONL sessions or Psychevo SQLite `messages` rows and
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

`peval-py` has no runtime dependencies outside the Python standard library, so
you can package it as a local executable. Build on the same operating system and
CPU architecture where you plan to run the file. Keep generated artifacts under
`.local/`; the repository ignores that directory.

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

For reporting, comparison, and custom adapter examples, read
[peval-py Lightweight Trajectory Reports](../../docs/evaluation/peval-py.md).
