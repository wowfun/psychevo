---
name: 200. pevo stats Attachment
psychevo_self_edit: deny
---

Define local usage and estimated-cost reporting for `pevo stats`.

This attachment is part of [200 pevo CLI](spec.md).

## Command Contract

`pevo stats` accepts:

- optional `--dir <path>` to select the workdir scope
- optional `--all` to include every session in the selected state database
- optional `--days <n>` to limit sessions updated in the last `n` days; `0`
  means today
- optional `--limit <n>` to bound top model, tool, and session rows
- optional `--json` for deterministic JSON output

The default scope is the canonical current workdir. The command reads only the
SQLite state database selected by `PSYCHEVO_DB` or `$PSYCHEVO_HOME/state.db`.
It must not contact model providers or refresh public catalogs.

## Report Shape

The report includes:

- total sessions and messages
- total estimated cost when any priced message exists
- token totals for context input, billable input, billable output, reasoning,
  cache read, cache write, and reported total
- provider/model usage grouped by persisted message provider and model
- top tool-result counts by tool name
- top sessions by estimated cost and token count

Unknown pricing is reported separately from known zero-cost messages. Cost is a
local estimate derived from persisted accounting columns and is not a bill.

## Related Topics

- [200 pevo CLI](spec.md) defines the product CLI surface.
- [031 SQLite Persistence](../031-storage-and-persistence/sqlite-persistence.md)
  defines persisted accounting columns.
