---
name: 120. Provider Registry Testing
psychevo_self_edit: deny
---

Define validation expectations for the first live-provider registry and
`pevo run` closure.

## Default Validation

CI/CD vocabulary and generic validation boundaries follow
[065 CI/CD](../065-ci-cd/spec.md).

The default validation gate is:

```bash
cargo xtask ci run --profile rust-broad
```

For this topic, the default gate is the broad deterministic validation path.

## Deterministic Acceptance

- The OpenAI Chat-compatible adapter handles split UTF-8, split SSE frames,
  CRLF, LF, bare CR, BOM, comments, multi-line `data:`, `[DONE]`, stream error
  objects, `tool_calls: null`, split tool-call id/name/arguments, and usage
  chunks.
- Config tests cover explicit `PSYCHEVO_CONFIG`, global config, project override,
  recursive object merge, scalar replacement, `.env` precedence, invalid TOML,
  invalid provider entries, and raw API-key rejection.
- Provider resolution tests cover aliases, unknown providers, auto order,
  missing credentials, missing model, multiple configured models, single
  configured model default, provider-qualified model strings, model object
  provider selection, reasoning-effort resolution, base-url env override,
  metadata precedence, `models.dev` cache lookup, base-url inference for
  user-defined providers, config metadata overrides, and Xiaomi's
  `XIAOMI_API_KEY`.
- Model catalog tests cover explicit provider `/models` fetch parsing,
  provider-style `pricing` aliases, persistent picker cache write/read,
  credential-fingerprint invalidation, raw-payload stripping, empty-result
  non-overwrite behavior, GUI/CLI/TUI cache hydration, `models.dev` cache
  enrichment, and official snapshot fallback without requiring live provider
  credentials.
- Cost-accounting tests cover billable input/output subtraction, cache read and
  write tokens, reasoning-as-output pricing, unknown pricing for missing
  nonzero-required bucket prices, known free pricing from explicit zero prices,
  aggregate status separation, and `context_over_200k` tier selection.
- Runtime tests cover live session metadata, persisted messages, context
  pruning, text-only mock provider completion, and mock provider tool-call
  completion.
- CLI tests cover final-answer stdout, `--format json` NDJSON, stdin prompt,
  empty prompt rejection, nonzero failures, and SQLite persistence.

## Live Opt-In Validation

Live provider tests are selected through the xtask-owned live registry. They are
run only as live opt-in validation, for example:

```bash
cargo xtask live run
cargo xtask live run --suite provider
```

The default live suite covers one provider: `xiaomi-token-plan` as the primary
Xiaomi-family provider. The runner must point `PSYCHEVO_HOME`,
`PSYCHEVO_CONFIG`, and `PSYCHEVO_DB` at isolated repo-local paths for each
check. The tests must not read third-party auth files or any other external auth
store. Live harnesses should pass a provider-qualified model for the provider
under test so a provider-qualified default model in the isolated home does not
mask the requested provider. Additional providers may be validated only when
explicitly selected with `cargo xtask live run --provider <id>`.

Live provider registry tests may use the repo-local development home defined by
[065 CI/CD](../065-ci-cd/spec.md), including
`.local/.psychevo-dev/config.toml` and `.local/.psychevo-dev/.env`. This is
still live opt-in validation and must not read the user's normal home.

Each live provider test creates an isolated temporary cwd fixture, asks the
model to use the `read` tool, and asserts:

- normal completion
- at least one successful `read` tool-result message
- durable session/message persistence

Explicit live catalog validation may fetch a configured provider `/models`
endpoint such as `xiaomi-token-plan`. It must write any provider picker cache to
an isolated temporary home, assert the cache is populated, and assert API-key
values are not present in the cache file.

TUI catalog validation must prove that a cache written by GUI or CLI fetch is
visible after constructing a fresh TUI state with empty in-process catalog
state, and that TUI fetch writes the same persistent cache file.

Live test failures do not block this topic's default validation path unless a
caller explicitly asks to validate live providers.

## Validation Boundaries

- Tests should compare behavior and stable JSON event categories, not full
  provider wire payloads.
- Mock SSE tests should use local deterministic servers.
- `.env` and process environment changes should stay isolated and be cleaned up.
- Snapshots or golden files should not include volatile provider catalogs,
  generated prose, or real provider responses.
- `pevo setup` tests should drive the wizard core with fake terminal input and
  local fake `/models` providers instead of PTYs, real API keys, or live
  provider endpoints.

## Related Topics

- [120 Provider Registry](spec.md) defines the provider/config contract.
- [200 pevo run](../200-pevo-cli/pevo-run.md) defines CLI output modes.
