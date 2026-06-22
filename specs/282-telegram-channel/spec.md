---
name: 282. Telegram Channel
psychevo_self_edit: deny
---

Define Psychevo's first-party Telegram channel behavior.

Telegram uses the Bot API. Polling is the default transport. Webhook mode is
allowed only when the connection has an explicit webhook secret and the managed
Gateway can expose the required endpoint safely.

## Scope

- Telegram bot token setup
- Telegram Bot API polling and optional webhook behavior
- DM, group, supergroup, and forum topic source identity
- allowlist and mention gating rules
- Telegram delivery limits, text fallback, and diagnostics

Out of scope:

- common channel configuration and thread invariants, owned by
  [028 Channels](../028-channels/spec.md)
- Workbench layout and setup UX, owned by
  [280 Channel UX](../280-channel-ux/spec.md)
- other messaging platforms

## Connection Shape

A Telegram connection uses:

- `channel = "telegram"`
- default `domain = "telegram"`
- default `transport = "polling"`
- default credential env `TELEGRAM_BOT_TOKEN`

The bot token is stored in profile `.env`. TOML stores the env name only.
Setup should refer users to BotFather for token creation but must not log,
echo, or store the raw token outside the profile secret boundary.

## Remote Source Identity

Telegram source identity records the connection id, Telegram domain, chat type,
chat id, optional user id, optional forum topic or thread id, message id, reply
target, and redacted routing metadata.

Direct messages are isolated by user/chat identity. Group and supergroup lanes
are isolated by chat id, and forum topics include the topic/thread id when
Telegram provides one.

Allowlists may contain direct user ids and group/chat ids. Group chats should
respect `require_mention` when enabled. A message that fails allowlist or
mention gating must not create or continue a local thread.

## Adapter Behavior

Polling uses Bot API `getUpdates` with offset tracking. Startup may clear stale
webhook state when the connection is configured for polling and doing so is
safe for the bot.

Inbound accepted text maps to a Gateway source-scoped turn. Images and files
enter the shared attachment pipeline when supported. Unsupported media receives
a bounded text explanation rather than raw platform payload leakage.

Outbound text uses `sendMessage`. Messages are split when they exceed Telegram
length limits. The adapter preserves reply targets when it can do so safely and
degrades to ordinary chat messages when it cannot.

Telegram can support typing indicators and markdown-like formatting, but the
adapter must choose the safest parse mode for generated content and avoid
syntax that can break delivery.

## Diagnostics

Runner diagnostics expose secret-free facts: polling state, last update offset,
last successful poll, last inbound/outbound timestamps, and Bot API error code
or status text when it contains no token or private chat content.

Webhook diagnostics must distinguish "configured but unavailable" from
"webhook secret missing" and from platform delivery failures.

## Related Topics

- [028 Channels](../028-channels/spec.md) defines the shared channel model.
- [280 Channel UX](../280-channel-ux/spec.md) defines setup and Settings UX.
- [021 Gateway](../021-gateway/spec.md) defines source, thread, and turn
  semantics.

## Attachments

- [Testing](testing.md) defines Telegram setup, adapter, and diagnostic
  validation scenarios.
