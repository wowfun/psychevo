# peval-py Agent Skill

## Skill Contract

The repo-distributed `skills/peval-py` package teaches agents how to use
`peval-py` for retained session inspection, report rendering, ATIF export, DB
session listing, local serving, analysis report creation, and Trial-cell import.
It is an instruction package only: it must not add a new CLI command, execute
agents, run live providers, mutate source databases, or install itself into
`.agents/skills`, `.psychevo/skills`, or a global skill directory. Because the
skill supports multiple agent surfaces, it must not include agent-specific
`agents/openai.yaml` metadata. Skill package structure is validated with the
skill validator, not with the `tools/peval-py` Python package unit tests.

`notes.md` remains a human/serve editing path, not an agent skill workflow. The
skill must not instruct agents to create, import, or validate `notes.md`, and
its helper checks must not include `annotations.notes[]` requirements.

Analysis reports are independent from report rendering and from Trial-cell
import. The skill must not imply that every use requires both `analysis.json` /
`analysis.md`, a generated report, or workspace placement. It should guide
agents to create one JSON or Markdown analysis report by default when analysis
is requested, or create both only when their contents are complementary. A
provided Trial cell path, or a session artifact path that contains exactly one
cell, identifies the analysis target and a possible later import target.
Agents should read that target's `agent/trajectory.json`,
`agent/trajectory_meta.json`, and existing analysis artifacts for direct
evidence when accuracy matters instead of generating a report to rediscover the
same identity. `view tr -p <cell-dir>` and `export tr -p <cell-dir>` are
supported convenience inputs for Trial cell artifact directories that contain
the retained `agent/trajectory.json` and `agent/trajectory_meta.json`; tolerant
cell globs and descendants are canonicalized by the CLI. Session artifact
directories still require choosing the target cell first. The
top-level skill instructions must stay compact: they may include minimal
workspace path recognition, but they should not embed Trial-cell path
derivation rules, compiled artifact field semantics, or the JSON `extra` merge
contract. They should also assume the peval-py workspace has already been
initialized and should not present `peval-py init` as a default top-level
workflow. Those details belong in references that are loaded only when needed.
The top-level skill must not end with a standalone `## References` catalog.
Reference entry points should appear inside the workflow that needs them:
`references/view-tr.md` for detailed `view tr` inspect, listing, saved snapshot,
selector, and raw report examples; `references/cli-workflows.md` for commands
other than `view tr`, such as `init`, `export tr`, `import analysis`, and `serve`;
and `references/analysis-guide.md` for deeper trajectory analysis methodology,
report formats, and Trial-cell import guidance. Reference files should read as
task instructions, not navigation metadata, and should avoid cross-reference-only
redirects or dependency notes that do not change the current action.
The top-level skill guardrails should also say that `view tr` inspect output is
an exploration aid whose counts, timing, token/cost, and error statistics may be
approximate or incomplete because they depend on retained trajectory format and
adapter mapping. When accuracy matters, the skill should guide agents to narrow
with `view tr` first, then read targeted trajectory, metadata, JSONL, or report
evidence directly. If `peval-py` cannot satisfy a user request, the skill should
tell agents to explain whether the gap is in CLI/report behavior, skill guidance,
or both, and ask whether to improve that surface instead of inventing an
unsupported workaround.
When the user wants an existing analysis report attached to peval-py reports or
`serve`, the skill should tell the agent to call
`peval-py import analysis -r <workspace> --run-path <cell-path> -p
<analysis-report>`.

`Cached analysis` is the peval/peval-py report concept for analysis loaded
from a workspace; the skill must not require every analysis report to be cached
or placed under `runs/...`. Trial-cell `analysis.json` is the fixed-format,
machine-readable compiled artifact owned by peval-py import. Imported JSON
analysis reports have standard fields `summary`, `status`, `findings`,
`recommendations`, `limitations`, and `confidence`, and may include additional
non-standard fields for artifact consumers. `peval-py import analysis` writes a
compiled `analysis.json` with the standard input fields, a default
`status = "analyzed"` when status is omitted, and `subject` derived from the
selected `--run-path`. It does not synthesize `metrics` or `commands`.

The importer tolerates non-standard input fields by preserving them under the
compiled `analysis.json.extra` object. If the input contains an `extra` field,
it must be a JSON object; its entries are merged with all other non-standard
top-level fields, and a non-standard top-level field wins when both sources use
the same key. Imported `subject`, `metrics`, and `commands` are preserved only
as `extra.subject`, `extra.metrics`, and `extra.commands`; they never override
the peval-py-owned compiled `subject` and they are not synthesized by the
importer.

When `peval-py import analysis --json` succeeds, the machine-readable result
includes a stable `warnings` array. Warnings are diagnostic only and must not
block import. The importer emits warnings for top-level fields that look like
peval-py analysis/report fields but are not compiled as standard input fields:
`subject`, `metrics`, `commands`, `analysis_status`, and `analysis_metrics`.
It also emits warnings when standard input fields `summary`, `status`,
`findings`, `recommendations`, `limitations`, or `confidence` appear inside
input `extra`, because nested values remain under `extra` and are not compiled
as top-level analysis fields. Each warning object has `code`, `field`,
`location`, `stored_as`, and `message`. Unknown custom fields that do not look
like peval-py analysis fields are silently preserved under `extra`.

Report generation creates one `annotations.analysis[]` entry for every Trial
so deterministic analysis metrics are available even before a cached analysis
artifact exists. `annotations.analysis[].analysis_metrics` is the single
analysis metric container. It has a peval-py-owned `auto` object computed from
the current `trajectory.final_metrics` and `trajectory_meta[]` facts at report
generation time. `analysis_metrics.auto` is a derived analysis projection, not a
second persisted source of truth: it must not repeat direct facts already stored
in `trajectory.final_metrics` or `trajectory_meta[]`. Human/agent metrics read
from cached analysis artifacts remain flat keys in the same `analysis_metrics`
object; imported metrics must not replace or mutate the report-owned `auto`
object.

Report generation reads compiled `analysis.json` into the same Trial analysis
entry and recognizes a typed whitelist of incremental fields: `status` as
`annotations.analysis[].analysis_status`, `subject`, `findings`,
`recommendations`, `limitations`, `commands`, `metrics` as flat keys under
`annotations.analysis[].analysis_metrics`, and `confidence`. Imported compiled
reports that keep input `metrics` under `extra.metrics` are also read as flat
`analysis_metrics` keys. Unknown top-level fields and recognized fields
with incompatible types are ignored so `analysis.json` remains a stable
machine-readable annotation contract rather than an arbitrary JSON passthrough.

`analysis.md` is the free-form analysis artifact readable by humans and
agents. Its format and content are intentionally unconstrained by the skill.
Current peval-py report generation reads this file into
`annotations.analysis[].md_report` and renders it in the selected Trial
Analysis section. When both `analysis.json` and `analysis.md` are written, they
should complement each other instead of duplicating the same analysis.

Reference docs may describe how to discover a missing Trial cell path from a
report, but the common import example should use the already-known
`<cell-path>` directly. The minimum workspace cues the skill may expose are
`<workspace>/peval-py.toml`, a Trial cell under
`runs/<eval>/<agent>/<session>/<cell>/`, and the cell-local
`agent/trajectory.json`, `agent/trajectory_meta.json`, `analysis.json`, and
`analysis.md` files. A cell is the minimum Trial unit: `analysis.json`,
`analysis.md`, and `agent/*` written under that cell all belong to that Trial
for this skill workflow. `analysis.json` and `analysis.md` directly under
`<session-id>/` are session-level artifacts reserved for a later
session-summary surface and are not read into Trial reports in this version.

When the workspace is known, the skill should pass `-r <workspace>` to
`view tr` validation commands so peval-py loads that workspace's config and
cached analysis overlays without requiring a directory change. Running from the
workspace root or a descendant remains valid through current-directory
discovery when `-r/--root` is omitted.
