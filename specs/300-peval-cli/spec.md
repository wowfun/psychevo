---
name: 300. peval CLI
psychevo_self_edit: deny
---

# 300. peval CLI

Define the `peval` command-line surface for checking, running, and viewing
evaluation work through `psychevo-eval`.

## Scope

- `peval` command families and process-level behavior
- offline checks versus real evaluation runs
- view-only reporting, comparison, and inspection positioning
- CLI artifact path defaults and user-facing diagnostics
- workspace initialization, discovery, and registry behavior

Out of scope:

- framework API design; see [095 Evaluation Framework](../095-evaluation-framework/spec.md)
- coding task semantics; see [350 Coding Evaluation](../350-coding-evaluation/spec.md)
- concrete agent and benchmark adapter internals

## CLI Position

`peval` is the product projection of `psychevo-eval`. It should be scriptable
first: commands must return structured diagnostics when JSON output is
requested, preserve useful exit codes, and avoid requiring an interactive
terminal for benchmark execution.

`peval` is service-backed. CLI handlers parse arguments, construct an explicit
service context from process cwd/environment, call `EvalService`, and render the
returned DTOs. CLI code must not duplicate workspace discovery, run selector
resolution, artifact loading, or view projection logic that belongs to the
service.

Within `psychevo-eval`, module boundaries mirror that service-backed shape.
Schema DTOs, workspace storage, run execution, view projection, serving, and CLI
argument/rendering code remain separate layers. The CLI layer owns process
arguments and user-facing output only; runner code owns evaluation execution and
artifact production; view code owns read-only report projection and static
rendering. Static report renderers and execution adapters must communicate
through stored artifacts and typed DTOs rather than reaching into each other's
private helpers. The crate root exposes only intentional public entrypoints and
DTOs; implementation helpers stay crate-private or module-private.

User-facing evaluation guides live under `docs/evaluation/`. Those guides may
show installation, getting-started, authoring, live evaluation, and automation
workflows, but this spec remains the source of truth for command behavior.

`peval check` is the offline safety gate. It validates eval configs, benchmark
manifests, registry resolution, schema versions, adapter declarations, command
availability, output paths, and view inputs without running agents, calling
providers, or downloading official datasets.

`peval check --live` is the explicit opt-in gate for checks that may probe real
providers, official benchmark tooling, Docker daemons, or network-backed
registries. Live checks still must not execute benchmark cases.

`peval prepare` is the explicit build and cache preparation entrypoint. It may
probe Docker, build or pull official benchmark task images, and prepare
container-resident ACP agent cache layers, but it must not execute benchmark
cases or call model providers.

`peval run` is the execution entrypoint. It may run real agents and call real
providers according to the selected agent and provider configuration. It still
defaults to small samples or explicit task limits for official or expensive
benchmarks, and user-facing benchmark docs should require explicit
`--task-set/--agent` selection.

`peval env` is the human-in-loop local task environment entrypoint. `env
create` prepares exactly one selected local task workspace under `runs/`
without running an agent or verifier. `env verify` scores that prepared
workspace after a human has edited it and writes a normal run fact using the
reserved `human-in-loop` candidate identity. Human-in-loop environments are
local-directory only in this version; container-backed tasks must fail with a
clear diagnostic. Human editing time is not inferred from wall clock time:
`env verify` requires an explicit duration argument and stores that value as
the trial duration.

## Artifact Layout

Current run facts default to
`<peval-workspace>/runs/<benchmark>/<agent-id>/<task-id>/<short-fingerprint>/`.
`run.json` stores the structured cell fact, and sidecar files such as
`trajectory.jsonl`, `evaluator.stdout`, and `evaluator.stderr` remain local
diagnostic artifacts. Callers may select the workspace with `--root/-r <dir>`
or `PEVAL_ROOT`; otherwise `peval` uses the nearest current-or-parent directory
containing workspace `peval.toml`, then `$PSYCHEVO_HOME/peval-config.toml`
`default_workspace`. Without an initialized workspace, store-backed commands
fail with a diagnostic that names `peval init`.

Human-in-loop task environments also live under `runs/`, using
`runs/<benchmark>/human-in-loop/<task-id>/<env-key>/`. Before verification,
these directories contain `workspace/`, `prompt.md`, `env.json`, and
`README.md` but deliberately do not contain `run.json`; report readers must
ignore them until `env verify` has produced a standard cell fact.

`--output-root <dir>` is isolated and does not participate in workspace reuse.
All explicit CLI paths resolve relative to process cwd; manifest paths keep
their manifest-owned resolution rules.

The peval workspace may contain schema v2 `peval.toml`, root-level
`*.eval.toml` starter eval configs, `scripts/`, `runs/`, `views/`, and
`datasets/<dataset-id>/dataset.toml` inventory records. Cell facts are the
source of truth for reuse and views. There is no current cache contract and no
generated workspace dashboard. Existing visible `index.json`,
namespace-level `latest.json`, `dashboard.html`, or v2 `summary.json` files are
legacy derived artifacts and current readers must not rely on them.

Workspace `peval.toml` and user `$PSYCHEVO_HOME/peval-config.toml` provide
reusable agent and benchmark registries. Eval configs may inline registry
overrides. Registry precedence is eval config, workspace, then user config.
Commands that accept evaluation input use `--config <eval-config.toml>` as the
primary entrypoint or `--benchmark <id-or-path>` for one-off use. Config
discovery prefers `eval.toml`; when absent, it accepts exactly one
non-recursive `*.eval.toml` match in the discovered directory and rejects
multiple matches as ambiguous.

`peval view` rendering is part of the local CLI surface. Formatting or
lint-only maintenance must preserve view semantics while keeping renderers
compatible with deterministic workspace validation. View schema v12 is the
Trial/MatrixCell public DTO. It keeps artifact schema v8 and the existing
physical run layout unchanged while renaming public cell identity to
`matrix_cell_key` and `trial_key`.
`peval view -i all` is a special include alias for the complete static
diagnostic report. It expands to
`summary,matrix,usage,warnings,artifacts,trajectory,trajectory-meta,analysis`
in that stable order. `timeline`, `atif`, `logs`, and `diff` are not supported
include aliases in v12 and must fail clearly. The alias is not serialized in
view DTOs; JSON reports expose the expanded include list. Trajectory step
timing metadata must describe the displayed step itself; grouped ACP steps use
their observed start/end span when available instead of labeling the gap from
the previous transcript row as the current row's duration. Expanded step blocks
render explicit reasoning before message content, and step metrics omit
unavailable key/value pairs instead of displaying placeholder dashes. Step span,
tool-call argument generation, and tool execution are distinct timings; HTML
rows and Metrics blocks must label step span separately and must not imply that
a long agent span is tool execution time.

`peval serve` is the local workspace viewer for stored Trial facts. It opens
the whole workspace by default, with config, benchmark, path, agent, task, and
status arguments acting as initial filters or focus. `--report KEY` selects
analysis profile overrides for explicit analysis actions. It binds localhost by
default, prints a generated token URL without opening a browser, serves offline
local HTML/JS/CSS assets, uses WebSocket JSON-RPC-lite for app data and analysis
events, and reserves HTTP for static assets and bounded file access.

The public selection term is `task-set`. `suite`, `--suite`, `suite_id`, and
`list --kind suites` are removed interfaces and must fail clearly rather than
aliasing to task-set behavior.

The CLI must report selected, executed, reused, overwritten, and failed cell
counts in successful human output and in machine output.

`peval check --json` reports benchmark metadata, selected case count, live-check
mode, and status. Official `harbor`, `swe_bench`, and `tau2` sources are opt-in
bridge declarations. `peval_agent` sources default to local execution;
`harbor` and `swe_bench` sources default to container execution. Source
manifests may override execution with `execution = "auto" | "container" |
"local"`. Setting an official source to `local` is allowed for advanced
experiments but must emit a prominent host-safety warning.

## Attachments

- [Commands](commands.md) defines `init`, `doctor`, `list`, `check`, `run`,
  `view`, and `dataset`.
- [Reporting](reporting.md) defines view formats and redaction behavior.
- [Testing](testing.md) defines CLI-specific deterministic validation.

## Related Topics

- [095 Evaluation Framework](../095-evaluation-framework/spec.md)
- [350 Coding Evaluation](../350-coding-evaluation/spec.md)
- [090 Artifacts](../090-evaluation/artifacts.md)
