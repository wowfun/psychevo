---
name: Model Configuration UX
psychevo_self_edit: deny
---

# Model Configuration UX

Define the Workbench profile-level model configuration surface.

## Scope

- Settings > Models in Workbench
- built-in and custom provider configuration from the GUI
- explicit model catalog fetches
- global/profile default model assignment
- auxiliary model assignments for title generation and context compression

Out of scope:

- project-local model settings in the GUI
- hot-swapping the currently running turn
- automatic live provider validation
- storing provider secrets in frontend storage or TOML
- automatic migration from legacy `tui-state.json` model fields

## Behavior

Settings > Models is an app-level configuration page. It must not replace or
remove the composer's session-scoped model control. Saving in Settings changes
future default behavior and does not silently mutate an active session turn.
The page's assignment rows are profile/global state: `model/settings/read` with
`scope: "global"` reads the profile config only for default and auxiliary model
assignments. Project-local `.psychevo/config.toml` model overrides may affect
composer/effective controls, but they must not change what Settings > Models
shows as the saved global default.

Composer model selection is shared UX state, not default configuration. GUI and
TUI composer pickers write `$PSYCHEVO_HOME/model-state.json` for the canonical
workdir and never write TOML defaults. If a user wants a saved default, they must
use Settings > Models, `pevo model set`, or another explicit default-saving
command.

`model-state.json` stores only bounded model-selection state: per-workdir
provider-qualified model, optional reasoning effort, update timestamp, and
recent model entries for picker ordering. It must not store provider secrets,
raw prompts, transcripts, reasoning text, model catalog payloads, or API
responses.

Legacy TUI model fields are not automatically migrated at startup. A one-time
maintenance script may read an existing `tui-state.json`, write equivalent
`model-state.json` model fields, and leave TUI-only flags in `tui-state.json`.

The page shows built-in providers, configured providers, and custom providers
in one compact list. Each provider row exposes its display label, configured
state, base URL, credential state, no-auth state, and model-catalog fetch
action. Provider API keys may be typed in the page but are sent only to Gateway
for durable `.env` writes; they are never persisted in frontend storage and are
never written into TOML.

OpenCode Zen is a built-in provider with id `opencode-zen`, aliases
`opencode` and `zen`, base URL `https://opencode.ai/zen/v1`, optional
`OPENCODE_ZEN_API_KEY`, and explicit public/free mode through
`provider.opencode-zen.options.no_auth = true`.

Model catalog fetches are explicit user actions. They use runtime provider
catalog helpers and deterministic timeouts. OpenCode Zen free models are marked
from live catalog/pricing metadata when available and from documented free ids,
including `*-free` ids and `big-pickle`. Free Zen selections must show a
privacy/data-retention warning before save.

Fetched catalogs are session-visible candidate models, not persisted model
configuration. Gateway keeps fetched catalog rows in process memory and merges
them into subsequent `model/settings/read` assignment options and
`settings/read.controls.modelDetails` for the composer. The GUI also reflects a
fresh fetch immediately in both Settings > Models and the composer selector.
This makes provider models selectable after configuration/fetch without writing
each catalog row into TOML or silently changing the active composer model.

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

After saving the default model, Workbench must refresh the current
`settings/read.controls` view and sync the App-level composer state to the
backend-resolved controls model and reasoning effort. This keeps the composer
label from displaying a stale in-memory model after a successful default save.
The refresh does not change scope precedence: an active session model or
workdir entry from `model-state.json` still wins over the global default.

## Gateway RPC

Workbench uses Gateway JSON-RPC methods:

- `model/settings/read`
  - For `scope: "global"`, returns default and auxiliary assignments from the
    profile/global config rather than the current workdir's effective config.
- `model/provider/save`
- `model/provider/catalog`
- `model/assignment/set`

All responses are camelCase, redacted, and typed in the shared protocol.
Catalog and save failures must be bounded, user-visible errors and must not
leave partially typed secrets in frontend state after successful save.

## Testing

Tests must use fake providers and isolated config/env files by default. No test
or broad validation path may require a real OpenCode Zen key or live catalog
unless explicitly opted in.
