---
name: 200. pevo CLI Testing
psychevo_self_edit: deny
---

Define acceptance coverage for the `pevo` product CLI.

## Long-Term Acceptance Contract

- `pevo` command families remain singular, discoverable through help output,
  and validated by clap-owned argument parsing.
- CLI commands that read or write profile/project state use isolated
  `PSYCHEVO_HOME`, cwd, config, and state paths in tests.
- JSON output is structured, secret-free, and uses the common error shape for
  command failures that intentionally report through stdout.
- Commands that can contact providers or live services use deterministic local
  fakes in default tests; real providers remain explicit live opt-in
  validation.
- Scoped writes default to the documented profile or project scope for that
  command family and never mutate unrelated global user config.

## Current Implementation Slice

This testing topic covers the current product CLI surface for init, run,
runtime config discovery, command metadata, sessions, models, config/auth,
plugins, permissions, install helpers, stats, and context inspection. Manual
broad validation for code changes is the Rust workspace gate defined by
[065 CI/CD](../065-ci-cd/spec.md), but acceptance coverage should
come from the focused command and smoke tests below.

## Init Coverage

- `pevo init` creates `config.toml`, `.env`, `state.db`, `sessions/`, `logs/`,
  and `cache/` under an isolated `PSYCHEVO_HOME`.
- Existing `config.toml` and `.env` are not overwritten.
- `pevo init --reset-state` stops the current profile's managed Gateway,
  backs up `state.db`, `state.db-wal`, and `state.db-shm`, and creates a fresh
  current-schema state database.
- The starter config resolves DeepSeek with `reasoning_effort = medium`.
- Success output lists paths and does not include credential values.
- Re-running init is idempotent.

## Run Coverage

- Positional prompt and multi-argument prompt.
- stdin-only prompt.
- positional prompt plus stdin append.
- empty prompt rejection before session creation.
- `--dir` controls the tool cwd.
- `--format json` emits typed transcript NDJSON beginning with `thread.started`
  and `turn.started`.
- `--format json` hides reasoning by default.
- `--format json --include-reasoning` emits typed reasoning transcript entries
  while keeping assistant transcript items sanitized.
- `--include-reasoning` without JSON format rejects.
- `--format json` runtime/config errors emit one stdout error JSON object.
- default-format errors remain human stderr failures.
- budget exhaustion emits a structured `turn.failed.terminalReason`, writes a
  default-format diagnostic to stderr, and exits non-zero.
- long tool workflows can exceed 32 tool turns before a final assistant answer.
  Long tool-turn smoke tests may serialize their local fake-provider subprocess
  loops so default parallel test execution does not make the mock SSE boundary
  nondeterministic.
- removed old `run` flags are rejected by argument parsing.
- `-m provider/model` works, while unqualified `-m model` rejects.
- `--variant` accepts the supported set, passes enabled values, and suppresses
  reasoning for `none`.
- `--runtime <id> --runtime-option mode=value` parses as a runtime-scoped
  selection and option pair, and does not introduce or accept a generic
  `--mode` flag for `pevo run`.
- `--continue` resumes the latest matching run session or creates a new session
  when none exists.
- `--continue` plus `--session` rejects.
- `PSYCHEVO_CONFIG` plus `PSYCHEVO_DB` allows isolated runs without global home.
- Repo-local live validation checks typed `entry.completed` reasoning and tool
  blocks, `turn.completed.finalAnswer`, and `thread.started` reuse for
  `--continue`.

## Runtime Coverage

- `PSYCHEVO_HOME` default discovery uses `$HOME/.psychevo`.
- `PSYCHEVO_HOME` overrides default home.
- `PSYCHEVO_CONFIG` replaces home/project config discovery.
- `PSYCHEVO_CONFIG` loads config-parent `.env` then project `.env`.
- default discovery loads home config/env then project config/env.
- `config.jsonc` files are ignored and do not satisfy missing `config.toml`.
- `PSYCHEVO_CONFIG_DIR` is ignored.
- missing home config rejects before `agent_start`.
- TOML `reasoning_effort` uses the same validation as CLI `--variant`.
- `none` disables lower-level reasoning effort.
- latest-session lookup filters by canonical cwd and `source = "run"`.
- SQLite state uses `user_version = 23`; older unsupported state databases reject with an
  explicit reset/cutover instruction.
- Reasoning is preserved locally as folded assistant content, without entering
  default visible output or cross-provider replay.

## Command Metadata Coverage

- `pevo --help` exposes subcommand descriptions aligned with the shared command
  contract vocabulary while argv parsing remains clap-owned.
- Representative command help, including `pevo run --help`, `pevo tui --help`,
  `pevo session --help`, `pevo session export --help`, `pevo skill --help`,
  `pevo plugin --help`, `pevo plugin doctor --help`,
  `pevo model fetch --help`, `pevo config provider add --help`, and
  `pevo auth set --help`, describes arguments, flags, local writes, provider
  calls, stdin secrets, JSON output, skill and plugin selection, and sensitive
  export includes where applicable.
- `pevo skill` is the only skill command family; obsolete `pevo skills` is
  rejected by argument parsing.
- `pevo plugin` is the only plugin command family; obsolete `pevo plugins` is
  rejected by argument parsing.
- `pevo session`, `pevo model`, `pevo config`, and `pevo auth` expose singular
  top-level command names.

## Session Coverage

- `pevo session list`, `show`, `rename`, `archive`, and `restore` operate on
  isolated SQLite state.
- `latest` resolves the latest active `run` or `tui` session for the current
  canonical cwd.
- Exact ids are not fuzzy matched.
- `--json` emits structured output; JSON errors use the common
  `{"type":"error","message":"..."}` shape.

## Model Coverage

- `pevo model list` and `pevo model current` read only local config/cache.
- `pevo model set <provider/model>` writes the current cwd local top-level
  model setting by default; `-g`/`--global` writes
  `$PSYCHEVO_HOME/config.toml`. It rejects unqualified model ids and unknown
  providers without contacting providers.
- `pevo model fetch <provider>` is the only model command that contacts
  provider `/models`, and tests use fake local providers only.
- `--json` emits structured output; JSON errors use the common error shape.

## Config And Auth Coverage

- Scoped config/auth writes default to the current cwd `.psychevo` scope;
  `-g`/`--global` writes global `$PSYCHEVO_HOME`.
- `--global` and `--local` are mutually exclusive.
- `--project` is rejected by argument parsing.
- `pevo config provider add` writes provider TOML without raw keys.
- `--api-key-env` records an env var name only; `--api-key-stdin` writes the
  secret to the selected `.env`.
- `pevo auth status` and `pevo auth set --api-key-stdin` never print raw
  secrets in human or JSON output.

## Plugin Coverage

- `pevo plugin list`, `view`, and `doctor` read isolated profile and project
  plugin stores and emit secret-free JSON with `--json`.
- `pevo plugin install <path>` defaults to active profile scope.
- `pevo plugin install <path> --local` writes the current cwd plugin store.
- `pevo plugin install <git-url> --ref <ref>` works with deterministic local
  Git repositories in tests.
- `pevo plugin uninstall`, `enable`, and `disable` honor default profile scope
  and `-g`/`--global`.
- `pevo plugin enable --local` and `disable --local` can resolve a
  profile-installed plugin selector while writing only current cwd
  `.psychevo/config.toml` policy.
- `--global` and `--local` are mutually exclusive for plugin write commands.
- Duplicate profile/project package records require scoped canonical selectors;
  bare `name` and `name@source` remain valid only for a unique match.
- `pevo plugin marketplace list/add/remove` manages source catalogs separately
  from plugin enablement policy.
- Plugin worker fixtures can expose a tool through the normal run tool surface
  without contacting a live provider.

## Permission Coverage

- `pevo run --permission-mode` accepts `default`, `acceptEdits`, `plan`,
  `dontAsk`, and `bypassPermissions`; `plan` selects the read-only runtime mode.
- `--dangerously-skip-permissions` selects `bypassPermissions`, while hard
  denies still apply.
- `dontAsk` denies actions that would otherwise prompt unless they already
  match `permissions.allow` or a safe default.
- `pevo config permissions list/remove` manages local allow, ask, and deny
  rules in the current cwd's project-local `.psychevo/config.toml`.
- `allow always` approval writes project-local TOML and skips exact duplicate
  rules.

## Install Script Coverage

- `scripts/install.sh` passes POSIX shell syntax validation with `sh -n`.
- Unknown-option coverage verifies removed installer modes fail as unsupported
  options rather than retaining compatibility aliases.
- Checkout detection coverage verifies the installer fails before dependency or
  network actions when it is not run from inside a Psychevo checkout.
- Install preflight coverage verifies missing native C compiler, missing
  Node.js, missing `pnpm`, mismatched `pnpm` warnings, Windows
  Git Bash native build-tool diagnostics, and unusable pnpm/Corepack shims
  without installing, initializing, network access, provider credentials, or
  global Psychevo state.
- Rust-version boundary coverage verifies that the installer rejects Rust
  1.96.1 and older toolchains, accepts the exact 1.97.0 minimum, and
  continues to accept later stable releases.
- Normal install preflight coverage verifies stderr progress breadcrumbs before
  Cargo/Rust, native build-tool, Node.js, and pnpm checks so a hang can be
  localized from the last printed stage.
- `--check` coverage verifies dependency and version diagnostics without
  installing `pevo` or mutating global Psychevo state, and reports mismatched
  `pnpm` as a warning rather than a failure. It reports `pnpm --version`
  failures as unusable tool failures.
- Cargo and pnpm failure coverage verifies enterprise-network diagnostics are
  printed without modifying proxy, registry, mirror, or CA configuration.
  Cargo install coverage verifies subprocess-scoped timeout/retry defaults,
  user overrides for those values, Windows Git Bash revocation-check defaults,
  effective Cargo network diagnostics, and Windows locked-`pevo.exe`
  replacement failures that produce targeted close-and-rerun guidance instead
  of generic network or build-tool guidance.
- Local checkout preflight failures for native and Web build prerequisites
  include the optional `cargo xtask doctor deps check --only install`
  diagnostic hint. Missing Cargo bootstrap failures do not require `xtask`.

## Stats Coverage

- `pevo stats` defaults to the current cwd and reads only local SQLite
  state.
- `pevo stats --all`, `--dir`, `--days`, `--limit`, and `--json` produce
  deterministic output.
- Empty stats output is bounded and does not require live providers.
- Stats distinguish unknown pricing from known free pricing and include model,
  tool, and top-session breakdowns.

## Context Coverage

- `pevo context` requires `--session <id|latest>`.
- `latest` resolves active `run`/`tui` sessions by canonical cwd and
  honors `--dir`.
- Exact session ids can inspect archived sessions.
- Text output includes only implemented categories and no unavailable rows.
- Text output uses the compact context layout without a bar, hides provider
  source labels, and lists skill index entry estimates in descending token
  order.
- `--json` emits a structured `context_snapshot`; JSON errors use the common
  `{"type":"error","message":"..."}` shape.

## Live Validation

Real provider tests remain live opt-in validation. They may use `PSYCHEVO_HOME`
or explicit `PSYCHEVO_CONFIG`/`PSYCHEVO_DB` isolation, but they do not run in
the default validation path.

Repo-local CLI and TUI live validation may use the shared repo-local
development home defined by [065 CI/CD](../065-ci-cd/spec.md). The canonical
entrypoints are:

```bash
cargo xtask init dev-env
cargo xtask live run
cargo xtask live run --suite provider
```

The live runner sets `PSYCHEVO_HOME`, `PSYCHEVO_CONFIG`, and `PSYCHEVO_DB`
explicitly for each check, uses `.local/.psychevo-dev/config.toml` and `.env`
for config and credentials, and must not copy credential files automatically.

VHS terminal captures are projection evidence. Their tape waits should anchor on
stable user-visible content from the exercised workflow instead of transient
debug/status-line labels when structured state or durable session evidence is
available separately.

## Related Topics

- [200 pevo CLI](spec.md) defines the product CLI surface.
- [200 pevo run](pevo-run.md) defines run output modes.
- [200 pevo install](install.md) defines the install helper script.
