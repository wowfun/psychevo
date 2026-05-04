---
name: 120. Provider Registry Implementation Plan
psychevo_self_edit: deny
---

Plan the first live-provider closure for Chat-compatible `pevo run`.

## Phase 1: Specification and Adapter Grounding

- Keep `pevo smoke` as the deterministic acceptance entrypoint.
- Harden the OpenAI Chat-compatible stream adapter before wiring live runtime
  entrypoints.
- Parse SSE from bytes so chunk boundaries may split UTF-8, lines, frames, and
  JSON payloads.
- Normalize Chat text, usage, metadata, and function tool-call deltas into the
  existing provider-neutral stream categories.

## Phase 2: Registry and Configuration

- Implement the static built-in registry from [120 Provider Registry](spec.md)
  in `psychevo-runtime`.
- Load JSONC config from explicit `PSYCHEVO_CONFIG` or from global plus project
  discovery.
- Merge global and project config by recursive object merge with scalar,
  array, and null replacement.
- Load Psychevo home/config-parent and project `.env` files into an invocation-local environment
  map without mutating process environment.
- Resolve provider, model, base URL, and API key before `agent_start`.
- Parse provider-qualified model strings and model objects before provider
  selection so same-name models can be disambiguated.
- Resolve optional model `reasoning_effort` and pass it into generation
  metadata for Chat-compatible requests.

## Phase 3: Runtime Live Run

- Add runtime `RunOptions` and `RunResult` for live coding-agent invocations.
- Parameterize SQLite session creation so smoke and live runs store their own
  source, provider, and model metadata.
- Assemble the same `coding-core` tools used by smoke for live runs.
- Use `OpenAiChatProvider` for every first-slice built-in and configured
  provider.
- Reject invalid config, unknown provider, missing credentials, or unresolved
  model before `agent_start`.

## Phase 4: CLI and Observation

- Add OpenCode-style `pevo run [message..]` with `--dir`, `-m/--model`,
  `--variant`, `-s/--session`, `-c/--continue`, and `--format`.
- Append non-TTY stdin to positional prompt input and reject empty prompt text
  before `agent_start`.
- Default stdout is final assistant text only.
- `--format json` writes NDJSON observation events, beginning with `run_start` and
  ending with `agent_end`.

## Phase 5: Validation

- Keep `scripts/validate.sh broad` as the default deterministic gate.
- Add mock HTTP SSE tests for text, tool-call, JSON output, and failure paths.
- Add ignored live DeepSeek and Xiaomi tests that read Psychevo config and
  `.env` through `PSYCHEVO_HOME` or explicit `PSYCHEVO_CONFIG`.

## Related Topics

- [120 Provider Registry](spec.md) defines the configuration and resolution
  contract.
- [120 Testing](testing.md) defines the acceptance matrix.
- [200 pevo run](../200-pevo-cli/pevo-run.md) defines the CLI behavior.
- [003 OpenAI Chat Stream](../003-ai-protocol/openai-chat-stream.md) defines
  the stream adapter contract.
