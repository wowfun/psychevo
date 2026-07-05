---
name: 305. peval-py
psychevo_self_edit: deny
---

# 305. peval-py

Define `peval-py`, the lightweight Python edition of `peval`. The current
capability is offline agent trajectory export and reporting; future
capabilities may add more evaluation-adjacent inspection scenarios under the
same command tree.

## Scope

- offline trajectory export of one session from JSONL or SQLite `messages` rows
- ATIF v1.7 trajectory projection
- single-session and session-comparison JSON/HTML report generation, including
  single-row HTML comparison panels
- minimal `peval-py serve` workspace initialization for local report state
- a local `serve` web UI over a saved peval-py workspace, backed by a
  Python-owned state layer, with active and archived source comparison views
  that recover to the target view when a batch source-state action empties the
  current view
- Source Manager import of complete Trial cells from local external `runs/`
  trees into the selected peval-py workspace
- read-only peval cell cached analysis and manual cell notes enrichment, plus
  explicit serve editing of cell-local `notes.md`
- a bundled `peval-py` agent skill that guides offline session diagnostics,
  report/export workflows, analysis report creation, and Trial-cell import
- config-selected English and Simplified Chinese HTML report UI localization
- translated canonical docs under `docs/i18n/<locale>/...`
- localized tool README files beside their original README files
- adapter-specific message readers for Psychevo, OpenCode, and Hermes
- deterministic local tests for the `peval-py` package

Out of scope:

- agent execution, benchmark execution, scoring, or reruns
- live providers, ACP server startup, official benchmark harnesses, remote bind,
  or multi-user service behavior
- a token or authentication model for `peval-py serve`
- benchmark/task comparison matrices; `peval-py` comparison is session-first
  and does not introduce benchmark or task axes
- generic runtime debug tables as canonical sources for v1 conversion

## Attachments

- [Testing](testing.md) defines deterministic validation expectations.
- [Agent Skill](skill.md) defines the bundled skill contract and analysis import guidance.
- [Inputs and Adapters](inputs.md) defines trajectory source, CLI input, config, and adapter behavior.
- [Serve Workspace State](serve-state.md) defines peval-py workspace persistence.
- [Outputs](outputs.md) defines report, HTML, serve UI, API, and redaction output behavior.
- [Architecture](architecture.md) defines non-adapter module boundaries,
  dependency direction, and asset bundling rules.

## Position

The CLI lives under `tools/peval-py/` and is runnable with `uv`. Its console
command is `peval-py`. It is a simplified Python companion to the Rust `peval`
CLI that is lightweight enough to install and use on its own. It is
independent from the Rust workspace. It may use Python runtime dependencies
declared in `tools/peval-py/pyproject.toml`; `pandas` is used for inspect-mode
tabular analysis.

The tool reads existing retained session material and produces derived files.
It must not update Psychevo state databases, benchmark artifacts, Rust peval
workspace registries, or live provider state. `init` creates only the
Python-owned files required by `peval-py serve`: `<workspace>/peval-py.toml`
and `peval_py_*` tables inside `<workspace>/state.db`. `serve` startup must not
depend on unrelated Rust peval workspace files such as `peval.toml`, `runs/`,
`datasets/`, `scripts/`, eval templates, or `$PSYCHEVO_HOME/peval-config.toml`.
CLI path input resolution treats Windows drive paths and UNC paths as
absolute-like values so Git Bash and WSL users can paste accessible Windows
paths without peval-py joining them to the current working directory.

## Normative Detail Files

The peval-py behavior contract is split by responsibility to keep each source
file reviewable. The files linked in Attachments are normative parts of this
spec and share this document's scope and out-of-scope boundaries. `view
trajectory` inspect selector syntax and content-bound behavior are owned by
[Inputs and Adapters](inputs.md), as is Trial cell path tolerance and input
precedence for `view/export trajectory`; deterministic coverage is defined by
[Testing](testing.md).

## Redaction

Reports redact obvious secret-bearing keys, authorization headers, bearer
tokens, and common provider key patterns by default. Raw report mode
`--no-redact` disables redaction explicitly. Redaction applies before writing
JSON and before embedding report data in HTML.

## Related Topics

- [300 peval CLI](../300-peval-cli/spec.md)
- [300 Reporting](../300-peval-cli/reporting.md)
- [340 Agent Evaluation](../340-agent-evaluation/spec.md)
- [340 Trajectory](../340-agent-evaluation/trajectory.md)
