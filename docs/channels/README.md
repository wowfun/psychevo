# Channels

Psychevo channels connect chat systems to the local Gateway. A channel is not a
separate bot process. The managed Gateway owns setup, status, adapter runtime,
access control, and optional start/restart.

Supported channels:

- WeChat through Tencent iLink Bot API polling
- Telegram through Bot API polling
- Feishu through Open Platform long connection
- Lark through Open Platform long connection

## Configure A Channel

Use `pevo gateway setup` from the profile you want to receive messages in:

```bash
printf '%s\n' "$TELEGRAM_BOT_TOKEN_VALUE" \
  | pevo gateway setup \
      --channel telegram \
      --id release \
      --label "Release Bot" \
      --allow-user 12345 \
      --credential-stdin \
      --enable \
      --json
```

The command writes channel configuration to the active profile `config.toml`.
Raw secrets are written only to the active profile `.env` when you use
`--credential-stdin`; TOML, JSON output, logs, and Workbench views show only env
var names and credential state.

The public TOML field is `channel`:

```toml
[[channels.connections]]
id = "release"
channel = "telegram"
enabled = true
label = "Release Bot"
transport = "polling"
credential_env = "TELEGRAM_BOT_TOKEN"
allow_users = ["12345"]
```

## Security Defaults

Channels fail closed by default. An enabled connection is still blocked until it
has both:

- a present credential env value
- at least one allowed user or group/chat id

Run status after setup:

```bash
pevo gateway status --json
```

The `channels` summary reports `configured`, `enabled`, `ready`, `blocked`, and
`setup_needed`.

## Start Or Restart Gateway

After setup, start the managed Gateway:

```bash
pevo gateway start
```

Or include `--start` or `--restart` in setup when you want configuration and
runtime lifecycle in one command.

## Workbench

Workbench exposes the same profile-local channels in Settings > Channels. Use it
to inspect rows, toggle enable switches, run Doctor checks, and open the
connection settings page.

## Live Checks

Default validation is local and deterministic. It checks config, env presence,
allowlists, and runtime shape without calling external chat APIs. Real channel
API checks require explicit live credentials and an opt-in validation path.

## References

- Telegram Bot API: https://core.telegram.org/bots/api
- Feishu long-connection callbacks: https://open.feishu.cn/document/ukTMukTMukTM/uYDNxYjL2QTM24iN0EjN/event-subscription-configure-/request-url-configuration-case
- Lark long-connection callbacks: https://open.larksuite.com/document/uAjLw4CM/ukTMukTMukTM/event-subscription-guide/callback-subscription/configure-callback-request-address
