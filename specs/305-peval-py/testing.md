---
name: 305. peval-py Testing
psychevo_self_edit: deny
---

# 305. peval-py Testing

Define deterministic validation for the `peval-py` Python CLI.

## Scope

- Python package unit tests
- fixture-backed `peval-py` smoke tests
- JSONL and SQLite input behavior
- ATIF and peval-compatible report shape
- session-comparison view behavior and CLI notes

Out of scope:

- live providers, ACP servers, official benchmark harnesses, Docker, or network
  services
- default Rust workspace validation changes

## Deterministic Coverage

Tests use only temporary directories, local fixtures, Python standard-library
SQLite, and checked-in JSONL fixture files.

Coverage must verify:

- Psychevo SQLite `messages` extraction orders rows by `session_seq`.
- SQLite `--session-id` is preserved as `trajectory.session_id`.
- Psychevo JSONL conversion preserves user, assistant, reasoning, tool-call,
  and tool-result material.
- matched tool-result observations are nested under the Agent step that issued
  the tool call.
- matched tool-call failures in the middle of a session remain nested under the
  issuing Agent step, mark tool meta as failed, count tool errors, and do not
  prevent later successful tool calls from being represented.
- unmatched tool results remain visible and produce a warning.
- tool execution duration prefers `metadata.elapsed_ms` and falls back to
  timestamp differences when metadata is absent.
- common JSONL conversion works through the OpenCode and Hermes adapters.
- malformed JSONL lines fail with a clear line-number diagnostic.
- ATIF step ids are sequential and tool observations link to source tool calls.
- final metrics aggregate available usage, accounting, turn, tool-call, and
  tool-error facts.
- report JSON contains the v17 subset top-level fields.
- repeated `-p` inputs in `view trajectory` create one trajectory per input
  and include a session-oriented comparison summary.
- `-d` with repeated `-s` reads each SQLite session independently.
- JSONL and DB input families cannot be mixed.
- `export trajectory` rejects multiple sessions clearly.
- JSONL view inputs without an embedded session id use the file stem as the
  displayed session id.
- `-n/--note 0=TEXT` creates report-level notes, `-n/--note N=TEXT` attaches
  to the one-based session index, repeated notes preserve CLI order, and
  out-of-range indexes fail clearly.
- comparison heatmap and table rows do not contain benchmark, task, task-set,
  task-family, or matrix task-axis fields.
- HTML escapes text, safely embeds JSON, exposes one step visibility toggle,
  and renders peval-style tool names, tool execution timing, and observations
  inside the corresponding Agent step.
- HTML does not render the old Summary, Session Heatmap, or Session Table
  labels in multi-session reports.
- HTML renders Report Notes, Visible Heatmap, metric controls, Leaderboard,
  selected Trial details, selected-state cues, note snippets, selected Trial
  notes, and safe note Markdown for multi-session reports.
- Visible Heatmap renders one session/trial row per input with a left-side
  session axis label and one heatmap cell; it does not create an unbounded
  horizontal column for every session.
- Visible Heatmap metric buttons switch displayed values for duration, tokens,
  tool calls, and turns, and heatmap/Leaderboard clicks update the selected
  Trial panel.
- HTML renders the peval-style Run, Result, Evidence, and Usage Breakdown
  sections for single-session reports.
- failed tool-call chips use the shared failure styling without applying that
  styling to later successful tools.
- step duration covers matched observations and is not computed as the
  previous-step gap.
- CLI smoke commands cover `view trajectory`, `export trajectory`, the `tr`
  scenario alias, and short flags including `-p`, `-a`, `-n`, and `-o`.
- legacy top-level `report` and `convert` commands are rejected.

## Validation

The primary validation command is:

```sh
UV_PROJECT_ENVIRONMENT=../../.local/peval-py-venv uv run --project tools/peval-py python -m unittest discover -s tools/peval-py/tests
```

Smoke validation should also run representative CLI commands against fixtures
and inspect generated JSON with:

```sh
python -m json.tool <output.json>
```

The repository broad validation script remains Rust-only for this feature. Do
not add Python package execution to `scripts/validate.sh broad`.

## Related Topics

- [305 peval-py](spec.md)
