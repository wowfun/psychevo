---
name: 248. Voice ASR/TTS
psychevo_self_edit: deny
---

# 248. Voice ASR/TTS

Define Psychevo voice input, spoken output, and provider-native realtime
conversation behavior across Workbench, Gateway, runtime providers, and
messaging channels.

Voice is a shared capability layer, not one mode. The product exposes small,
reversible controls for dictation, read-aloud, auto-speak, and realtime
conversation while keeping provider choice and final transcript ownership in
the existing Gateway/runtime contracts.

## Scope

- ASR and TTS provider contracts, config, validation, and deterministic fakes
- Workbench dictation, per-message read-aloud, auto-speak, and realtime session
  controls
- Gateway JSON-RPC methods and live-only realtime notifications
- provider-native realtime lifecycle and transport bootstrap
- channel voice reply policy and text fallback behavior
- opt-in live validation for Xiaomi ASR/TTS

Out of scope:

- a separate Voice settings page
- native WeChat raw voice download or voice send until iLink media transfer is
  confirmed by fake CDN or live evidence
- default OpenAI realtime live validation
- persisting realtime audio frames or partial transcript deltas
- using frontend storage for provider secrets, raw audio, or synthesized audio

## Evidence Basis

Local Psychevo code currently has text, image, and context
`GatewayInputPart` values; it has no voice RPC family or audio input part.
WeChat currently preserves `voice_item.media` as bounded metadata and marks
voice download as not enabled. Existing provider configuration already treats
Xiaomi Token Plan as an OpenAI-compatible provider.

The local Hermes reference separates dictation, full voice conversation,
read-aloud, auto-speak, messaging `/voice on|tts|off`, and TTS fallback. The
useful Psychevo lesson is to keep ASR input, TTS output, and voice policy as
separate controls and runtime contracts.

The local Codex reference models provider-native voice as thread-scoped
realtime conversation with `thread/realtime/*` requests and live transcript,
SDP, output audio, error, and close notifications. The useful Psychevo lesson
is that realtime audio is a live transport over the same thread, while final
text is the durable transcript.

## Configuration

Voice configuration lives under Settings > Models and profile/project TOML.
There is no separate Voice page.

```toml
[voice.asr]
provider = "xiaomi-token-plan"
model = "mimo-v2.5-asr"
language = "auto"

[voice.tts]
provider = "xiaomi-token-plan"
model = "mimo-v2.5-tts"
voice = "mimo_default"
format = "wav"

# Absent by default. Users configure provider-native realtime explicitly.
[voice.realtime]
provider = "openai"
model = "gpt-realtime-2"
transport = "webrtc"
voice = "marin"
```

ASR and TTS default to Xiaomi Token Plan. Realtime has no default provider or
model and remains unavailable until configured. Raw API keys are never accepted
inside `voice.*`; voice provider credentials resolve through the same provider
environment rules as model providers.

Supported ASR input formats are `wav` and `mp3`. The Gateway rejects encoded
audio payloads over `10 MB` before provider dispatch. Supported first-slice
TTS synthesize formats are `wav` and `pcm16`; Workbench playback consumes
browser-playable data URLs for `wav` and may use `pcm16` only through a
streaming/realtime path that explicitly understands raw PCM.

## Provider Contracts

`psychevo-ai` owns ASR, TTS, and realtime provider traits:

- ASR receives validated encoded audio, format, optional language, provider,
  and model; it returns final transcript text plus bounded provider metadata.
- TTS receives text, voice, format, provider, and model; it returns encoded
  audio or a stream of audio chunks plus bounded provider metadata.
- Realtime starts one thread-scoped session, accepts audio/text/speech appends,
  emits normalized live events, and closes with a reason.

Provider errors are structured and bounded. Callers must fall back to text when
TTS or realtime delivery fails.

Xiaomi ASR/TTS reuse the OpenAI-compatible `chat/completions` endpoint shape:

- ASR sends `input_audio` and `asr_options.language` with model
  `mimo-v2.5-asr`.
- TTS sends assistant text plus `audio.format` and `audio.voice` with model
  `mimo-v2.5-tts`.
- Streaming TTS parses audio deltas from SSE without exposing raw provider
  error bodies beyond bounded diagnostics.

## Gateway RPC

Gateway exposes these JSON-RPC requests:

- `voice/asr/transcribe`
- `voice/tts/synthesize`
- `voice/policy/read`
- `voice/policy/update`
- `thread/realtime/start`
- `thread/realtime/appendAudio`
- `thread/realtime/appendText`
- `thread/realtime/appendSpeech`
- `thread/realtime/stop`
- `thread/realtime/listVoices`

Gateway emits these notifications:

- `thread/realtime/started`
- `thread/realtime/sdp`
- `thread/realtime/itemAdded`
- `thread/realtime/transcript/delta`
- `thread/realtime/transcript/done`
- `thread/realtime/outputAudio/delta`
- `thread/realtime/error`
- `thread/realtime/closed`

Realtime audio frames, SDP, provisional transcript deltas, and output audio are
live-only. The durable transcript records only final user and assistant text
through ordinary thread entries.

## Workbench UX

Workbench uses compact controls in the existing composer/transcript surfaces:

- mic dictation records one utterance, transcribes it, and inserts the final
  transcript into the composer draft; the button is inline with Send/Interrupt
  and shows listening state through button animation rather than a feedback
  popover. Successful insertion is silent; only errors and missing audio need
  composer feedback.
- read-aloud appears on assistant messages and synthesizes only that message
- auto-speak is a labelled switch in the composer `+` drawer, off by default,
  and speaks assistant replies only after a successful assistant final text
- realtime conversation is a labelled switch in the composer `+` drawer and is
  a distinct thread control with visible listening, speaking, error, and closed
  states

Controls do not explain implementation details in the UI. Errors state the
action that failed and the next available action. Browser microphone
permission, unsupported recording formats, missing voice config, and provider
failure are all user-visible but bounded.

Workbench must pause or cancel recording while local playback is active unless
the active realtime provider supports echo cancellation for that session. This
prevents microphone/speaker feedback loops.

## Channel Voice Policy

Channels support a shared voice policy:

- `off`: text-only replies
- `voice_only`: voice replies only after voice input
- `all`: voice replies for all assistant final replies

Messaging `/voice on` sets `voice_only`, `/voice tts` sets `all`, and
`/voice off` sets `off`. The policy belongs to the remote lane or local source
binding and must not alter the underlying thread transcript.

If a platform cannot deliver native voice, if TTS fails, or if a policy is
unsupported for that adapter, the channel sends text and records a bounded
diagnostic. Inbound platform voice uses platform-provided transcripts or
confirmed downloaded media for ASR; otherwise the adapter preserves bounded
metadata.

## Acceptance Criteria

- `voice.*` TOML parsing accepts the documented blocks, rejects raw keys,
  rejects unsupported formats/transports, and resolves provider credentials via
  existing provider env rules.
- Protocol generation exports all voice request, result, and notification
  types to TypeScript and JSON schema.
- `voice/asr/transcribe` validates input format and size before provider
  dispatch, and deterministic fake providers can transcribe without network.
- `voice/tts/synthesize` returns browser-playable `wav` data for deterministic
  fake providers and uses text fallback on provider failure.
- Realtime methods return bounded unavailable errors until `voice.realtime` is
  configured and can emit deterministic fake started/transcript/audio/closed
  events in tests.
- Workbench dictation inserts transcript into the composer draft without
  submitting it; read-aloud and auto-speak do not mutate transcript history.
- Realtime live notifications do not create durable transcript entries until a
  final transcript text is explicitly committed.
- Channel voice policy handles `/voice on`, `/voice tts`, and `/voice off`
  without forking the shared command, turn, or delivery pipelines.
- WeChat voice remains metadata-only unless the media contract is confirmed by
  fake fixtures or live evidence.

## Attachments

- [Testing](testing.md) defines deterministic and opt-in live validation.

## Related Topics

- [021 Gateway](../021-gateway/spec.md) defines source, thread, turn, and live
  projection ownership.
- [028 Channels](../028-channels/spec.md) defines shared channel delivery and
  attachment behavior.
- [125 Model Config](../125-model-config/spec.md) defines Settings > Models as
  the shared provider configuration surface.
- [240 pevo Web](../240-pevo-web/spec.md) defines concrete Workbench composer
  and transcript layout.
- [281 WeChat Channel](../281-wechat-channel/spec.md) defines the current
  iLink media and voice metadata boundary.
