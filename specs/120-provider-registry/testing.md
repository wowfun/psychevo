---
name: 120. Provider Registry Testing
psychevo_self_edit: deny
---

Define validation expectations for the first live-provider registry and
`pevo run` closure.

## Default Validation

Automation vocabulary and generic validation boundaries follow
[060 Automation](../060-automation/spec.md).

The default validation gate is:

```bash
scripts/validate.sh broad
```

For this topic, the default gate is the broad deterministic validation path.

## Deterministic Acceptance

- The OpenAI Chat-compatible adapter handles split UTF-8, split SSE frames,
  CRLF, LF, bare CR, BOM, comments, multi-line `data:`, `[DONE]`, stream error
  objects, `tool_calls: null`, split tool-call id/name/arguments, and usage
  chunks.
- Config tests cover explicit `PSYCHEVO_CONFIG`, global config, project override,
  recursive object merge, scalar replacement, `.env` precedence, invalid JSONC,
  invalid provider entries, and raw API-key rejection.
- Provider resolution tests cover aliases, unknown providers, auto order,
  missing credentials, missing model, multiple configured models, single
  configured model default, provider-qualified model strings, model object
  provider selection, reasoning-effort resolution, base-url env override, and
  Xiaomi's `XIAOMI_API_KEY`.
- Runtime tests cover live session metadata, persisted messages, context
  pruning, text-only mock provider completion, and mock provider tool-call
  completion.
- CLI tests cover final-answer stdout, `--format json` NDJSON, stdin prompt,
  empty prompt rejection, nonzero failures, and SQLite persistence.

## Live Opt-In Validation

Live provider tests are ignored by default. They are run only as live opt-in
validation, for example:

```bash
cargo test --workspace --all-targets -- --ignored live
```

The first live suite covers DeepSeek and Xiaomi. It must require `PSYCHEVO_HOME`
or explicit `PSYCHEVO_CONFIG` to point at isolated Psychevo configuration. The
tests must not read OpenCode auth files or any other external auth store.

Each live provider test creates an isolated temporary workdir fixture, asks the
model to use the `read` tool, and asserts:

- normal completion
- at least one successful `read` tool-result message
- durable session/message persistence

Live test failures do not block this topic's default validation path unless a
caller explicitly asks to validate live providers.

## Validation Boundaries

- Tests should compare behavior and stable JSON event categories, not full
  provider wire payloads.
- Mock SSE tests should use local deterministic servers.
- `.env` and process environment changes should stay isolated and be cleaned up.
- Snapshots or golden files should not include volatile provider catalogs,
  generated prose, or real provider responses.

## Related Topics

- [120 Provider Registry](spec.md) defines the provider/config contract.
- [120 Implementation Plan](plan.md) defines the implementation phases.
- [200 pevo run](../200-pevo-cli/pevo-run.md) defines CLI output modes.
