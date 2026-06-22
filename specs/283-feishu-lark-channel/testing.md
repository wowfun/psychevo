---
name: 283. Feishu / Lark Channel Testing
psychevo_self_edit: deny
---

Feishu/Lark tests use fake SDK/event harnesses and isolated profile state by
default. Real platform checks are live opt-in only.

## Long-Term Acceptance Contract

- Feishu and Lark share adapter shape but keep domain, endpoint, tenant, and
  credential boundaries separate.
- Long-connection runtime behavior is validated with fake SDK/event harnesses
  by default.
- App secrets stay inside profile secret boundaries and never appear in JSON,
  logs, transcripts, diagnostics, or UI output.
- Real platform checks are live opt-in only.

## Current Implementation Slice

Current automation should use fake SDK/event harnesses and isolated profile
state for setup, long-connection events, source identity, allowlists, delivery,
and diagnostics.

## Scenario Matrix

- Feishu setup tests cover default `FEISHU_APP_ID` and `FEISHU_APP_SECRET`,
  custom env names, hidden or stdin secret capture, secret-free `--json`, and
  `.env` writes.
- Lark setup tests cover default `LARK_APP_ID` and `LARK_APP_SECRET`, custom
  env names, hidden or stdin secret capture, secret-free `--json`, and `.env`
  writes.
- Domain tests assert Feishu and Lark do not share endpoints or credentials
  implicitly.
- Missing app id, missing app secret, tenant mismatch, and missing app
  permission diagnostics are explicit and secret-free.
- Long-connection tests cover SDK event parsing, reconnect handling, accepted
  inbound text, partial adapter startup, and platform error classification.
- Source identity tests cover domain, tenant/app context, direct users, groups,
  chat ids, message ids, and thread ids.
- Allowlist tests cover direct users, groups, unknown senders, unknown chats,
  and group mention gating.
- Delivery tests cover text messages, card/button approvals, Ask responses,
  text fallback, and outbound failure diagnostics.
- Attachment tests cover supported media entering the shared attachment
  pipeline and unsupported media receiving bounded guidance.
- Runner diagnostics include long-connection state, timestamps, app id env
  status, credential env status, and secret-free platform error codes.
- Diagnostics must not expose app secrets, private message text, raw SDK
  payloads, or tenant data beyond the redacted identity needed for debugging.

## Validation Boundaries

- Fake SDK/event tests are the default validation path.
- Live platform checks require explicit opt-in and isolated profile state.
- Assertions should focus on domain isolation, source identity, allowlist
  decisions, delivery fallback, and secret boundaries.
