---
name: 283. Feishu / Lark Channel
psychevo_self_edit: deny
---

Define Psychevo's first-party Feishu and Lark channel behavior.

Feishu and Lark share one adapter family with domain-specific endpoints and
credential defaults. The channel uses long-connection event delivery by
default.

## Scope

- Feishu and Lark app credential setup
- domain-specific endpoint and credential defaults
- long-connection event handling
- user, group, chat, and tenant source identity
- cards/buttons capability and text fallback
- platform-specific diagnostics

Out of scope:

- common channel configuration and thread invariants, owned by
  [028 Channels](../028-channels/spec.md)
- Workbench layout and setup UX, owned by
  [280 Channel UX](../280-channel-ux/spec.md)
- WeChat and Telegram behavior

## Connection Shape

A Feishu connection uses:

- `channel = "feishu"`
- default `domain = "feishu"`
- default `transport = "long_connection"`
- default credential env `FEISHU_APP_SECRET`
- default app id env `FEISHU_APP_ID`

A Lark connection uses:

- `channel = "lark"`
- default `domain = "lark"`
- default `transport = "long_connection"`
- default credential env `LARK_APP_SECRET`
- default app id env `LARK_APP_ID`

The app id and app secret are stored in profile `.env`. TOML stores env names
only. Feishu and Lark must not share credentials implicitly because tenants,
regions, domains, and app registrations differ.

## Remote Source Identity

Feishu/Lark source identity records the connection id, domain, tenant or app
context when available, chat type, chat id, optional user/open id, optional
thread or message id, reply target, and redacted routing metadata.

Tenant, domain, and connection id are part of isolation. Identical-looking chat
or user ids from different domains or tenants must not map to the same remote
source lane.

Allowlists may contain direct user/open ids and group/chat ids. Group messages
respect `require_mention` when enabled. Messages that fail allowlist or mention
gating must not create or continue a local thread.

## Adapter Behavior

The adapter uses the platform SDK long-connection event stream. It normalizes
accepted inbound text and supported media into Gateway source-scoped turns.

Feishu/Lark can support richer delivery than text-only channels. Cards and
buttons may be used for approvals, Ask responses, and structured status when
the app permissions and platform capabilities allow them. The adapter must keep
a text fallback for every interactive control.

Outbound delivery should prefer the platform's safe message form for generated
content. When markdown, cards, or buttons are unavailable or rejected, the
adapter degrades to text without changing the local thread transcript.

## Diagnostics

Runner diagnostics expose secret-free facts: connection state, SDK event stream
state, last inbound/outbound timestamps, app id env status, credential env
status, and platform error codes that do not contain secrets or private chat
content.

Domain diagnostics must distinguish Feishu and Lark endpoint selection,
credential presence, tenant/app mismatch, missing app permissions, and
long-connection failures.

## Related Topics

- [028 Channels](../028-channels/spec.md) defines the shared channel model.
- [280 Channel UX](../280-channel-ux/spec.md) defines setup and Settings UX.
- [021 Gateway](../021-gateway/spec.md) defines source, thread, and turn
  semantics.

## Attachments

- [Testing](testing.md) defines Feishu/Lark setup, adapter, and diagnostic
  validation scenarios.
