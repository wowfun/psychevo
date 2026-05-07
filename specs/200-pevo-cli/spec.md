---
name: 200. pevo CLI
psychevo_self_edit: deny
---

Define the concrete `pevo` command-line product surface.

This product surface builds on [025 CLI](../025-cli/spec.md) and routes agent
work through `psychevo-runtime`. It owns command spelling, user-facing process
behavior, and product-level environment variables.

## Scope

- `pevo` command families
- global Psychevo home layout
- `pevo init`
- `pevo run`
- `pevo smoke` product positioning
- `pevo tui` product positioning
- reserved future command family boundaries

Out of scope:

- approvals, file attachments, fork/share/server attach,
  provider login, or auth stores
- provider transport semantics, provider catalogs, OAuth, or credential pools
- SQLite schema details beyond product path selection
- SDK, HTTP, or MCP transports

## Psychevo Home

`PSYCHEVO_HOME` is the single global directory concept for the `pevo` product
CLI. When unset, it defaults to `~/.psychevo`. `~` expands to the user's home
directory, and relative `PSYCHEVO_HOME` values resolve relative to the process
cwd.

The initialized home tree contains:

- `config.jsonc`
- `.env`
- `state.db`
- `sessions/`
- `logs/`
- `cache/`

`state.db` is the only first-slice session/message store. The reserved
`sessions/` directory is not used for JSON or JSONL transcript sidecars in this
slice.

`PSYCHEVO_DB` may point at `:memory:` or a SQLite path. `~` expands, and
relative paths resolve relative to the process cwd. When unset, `pevo run` uses
`$PSYCHEVO_HOME/state.db`.

`PSYCHEVO_CONFIG` may point at one JSONC config file. When set, it replaces
home and project config discovery for provider configuration.

## Command Families

Implemented first-slice commands:

- `pevo init`
- `pevo run`
- `pevo smoke`
- `pevo tui`

Reserved command families:

- `pevo models`
- `pevo session`
- `pevo auth`

Reserved command families do not define accepted arguments or behavior in this
slice.

`pevo smoke` is a deterministic development and validation harness. It keeps
its explicit fake-provider flags and is not redesigned as a live-provider
product entrypoint in this topic.

`pevo tui` owns interactive terminal projection. It accepts `--debug` for
TUI-local debug projections such as usage parts and allowlisted provider
metadata summaries. Debug projection does not change `pevo run --format json`,
does not expose folded reasoning in sanitized transcript messages, and does not
turn provider metadata into transcript content.

## Related Topics

- [025 CLI](../025-cli/spec.md) defines command-line foundation semantics.
- [200 pevo init](pevo-init.md) defines global home initialization.
- [200 pevo run](pevo-run.md) defines the live coding-agent command.
- [210 pevo TUI](../210-pevo-tui/spec.md) defines the fullscreen interactive
  terminal command.
- [200 Implementation Plan](plan.md) defines this slice's implementation order.
- [200 Testing](testing.md) defines acceptance coverage.
- [120 Provider Registry](../120-provider-registry/spec.md) defines
  provider/model configuration and resolution.
