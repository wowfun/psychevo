---
name: 200. pevo CLI Testing
psychevo_self_edit: deny
---

Define acceptance coverage for the `pevo` product CLI.

## Default Validation

Automation vocabulary and generic validation boundaries follow
[060 Automation](../060-automation/spec.md).

The default deterministic gate is:

```bash
scripts/validate.sh broad
```

## Init Coverage

- `pevo init` creates `config.toml`, `.env`, `state.db`, `sessions/`, `logs/`,
  and `cache/` under an isolated `PSYCHEVO_HOME`.
- Existing `config.toml` and `.env` are not overwritten.
- `pevo init --reset-state` backs up `state.db`, `state.db-wal`, and
  `state.db-shm` before creating a fresh v5 state database.
- The starter config resolves DeepSeek with `reasoning_effort = medium`.
- Success output lists paths and does not include credential values.
- Re-running init is idempotent.

## Run Coverage

- Positional prompt and multi-argument prompt.
- stdin-only prompt.
- positional prompt plus stdin append.
- empty prompt rejection before session creation.
- `--dir` controls the tool workdir.
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
- latest-session lookup filters by canonical workdir and `source = "run"`.
- SQLite state uses `user_version = 14`; older unsupported state databases reject with an
  explicit reset/cutover instruction.
- Reasoning is preserved locally as folded assistant content, without entering
  default visible output or cross-provider replay.

## Command Metadata Coverage

- `pevo --help` exposes subcommand descriptions aligned with the shared command
  contract vocabulary while argv parsing remains clap-owned.
- Representative command help, including `pevo run --help`, `pevo tui --help`,
  `pevo session --help`, `pevo session export --help`, `pevo skill --help`,
  `pevo model fetch --help`, `pevo config provider add --help`, and
  `pevo auth set --help`, describes arguments, flags, local writes, provider
  calls, stdin secrets, JSON output, skill selection, and sensitive export
  includes where applicable.
- `pevo skill` is the only skill command family; obsolete `pevo skills` is
  rejected by argument parsing.
- `pevo session`, `pevo model`, `pevo config`, and `pevo auth` expose singular
  top-level command names.

## Session Coverage

- `pevo session list`, `show`, `rename`, `archive`, and `restore` operate on
  isolated SQLite state.
- `latest` resolves the latest active `run` or `tui` session for the current
  canonical workdir.
- Exact ids are not fuzzy matched.
- `--json` emits structured output; JSON errors use the common
  `{"type":"error","message":"..."}` shape.

## Model Coverage

- `pevo model list` and `pevo model current` read only local config/cache.
- `pevo model set <provider/model>` writes the current workdir local top-level
  model setting by default; `-g`/`--global` writes
  `$PSYCHEVO_HOME/config.toml`. It rejects unqualified model ids and unknown
  providers without contacting providers.
- `pevo model fetch <provider>` is the only model command that contacts
  provider `/models`, and tests use fake local providers only.
- `--json` emits structured output; JSON errors use the common error shape.

## Config And Auth Coverage

- Scoped config/auth writes default to the current workdir `.psychevo` scope;
  `-g`/`--global` writes global `$PSYCHEVO_HOME`.
- `--global` and `--local` are mutually exclusive.
- `--project` is rejected by argument parsing.
- `pevo config provider add` writes provider TOML without raw keys.
- `--api-key-env` records an env var name only; `--api-key-stdin` writes the
  secret to the selected `.env`.
- `pevo auth status` and `pevo auth set --api-key-stdin` never print raw
  secrets in human or JSON output.

## Permission Coverage

- `pevo run --permission-mode` accepts `default`, `acceptEdits`, `plan`,
  `dontAsk`, and `bypassPermissions`; `plan` selects the read-only runtime mode.
- `--dangerously-skip-permissions` selects `bypassPermissions`, while hard
  denies still apply.
- `dontAsk` denies actions that would otherwise prompt unless they already
  match `permissions.allow` or a safe default.
- `pevo config permissions list/remove` manages local allow, ask, and deny
  rules in the current workdir's project-local `.psychevo/config.toml`.
- `allow always` approval writes project-local TOML and skips exact duplicate
  rules.

## Install Script Coverage

- `scripts/install.sh` passes POSIX shell syntax validation with `sh -n`.
- Dry-run coverage verifies local `--source` install planning, clone-mode
  defaults and overrides, default post-install initialization, `--no-init`, and
  `--with-peval` planning. It also verifies Git Bash/MSYS/MINGW binary naming.
- Dry-run output is deterministic and does not require `git`, `cargo`, network
  access, provider credentials, or global Psychevo state.

## Stats Coverage

- `pevo stats` defaults to the current workdir and reads only local SQLite
  state.
- `pevo stats --all`, `--dir`, `--days`, `--limit`, and `--json` produce
  deterministic output.
- Empty stats output is bounded and does not require live providers.
- Stats distinguish unknown pricing from known free pricing and include model,
  tool, and top-session breakdowns.

## Context Coverage

- `pevo context` requires `--session <id|latest>`.
- `latest` resolves active `run`/`tui` sessions by canonical workdir and
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

The repo-local live development environment uses `.local/.psychevo-dev/` as an
isolated `PSYCHEVO_HOME`. Live validation scripts may use that home, but they
must not copy credential files automatically.

## Related Topics

- [200 pevo CLI](spec.md) defines the product CLI surface.
- [200 pevo run](pevo-run.md) defines run output modes.
- [200 pevo install](install.md) defines the install helper script.
