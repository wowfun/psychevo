---
name: 125. Model Config Testing
psychevo_self_edit: deny
---

# 125. Model Config Testing

Define deterministic acceptance coverage for saved model defaults, composer
model state, provider setup, and explicit catalog fetch UX.

## Default Validation

Tests must use fake providers and isolated config, env, home, and cwd state
by default. No default test or broad validation path may require a real provider
key, live OpenCode Zen request, or live model catalog fetch.

Documentation-only changes in this topic require `git diff --check` and link
validation for changed spec references. Code changes must run the closest
gateway, Workbench, TUI, or provider-registry test that owns the changed
behavior.

## Deterministic Acceptance

- Settings > Models reads and saves profile/global model defaults without being
  masked by the current project `.psychevo/config.toml`.
- Composer model pickers in GUI and TUI write `model-state.json` for the
  canonical cwd and do not write TOML defaults.
- Provider save flows write durable secrets only through Gateway/runtime `.env`
  handling and never persist raw keys in frontend storage or TOML.
- Explicit fake catalog fetches update assignment controls and composer model
  options immediately without persisting fetched model rows as config.
- Default, title-generation, and context-compression assignments save
  independently and preserve unrelated assignment rows.
- Assignment controls support model-specific reasoning-effort options, persist
  the selected effort with the assignment, and display `none` as `Default`.
- Settings > Models does not expose manual model-id text entry for assignments.
- OpenCode Zen free-model selections show a privacy/data-retention warning
  before save.
- After saving the global default, Workbench refreshes
  `settings/read.controls` and syncs the in-memory composer label to the
  backend-resolved current-scope model.
- Saving defaults or assignments does not hot-swap an already running turn, and
  active session or cwd composer overrides continue to win over the global
  default.
- Legacy `tui-state.json` model fields are not automatically migrated during
  normal startup.

## Live Opt-In Validation

Live provider and live catalog validation is opt-in only. When explicitly run,
it must use isolated `PSYCHEVO_HOME`, `PSYCHEVO_CONFIG`, cwd, and temp state
unless the caller explicitly chooses real user configuration.

## Related Topics

- [125 Model Config](spec.md) defines the product-level model configuration UX
  contract.
- [120 Provider Registry Testing](../120-provider-registry/testing.md) defines
  provider/config deterministic and live opt-in validation.
- [240 pevo Web Testing](../240-pevo-web/testing.md) defines Workbench browser
  and component validation.
- [210 pevo TUI Testing](../210-pevo-tui/testing.md) defines terminal model
  picker validation.
