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
- Psychevo DB conversion reads current `sessions` and `messages` tables,
  defaults to the most recently updated session when `--session-id` is omitted,
  and supports explicit `--session-id` selection.
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
- built-in adapter registry discovery includes Psychevo, OpenCode, and Hermes.
- installed Python entry points in the `peval_py.adapters` group can register a
  third-party adapter without editing the core adapter list.
- duplicate adapter ids and unknown adapter ids fail with clear diagnostics.
- adapter TOML tables under `[adapters.<adapter-id>]` are passed to the
  effective adapter as raw options, including when CLI `--adapter` overrides
  the configured default.
- `-a ADAPTER` applies one default adapter to all inputs, while `-a pN=ADAPTER`
  and `-a dN=ADAPTER` override individual path and DB inputs.
- invalid adapter selectors, duplicate selectors, out-of-range selectors, and
  unknown effective adapter ids fail with clear diagnostics.
- path adapters can handle `-p/--path` without the default JSONL loader.
- `-p/--path` can read an exported ATIF JSON trajectory object directly without
  reparsing it through a message adapter, and does not require the configured
  default adapter to be installed.
- DB adapters can handle `-d/--db` without the generic SQLite `messages` loader.
- OpenCode DB conversion reads current `session`, `message`, and `part` tables,
  defaults to the most recently updated session when `--session-id` is omitted,
  and supports explicit `--session-id` selection.
- Hermes DB conversion reads current `sessions` and `messages` tables, includes
  stored `sessions.system_prompt` as a system step, defaults to the most
  recently active, ended, or started session when `--session-id` is omitted,
  and supports explicit `--session-id` selection.
- adapters used with `--db` that support neither native DB input nor record
  conversion fail with a clear unsupported-input diagnostic.
- locale config defaults to English, accepts the `en-US`, `zh-CN`, and `zh`
  aliases, and rejects unsupported values with a clear config error.
- malformed JSONL lines fail with a clear line-number diagnostic.
- ATIF step ids are sequential and tool observations link to source tool calls.
- final metrics aggregate available usage, accounting, turn, tool-call, and
  tool-error facts.
- report JSON contains the v17 subset top-level fields.
- repeated `-p` inputs in `view trajectory` create one trajectory per input
  and include a session-oriented comparison summary.
- `-d` with repeated `-s` reads each SQLite session independently through the
  effective adapter.
- repeated `-d` inputs may use different adapters and generate one comparison
  report.
- JSONL path and DB input families can be mixed in `view trajectory`.
- with multiple DB inputs, `-s dN=ID` binds session ids to the one-based DB
  input, while bare `-s ID` fails clearly.
- with one DB input, bare `-s ID` remains compatible.
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
- HTML renderer source CSS and JavaScript live in package asset files and are
  still inlined into the emitted offline HTML report.
- HTML does not render the old Summary, Session Heatmap, or Session Table
  labels in multi-session reports.
- HTML renders Report Notes, Visible Heatmap, metric controls, Leaderboard,
  selected Trial details, selected-state cues, note snippets, selected Trial
  notes, and safe note Markdown for multi-session reports.
- Visible Heatmap and Leaderboard comparison panels render only their primary
  heading, without duplicate eyebrow text; `Leaderboard` remains English in
  localized reports.
- Visible Heatmap renders one session/trial row per input with a left-side
  session axis label and one heatmap cell; it does not create an unbounded
  horizontal column for every session.
- Visible Heatmap metric buttons switch displayed values for duration, tokens,
  tool calls, and turns, and heatmap/Leaderboard clicks update the selected
  Trial panel.
- HTML renders the peval-style Run, Result, Evidence, and Usage Breakdown
  sections for single-session reports.
- HTML report typography keeps the body text baseline at 15px and compact
  labels, chips, table headers, and code blocks at 12px or larger.
- HTML timed chips in the rendered Steps rail can show proportional fill for
  step duration, elapsed time, and tool execution time, while missing or zero
  timing values keep the plain chip style.
- HTML shows visibly marked estimated token chips for steps that lack real
  token metrics, preserves exact token chips when real step metrics exist, can
  use an optional `tiktoken` module, falls back to a deterministic byte-length
  estimate, resolves estimates through the selected Trial key in the rendered
  Steps rail, and does not mutate report JSON data while rendering.
- HTML report title and comparison UI labels remain English by default and
  switch to Simplified Chinese only when the normalized locale is `zh-CN`, while
  the selected Trial Run, Result, Notes, and Evidence sections also localize and
  only the final Steps detail section remains English. Simplified Chinese
  reports preserve selected domain terms in English, including Run, Result,
  Notes, Evidence, Steps/events, Session, variant, evaluator, reasoning,
  selected trial trajectory, Turns, Tool Calls, tool success / total, cache
  read, and cache write.
- failed tool-call chips use the shared failure styling without applying that
  styling to later successful tools.
- step duration covers matched observations and is not computed as the
  previous-step gap.
- CLI smoke commands cover `view trajectory`, `export trajectory`, the `tr`
  scenario alias, localized HTML output from `[defaults].locale = "zh-CN"`, and
  short flags including `-p`, `-a`, `-n`, and `-o`.
- legacy top-level `report` and `convert` commands are rejected.
- translated evaluation docs exist under `docs/i18n/zh-CN/...`, the peval-py
  tool README translation exists beside `tools/peval-py/README.md`, English
  docs link to their Chinese counterparts, Chinese docs link to translated
  pages when available, and spec links still target canonical specs.

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
