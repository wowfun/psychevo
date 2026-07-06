---
name: 281. WeChat Channel
psychevo_self_edit: deny
---

Define Psychevo's first-party WeChat channel behavior.

WeChat uses the Tencent iLink Bot API as a polling channel. The connection is
QR-first and DM-first. It follows the common channel model in
[028 Channels](../028-channels/spec.md) and the shared setup UX in
[280 Channel UX](../280-channel-ux/spec.md).

## Scope

- WeChat iLink credential and account setup
- QR login and reconnect behavior
- WeChat source identity and DM/group limitations
- iLink polling, outbound sendmessage, media transfer, and timeout
  classification
- WeChat capability map, slash-command behavior, and text fallback behavior
- WeChat-specific runner diagnostics

Out of scope:

- common channel configuration and thread invariants, owned by
  [028 Channels](../028-channels/spec.md)
- Workbench layout, staged save, and QR presentation details, owned by
  [280 Channel UX](../280-channel-ux/spec.md)
- Telegram, Feishu, and Lark platform behavior

## Connection Shape

A WeChat connection uses:

- `channel = "wechat"`
- default `domain = "wechat"`
- default `transport = "polling"`
- default credential env `WECHAT_BOT_TOKEN`
- default account env `WECHAT_ACCOUNT_ID`
- default base URL env `WECHAT_ILINK_BASE_URL`

The token, account id, and base URL are stored as profile `.env` values. TOML
stores env names only. `WECHAT_ACCOUNT_ID` and `WECHAT_ILINK_BASE_URL` are
internal connection fields and should not appear as default editable Workbench
settings.

WeChat readiness requires the bot token and account id. Base URL uses the
default iLink endpoint when the env value is absent and the adapter can do so
safely.

## QR Login

WeChat setup is QR-first. `pevo gateway setup --channel wechat --qr` requests
an iLink Bot QR code, displays a terminal QR with a URL fallback, polls login
status, refreshes expired QR codes, follows iLink redirect hosts, and writes
the confirmed bot token, bot account id, and iLink base URL to the active
profile `.env`.

A QR `confirmed` response means iLink returned credentials. Setup must persist
the returned token, account id, and base URL immediately. iLink `getupdates`
probes are runtime and Doctor diagnostics, not a pre-persistence gate.

QR reconnect upserts the existing WeChat connection id instead of failing on
duplicates. It updates credential, account, base URL env names and secret
values while preserving existing cwd, model, permission mode, requested
enablement, and allowlists unless iLink returns an explicit new DM user id.

Manual fallback setup may accept `--credential-stdin`, `--account-id`,
`--account-env`, and `--ilink-base-url`, but QR remains the primary product
path.

## Remote Source Identity

The QR login identity is an iLink bot, not the personal WeChat account used to
scan the QR. A WeChat remote source records the connection id, `wechat` domain,
chat type, chat id, sender/operator id when available, reply target, and
redacted iLink routing metadata.

The default setup path prioritizes direct messages. After QR login, setup may
offer DM pairing: the user sends one direct message to the connected iLink bot,
setup captures the sender id from polling, and the user can add that id to
`allow_users`.

Group configuration is advanced. Ordinary WeChat group events may not be
delivered by iLink. Group allowlists require an explicit warning and remain
blocked if iLink never emits group events.

## Capability Map

WeChat supports plain text inbound/outbound and text fallback controls. It does
not expose rich cards, buttons, message edits, or native thread controls in the
current product contract.

WeChat should accept image and file attachments when the iLink media contract is
known. The observed iLink item type mapping is:

- `1`: text, using `text_item.text`
- `2`: image, using `image_item.media`
- `3`: voice, using `voice_item.media`
- `4`: file, using `file_item.media` plus file metadata
- `5`: video, using `video_item.media`

Media transfer uses iLink CDN references. Inbound media contains encrypted CDN
download parameters and an AES key. Outbound media requires `getuploadurl`, CDN
upload, and the resulting encrypted download parameter in `sendmessage`.
Gateway and runtime must not see raw iLink CDN URLs or encrypted query params.

Media support has a stricter implementation gate than text:

- adapter code may depend only on item fields covered by fake iLink tests;
- CDN upload/download, encryption, filename, MIME, and size handling must live
  behind the WeChat adapter and shared attachment pipeline;
- when CDN transfer is not implemented or fails, the adapter must preserve the
  message as bounded attachment metadata rather than silently dropping it;
- live iLink checks validate the fake contract but are never the default test
  gate.

Voice and video are metadata-only unless iLink exposes them through the same
confirmed media path with no additional user-facing semantics. Shared ASR/TTS
policy may consume platform-provided voice transcripts or confirmed downloaded
media, but WeChat raw voice download/send stays out of scope until fake iLink
or live evidence confirms the media contract.

## Adapter Behavior

The WeChat adapter polls iLink `getupdates`, submits accepted inbound messages
as Gateway source-scoped turns, and sends final assistant output through iLink
`sendmessage`.

Approval and Ask requests use bounded text commands. Slash commands use the
shared command catalog filtered for messaging capabilities. Unsupported
commands return concise guidance instead of falling through as prompts.

When WeChat CDN transfer is implemented and enabled, inbound images are
downloaded, validated, cached, and passed as Gateway image input. Inbound
text-like files are downloaded, validated, cached, and passed as visible
bounded context. Until that transfer contract is covered by fake iLink and
fake CDN tests, image, file, voice, and video items are preserved as bounded
metadata instead of silently disappearing or leaking raw iLink CDN details.
Other files remain bounded metadata until Gateway/runtime has native file input
semantics.

Outbound attachment delivery is allowed only for explicit, validated file
references produced by Psychevo. The adapter must never send arbitrary model
text that merely looks like a local path.

The adapter may persist owner-only context tokens under the active profile home
when iLink requires them. Context tokens are secrets and follow the same
redaction rules as profile `.env` values.

## Timeout And Reconnect Diagnostics

WeChat iLink `getupdates` has two timeout-like outcomes with different
semantics:

- A local HTTP/read timeout during long polling is a healthy empty poll and
  keeps the runner running.
- An iLink business response with `ret=-14` or `errcode=-14` and messages such
  as `session timeout` usually means the QR login session expired.

After fresh QR credentials are saved, the runner enters a short
`qr_login_pending` grace window. During that window, `-14` session-timeout
responses keep the runner running with reason `qr_login_pending` while polling
retries. After the grace expires, the same response becomes a blocked runner
with reason `needs_qr_login` until QR reconnect succeeds.

Runner diagnostics expose only secret-free facts: runner state, reason, last
healthy poll, last inbound/outbound timestamps, and the last non-secret iLink
error code.

## Related Topics

- [028 Channels](../028-channels/spec.md) defines the shared channel model.
- [280 Channel UX](../280-channel-ux/spec.md) defines QR and reconnect UX.
- [021 Gateway](../021-gateway/spec.md) defines source, thread, and turn
  semantics.
- [248 Voice ASR/TTS](../248-voice-asr-tts/spec.md) defines shared ASR/TTS
  policy and WeChat fallback boundaries.

## Attachments

- [Testing](testing.md) defines WeChat adapter, setup, and UX validation
  scenarios.
