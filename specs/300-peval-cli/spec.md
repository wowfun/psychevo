---
name: 300. peval CLI
psychevo_self_edit: deny
---

# 300. peval CLI

Define the `peval` command-line surface for running, checking, reporting, and
replaying evaluation work through `psychevo-eval`.

## Scope

- `peval` command families and process-level behavior
- offline checks versus real evaluation runs
- report, compare, and replay command positioning
- CLI artifact path defaults and user-facing diagnostics

Out of scope:

- framework API design; see [095 Evaluation Framework](../095-evaluation-framework/spec.md)
- coding task semantics; see [350 Coding Evaluation](../350-coding-evaluation/spec.md)
- concrete agent and benchmark adapter internals

## CLI Position

`peval` is the product projection of `psychevo-eval`. It should be scriptable
first: commands must return structured diagnostics when JSON output is
requested, preserve useful exit codes, and avoid requiring an interactive
terminal for benchmark execution.

User-facing evaluation guides live under `docs/evaluation/`. Those guides may
show installation, getting-started, authoring, live evaluation, and automation
workflows, but this spec remains the source of truth for command behavior.

`peval check` is the offline safety gate. It validates manifests, schema
versions, local fixtures, adapter declarations, command availability, output
paths, and report inputs without running live providers or downloading official
datasets.

`peval run` is the execution entrypoint. It may run real agents and call real
providers when the selected manifest and environment allow it. It still defaults
to small samples or explicit task limits for official or expensive benchmarks.

## Artifact Layout

Run artifacts default to
`<peval-root>/<namespace>/<run-id>`, where `<namespace>` comes from
`eval.toml` `output_root` or defaults to `runs/<project-slug>`. Callers may
select the store with `--root <dir>` or `PEVAL_ROOT`; otherwise `peval` reads
`$PSYCHEVO_HOME/peval.toml`, created by `peval init`. Without an initialized
config or explicit root, store-backed commands fail with a diagnostic that
names `peval init`.

`--output-root <dir>` keeps its per-run meaning and writes `<dir>/<run-id>`
without registering that run in the store index or dashboard. All explicit CLI
paths resolve relative to process cwd; manifest paths keep their manifest-owned
resolution rules.

The persistent store may contain `index.json`, namespace-level `latest.json`,
`dashboard.html`, run reports, and `datasets/<dataset-id>/dataset.toml`
inventory records. Shared caches, official dataset caches, and Python sidecar
caches live outside per-run artifacts unless a later spec promotes them into
the store.

Report and dashboard rendering are part of the local CLI surface. Formatting or
lint-only maintenance must preserve generated report semantics while keeping
the renderer compatible with default workspace validation.

The CLI must report the run artifact root in successful human output and in
machine output.

## Attachments

- [Commands](commands.md) defines `doctor`, `list`, `check`, `run`, `report`,
  `compare`, and `replay`.
- [Reporting](reporting.md) defines report formats and redaction behavior.
- [Testing](testing.md) defines CLI-specific deterministic validation.

## Related Topics

- [095 Evaluation Framework](../095-evaluation-framework/spec.md)
- [350 Coding Evaluation](../350-coding-evaluation/spec.md)
- [090 Artifacts](../090-evaluation/artifacts.md)
