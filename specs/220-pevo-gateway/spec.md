---
name: 220. pevo Gateway
psychevo_self_edit: deny
---

# 220. pevo Gateway

Define the concrete `pevo gateway` product surface and managed Web Shell
behavior.

## Scope

- `pevo gateway open/start/status/stop/restart` lifecycle behavior
- managed local server state and browser launch bootstrap
- Web Shell layout, panels, source binding, and reconnection behavior
- browser/PWA first-slice behavior

Out of scope:

- public LAN, relay, TLS, account, or hosted service behavior
- native desktop or mobile shell packaging
- arbitrary config-file editing or provider secret storage in the browser
- headless API contract, which belongs to [221 pevo Serve](../221-pevo-serve/spec.md)

## Lifecycle

`pevo gateway` with no subcommand is equivalent to `pevo gateway open`.
Lifecycle commands emit exactly one JSON object to stdout so tests, desktop
shells, and automation can parse them without scraping human text.

Managed state lives under `$PSYCHEVO_HOME/gateway/`:

- `server.json`: non-secret pid, address, version, asset mode, and timestamps
- `token`: the managed server bearer token, owner-readable only
- `lock`: lifecycle mutual-exclusion lock
- `server.log`: appended stdout/stderr from the background server

The directory is owner-only. `server.json` must not contain the token.

`open` and `start` reuse the same server implementation as `pevo serve`.
Managed mode passes internal flags to mount Web Shell assets, generated token
state, and launch bootstrap state. The public `pevo serve` command remains
headless.

## Launch Bootstrap

`pevo gateway open --dir <dir>` canonicalizes the workdir, ensures the managed
server is running, records a launch entry, and opens the browser unless
`--no-browser` is set. `--print-url` prints the launch URL in the JSON response
for Playwright and desktop shells.

The launch URL carries only opaque launch material. It must not contain the raw
absolute workdir. Launch entries are in-memory, single-use, and expire after 30
seconds. A successful launch sets an HttpOnly SameSite=Lax browser-session
cookie and redirects to a clean Web Shell URL.

The managed cookie authorizes only workdirs that were granted by a launch/open
flow in the current server process. Direct Bearer API clients may request any
local workdir accessible to the Psychevo process.

## Web Shell

The first Web Shell is `apps/workbench`, served from a prebuilt
`apps/workbench/dist`. `pevo gateway open` does not run `pnpm` implicitly. If
assets are missing, lifecycle JSON reports the missing asset condition and the
build command to run.

The Web Shell source kind is `web`. Source identity is derived from source kind
plus canonical workdir unless the client provides an explicit `rawId`. Multiple
managed browser clients for the same workdir share one source/thread, active
queue, event stream, and control surface.

Startup and reconnect call source-default `thread/resume` with `params.scope`.
Gateway returns the current thread snapshot when a binding exists, or an empty
source snapshot before the first turn. The client treats Gateway snapshot data
as authoritative and does not infer active turns, queues, permissions, or
clarify requests from stale local state.

## Workbench Layout

The desktop layout is a dense three-column workbench:

- left: history list and session lifecycle actions
- center: transcript and composer
- right: status/queue, settings/auth/model, diff, and export/share panels

Narrow layouts keep transcript and composer as the primary surface and collapse
history and utility panels into bottom tabs or drawers. The UI should present
as an operational workbench, not a landing page.

First-slice panels include transcript, composer, history, status/queue,
settings/auth/model, diff placeholder, export/share, permission, and clarify.
Memory and resource surfaces are status-only in the first Web slice.

## Validation

Browser validation uses Playwright against the built Workbench served by
`pevo gateway open --no-browser --print-url`, with isolated config, SQLite
state, and workdir by default. It covers desktop and narrow viewport layout,
Gateway connection, source/thread startup, history management, composer
submission, permission/clarify surfaces, and download flows.

Live model validation is explicit opt-in. When enabled, Playwright uses the
configured live provider/model in an isolated workdir and must not print
tokens or secrets.

## Related Topics

- [021 Gateway](../021-gateway/spec.md) defines source/thread/turn transport behavior.
- [022 UI](../022-ui/spec.md) defines shared frontend package boundaries.
- [200 pevo CLI](../200-pevo-cli/spec.md) defines the `pevo` command product surface.
- [221 pevo Serve](../221-pevo-serve/spec.md) defines the headless API server.
