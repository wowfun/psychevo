---
name: 280. Channels
psychevo_self_edit: deny
---

# 280. Channels

Define Psychevo's first-party messaging channel surface for WeChat, Telegram,
Feishu, and Lark.

Channels are profile-local connections that adapt remote chat messages into
Gateway source-scoped turns and deliver Gateway observations back to the
originating chat. Channels build on the Gateway IM adapter boundary instead of
owning thread, turn, permission, clarify, or transcript semantics directly.

## Scope

- channel profile configuration and secret-storage boundaries
- `pevo gateway setup` channel configuration surface
- channel lifecycle, enablement, doctor checks, and setup flows
- first-party adapter behavior for WeChat, Telegram, Feishu, and Lark
- Workbench Settings > Channels behavior

Out of scope:

- public internet relay, LAN exposure, cloud wake-up, hosted onboarding, or TLS
- WeCom, WeChat Official Account, Slack, Discord, or other channel families
- media/file attachment delivery beyond placeholder-safe text fallbacks
- media/file support, admin command tiers, and service-install automation beyond
  the first text adapters

## Channel Configuration

Channel configuration lives in the active profile config under `[channels]` and
`[[channels.connections]]`. A connection records:

- `id`: stable profile-local id
- `channel`: `wechat`, `telegram`, `feishu`, or `lark`
- `domain`: channel domain where needed, especially `feishu` or `lark`
- `enabled`: requested runtime enablement
- `label`: user-visible connection name
- `transport`: `polling`, `webhook`, or `long_connection`
- `workdir`, `model`, and `permission_mode`: runtime defaults for turns from
  this channel
- `require_mention`: group-chat gating
- credential, account, app, and base URL env names, never secret values
- direct-user and group/chat allowlists

Missing credentials or missing allowlists are blocking diagnostics. A
connection whose `enabled` flag is true still starts as blocked when these
checks fail. Config readiness and runtime liveness are separate states:
`ready` means local config/env checks passed, while `runner` reports whether a
Gateway-owned adapter loop is `running`, `stopped`, `blocked`, or `error`.
Runner diagnostics also expose a secret-free `reason` such as
`polling_empty`, `needs_qr_login`, or `blocked_allowlist`, the last healthy
poll timestamp, and the last non-secret channel API error code when available.
Channel adapters must fail closed instead of accepting messages from unknown
users, chats, groups, tenants, or topics.

Secret values must not be written to TOML, argv, JSON output, frontend storage,
or logs. Setup may write profile `.env` entries or owner-only channel state
under the active profile home. Secret display is always redacted after capture.

## CLI Surface

`pevo gateway setup` owns local channel setup. The default mode is an
interactive Gateway setup wizard showing existing channel state and offering to
start or restart the managed Gateway when setup finishes. Script mode uses
flags:

- `pevo gateway setup --channel <wechat|telegram|feishu|lark>`
- optional `--id`, `--label`, `--credential-env`, `--credential-stdin`,
  `--allow-user`, and `--allow-group`
- WeChat-specific QR setup via `--qr`, plus manual fallback flags
  `--account-id`, `--account-env`, and `--ilink-base-url`
- optional `--enable` or `--disable` to set requested enablement during setup
- optional `--start` or `--restart` to start/restart the managed Gateway after
  configuration
- optional `--json` for secret-free structured output

Gateway status exposes a compact channel summary for operations. Detailed
channel editing remains in `pevo gateway setup` and Workbench Settings.

Default setup checks are local and deterministic: config parseability,
credential presence, allowlist presence, selected transport, model/workdir
resolution, and Gateway/channel runner status. Real channel API checks remain
explicit opt-in only and are not part of default validation.

Setup commands must mirror provider auth ergonomics: env-var names are shown
and configurable, raw secret values are accepted only through hidden prompts or
stdin-style flows, and summaries are secret-free.

WeChat setup is QR-first. `pevo gateway setup --channel wechat --qr` requests an
iLink Bot QR code, displays a terminal QR with a URL fallback, polls login
status, refreshes expired QR codes, follows iLink redirect hosts, and writes the
confirmed bot token, bot account id, and iLink base URL to the active profile
`.env`. Generated TOML records `credential_env = "WECHAT_BOT_TOKEN"`,
`account_env = "WECHAT_ACCOUNT_ID"`, and `base_url_env =
"WECHAT_ILINK_BASE_URL"` by default. A QR `confirmed` response means iLink has
returned credentials; setup must persist the returned token, account id, and
base URL immediately. iLink `getupdates` probes are runtime and Doctor
diagnostics, not a pre-persistence gate. After fresh QR credentials are saved,
the WeChat runner enters a short `qr_login_pending` grace window before
`ret=-14` or `errcode=-14` session-timeout responses become final
`needs_qr_login` reconnect failures. QR reconnect upserts the existing WeChat
connection id instead of failing on duplicates; it updates credential/account
env names and secret values while preserving existing workdir, model,
permission, enablement, and allowlist settings unless iLink returns an explicit
new DM user id. The wizard then offers DM pairing: the user sends one direct
message to the connected iLink bot, setup captures the sender id from polling,
and the user can add that id to `allow_users`.

WeChat group configuration is advanced. The QR login identity is an iLink bot,
not the personal WeChat account used to scan the QR, and ordinary WeChat group
events may not be delivered by iLink. The default setup path prioritizes DMs;
group allowlists require an explicit warning and remain blocked if iLink never
emits group events.

## Gateway And Adapter Behavior

The Channel Gateway runner owns adapter lifecycle for configured connections
when the managed Gateway starts. It builds adapters only for enabled,
config-ready connections, records blocked diagnostics for everything else, and
keeps a secret-free status snapshot with last poll, last inbound, last
outbound, and last error timestamps/messages. It normalizes inbound messages
into Gateway sources with persistent lifetime and source identity containing
connection id, channel/domain, chat type, chat id, optional thread/topic id,
optional user/operator id, reply target, and redacted routing ids.

Inbound text maps to Gateway turn text input via the normal `Gateway::send_turn`
path. The final assistant answer is delivered back to the originating chat by
the same adapter. Channel context is included only as explicit model-visible
context when the adapter marks it safe and useful. Permission and clarify
replies are routed back through the originating channel source. Outbound
observations are chunked, rate-limited, and degraded to text when a channel
cannot render richer controls.

Channel defaults:

- Telegram uses Bot API polling by default. Webhook mode is allowed only with a
  configured webhook secret. Polling uses `getUpdates`; stale-webhook cleanup
  belongs to managed adapter startup.
- Feishu and Lark share one long-connection adapter family with domain-specific
  endpoints, app id/secret env setup, and SDK-backed WebSocket event delivery.
- WeChat uses Tencent iLink Bot API polling, text-only approval/clarify
  commands, `sendmessage`, owner-only persisted context tokens, and QR login
  setup through `get_bot_qrcode` / `get_qrcode_status`.

WeChat iLink `getupdates` has two timeout-like outcomes with different
semantics. A local HTTP/read timeout during long-polling is a healthy empty poll
and must keep the runner running. An iLink business response with `ret=-14` or
`errcode=-14` and messages such as `session timeout` usually means the QR login
session has expired. During the fresh-login grace window, the runner stays
running with reason `qr_login_pending` and retries polling; after that grace
expires, it becomes blocked with reason `needs_qr_login` until a fresh QR
reconnect succeeds.

## Workbench Settings

Workbench exposes Channels as a Settings subpage. The page uses the current
prototype direction:

- header actions: `Doctor`, `Add custom`, `Start`
- no top overview metrics
- no `All`, `Enabled`, or `Needs setup` filter tabs
- no right-side detail pane or drawer
- connected channels render as Agents-style rows with channel identity,
  connection label, status, credential state, allowlist state, runtime summary,
  Test, Settings, and an enable switch
- selecting a configured channel opens an independent settings page with Back,
  enable switch, Test, Save, and sectioned settings groups; the page must keep
  operational hierarchy compact with a header, status summary strip, doctor
  checks, and dense configuration groups instead of floating status pills or a
  sparse hero area
- Add channel remains inline below the connected list with WeChat, Telegram,
  Feishu, and Lark setup cards
- Doctor results open inline under the header/list area

Workbench WeChat setup is QR-first and can complete real local configuration.
The Gateway start action must preserve iLink QR display payloads correctly:
when `qrcode_img_content` is a `data:image/...` value, Workbench renders that
image directly; when it is a URL or plain scan payload, Gateway may generate a
display SVG from that payload. The internal `qrcode` token remains the poll key
only. Workbench shows a one-second countdown for QR expiry independent of the
poll interval, stops countdown/polling on connected or expired states, and
persists confirmed token/account/base URL only through profile `.env`.
If a QR poll session is missing, expired, completed, or lost across Gateway
restart, Workbench must clear the QR image, countdown, session id, and Check
status affordance. It must show a fresh Generate/Reconnect QR action rather
than leaving a stale scannable code on screen. Existing connected WeChat rows
whose runner reason is `needs_qr_login` must present reconnect as the primary
setup action instead of claiming the channel is connected. Existing WeChat rows
whose runner reason is `qr_login_pending` must present a neutral "polling is
starting" state: credentials have been saved, the Gateway is retrying iLink
polling, and the user should send a DM while the runner settles.

The enable switch reflects requested enablement. If production checks block
enablement, the UI must surface the blocking diagnostic and leave runtime
status blocked rather than pretending the adapter is active.

## Related Topics

- [021 Gateway](../021-gateway/spec.md) defines transport-neutral source,
  thread, turn, and interaction semantics.
- [057 Profiles](../057-profiles/spec.md) defines profile-local configuration
  and managed Gateway ownership.
- [200 pevo CLI](../200-pevo-cli/spec.md) defines command spelling.
- [240 pevo Web](../240-pevo-web/spec.md) defines Workbench product behavior.
- [060 Automation](../060-automation/spec.md) defines repo-local validation and
  live opt-in boundaries.
