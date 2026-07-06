---
name: 248. Voice ASR/TTS Testing
psychevo_self_edit: deny
---

# 248. Voice ASR/TTS Testing

Default validation is deterministic and local. Real provider checks are
opt-in.

## Deterministic Coverage

- Rust config tests cover `voice.asr`, `voice.tts`, `voice.realtime`, invalid
  formats, invalid transports, and raw secret rejection.
- Rust provider tests cover fake ASR, fake TTS, Xiaomi request body creation,
  Xiaomi ASR response parsing, and Xiaomi TTS SSE audio parsing.
- Gateway protocol/codegen tests verify voice methods and realtime
  notifications are exported to TypeScript and JSON schema.
- Gateway handler tests cover ASR/TTS success, missing config, input-size
  rejection, realtime unavailable, and fake realtime event fanout.
- Workbench/component tests cover dictation insertion, read-aloud action,
  auto-speak gating, realtime status transitions, microphone permission errors,
  and text fallback states.
- Channel tests cover `/voice on`, `/voice tts`, `/voice off`, and text
  fallback when voice delivery is unsupported.

## Opt-In Live Coverage

Xiaomi live voice validation is enabled only when both are present:

- `PSYCHEVO_LIVE_XIAOMI_VOICE=1`
- `XIAOMI_TOKEN_PLAN_API_KEY`

The opt-in live suite checks:

- ASR `wav`
- ASR `mp3`
- ASR streaming when the provider supports it
- TTS `wav`
- TTS `pcm16` streaming

OpenAI/provider-native realtime is not part of the default live gate. It may be
validated separately when realtime credentials and explicit user approval are
available.

## Required Broad Gates

For implementation work, run the closest changed-package tests first, then:

- `cargo xtask ci run --profile rust-broad`
- `cargo xtask ci run --profile visual`
- `cargo xtask live run --all --env shared`

For explicitly exhaustive live requests, also run the direct Workbench ACP
Playwright live spec with the xtask live context if it is not already proven by
the current live registry output.
