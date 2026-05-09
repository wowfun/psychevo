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
- JSONC configuration loading
- global and project configuration merge for live runs
- `.env` credential loading
- provider/model resolution for `pevo run`

Out of scope:

- remote model catalogs, live model probes, provider fallback execution, or
  dynamic routing
- OAuth, browser login, device code flows, auth stores, or credential refresh
- billing, rate-limit accounting, cost catalogs, or pricing
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

`pevo run` reads JSONC configuration from:

- explicit `PSYCHEVO_CONFIG`
- global `$PSYCHEVO_HOME/config.jsonc`
- global `~/.psychevo/config.jsonc` when `PSYCHEVO_HOME` is unset
- project `<workdir>/.psychevo/config.jsonc`

When `PSYCHEVO_CONFIG` is supplied, it replaces config discovery and only that
file is loaded as configuration. CLI model and variant overrides still have
highest resolution precedence.

When discovery is used, project config overrides global config by deep object
merge. Objects merge recursively. Scalars, arrays, and null replace the lower
layer value.

`PSYCHEVO_CONFIG_DIR` is not read, aliased, or used as fallback.

The configuration format is JSONC: JSON with comments and trailing commas.
Unknown fields are ignored. Invalid syntax, invalid field types, or invalid
provider entries reject before `agent_start`.

Configuration may define:

- top-level `model` as a default model string or model object
- top-level `provider` map keyed by provider id or user provider name
- optional per-provider `label` for display in human-facing selectors and
  status surfaces
- per-provider `options.base_url`
- per-provider `options.api_key_env`
- per-provider `models` map keyed by configured model id
- optional model `reasoning_effort` as the first-slice model thinking
  intensity hint. Valid values are `none`, `minimal`, `low`, `medium`, `high`,
  `xhigh`, and `max`; `none` disables the request field.

Example:

```jsonc
{
  "model": "deepseek/deepseek-chat",
  "provider": {
    "deepseek": {
      "options": {
        "base_url": "https://api.deepseek.com/v1",
        "api_key_env": "DEEPSEEK_API_KEY"
      },
      "models": {
        "deepseek-chat": {
          "reasoning_effort": "medium"
        }
      }
    }
  }
}
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

Configuration must not contain raw API keys. Credentials are resolved from the
local environment map through `api_key_env` or built-in credential environment
variable candidates.

Provider labels are display-only. They do not change provider identity,
selection, config merge keys, or the `provider/model` model-spec form.

Interactive clients may create user-defined OpenAI Chat-compatible providers in
the global config. The created provider id must be a new normalized user
provider id and must not collide with built-in provider ids or aliases. The
generated credential variable is stored in `options.api_key_env`; raw API keys
must be written only to `.env` files, never JSONC configuration.

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
2. JSONC configuration
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

`qwen` is a built-in alias for a Chat-compatible endpoint in this slice.
Browser-based portal OAuth is explicitly deferred.

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
- [120 Testing](testing.md) defines deterministic and live opt-in validation.
