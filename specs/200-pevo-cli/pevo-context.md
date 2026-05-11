---
name: 200. pevo context Attachment
psychevo_self_edit: deny
---

Define local context-window usage inspection for `pevo context`.

This attachment is part of [200 pevo CLI](spec.md).

## Command Contract

`pevo context` accepts:

- required `--session <id|latest>`
- optional `--dir <path>` for resolving `--session latest`
- optional `--json`

Bare `pevo context` is a usage error. The literal `latest` resolves the latest
active `run` or `tui` session for the canonical current workdir, or for
`--dir` when provided. Exact session ids may refer to active or archived
sessions and use the session's persisted workdir.

The command requires initialized `PSYCHEVO_HOME`, reads the SQLite state
database selected by `PSYCHEVO_DB` or `$PSYCHEVO_HOME/state.db`, and may read
local configuration for metadata fallback. It must not contact providers,
refresh model catalogs, or perform what-if model, mode, or skill overrides.

## Output

Default output is a compact text block showing headline token usage, known
categories, projection scope, model, and at most three bounded advice rows.
Script-friendly output does not include a colored context bar. Human text hides
provider-reported source labels; only estimated headline totals append
`estimated`.

Text token counts use integer `tokens` below 1k, one decimal place for
`k`/`M` compact values, and compact headline ratios such as
`tokens: 34.0k/1.0M (3.2%)`. The projection scope and model are shown at the
bottom, separated from `free_space` by one blank line when free space is shown.
Skill index details list every skill entry with estimated token counts in
descending token order.
Human text renders the model-facing `messages` category as `input_messages`,
and role count rows use `input msg` or `input msgs`; JSON retains the
structured `categories.messages` key.

`--json` writes one `context_snapshot` JSON object. Runtime/configuration
errors use:

```json
{"type":"error","message":"..."}
```

The snapshot includes structured category objects keyed by snake_case category
name, tokenizer metadata, selected skill names when known, and structured
advice. It must not include full prompt text, message text, skill body text, or
tool schema bodies.

## Related Topics

- [200 pevo CLI](spec.md) defines the product CLI surface.
- [200 pevo run](pevo-run.md) defines live run JSON context snapshot events.
- [006 Context Assembly](../006-context-assembly/spec.md) defines runtime-owned
  context usage projection boundaries.
