---
name: 281. WeChat Channel Testing
psychevo_self_edit: deny
---

WeChat tests use fake iLink servers and isolated profile state by default. Real
iLink checks are live opt-in only.

## Long-Term Acceptance Contract

- QR setup captures and persists WeChat iLink credentials without exposing
  secret values.
- Runtime polling distinguishes healthy empty polls from expired QR sessions.
- WeChat stays DM-first, uses text fallback controls, and enables image/file
  media only after the iLink item and transfer contract is confirmed and
  covered by fake iLink tests.
- Reconnect updates credentials while preserving user-owned runtime defaults
  and allowlists.
- Real iLink checks are live opt-in only.

## Current Implementation Slice

Current automation uses fake iLink servers, isolated profile homes, and
Workbench harnesses for QR setup, reconnect, runner diagnostics, text command
fallbacks, and any confirmed media item shapes.

## Scenario Matrix

- QR setup tests use a fake local iLink server for successful login, expired QR
  refresh, redirect host handling, timeout, secret redaction, `.env` writes,
  TOML env-field writes, and DM allow-user discovery.
- QR confirmation persists env-backed config immediately. A fake `getupdates`
  endpoint returning `errcode=-14` or `ret=-14` must not block QR credential
  persistence.
- QR reconnect tests cover existing connection upsert: ordinary setup still
  rejects duplicate ids, while reconnect updates env-backed credentials and
  preserves cwd, model, permission mode, enablement, and existing
  allowlists unless iLink returns a new DM user id.
- Manual fallback tests cover `--credential-stdin`, `--account-id`,
  `--account-env`, `--ilink-base-url`, and `--json`.
- iLink adapter tests cover polling, accepted inbound DM text, fail-closed
  allowlists, `sendmessage`, context-token persistence, and text fallback
  approval/clarify commands.
- Current media fallback tests cover inbound image and file items becoming
  bounded metadata instead of being silently ignored.
- Confirmed CDN media tests, when enabled, must cover inbound image items,
  inbound file items, required media download requests, validation failures,
  cache writes, and the Gateway input shape created from each media kind.
- Outbound media tests, when enabled, cover explicit validated attachment
  references, sendmessage payload shape, upload failure fallback, and rejection
  of arbitrary local paths in model text.
- If iLink media item or endpoint shapes are not confirmed, tests must assert
  that media is disabled with bounded diagnostics rather than silently ignored
  or guessed.
- Local long-poll read timeouts are treated as empty healthy polls.
- iLink `ret=-14` or `errcode=-14` during fresh-login grace keeps the runner
  running with reason `qr_login_pending`.
- The same iLink expired-session response outside the grace window marks the
  runner blocked with reason `needs_qr_login`, records the non-secret iLink
  error code, and stops the active poll loop until reconnect.
- Group tests cover the advanced warning path and blocked diagnostics when
  iLink never emits group events.
- QR payload tests cover direct iLink QR images (`data:image/...`) and URL or
  plain scan payloads. Direct images must not be re-encoded into a second QR.
- Workbench does not render `WECHAT_ACCOUNT_ID` or `WECHAT_ILINK_BASE_URL` as
  default editable settings.
- Existing WeChat rows whose runner reason is `needs_qr_login` present
  reconnect as the primary setup action.
- Existing WeChat rows whose runner reason is `qr_login_pending` present a
  neutral starting-polling state.

## Validation Boundaries

- Fake iLink tests are the default validation path.
- Live iLink checks require explicit opt-in and must use isolated profile
  state.
- Assertions should compare state transitions and secret boundaries, not raw
  iLink payload snapshots.
