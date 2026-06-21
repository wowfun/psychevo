---
name: 280. Channels Testing
psychevo_self_edit: deny
---

# 280. Channels Testing

Channels default validation uses deterministic local harnesses, fake adapters,
and isolated profile state. Real channel API checks are live opt-in only.

## Unit And Config Tests

- Parse `[channels]` and `[[channels.connections]]`, including WeChat,
  Telegram, Feishu, and Lark channel/domain combinations.
- Reject unknown channels, duplicate ids, invalid transports, missing ids, and
  malformed allowlists with local, secret-free errors.
- Preserve secret redaction: TOML, JSON output, logs, and Workbench views expose
  env names and credential status only, never raw values.
- Assert enablement fails closed when credentials or allowlists are missing.
- Assert WeChat readiness also fails closed when the account env value is
  missing, and reports group limitations separately from credential failure.
- Assert source-key hashing excludes raw channel identifiers and local paths.

## CLI Tests

- `pevo gateway setup --channel <name>` should work against temporary
  `PSYCHEVO_HOME`.
- `pevo channel ...` should fail as an unknown top-level command.
- `pevo gateway status --json` should include configured, enabled, ready,
  blocked, and setup-needed channel counts.
- `--json` output should be structured, stable, and secret-free.
- Missing credential and missing allowlist diagnostics should be explicit.
- WeChat QR setup tests use a fake local iLink server for successful login,
  expired QR refresh, redirect host handling, timeout, secret redaction, `.env`
  writes, TOML env-field writes, and DM allow-user discovery.
- WeChat QR confirmation persists env-backed config immediately. A fake
  `getupdates` endpoint returning `errcode=-14` or `ret=-14` must not block QR
  credential persistence; live `getupdates` remains an explicit Doctor/runtime
  diagnostic.
- WeChat QR reconnect tests cover existing connection upsert: ordinary setup
  still rejects duplicate ids, while QR reconnect updates env-backed
  credentials and preserves workdir, model, enablement, and existing allowlists
  unless iLink returns a new DM user id.
- WeChat manual fallback tests cover `--credential-stdin`, `--account-id`,
  `--account-env`, `--ilink-base-url`, and `--json`.
- Live opt-in should be the only command path that performs real channel
  API checks.

## Adapter And Gateway Tests

- Telegram adapter tests cover Bot API polling request shape, update offset
  handling, numeric allowlists, group mention gating, and message splitting.
- Feishu/Lark adapter tests cover domain mapping, app credential presence,
  SDK long-connection event parsing, group mention gating, cards, and text
  fallback.
- WeChat iLink adapter tests cover polling, sendmessage, context-token
  persistence, and text-only approval/clarify commands.
- WeChat iLink adapter tests cover timeout semantics: local long-poll read
  timeouts are empty healthy polls, while iLink `-14/session timeout` is an
  expired-login signal surfaced as `needs_qr_login`.
- WeChat QR start tests cover both direct iLink QR images
  (`data:image/...`) and URL/plain scan payloads. Direct images must not be
  re-encoded into a second QR; URL/plain payloads may produce generated SVG.
- Gateway tests cover persistent source binding, permission/clarify response
  routing, per-source ordering, and partial adapter startup.
- Channel runner tests cover enabled config-ready channels polling inbound
  messages, invoking `Gateway::send_turn`, sending the final assistant answer
  back through the adapter, and recording secret-free runner diagnostics.
- Channel runner tests cover WeChat expired-login classification: `-14/session
  timeout` during fresh-login grace retries with reason `qr_login_pending`,
  while the same response outside the grace marks the runner blocked with
  reason `needs_qr_login`, records the non-secret iLink error code, and stops
  the active poll loop until reconnect.
- Gateway status and RPC tests distinguish local config readiness from runner
  liveness. A channel can be `ready` while the runner is `stopped` only when
  the managed Gateway is not running.

## Workbench Tests

- Settings > Channels has no top overview metrics, no filter tabs, and no
  right-side detail pane.
- Connected channel rows show status, credential state, allowlist state,
  runtime summary, Test, Settings, and enable switches.
- Selecting a configured channel opens the independent settings page, and Back
  returns to the list.
- Channel settings detail visual checks cover the compact header, status
  summary strip, doctor checks, sectioned configuration groups, readable dark
  theme controls, and absence of floating/oversized status pills.
- Enable switch state syncs between list and settings page and surfaces blocked
  diagnostics when production checks fail.
- Add channel setup cards switch content for WeChat, Telegram, Feishu, and
  Lark.
- WeChat setup renders direct QR images when provided, generated SVG otherwise,
  updates the visible expiry countdown once per second, keeps status polling on
  the Gateway-provided interval, and keeps action buttons readable in dark
  theme loading/disabled states.
- WeChat setup clears stale QR images, countdowns, session ids, and Check
  status controls when the Gateway reports a missing, expired, completed, or
  restart-lost session. Existing WeChat connections with runner reason
  `needs_qr_login` render a reconnect-first setup card instead of a connected
  card. Existing WeChat connections with runner reason `qr_login_pending`
  render a neutral starting-polling card instead of connected or reconnect
  states.
- Desktop and mobile Playwright checks assert no horizontal overflow.

## Validation Commands

Narrow validation should run the closest touched tests first:

- channel config and CLI tests for Rust-only changes
- Gateway fake-adapter tests for runner or adapter changes
- Workbench unit and Playwright tests for Settings UI/protocol changes
- generated protocol check when Rust schema changes

Before handoff, run `scripts/validate.sh broad` unless the change is
documentation-only or a host prerequisite blocks it. Live channel validation
requires explicit credentials prepared under `.local/.psychevo-dev`, explicit
`PSYCHEVO_HOME`, and `doctor --live`.
