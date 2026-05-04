---
name: 200. pevo CLI Testing
psychevo_self_edit: deny
---

Define acceptance coverage for the `pevo` product CLI.

## Default Validation

The default deterministic gate is:

```bash
scripts/validate.sh broad
```

Default validation must not require live providers, real credentials, user
config, or global host state.

## Init Coverage

- `pevo init` creates `config.jsonc`, `.env`, `state.db`, `sessions/`, `logs/`,
  and `cache/` under an isolated `PSYCHEVO_HOME`.
- Existing `config.jsonc` and `.env` are not overwritten.
- `pevo init --reset-state` backs up `state.db`, `state.db-wal`, and
  `state.db-shm` before creating a fresh v2 state database.
- The starter config resolves DeepSeek with `reasoning_effort = medium`.
- Success output lists paths and does not include credential values.
- Re-running init is idempotent.

## Run Coverage

- Positional prompt and multi-argument prompt.
- stdin-only prompt.
- positional prompt plus stdin append.
- empty prompt rejection before session creation.
- `--dir` controls the tool workdir.
- `--format json` emits NDJSON beginning with `run_start`.
- `--format json` hides reasoning by default.
- `--format json --include-reasoning` emits separate `reasoning_delta` and
  `reasoning_end` events while keeping `message_*` events sanitized.
- `--include-reasoning` without JSON format rejects.
- `--format json` runtime/config errors emit one stdout error JSON object.
- default-format errors remain human stderr failures.
- removed old `run` flags are rejected by argument parsing.
- `-m provider/model` works, while unqualified `-m model` rejects.
- `--variant` accepts the supported set, passes enabled values, and suppresses
  reasoning for `none`.
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
- `PSYCHEVO_CONFIG_DIR` is ignored.
- missing home config rejects before `agent_start`.
- JSONC `reasoning_effort` uses the same validation as CLI `--variant`.
- `none` disables lower-level reasoning effort.
- latest-session lookup filters by canonical workdir and `source = "run"`.
- SQLite state uses `user_version = 2`; older state databases reject with an
  explicit reset/cutover instruction.
- Reasoning is preserved locally as folded assistant content, without entering
  default visible output or cross-provider replay.

## Live Validation

Real provider tests remain ignored and opt-in. They may use `PSYCHEVO_HOME` or
explicit `PSYCHEVO_CONFIG`/`PSYCHEVO_DB` isolation, but they must not run in the
default validation path.

The repo-local live development environment uses `.local/.psychevo-dev/` as an
isolated `PSYCHEVO_HOME`. Live validation scripts may use that home, but they
must not copy credential files automatically.

## Related Topics

- [200 pevo CLI](spec.md) defines the product CLI surface.
- [200 Implementation Plan](plan.md) defines implementation sequencing.
- [200 pevo run](pevo-run.md) defines run output modes.
