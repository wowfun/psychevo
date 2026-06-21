# WeChat Channel Setup

Psychevo connects WeChat through Tencent iLink Bot API. The adapter uses
`getupdates` for inbound polling and `sendmessage` for outbound text replies.
Media, voice, typing indicators, and service installation are not part of this
first setup path.

## Prerequisites

You need an iLink bot token for the WeChat account you want Psychevo to answer
from. Keep the raw token out of shell history when possible.

Decide which WeChat users or group chats are allowed to reach Psychevo before
enabling the channel. Missing allowlists block runtime delivery.

## Configure

Write the token to the active profile `.env` and add the channel config:

```bash
printf '%s\n' "$WECHAT_ILINK_TOKEN_VALUE" \
  | pevo gateway setup \
      --channel wechat \
      --id wechat \
      --label "WeChat" \
      --allow-user WECHAT_USER_ID \
      --credential-stdin \
      --enable \
      --json
```

For a group chat, use `--allow-group`:

```bash
printf '%s\n' "$WECHAT_ILINK_TOKEN_VALUE" \
  | pevo gateway setup \
      --channel wechat \
      --id team-wechat \
      --allow-group WECHAT_GROUP_OR_ROOM_ID \
      --credential-stdin \
      --enable
```

By default the credential env var is `WECHAT_BOT_TOKEN`. Override it when you
need a different name:

```bash
pevo gateway setup \
  --channel wechat \
  --id wechat \
  --credential-env PSYCHEVO_WECHAT_TOKEN \
  --allow-user WECHAT_USER_ID
```

## Expected Config

The generated profile TOML uses `channel`, not `platform`:

```toml
[[channels.connections]]
id = "wechat"
channel = "wechat"
domain = "wechat"
enabled = true
label = "WeChat"
transport = "polling"
require_mention = true
credential_env = "WECHAT_BOT_TOKEN"
allow_users = ["WECHAT_USER_ID"]
```

The profile `.env` contains the raw token:

```dotenv
WECHAT_BOT_TOKEN=...
```

## Verify

Check local readiness:

```bash
pevo gateway status --json
```

The channel is ready only when the credential env value is present and the
allowlist is configured. Start or restart the managed Gateway after setup:

```bash
pevo gateway restart
```

## Runtime Notes

- Inbound delivery uses iLink long polling.
- Outbound text replies use `sendmessage`.
- iLink context tokens are captured from inbound messages and reused for replies
  to the same peer when available.
- If a context token is missing or expired, outbound sends may fail until the
  peer sends a fresh message.
- Default Doctor checks do not contact WeChat; live API validation is opt-in.
