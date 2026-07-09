---
name: 028. Channels
psychevo_self_edit: deny
---

Define the common model and shared contracts for Psychevo messaging channels.

Channels are user-facing entrypoints that run on external messaging platforms.
A channel connection adapts a remote chat lane into Gateway source-scoped turns
and delivers Gateway observations back to that lane. The channel layer does not
own thread, turn, permission, command, clarify, transcript, or runtime
execution semantics. It binds messaging platforms into those existing
contracts.

## Scope

- channel domain vocabulary and invariants
- profile-local channel connection configuration
- secret reference and redaction boundaries
- remote source identity and source-to-thread binding
- channel default runtime policy for new channel-created threads
- normalized inbound turn and outbound delivery contracts
- delivery capability, slash command, attachment, approval, and Ask boundaries
- channel-independent diagnostics and fail-closed behavior

Out of scope:

- concrete Workbench, CLI, and IM interaction UX, owned by
  [280 Channel UX](../280-channel-ux/spec.md)
- concrete WeChat behavior, owned by
  [281 WeChat Channel](../281-wechat-channel/spec.md)
- concrete Telegram behavior, owned by
  [282 Telegram Channel](../282-telegram-channel/spec.md)
- concrete Feishu and Lark behavior, owned by
  [283 Feishu / Lark Channel](../283-feishu-lark-channel/spec.md)
- Gateway thread, turn, source, and observation semantics, owned by
  [021 Gateway](../021-gateway/spec.md)
- public relay, LAN exposure, hosted onboarding, TLS, or service installation

## Conceptual Model

`ChannelConnection` is a profile-local platform connection and default runtime
policy. It records a stable connection id, channel kind, optional domain,
transport, label, requested enablement, credential references, allowlists, and
defaults such as cwd, Runtime Profile id, model, and permission mode.

`User entrypoint` is the product concept for a place where a user interacts
with Psychevo. CLI, TUI, Workbench, ACP, and each configured Channel are user
entrypoints. Code and protocols should prefer concrete Gateway vocabulary such
as turn, input, source, thread, and delivery instead of introducing
`Surface*` type names.

`RemoteSource` identifies the remote lane that produced a message. It includes
the platform, connection id, domain, chat type, chat id, optional user id,
optional thread or topic id, reply target, and redacted routing metadata. It
answers "which remote conversation is this?".

`ThreadBinding` maps a deterministic remote source key to a local Gateway
thread id. Resetting or starting a new conversation may rotate the local thread
while keeping the same remote source lane.

`Thread` is the local durable conversation. It owns the actual cwd,
transcript, runtime history, snapshots, and execution continuity. Channel
connections do not rewrite an existing thread's cwd or history.

`DeliveryCapabilities` describe what an entrypoint can render or accept,
including text, markdown, image and file attachments, voice, typing, edit
streaming, buttons, cards, native threads, message deletion, and length limits.
Capabilities are adapter facts, not product promises. Shared channel logic must
branch on capabilities instead of platform names whenever possible.

## Configuration

Channel configuration lives in the active profile config under `[channels]` and
`[[channels.connections]]`. A connection records:

- `id`: stable profile-local id
- `channel`: `wechat`, `telegram`, `feishu`, or `lark`
- `domain`: platform domain where needed
- `enabled`: requested runtime enablement
- `label`: user-visible connection name
- `transport`: `polling`, `webhook`, or `long_connection`
- `cwd`, `runtime_ref`, `model`, and `permission_mode`: defaults for new
  channel threads
- `require_mention`: group-chat gating
- credential, account, app, and base URL env names, never secret values
- direct-user and group/chat allowlists

Secret values must not be written to TOML, argv, JSON output, frontend storage,
transcripts, model-visible context, or logs. Setup flows may write profile
`.env` entries or owner-only channel state under the active profile home.
Secret display is always redacted after capture.

Missing credentials or missing allowlists are blocking diagnostics. A
connection whose `enabled` flag is true still starts as blocked when local
checks fail. Channel adapters fail closed instead of accepting messages from
unknown users, chats, groups, tenants, topics, or operators.

Config readiness and runner liveness are separate states. `ready` means local
config and env checks passed. Runner status reports whether a Gateway-owned
adapter loop is `running`, `stopped`, `blocked`, or `error`, with secret-free
reason strings and timestamps.

## Workspace And Thread Rules

A channel connection's `cwd` is the default workspace for new local threads
created from that connection. Blank cwd means the profile or Gateway default
cwd.

Changing a channel connection's cwd must not migrate existing local
threads. Existing threads keep their stored cwd and transcript. The product
may invalidate existing Channel source bindings for that connection so the next
ordinary inbound turn starts a fresh local thread in the new default cwd.
This is a binding rotation, not a thread migration.

This rule keeps the model stable:

- `ChannelConnection` answers "what defaults should new channel threads use?"
- `RemoteSource` answers "which remote conversation sent this?"
- `ThreadBinding` answers "which local thread continues this lane?"
- `Thread` answers "which cwd and history does this conversation own?"

## Inbound Turns

Platform adapters normalize inbound messages into Gateway source-scoped input.
The normalized input may include text, images, files, reply context, mentions,
and explicit model-visible context. Raw platform payloads, secrets, local
paths, and unsafe chat metadata are not model-visible by default.

Gateway owns the ingress/router boundary for channel input. The channel runtime
passes normalized input, source identity, connection defaults, delivery target,
and delivery capabilities to that boundary. The router handles shared control
flow such as slash commands, interrupt, new/reset conversation, permission
approvals, and Ask replies before constructing the lower-level runtime turn
request.

Channel sources use Gateway persistent source lifetime. Source keys are
deterministic for the remote lane and exclude raw local paths. Public or
diagnostic keys must not leak raw platform identifiers when a hashed or
redacted form is enough.

Inbound turns use the normal Gateway queue, steering, interrupt, permission,
clarify, and transcript projection paths. Channel-specific runtime code must
not create a second turn scheduler, command parser, approval path, or
permission system.

## Commands, Attachments, And Approvals

Channels use the shared command catalog from
[026 Commands](../026-commands/spec.md). Slash commands are not a terminal-only
feature. Each command declares surface capability requirements, availability
while a turn is active, permission level, and whether it is read-only or can
mutate local state. A channel advertises and executes only the commands it can
represent safely.

Channel agent discovery is shared agent discovery, not a channel-specific
registry. In messaging channels, `/agents` answers "which agents can this lane
call from the current workspace?" and therefore prioritizes `subagent`
entrypoints that can be invoked with `@agent-name <task>` during a normal
channel turn. Peer runtimes may be shown as secondary diagnostics, but they do
not replace callable agents unless the channel also has a peer-runtime
selection flow.

Channels use Runtime Profiles for runtime switching. `/profile` and its
subcommands select or inspect the source-bound Runtime Profile for future turns
on the lane. Destructive native runtime session actions remain outside Channel
commands because Channels cannot consistently provide confirmation UI.

Attachment handling is a shared pipeline. For media kinds with a confirmed
transfer contract, the adapter downloads platform media, checks size and MIME
constraints, stores it under a session or workspace attachment cache, and
passes normalized attachment references to Gateway and runtime. When transfer
is unavailable, unconfirmed, or fails validation, the adapter emits bounded
attachment metadata instead of dropping the message or exposing raw platform
URLs. Runtime code should not consume platform URLs directly.

The first shared attachment contract maps images to Gateway image input and
maps files to validated model-visible context or bounded metadata. A future
native file input part may replace that fallback, but channels must not invent
platform-specific runtime file semantics.

Permission approvals and Ask requests route through the originating surface.
When a platform cannot render buttons or cards, the channel degrades to bounded
text prompts with explicit reply commands. The approval meaning stays the same
as other user surfaces.

## Outbound Delivery

Gateway observations are delivered back to the originating remote lane through
the same adapter. Adapters may render text, markdown, progress, cards, buttons,
files, or platform-native threads according to their `DeliveryCapabilities`.

Outbound delivery must chunk, rate-limit, and degrade when the platform cannot
render the richer shape. Delivery failures produce secret-free diagnostics and
must not alter the underlying local thread transcript.

Channel voice reply policy is shared across adapters and defined by
[248 Voice ASR/TTS](../248-voice-asr-tts/spec.md). `/voice on` means spoken
replies only after voice input, `/voice tts` means spoken replies for all final
assistant replies, and `/voice off` means text-only. If TTS or native voice
delivery is unavailable, channels fall back to text with bounded diagnostics.

## Acceptance Criteria

- Channel config parsing rejects unknown channels, duplicate ids, invalid
  transports, invalid env names, and malformed allowlists with local,
  secret-free errors.
- Channel update writes only intended TOML fields, normalizes blank runtime
  defaults, validates env names, preserves raw `.env` secrets, and returns
  secret-free views.
- Channel delete removes only the configured connection and does not clear
  profile `.env` values.
- Source-key hashing is deterministic for the remote lane and excludes raw
  local paths.
- A channel connection cwd is used only when creating a new local thread.
  An already-bound remote source must execute against the bound thread's stored
  cwd until that binding is explicitly reset, rotated, or rebound.
- Reset or new-thread flows can bind the same remote source lane to a new local
  thread according to Gateway session rules.
- Updating a channel connection cwd can rotate that connection's existing
  Channel source bindings for the next ordinary inbound turn without
  interrupting currently running work.
- Channel input goes through the shared Gateway ingress/router path before
  runtime execution, so slash commands, interrupts, permission approvals, Ask
  replies, source ordering, queueing, and transcript projection do not fork
  into channel-only implementations.
- Channel `/agents` lists workspace-discovered callable subagents by default
  and does not hide ordinary project Markdown agents behind peer-runtime
  filtering.
- Downloaded attachment bytes pass through a shared validation and cache
  pipeline before reaching Gateway/runtime; unsupported or unconfirmed media
  becomes bounded metadata; runtime code does not consume raw platform URLs.
- Diagnostics, JSON views, logs, transcripts, model-visible context, and
  frontend state remain secret-free.

## Related Topics

- [020 Interfaces](../020-interfaces/spec.md) defines caller-facing interface
  semantics.
- [021 Gateway](../021-gateway/spec.md) defines source, thread, turn, queue,
  and observation semantics.
- [026 Commands](../026-commands/spec.md) defines shared slash command
  contracts.
- [041 Permissions](../041-permissions/spec.md) defines permission and approval
  policy.
- [057 Profiles](../057-profiles/spec.md) defines profile-local configuration
  and secret storage boundaries.
- [052 Agent Runtimes](../052-agent-runtimes/spec.md) defines Runtime Profiles
  and Channel `/profile` commands.
- [280 Channel UX](../280-channel-ux/spec.md) defines user-facing setup and
  operation.
- [248 Voice ASR/TTS](../248-voice-asr-tts/spec.md) defines shared voice
  policy and fallback behavior.
