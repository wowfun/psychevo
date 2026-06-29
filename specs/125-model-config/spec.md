---
name: 125. Model Config
psychevo_self_edit: deny
---

# 125. Model Config

Define the product-level model configuration UX across Workbench, TUI, Gateway,
and shared composer state.

This topic owns the user-facing boundary between saved model defaults, transient
composer selection, provider setup, and explicit model catalog fetches. Runtime
provider identity, TOML schema, credential resolution, provider metadata, and
cost semantics belong to [120 Provider Registry](../120-provider-registry/spec.md).

## Scope

- Workbench Settings > Models as the profile/global model configuration surface
- GUI and TUI composer model pickers as current-cwd UX state
- shared `$PSYCHEVO_HOME/model-state.json` model-selection state
- Gateway model settings, provider save/catalog, and assignment RPC semantics
- explicit model catalog fetch UX and immediate option propagation
- global/profile default model assignment
- auxiliary model assignments for title generation and context compression

Out of scope:

- runtime provider registry facts, config schema, metadata precedence, and cost
  accounting; these belong to [120 Provider Registry](../120-provider-registry/spec.md)
- concrete Web layout, CSS, and component composition; these belong to
  [240 pevo Web](../240-pevo-web/spec.md)
- concrete terminal layout, key handling, and panel rendering; these belong to
  [210 pevo TUI](../210-pevo-tui/spec.md)
- project-local model settings in Workbench Settings
- hot-swapping the currently running turn
- automatic live provider validation
- storing provider secrets in frontend storage or TOML
- automatic migration from legacy `tui-state.json` model fields

## Saved Defaults And Composer State

Settings > Models is an app-level profile/global configuration page. It must not
replace or remove the composer's session-scoped model control. Saving in
Settings changes future default behavior and does not silently mutate an active
session turn.

Settings assignment rows are profile/global state. `model/settings/read` with
`scope: "global"` reads profile config only for default and auxiliary model
assignments. Project-local `.psychevo/config.toml` model overrides may affect
composer/effective controls, but they must not change what Settings > Models
shows as the saved global default.

Composer model selection is shared UX state, not default configuration. GUI and
TUI composer pickers write `$PSYCHEVO_HOME/model-state.json` for the canonical
cwd and never write TOML defaults. If a user wants a saved default, they must
use Settings > Models, `pevo model set`, or another explicit default-saving
command.

`model-state.json` stores only bounded model-selection state: per-cwd
provider-qualified model, optional reasoning effort, update timestamp, and
recent model entries for picker ordering. It must not store provider secrets,
raw prompts, transcripts, reasoning text, model catalog payloads, or API
responses.

Legacy TUI model fields are not migrated. Pre-release development state should
use fresh profile state or `pevo init --reset-state` instead of carrying forward
old `tui-state.json` model selections.

## Provider Configuration UX

Settings > Models shows built-in providers, configured providers, and custom
providers in one compact list. Each provider row exposes its display label,
configured state, base URL, credential state, no-auth state, and model-catalog
fetch action. Provider API keys may be typed in the page but are sent only to
Gateway for durable `.env` writes; they are never persisted in frontend storage
and are never written into TOML.

Model catalog fetches are explicit user actions. They use runtime provider
catalog helpers and deterministic timeouts. Free OpenCode Zen selections must
show a privacy/data-retention warning before save; the provider id, aliases,
base URL, credential env, no-auth support, and free-model classification are
defined by [120 Provider Registry](../120-provider-registry/spec.md).

Fetched catalogs are picker/display candidate models, not persisted model
configuration and not runtime metadata precedence. Successful explicit fetches
from Workbench, CLI, or TUI write the shared provider-model picker cache at
`$PSYCHEVO_HOME/cache/provider_models_cache.json`. `model/settings/read` and
Settings read paths hydrate matching cached rows without contacting provider
`/models` endpoints, and a fresh fetch is reflected immediately in Settings,
Workbench, CLI, and TUI picker surfaces. This makes provider models selectable
after configuration/fetch without writing each catalog row into TOML or silently
changing the active composer model.

The page offers independent save controls for:

- default model, persisted as top-level `model`
- title generation, persisted as `auxiliary.title_generation.provider/model`
- context compression, persisted as `auxiliary.compression.provider/model`

Each assignment row uses catalog-backed pickers only. The GUI does not support
manual model-id text entry. A model picker is paired with a reasoning-effort
picker derived from the selected model's capability metadata; `none` is shown as
`Default`. Saving a reasoning effort persists it with the assignment model
selection and leaves unrelated assignment rows unchanged. The page avoids
duplicating values that controls already communicate: selected model,
inherit/default reasoning, API-key env names, no-auth state, and fetched catalog
counts should not be repeated as secondary row copy.

`compression.*` continues to own compaction thresholds and enabled/auto flags.
`compression.model` remains a legacy fallback, but new GUI writes use
`auxiliary.compression`.

## Composer Refresh

After saving the default model, Workbench must refresh the current
`settings/read.controls` view and sync the App-level composer state to the
backend-resolved controls model and reasoning effort. This keeps the composer
label from displaying a stale in-memory model after a successful default save.
The refresh does not change scope precedence: an active session model or
cwd entry from `model-state.json` still wins over the global default.

## Gateway RPC

Workbench uses Gateway JSON-RPC methods:

- `model/settings/read`
  - For `scope: "global"`, returns default and auxiliary assignments from the
    profile/global config rather than the current cwd's effective config.
- `model/provider/save`
- `model/provider/catalog`
- `model/assignment/set`

All responses are camelCase, redacted, and typed in the shared protocol.
Catalog and save failures must be bounded, user-visible errors and must not
leave partially typed secrets in frontend state after successful save.

## Attachments

- [Testing](testing.md) defines deterministic and live opt-in validation.

## Related Topics

- [120 Provider Registry](../120-provider-registry/spec.md) defines provider
  identity, TOML configuration, credential resolution, catalog metadata, and
  runtime resolution.
- [210 pevo TUI State and Models](../210-pevo-tui/state-and-models.md) defines
  concrete TUI model picker and state behavior.
- [240 pevo Web](../240-pevo-web/spec.md) defines concrete Workbench Settings
  and composer UI behavior.
- [057 Profiles](../057-profiles/spec.md) defines the active profile home that
  owns global config and shared model state.
- [200 pevo CLI](../200-pevo-cli/spec.md) defines CLI model and auth commands.
