---
name: 300. peval CLI Testing
psychevo_self_edit: deny
---

Define deterministic validation for the `peval` command-line surface.

## Scope

- CLI parsing and help coverage
- offline check behavior
- artifact path and view rendering behavior
- fake adapter command integration

Out of scope:

- real provider calls
- OpenCode, Hermes, or Psychevo live agent execution
- official benchmark downloads or harness runs

## Deterministic Coverage

CLI tests should use temporary homes, temporary output roots, fake manifests,
fake agents, local evaluators, and generated local task sets. They should verify
that `doctor`, `list`, `check`, `run`, and `view` can be exercised without user
credentials.

`peval check` coverage must prove that live provider work is not triggered.
`peval run` coverage may execute fake agents and local evaluators only.

View tests should assert structured view data, redaction behavior, JSON artifact
paths, and HTML omission of raw trajectory and log bodies. They should
avoid brittle snapshots of full HTML when structured comparison can cover the
same behavior. Include parsing coverage must verify `-i all`, mixed
`summary,all`, duplicate include de-duplication, stable expanded ordering, and
JSON reports serializing the expanded include list. Diagnostic view coverage
must verify artifact indexes, bounded timeline/log previews, derived ATIF v1.7,
view schema v11 Trial prompt references, compact `trajectory` plus
`trajectory_meta`, removed `atif` include diagnostics, flat leaderboard aggregate/task/trial
rows, leaderboard enum-column multi-select filtering and numeric-column sorting
controls, latest MatrixCell representative selection, status-colored heatmap
cells when the selected metric is missing, prompt-unavailable compatibility,
fine-grained ACP trajectory steps, optional system prompt steps, step
duration/elapsed derivation from event timestamps, non-collapsible redacted
system/reasoning HTML blocks, de-duplicated Run/Result/Files trajectory panels,
HTML/JSON-only view formats, analysis cache
discovery, diff file precedence, and trajectory-diff fallback without executing
providers or agents.

Persistent-workspace tests should verify `peval init`, `peval init --default`,
`$PSYCHEVO_HOME/peval-config.toml` default workspace loading, `--root/-r`,
`PEVAL_ROOT`, current-or-parent workspace discovery, registry precedence,
explicit `--output-root` bypass behavior, safe workspace-relative cell layout,
artifact-scan-backed views, config-free dataset listing, dataset import/listing,
root-level default `*.eval.toml` templates, unique `*.eval.toml` config
discovery, ambiguous template diagnostics, and view rendering without embedding
raw trajectory or log bodies. Tests should assert that current code does not
create visible
workspace `index.json`, namespace `latest.json`, hidden `.cache` indexes, or
`dashboard.html`, and that legacy visible files are ignored and left untouched.

Service-backed tests should verify service context isolation from process cwd
and environment, read/write/execute capability enforcement, structured
diagnostics in CLI JSON outputs, `peval view` include parsing and JSON/HTML
rendering, artifact v7 readers, old artifact scan skipping, and
benchmark/eval config/evaluator-result readers.

Black-box integration tests under `crates/psychevo-eval/tests/` should cover
public CLI contracts that users rely on. The checked-in `pidx-coding` benchmark
must have an integration test that invokes the `peval` binary with `--benchmark`
or a template-derived eval config and `--json`, isolates user home/config
environment, and asserts the public JSON shape for matrix check output without
running providers or external agents.

Generated CLI test projects are internal validation assets, not public benchmark
claims. Checked-in user-facing examples live under
`crates/psychevo-eval/benchmarks/` and must be runnable as normal peval
benchmarks through eval configs.

## Repo-Local Dev Home

The repo-local peval development environment may use `.local/.psychevo-dev/` as
an isolated `PSYCHEVO_HOME`. Its `peval-config.toml` may point the default
workspace at `.local/evals-dev/` so local peval validation can omit `--root`
while still avoiding the user's normal Psychevo home.

Commands and scripts that rely on this dev home must set `PSYCHEVO_HOME`
explicitly. Live provider validation that uses `.local/.psychevo-dev/` remains
opt-in and must not enter the default validation path.

## Validation

The default validation path must not require Python sidecar
dependencies, Docker, provider API keys, official datasets, or installed third
party agents. Those checks belong behind explicit live or integration
validation commands.

## Related Topics

- [300 Commands](commands.md)
- [300 Reporting](reporting.md)
- [330 Local Benchmark Integration](../330-benchmark-integrations/local.md)
- [350 Coding Evaluation Testing](../350-coding-evaluation/testing.md)
- [356 Pidx Coding Benchmark](../356-pidx-coding-benchmark/spec.md)
