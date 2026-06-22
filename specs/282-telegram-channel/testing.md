---
name: 282. Telegram Channel Testing
psychevo_self_edit: deny
---

Telegram tests use fake Bot API servers and isolated profile state by default.
Real Bot API checks are live opt-in only.

## Long-Term Acceptance Contract

- Telegram setup and runtime behavior is validated with fake Bot API servers by
  default.
- Bot tokens stay inside profile secret boundaries and never appear in JSON,
  logs, transcripts, diagnostics, or UI output.
- Polling is the default transport; webhook mode is blocked unless its local
  prerequisites are explicit and available.
- Real Bot API checks are live opt-in only.

## Current Implementation Slice

Current automation should use fake Bot API servers and isolated profile state
for setup, polling, source identity, allowlists, outbound delivery, and
diagnostics.

## Scenario Matrix

- Setup tests cover default `TELEGRAM_BOT_TOKEN`, custom credential env names,
  hidden or stdin token capture, secret-free `--json`, and `.env` writes.
- Missing token diagnostics are explicit and secret-free.
- Polling is the default transport.
- Webhook mode is rejected or blocked when the required webhook secret or
  endpoint prerequisites are missing.
- Polling tests cover Bot API `getUpdates` request shape, offset handling,
  empty polling responses, transient platform errors, and stale-webhook cleanup
  when polling owns the bot.
- Source identity tests cover DMs, groups, supergroups, and forum topic/thread
  ids.
- Allowlist tests cover numeric user ids, chat ids, unknown users, unknown
  groups, and group mention gating.
- Delivery tests cover `sendMessage`, message splitting, safe parse mode,
  reply target preservation, and text fallback when rich rendering is
  unavailable.
- Attachment tests cover supported media entering the shared attachment
  pipeline and unsupported media receiving bounded guidance.
- Runner diagnostics include polling status, last update offset, timestamps,
  and secret-free Bot API errors.
- Diagnostics must not expose bot tokens, raw private message text, or raw
  platform payloads.

## Validation Boundaries

- Fake Bot API tests are the default validation path.
- Live Bot API checks require explicit opt-in and isolated profile state.
- Assertions should focus on Bot API behavior, source identity, allowlist
  decisions, and secret boundaries.
