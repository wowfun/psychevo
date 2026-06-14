---
name: 200. pevo init Attachment
psychevo_self_edit: deny
---

Define `pevo init`, the global Psychevo home initializer.

This attachment is part of [200 pevo CLI](spec.md).

## Scope

- global Psychevo home creation
- starter provider config
- `.env` credential template
- first SQLite state initialization
- idempotent non-overwrite behavior

Out of scope:

- provider login, OAuth, auth stores, credential validation, or live probes
- project-local initialization
- plugin, skill, cache, or log content initialization

## Command Contract

`pevo init` accepts optional `--reset-state`.

It resolves `PSYCHEVO_HOME`, or `~/.psychevo` when unset. `~` expands to the
user's home directory, and relative paths resolve relative to process cwd.

The command creates:

- `config.toml`
- `.env`
- `state.db`
- `sessions/`
- `logs/`
- `cache/`

Existing `config.toml` and `.env` are not overwritten. Missing files or
directories are created.

`state.db` is initialized by opening it through the default SQLite store. The
command does not write session sidecar files.

When `--reset-state` is supplied, existing `state.db`, `state.db-wal`, and
`state.db-shm` files are moved into a timestamped backup directory under
`backups/` before a fresh state database is created. The command still must not
overwrite existing `config.toml` or `.env` files.

## Starter Config

The generated `config.toml` is DeepSeek-only:

```toml
model = "deepseek/deepseek-chat"

[provider.deepseek.options]
base_url = "https://api.deepseek.com/v1"
api_key_env = "DEEPSEEK_API_KEY"

[provider.deepseek.models.deepseek-chat]
reasoning_effort = "medium"
```

The generated `.env` is a comment-only template and must not contain raw API
keys.

## Output

On success, stdout prints a short path summary containing the resolved home,
config, env, state, sessions, logs, and cache paths. The output must not print
credential values.

## Related Topics

- [200 pevo CLI](spec.md) defines the product CLI surface.
- [120 Provider Registry](../120-provider-registry/spec.md) defines the config
  schema consumed by `pevo run`.
- [031 SQLite Persistence](../031-storage-and-persistence/sqlite-persistence.md)
  defines the SQLite persistence shape.
