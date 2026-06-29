---
name: 280. Channel UX Testing
psychevo_self_edit: deny
---

Channel UX validation uses deterministic local harnesses, fake adapters, and
isolated profile state. Real platform API checks are live opt-in only.

## Long-Term Acceptance Contract

- Channel setup and status UX is deterministic by default, secret-free, and
  isolated from real user profiles.
- Workbench Settings presents Channels as a compact, centered Settings surface
  with staged detail editing and no default exposure of internal credentials or
  runner internals.
- Workspace selection clearly configures only the default workspace for newly
  created channel threads.
- QR, reconnect, approval, Ask, command, and diagnostic fallbacks preserve the
  same meaning as other Gateway entrypoints while degrading to platform-safe
  text when needed.
- Real platform checks are live opt-in only.

## Current Implementation Slice

Current automation focuses on CLI setup/status, Workbench Channels Settings,
workspace picker behavior, QR/reconnect presentation, and deterministic
Playwright desktop/mobile coverage. Channel runtime invariants are owned by
[028 Channels](../028-channels/spec.md).

## Scenario Matrix

- `pevo gateway setup --channel <name>` works against temporary
  `PSYCHEVO_HOME`.
- `pevo channel ...` fails as an unknown top-level command.
- `pevo gateway status --json` includes configured, enabled, ready, blocked,
  and setup-needed channel counts.
- `--json` output is structured, stable, and secret-free.
- Missing credential and missing allowlist diagnostics are explicit.
- Settings > Channels has no top overview metrics, no filter tabs, and no
  right-side detail pane.
- Full-screen Settings keeps the local Settings nav anchored while the right
  configuration pane is centered in the available workspace. Mobile remains
  full-width.
- Wheel scrolling over right-pane blank gutters scrolls the centered Settings
  content.
- Connected channel rows show status, credential state, allowlist state,
  runtime summary, Test, Settings, and enable switches.
- Selecting a configured channel opens the independent settings page, and Back
  returns to the list.
- Channel detail renders editable staged controls for label, enablement,
  allowlists, group mention, model, cwd, and permission mode.
- Channel detail visual checks assert a single-column open section stack with
  row-style label/help copy on the left and controls on the right.
- Channel detail default-surface checks assert that internal WeChat env names
  such as `WECHAT_ACCOUNT_ID` and `WECHAT_ILINK_BASE_URL` are not rendered, and
  runner diagnostics are collapsed until Test or advanced details are opened.
- Save sends `channel/update`, updates the selected channel row, and clears the
  dirty state.
- Back with dirty edits does not silently lose changes.
- Saving a WeChat channel from the default UI does not send hidden `accountEnv`
  or `baseUrlEnv` fields.
- Model defaults, current custom values, unknown configured values, and
  profile-default blank selections remain selectable without dropping data.
- Allowlist input accepts comma and newline separated values and saves
  structured, de-duplicated arrays.
- Danger zone removal sends `channel/delete` only after confirmation and keeps
  saved secrets out of the UI.
- Enable switch state syncs between list and settings page and surfaces
  blocked diagnostics when production checks fail.
- Add channel setup cards switch content for WeChat, Telegram, Feishu, and
  Lark.
- Channel detail renders recent workspace options from session-browser
  workspace groups.
- Selecting a recent workspace and saving sends that `cwd` through
  `channel/update`.
- Selecting `Profile default` and saving sends a blank `cwd`.
- Manual path entry saves paths that are not present in recent workspace
  options.
- Changing a channel cwd does not present itself as migrating existing
  channel threads.
- QR setup renders direct QR images when provided and generated SVG fallback
  otherwise.
- QR setup updates the visible expiry countdown once per second while keeping
  status polling on the Gateway-provided interval.
- Workbench clears stale QR images, countdowns, session ids, and Check status
  controls when the Gateway reports a missing, expired, completed, or
  restart-lost session.
- Existing connections with a reconnect-required runner reason render a
  reconnect-first setup card instead of a connected card.
- Freshly confirmed connections render a neutral starting-polling card while
  the runner settles.
- Approval and Ask prompts render rich controls when supported and text
  fallback when unavailable.
- Text fallback prompts include bounded reply instructions and no raw secrets
  or raw internal ids.
- Slash command discovery is capability-filtered for channel entrypoints.
- Explicit unsupported slash commands return bounded guidance.
- Channel `/help` reflects the shared command catalog filtered for messaging
  capabilities, including dynamic skills where available.
- Channel `/agents` lists ordinary project Markdown agents that default to the
  `subagent` entrypoint, explains `@agent-name <task>`, and does not only show
  peer runtimes such as `opencode`.
- Peer-only agents, when present, are separated from callable agents and do not
  suppress the callable list.
- Channel skill and agent commands execute through the shared runtime path, not
  a channel-only prompt rewrite.
- Native IM image and file attachments are represented as validated Gateway
  input or bounded context; unsupported media produces a bounded explanation.
- Advanced diagnostics can show recent remote source lanes, local thread ids,
  cwds, and running/queued state without exposing raw secrets.
- Desktop and mobile Playwright checks assert no horizontal overflow.

## Validation Boundaries

- Narrow validation should run the closest touched tests first.
- CLI setup tests cover setup or status UX changes.
- Workbench unit and Playwright tests cover Settings UI changes.
- Generated protocol checks are required when Rust protocol schemas change.
- Before handoff, run `cargo xtask ci run --profile rust-broad` unless the
  change is documentation-only or a host prerequisite blocks it.
