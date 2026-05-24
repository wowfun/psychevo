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

The framework runner loads manifests, validates adapters, expands factors,
prepares environments, invokes candidates, runs scorers or imports benchmark
results, and writes artifacts.

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
scorers, and containers default to no external network unless the suite opts in.
Live provider execution is additionally gated by the project manifest's
`allow_live` value; the default is `false`.

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

Each run writes a structured summary, per-case results, trajectory artifacts,
and diagnostic logs under the selected artifact root. Derived reports are
generated from those artifacts rather than from live agent state.

Adapters that execute a candidate through a structured observation interface
must preserve the candidate's emitted observation events in the case
trajectory. Process exit status alone is not enough for replay or failure
diagnosis when the candidate can report turns, messages, model calls, and tool
executions.

The CLI-selected evaluation store root is user-level state. It is supplied
explicitly, read from `PEVAL_ROOT`, or read from user-level `peval.toml` after
initialization. Run artifacts default to
`<store-root>/<namespace>/<run-id>`, where the namespace comes from the
evaluation config's store-relative `output_root` or defaults to
`runs/<project-slug>`. An explicit per-run output root is an escape hatch and
writes `<output-root>/<run-id>` without registering the run in the persistent
store.

The store may maintain `index.json`, namespace-level `latest.json`, and static
dashboard artifacts. These indexes are derived from run summaries; readers
must fall back to scanning run summaries when an index cannot be read. The run
id is generated when omitted and can be supplied explicitly by callers.
Persisted artifacts include `schema_version`; the first implementation rejects
unsupported versions instead of migrating them.

Compare and replay flows are artifact-only. Compare reads stored summaries and
case results. Replay reads stored trajectories and diagnostics. Neither flow
invokes agents, scorers, setup commands, or live providers.

## Related Topics

- [090 Runner](../090-evaluation/runner.md)
- [090 Artifacts](../090-evaluation/artifacts.md)
- [095 Sidecar](sidecar.md)
