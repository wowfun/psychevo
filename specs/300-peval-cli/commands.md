---
name: 300. peval CLI Commands Attachment
psychevo_self_edit: deny
---

Define command behavior for the `peval` binary.

This attachment is part of [300 peval CLI](spec.md).

## Scope

- command family purposes
- offline versus execution behavior
- high-consequence command boundaries
- expected structured output posture

## Commands

The current `peval` implementation exposes `init`, `doctor`, `list`, `check`,
`run`, `view`, `serve`, and `dataset`. Commands that need an evaluation target accept
`--config/-c <path-to-eval-config.toml>` or `--benchmark <id-or-path>`.
Current-directory `eval.toml` discovery is a convenience for eval config files.

`peval init [--root/-r <dir>] [--default] [--force]` creates or repairs a peval
workspace. Without `--root`, it initializes the current directory. It creates
`peval.toml`, `runs/`, `datasets/`, and `scripts/`, and copies missing
workspace script templates without overwriting user-edited scripts. It does not
create or edit `.gitignore`. `peval.toml` is a schema v2 registry config with
empty agent and benchmark registries by default. `--default` records the
workspace at `$PSYCHEVO_HOME/peval-config.toml`; changing that default requires
`--force`. `peval init` is independent of `pevo init`.

`peval project` is removed. Invocations fail clearly and tell users to select
eval configs with `--config`, one-off benchmarks with `--benchmark`, and
registry entries through eval, workspace, or user config files.

`peval doctor` inspects local readiness. It checks installed commands,
configured sidecar support, known agent preset readiness, Docker availability
when requested, provider credential allowlists, and cache/output writability.
It does not execute benchmark tasks.

`peval list` enumerates discoverable task sets, adapters, presets, view formats,
datasets, or artifacts from configured locations. Listing is observational and
must not download official datasets unless the user explicitly asks for remote
refresh behavior in a later spec.

`peval check` validates eval configs or one-off benchmark selections. It
resolves benchmark and agent registries, expands factors, validates schema
versions, verifies evaluator declarations and typed task specs, checks output
paths, and resolves adapter readiness far enough to report setup problems. It
is the default command for CI and local spec conformance because it stays
offline. `--live` is an explicit opt-in for provider, official-tool, Docker, or
network readiness probes; live checks still do not execute benchmark cases.

`peval run` executes or reuses an expanded matrix. It records every cell
outcome, continues after per-cell setup/runtime/scoring failures, and writes a
cell fact under
`runs/<benchmark>/<agent-id>/<task-id>/<short-fingerprint>/run.json`.
The short fingerprint is derived from benchmark identity, task set, task and
workspace content, evaluator, agent/adapter/model/options, factors, and
artifact or runner version. Repeated runs reuse completed cells by default. Missing,
malformed, setup-failed, and runtime-failed cells are retried. Completed
failed, evaluator-failed, and timeout cells are reused because they are terminal
evaluation facts. `--overwrite` reruns selected cells and replaces existing
cell facts. `--run-id` is not supported.

`peval view [--config PATH|--benchmark ID_OR_PATH] [--report KEY] [--path PATH]...
[--task-set ID]
[--agent ID] [--task ID] [--status STATUS] [--group-by agent,task,task-set,status]
[-i/--include all|summary,matrix,usage,warnings,artifacts,trajectory,atif,logs,analysis,diff]
[--format json|markdown|html]
[-o|--output [PATH]]` renders dynamic logical views over cell facts. Without
`--path`, the scope is the selected benchmark under `runs/<benchmark-id>`.
With `--path`, the path may point at `runs/<benchmark>`,
`runs/<benchmark>/<agent>`, `runs/<benchmark>/<agent>/<task>`, or a concrete
cell directory. Filters are applied after path scoping. `--report KEY` selects
the optional report profile used by analysis overrides. Format defaults to
Markdown and is inferred from `.json`, `.md`, or `.html` output extensions when
`--format` is omitted and an explicit output path is present. With `-o` or
`--output` and no path, views mirror the selected `runs/` scope under
`<workspace>/views/`. JSON exposes schema v7 Trial/MatrixCell DTOs and
structured data references rather than legacy public `cell_key` fields. Static
Markdown and JSON stay summary plus references by default; HTML may inline
trajectory, small image, and key diff data needed for visualization, while logs
and ordinary artifacts remain reference-only.
`-i all` expands to every include in the documented order and may be mixed with
specific include names; duplicate includes are removed before rendering.
`timeline` is removed from the include grammar and fails clearly.

`peval serve [--config PATH|--benchmark ID_OR_PATH] [--report KEY] [--path PATH]
[--task-set ID] [--agent ID] [--task ID] [--status STATUS]
[--root DIR] [--host ADDR] [--port PORT]` starts a local-only Harbor-inspired
viewer over stored Trial facts. Without an eval target it opens the whole
workspace; target and filter flags become initial UI focus. It is read-only for
run artifacts except for explicit analysis cache writes. It binds localhost by
default, prints a tokenized URL, does not open the browser, serves offline
local assets, uses WebSocket JSON-RPC-lite for app data and analysis events,
and uses HTTP for static assets plus bounded file/download endpoints. The
server may expose local trajectory, artifact, verifier, log, image, ATIF, and
analysis content after token, path-containment, MIME, and 1 MiB size checks. It
must not execute benchmark cases.

`peval dataset import <path>` registers a local benchmark payload in the
persistent store. The first implementation records a manifest and references or
links the source payload; it does not copy large datasets by default and does
not download official benchmark data.

`peval list --kind datasets` is store-only and does not require an eval config.
`peval list --kind agents|benchmarks` reads resolved registries. Task-set,
task, and all-listing modes need a resolved benchmark from `--config` or
`--benchmark`. Stored result inspection uses `peval view`, not `peval list
--kind runs`.

## Machine Output

Commands that support machine output should emit one parseable stream or
document per invocation. Error JSON uses a stable error type and message, and
should include the command phase when the failure came from readiness, manifest
validation, execution, scoring, view rendering, or artifact loading.

Machine errors are structured diagnostics. Diagnostics include a stable code,
message, optional hint, severity, and source path when available. CLI JSON and
future local Web surfaces should render diagnostics from the same service
diagnostic model.

## Related Topics

- [300 Reporting](reporting.md)
- [095 Manifest](../095-evaluation-framework/manifest.md)
- [095 Execution](../095-evaluation-framework/execution.md)
