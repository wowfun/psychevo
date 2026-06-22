---
name: 280. Channel UX
psychevo_self_edit: deny
---

Define the user-facing setup and operation experience for Psychevo Channels.

Channels are defined by [028 Channels](../028-channels/spec.md) as messaging
user surfaces. This topic owns how users configure, inspect, test, reconnect,
and operate those surfaces from CLI setup, Workbench Settings, and IM fallback
flows.

## Scope

- `pevo gateway setup` channel setup experience
- Gateway status and Doctor presentation for channels
- Workbench Settings > Channels list, detail, staged save, and deletion UX
- workspace picker behavior in Channel Settings
- QR setup and reconnect presentation
- user-facing approval, Ask, slash command, and diagnostic fallback behavior on
  IM surfaces

Out of scope:

- channel domain model and runtime invariants, owned by
  [028 Channels](../028-channels/spec.md)
- concrete WeChat adapter and auth behavior, owned by
  [281 WeChat Channel](../281-wechat-channel/spec.md)
- concrete Telegram adapter and auth behavior, owned by
  [282 Telegram Channel](../282-telegram-channel/spec.md)
- concrete Feishu and Lark adapter behavior, owned by
  [283 Feishu / Lark Channel](../283-feishu-lark-channel/spec.md)
- Gateway source, thread, turn, and observation semantics, owned by
  [021 Gateway](../021-gateway/spec.md)

## CLI Setup

`pevo gateway setup` owns local channel setup. The default mode is an
interactive Gateway setup wizard that shows existing channel state and offers
to start or restart the managed Gateway when setup finishes.

Script mode uses:

- `pevo gateway setup --channel <wechat|telegram|feishu|lark>`
- optional `--id`, `--label`, `--credential-env`, `--credential-stdin`,
  `--allow-user`, and `--allow-group`
- platform-specific flags such as WeChat QR setup and Feishu/Lark app id envs
- optional `--enable` or `--disable`
- optional `--start` or `--restart`
- optional `--json` for secret-free structured output

Default setup checks are local and deterministic: config parseability,
credential presence, allowlist presence, selected transport, model/workdir
resolution, and Gateway/channel runner status. Real platform API checks remain
explicit live opt-in checks.

Setup commands mirror provider auth ergonomics. Env-var names are shown and
configurable. Raw secret values are accepted only through hidden prompts,
stdin-style flows, QR setup, or platform-specific reconnect flows. Summaries
and JSON output are secret-free.

Gateway status exposes a compact channel summary for operations. Detailed
channel editing remains in `pevo gateway setup` and Workbench Settings.

## Workbench Settings

Workbench exposes Channels as a Settings subpage.

- Full-screen Settings keeps the local Settings nav anchored while centering
  the right-side configuration content within the remaining workspace.
- The right pane owns scrolling, so blank gutters around the centered content
  scroll the same Settings surface.
- Mobile keeps content full-width without artificial centering gutters.
- Header actions are `Doctor`, `Add custom`, and `Start`.
- The page has no top overview metrics, no `All` / `Enabled` / `Needs setup`
  filter tabs, and no right-side detail pane or drawer.
- Connected channels render as Agents-style rows with channel identity,
  connection label, status, credential state, allowlist state, runtime summary,
  Test, Settings, and an enable switch.
- Add channel stays inline below the connected list with WeChat, Telegram,
  Feishu, and Lark setup cards.
- Doctor results open inline under the header/list area.

Selecting a configured channel opens an independent settings page with Back,
sectioned settings groups, and a compact header action cluster. The detail
header shows Save and, only while dirty, Cancel. Detail edits are staged until
Save so users can adjust runtime defaults and allowlists together. List-row
enable switches and Test actions remain on the connected channel rows through
`channel/enable` and `channel/doctor`; detail-page editing does not duplicate
those operational controls.

The channel detail page exposes behavioral configuration, not internal
machinery:

- label
- requested enablement
- allow users
- allow groups
- group-mention requirement
- model
- workdir
- permission mode

The page keeps operational hierarchy compact with a header, one concise health
summary row, and editable configuration groups. Doctor checks and runner
internals are diagnostics. They stay hidden until Test runs or the user opens
advanced details.

Detail configuration groups use a single-column open section stack. Each
section uses form rows with label/help copy on the left and controls on the
right. The section grid must not split into two columns on desktop.

Back returns to the list when clean. If a draft has unsaved changes, Back must
either keep the user on the detail page or discard only the local draft after
explicit confirmation.

The detail header owns Save and dirty Cancel. Dirty-state notices must not
duplicate Save or Cancel; a clean header is preferred over an extra alert row.

## Workspace Picker

Channel detail workspace selection uses Workbench's existing session-browser
workspace groups from `thread/browser`.

The picker offers:

- `Profile default`, saved as a blank workdir
- recent workdirs from stored human-visible sessions
- manual path entry for paths not present in recent sessions

The picker is not a full `workspaces.root` directory listing and does not
promise to show empty workspaces with no session history.

Saving a changed channel workdir starts fresh channel threads on the next
ordinary IM message for that connection. Current running work is not
interrupted, and existing threads keep the workdir and history they already
own, as defined by [028 Channels](../028-channels/spec.md).

## Credentials And Deletion

Channel detail credentials never accept or display raw secret values. Secret
capture stays in QR setup, hidden CLI prompts, stdin-style setup flows, or
platform-specific reconnect flows.

Default Channel detail must not display internal WeChat env names such as
`WECHAT_ACCOUNT_ID` or `WECHAT_ILINK_BASE_URL`. QR setup and reconnect remain
the user-facing paths for managing those values.

Workbench channel configuration writes use existing RPCs:

- `channel/update` updates one existing connection and returns the refreshed
  `ChannelConfigView`.
- `channel/delete` removes one existing connection and returns the refreshed
  `ChannelListResult`.

Workbench may omit advanced env-name fields from `channel/update` when they
are not visible in the UI. Omitted fields preserve existing TOML values. Blank
`workdir`, `model`, and `permissionMode` mean profile defaults. Blank
credential env fields normalize back to the platform default env name rather
than removing the credential boundary.

Allowlist text edited in Workbench is normalized into structured string arrays
before RPC submission, trimming empty entries and de-duplicating while
preserving first occurrence order.

The Danger zone may remove the TOML channel connection after confirmation, but
it must not clear profile `.env` secret values.

## QR And Reconnect UX

QR setup and reconnect are platform-owned flows, but the product UX is shared:

- QR images, fallback URLs, expiry, polling state, and confirmation state are
  presented as setup progress, not as raw adapter internals.
- If a QR poll session is missing, expired, completed, or lost across Gateway
  restart, Workbench clears the QR image, countdown, session id, and Check
  status affordance.
- Existing rows with a runner reason that requires reconnect present reconnect
  as the primary action instead of claiming the channel is connected.
- Freshly confirmed credentials may show a neutral starting state while the
  runner settles and the user sends the first DM or test message.

The enable switch reflects requested enablement. If production checks block
enablement, the UI surfaces the blocking diagnostic and leaves runtime status
blocked instead of presenting the adapter as active.

## IM Surface Fallbacks

Channels should feel like real user surfaces, not message pipes. IM users can
submit text, answer approvals and Ask requests, use supported slash commands,
and receive progress or final answers according to platform capabilities.

When the platform cannot render richer controls, the channel degrades to
bounded text instructions. Fallbacks must name the available reply commands,
avoid exposing raw internal ids, and keep permission meaning identical to other
surfaces.

Command discovery is capability-filtered. Unsupported commands return short
guidance when typed explicitly.

## Related Topics

- [028 Channels](../028-channels/spec.md) defines the common model and shared
  contracts.
- [021 Gateway](../021-gateway/spec.md) defines transport-neutral source,
  thread, turn, and interaction semantics.
- [200 pevo CLI](../200-pevo-cli/spec.md) defines command spelling.
- [240 pevo Web](../240-pevo-web/spec.md) defines Workbench product behavior.
- [060 Automation](../060-automation/spec.md) defines repo-local validation and
  live opt-in boundaries.

## Attachments

- [Testing](testing.md) defines the Channel UX acceptance and validation
  scenarios.
