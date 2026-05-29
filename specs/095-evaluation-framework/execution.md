---
name: 095. Evaluation Framework Execution Attachment
psychevo_self_edit: deny
---

Define framework execution, environment, concurrency, and failure behavior.

This attachment is part of [095 Evaluation Framework](spec.md).

## Scope

- runner orchestration
- local and Docker environment provider expectations
- conservative concurrency
- failure continuation
- artifact writing responsibilities

Out of scope:

- concrete domain scoring semantics
- report rendering details
- exact process protocol for each external agent

## Runner Orchestration

The framework runner loads the eval config, resolves benchmark and agent
registries, validates adapters, expands factors, prepares environments, invokes
candidates, runs evaluators or imports benchmark results, and writes artifacts.

Readiness checks should happen before executing a case when the required
command, sidecar, dataset, feature, model mapping, credential, or environment
provider is missing. A readiness failure records a setup failure for affected
cases instead of erasing them from the matrix.

## Environment Providers

The first framework environment providers are:

- local temporary workspace
- Docker-backed workspace

Each task attempt gets an isolated workspace unless a lower spec explicitly
allows shared state. User home directories, agent config roots, and environment
variables are isolated by default and populated only through manifest
allowlists.

Provider API network access may be allowed for agent calls. Task workspaces,
evaluators, and containers default to no external network unless the task set or
task opts in.
Deterministic tests use local mock provider endpoints while exercising the same
agent command path as real runs.

## Concurrency

Framework execution defaults to conservative concurrency. Implementations may
support explicit jobs or parallelism options, but must avoid assuming that
external agents, Docker environments, provider quotas, ports, or local caches
are safe for unbounded concurrent use.

## Failure Continuation

The framework records setup, runtime, scoring, timeout, cancelled, and skipped
states per case. A failed case does not terminate the entire run unless the
caller requested a strict fail-fast mode.

## Artifacts

Each executable cell writes a structured `run.json` fact, trajectory artifact,
and diagnostic logs under
`runs/<benchmark>/<agent-id>/<task-id>/<short-fingerprint>/`.
Derived views are generated from those artifacts rather than from live agent
state.

Adapters that execute a candidate through a structured observation interface
must preserve the candidate's emitted observation events in the case
trajectory. Process exit status alone is not enough for replay or failure
diagnosis when the candidate can report turns, messages, model calls, and tool
executions.

The CLI-selected evaluation store root is a peval workspace. It is supplied
with `--root/-r`, read from `PEVAL_ROOT`, discovered from the current directory
or a parent directory containing workspace `peval.toml`, or read from
`$PSYCHEVO_HOME/peval-config.toml` after `peval init --default`.
Run facts default to
`<workspace>/runs/<benchmark>/<agent-id>/<task-id>/<short-fingerprint>`.
An explicit per-run output root is an escape hatch and does not participate in
workspace reuse.

The workspace may maintain `datasets/`, `runs/`, `views/`, and workspace-owned
scripts. Cell facts are the source of truth. This slice has no cache contract and no
generated static dashboard. Existing visible `index.json`,
namespace-level `latest.json`, `dashboard.html`, or v2 per-invocation
`summary.json` files are legacy derived artifacts and must not be required for
reads. Persisted artifacts include `schema_version`; current v6 readers reject
unsupported versions on direct reads and ignore unsupported versions during
workspace scans.

View flows are artifact-only. They read stored cell facts, trajectories only
when a future explicit raw diagnostic API asks for them, and diagnostics
metadata. They do not invoke agents, evaluators, setup commands, or live
providers.

Service-backed callers use an explicit context instead of implicit process
globals. The CLI may construct that context from process cwd and environment,
but tests, future Web surfaces, and embedded callers should inject cwd,
environment variables, default config home, and capabilities explicitly.

Store reads scan cell facts and dataset records for correctness. Write-capable
flows write only current source-of-truth artifacts and workspace metadata; they
do not refresh hidden caches or generated dashboards in this slice.

## Related Topics

- [090 Runner](../090-evaluation/runner.md)
- [090 Artifacts](../090-evaluation/artifacts.md)
- [095 Sidecar](sidecar.md)
