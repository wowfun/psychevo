---
name: 120. Provider Registry
psychevo_self_edit: deny
---

Define the first live-provider registry and configuration contract.

This topic owns runtime provider/model configuration and resolution policy for
the implementation slice. Concrete `pevo` CLI spelling belongs to
[200 pevo CLI](../200-pevo-cli/spec.md). This topic does not redefine the
provider-neutral AI protocol in [003 AI Protocol](../003-ai-protocol/spec.md).

## Scope

- static provider registry entries
- user-defined Chat-compatible provider entries
- TOML configuration loading
- global and project configuration merge for live runs
- `.env` credential loading
- provider/model resolution for `pevo run`
- public model metadata resolution for context limits, capabilities, and cost

Out of scope:

- provider `/models` catalog fetching outside explicit user-triggered catalog
  fetch flows, provider fallback execution, or dynamic routing
- OAuth, browser login, device code flows, auth stores, or credential refresh
- rate-limit accounting or provider-side billing reconciliation
- provider-native APIs outside the Chat-compatible family, hosted agent-product
  transports, external portal transports, or tool-protocol provider transports
- external auth file reading, credential pools, or setup commands

## Registry Contract

The first live-provider registry is internal and static. Every built-in entry
uses the OpenAI Chat Completions-compatible transport.

The built-in provider ids and aliases are:

- `openrouter`
- `openai`
- `xai`
- `zai`, with aliases `z.ai`, `z-ai`, `glm`
- `deepseek`
- `dashscope`, with aliases `alibaba`, `qwen`
- `xiaomi`, with alias `mimo`
- `lmstudio`
- `custom`

Provider entries define:

- canonical provider id
- display label
- default base URL
- accepted alias names
- credential environment-variable candidates
- optional base-url environment-variable candidate
- whether a no-auth local placeholder is allowed

Unknown built-in provider ids are rejected before `agent_start`. User-defined
providers may be configured by name and must use the same Chat-compatible
transport.

## Configuration

`pevo run` reads TOML configuration from:

- explicit `PSYCHEVO_CONFIG`
- global `$PSYCHEVO_HOME/config.toml`
- global `~/.psychevo/config.toml` when `PSYCHEVO_HOME` is unset
- project `<workdir>/.psychevo/config.toml`

When `PSYCHEVO_CONFIG` is supplied, it replaces config discovery and only that
file is loaded as configuration. CLI model and variant overrides still have
highest resolution precedence.

When discovery is used, project config overrides global config by deep object
merge. Objects merge recursively. Scalars, arrays, and null replace the lower
layer value.

`PSYCHEVO_CONFIG_DIR` is not read, aliased, or used as fallback.

The configuration format is TOML. Unknown fields are ignored. Invalid syntax,
invalid field types, or invalid provider entries reject before `agent_start`.
`config.jsonc` is not a configuration input and is ignored if present; runtime
does not read it, parse it, or migrate it.

Configuration may define:

- top-level `model` as a default model string or model object
- top-level `provider` map keyed by provider id or user provider name
- optional per-provider `label` for display in human-facing selectors and
  status surfaces
- per-provider `options.base_url`
- per-provider `options.api_key_env`
- per-provider `options.no_auth`
- per-provider `models` map keyed by configured model id
- optional model `reasoning_effort` as the first-slice model thinking
  intensity hint. Valid values are `none`, `minimal`, `low`, `medium`, `high`,
  `xhigh`, and `max`; `none` disables the request field.
- optional model `limit` object with `context`, `input`, and `output` token
  limits
- optional model `cost` object with USD-per-million-token `input`, `output`,
  `cache_read`, `cache_write`, and optional `context_over_200k` tier
- optional model capability overrides using `reasoning`, `tool_call`,
  `temperature`, `attachment`, `structured_output`, `interleaved`, and
  `modalities.input`/`modalities.output`
- optional top-level `compression` object for context compaction defaults:
  `enabled`, `auto`, `threshold_percent`, `reserve_tokens`,
  `keep_recent_tokens`, optional summary `model`, and optional
  `reasoning_effort`

The legacy model `context_limit` field is rejected. Configurations must use
`limit.context` for context-window token limits.

Example:

```toml
model = "deepseek/deepseek-chat"

[provider.deepseek.options]
base_url = "https://api.deepseek.com/v1"
api_key_env = "DEEPSEEK_API_KEY"

[provider.deepseek.models.deepseek-chat]
reasoning_effort = "medium"
```

When `model` is a string in `provider/model` form and the prefix matches a
built-in provider id, provider alias, or configured provider key, runtime uses
that prefix as the selected provider and the remainder as the model id. This
allows multiple providers to expose the same model id without ambiguity. Model
ids containing `/` remain valid when the first path segment is not a known or
configured provider.

When `model` is an object, it may define `id`, optional `provider`, and
optional `reasoning_effort`. The object form is equivalent to the string form
plus explicit model-level options.

When `compression.model` is present, it uses the same provider/model parsing
rules as top-level `model`. When it is absent, runtime may use the current
invocation model for context compaction summaries. `compression.reasoning_effort`
uses the same valid values as model `reasoning_effort`; `none` disables the
request field for summary generation.

Configuration must not contain raw API keys. Credentials are resolved from the
local environment map through `api_key_env` or built-in credential environment
variable candidates.

`options.no_auth = true` explicitly marks a provider as requiring no bearer
credential. It conflicts with `options.api_key_env`. It is accepted for
user-defined providers and for built-in providers whose registry entry permits
no-auth use. Explicit no-auth may target any base URL, including non-loopback
URLs, but setup and config diagnostics should warn on non-loopback no-auth
because requests will be sent without an Authorization header.

Provider labels are display-only. They do not change provider identity,
selection, config merge keys, or the `provider/model` model-spec form.

Interactive clients and CLI config commands may create user-defined OpenAI
Chat-compatible providers in the global config or the current workdir's local
`.psychevo/config.toml`. The created provider id must be a new normalized user
provider id and must not collide with built-in provider ids or aliases. The
credential variable is stored in `options.api_key_env`; raw API keys must be
written only to `.env` files, never TOML configuration. CLI provider/auth
writes default to the current workdir local scope and use `-g`/`--global` for
the global scope.

Shared setup flows may initialize a minimal config file before writing provider
settings. When `PSYCHEVO_CONFIG` points at one file, setup writes provider and
model configuration to that file. Local credential writes use the config file's
parent directory; global credential writes use `$PSYCHEVO_HOME`. Scope flags do
not override the `PSYCHEVO_CONFIG` target for configuration and should emit a
warning when the requested scope cannot affect the config file path.

Interactive clients and CLI model commands may explicitly set the scoped
default model by writing the top-level `model` field in the selected scope's
`config.toml`. CLI `model set` writes `model = "provider/model"`. Interactive
model pickers may write the equivalent object form when persisting a selected
`reasoning_effort`: `model = { id = "provider/model", reasoning_effort =
"high" }`. This write defaults to the current workdir `.psychevo/config.toml`
and uses `-g`/`--global` for `$PSYCHEVO_HOME/config.toml`. It must require
provider-qualified `provider/model` input, must validate that the provider is
built-in or present in the selected scope's effective provider set, must
validate any persisted `reasoning_effort`, must not contact provider `/models`
endpoints, must not write API keys or model metadata, and must preserve
unrelated configuration values.

## Environment

`pevo run` builds an invocation-local environment map from inherited process
environment and Psychevo `.env` files.

When discovery is used, runtime loads:

- `$PSYCHEVO_HOME/.env` or `~/.psychevo/.env`
- project `<workdir>/.psychevo/.env`

When `PSYCHEVO_CONFIG` is supplied, runtime loads `.env` from the config file's
parent directory when present, then project `<workdir>/.psychevo/.env`.

Later `.env` files override earlier `.env` files and inherited process
environment values. Runtime must not write these values into global process
environment.

The `.env` format is line-oriented `NAME=VALUE`. Blank lines and `#` comments
are ignored. Quotes around values are optional and stripped. Invalid variable
names are ignored.

## Resolution

Provider resolution precedence is:

1. CLI model and variant overrides
2. TOML configuration
3. loaded `.env` and inherited environment

The environment override variables are:

- `PSYCHEVO_INFERENCE_PROVIDER`
- `PSYCHEVO_INFERENCE_MODEL`

When provider is `auto`, runtime selects the first provider with usable
credentials in this order:

1. `openrouter`
2. `openai`
3. `xai`
4. `zai`
5. `deepseek`
6. `dashscope`
7. `xiaomi`
8. `lmstudio`
9. `custom`

The selected provider must have a model from CLI, configuration, or
`PSYCHEVO_INFERENCE_MODEL`. CLI and env model values may use the
`provider/model` form; the `pevo run` CLI requires this form for `-m/--model`.
The first implementation does not hardcode a latest or default model.

When a provider is selected and no explicit model is available, a configured
provider entry with exactly one `models` key supplies that model. If a provider
entry has multiple configured models, runtime rejects before `agent_start`
unless the model is explicit.

`base_url` is resolved from provider config, provider-specific base-url
environment variable, or built-in default. `api_key_env` is resolved from
provider config or built-in credential environment candidates. `api_key` is
resolved only from the invocation-local environment map.

`reasoning_effort` is resolved from CLI variant override, the selected model
object, or the selected provider entry's `models.<model>.reasoning_effort`.
Runtime passes enabled values as a generation metadata hint to the
Chat-compatible adapter. `none` suppresses the request field. Providers that do
not support an enabled request field may reject the live invocation.

Model metadata is resolved as advisory local metadata for status surfaces,
request shaping, cost estimation, and future context-budget decisions. Runtime
keeps a typed core view plus the raw public registry model JSON for future
fields. Resolution order is:

1. built-in metadata fallback table for known model families
2. cache-first `models.dev` public registry lookup
3. metadata present in an explicitly fetched provider `/models` response
4. explicit per-model metadata from TOML configuration
5. unknown as `None`

The `models.dev` cache is stored under `$PSYCHEVO_HOME/models_dev_cache.json`.
It is a pruned cache of user-relevant models, not a full public registry mirror.
Provider resolution and `pevo run` never fetch `models.dev`; they only read the
existing local cache. TUI may start one non-blocking cache warmup when the cache
file is absent, and `/model` provides an explicit metadata refresh action. Both
paths are best effort, use a bounded timeout, fetch the public registry as a
source, and write only models from the current intended model selection, TUI
recent models, and locally configured model entries. They must not fail provider
resolution or startup. When refresh fails, runtime keeps the old cache if one
exists. Built-in fallbacks may provide locally known typed metadata, but remain
advisory and overridable by `models.dev` or explicit config. When no cache or
fallback matches, the context limit and other metadata remain unknown.

Provider matching first uses known provider-id mappings such as `deepseek`,
`xiaomi`, and `xiaomi-token-plan-cn`. If the configured provider id differs
from `models.dev`, runtime may infer the registry provider by matching the
configured base URL to a registry provider `api` URL. This keeps user-defined
provider ids such as `xiaomi-token-plan` stable while still resolving
`xiaomi-token-plan-cn` metadata.

Unknown capabilities are permissive. Only an explicit `false` capability may
degrade a request projection. For example, runtime suppresses
`reasoning_effort` when resolved model metadata says `reasoning = false`, and
may suppress unsupported tool or attachment request fields rather than failing
startup. Unknown or absent capability data must not block a run.

Cost values are interpreted as USD per one million tokens. Estimated local
cost uses normalized provider usage, subtracts cache read/write tokens from
billable input, subtracts reasoning tokens from billable output, charges
reasoning tokens at the output rate, and applies `context_over_200k` when
billable input plus cache read exceeds 200,000 tokens. Missing pricing remains
unknown; known zero pricing is a valid free model cost.

`qwen` is a built-in alias for a Chat-compatible endpoint in this slice.
Browser-based portal OAuth is explicitly deferred.

## Attachments

- [Testing](testing.md) defines deterministic and live opt-in validation.

## Related Topics

- [003 AI Protocol](../003-ai-protocol/spec.md) defines provider-neutral
  generation semantics.
- [003 OpenAI Chat Stream](../003-ai-protocol/openai-chat-stream.md) defines
  the Chat-compatible stream adapter contract.
- [200 pevo CLI](../200-pevo-cli/spec.md) defines the concrete product CLI.
- [200 pevo run](../200-pevo-cli/pevo-run.md) defines live CLI entrypoint
  behavior.
- [100 Runtime Assembly](../100-coding-agent/runtime-assembly.md) defines how
  the built-in coding agent is assembled by runtime.
