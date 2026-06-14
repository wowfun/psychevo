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
- provider secret storage in the browser, and arbitrary host-file editing
  outside the active project root
- headless API contract, which belongs to [221 pevo Serve](../221-pevo-serve/spec.md)

## Lifecycle

`pevo gateway` with no subcommand is equivalent to `pevo gateway open`.
Lifecycle commands emit exactly one JSON object to stdout so tests, desktop
shells, and automation can parse them without scraping human text.

`pevo web` is a top-level convenience alias for `pevo gateway open`. It keeps
the same JSON-only stdout contract and defaults to opening the current working
directory. GUI or desktop-shell no-project entrypoints may request the default
workspace workdir instead of the launcher cwd.

Managed state lives under `$PSYCHEVO_HOME/gateway/`:

- `server.json`: non-secret pid, address, version, executable fingerprint,
  static asset directory, asset mode, and timestamps
- `token`: the managed server bearer token, owner-readable only
- `lock`: lifecycle mutual-exclusion lock
- `server.log`: appended stdout/stderr from the background server

The directory is owner-only. `server.json` must not contain the token.
`$PSYCHEVO_HOME` is the resolved active profile home from
[057 Profiles](../057-profiles/spec.md). One managed Gateway server belongs to
one active profile; lifecycle commands do not start, stop, or reuse managed
servers from other profiles.

`open` and `start` reuse the same server implementation as `pevo serve`.
Managed mode passes internal flags to mount Web Shell assets, generated token
state, and launch bootstrap state. The public `pevo serve` command remains
headless.

Managed `open`, `start`, and `restart` spawn the `serve` child as an
independent long-lived process. The child must keep running after the opener
command exits, so a ready `server.json` cannot immediately become stale because
the caller's shell, terminal, or test harness closed its process group.
When no `--bind` is provided, managed commands prefer `127.0.0.1:58080` and may
fall back through `127.0.0.1:58099` when a lower port is already in use. The
actual bound address is persisted in `server.json` and reported through
`baseUrl`/`readyzUrl`. An explicit `--bind` disables fallback and must either
reuse a matching managed server or start exactly on the requested address.

Managed server reuse must prove that the running process is the same local
build and asset set that the caller would start now. `open` and `start` may
reuse an existing server only when the pid is alive, `server.json` includes an
executable fingerprint, that fingerprint matches the current `pevo` executable,
the running process executable is not a deleted Unix inode, and the recorded
static asset directory matches the directory resolved for the current command.
Default-bind callers may reuse only a server bound inside the managed fallback
range. Explicit-bind callers may reuse only a server whose recorded address
matches the requested address.
Old-style `server.json` files without those fields are stale. A stale managed
server is stopped, its token/state are rotated, and a new `serve` child is
started. `gateway status` reports stale managed state with `stale: true` and a
machine-readable `staleReason` instead of reporting a live pid as healthy only
because it still exists.

## Launch Bootstrap

`pevo gateway open --dir <dir>` canonicalizes the workdir, ensures the managed
server is running, records a launch entry, and opens the browser unless
`--no-browser` is set. `pevo gateway open --default-workspace` resolves the
configured workspace root, creates `<root>/general` on demand, and launches it
as an ordinary workdir. `--print-url` prints the one-time launch URL and expiry
metadata in the JSON response for Playwright and desktop shells.

The launch URL carries only opaque launch material. It must not contain the raw
absolute workdir. Launch entries are in-memory, single-use, and expire after 30
seconds. A successful launch sets an HttpOnly SameSite=Lax browser-session
cookie and redirects to a clean Web Shell URL. Reopening a consumed launch URL
with a valid browser-session cookie redirects to the clean shell. Reopening it
without a valid browser-session cookie returns a launch-expired diagnostic page
with the recovery command.

The managed cookie authorizes workdirs granted by a launch/open flow in the
current server process, workdirs created by browser workspace-management RPCs,
and workdirs explicitly adopted from human-visible global session groups. A
browser session may adopt another workdir by resuming a stored session or by
starting a new draft from that workdir group in the Sessions browser, but it
may not request arbitrary workdirs that have no visible stored session. Direct
Bearer API clients may request any local workdir accessible to the Psychevo
process.

Direct browser visits to the managed base URL without a valid browser-session
cookie are not authorized Web Shell launches. They should return a local
launch-required diagnostic page with the recovery command, rather than mounting
the Workbench SPA and letting it fail later with a generic WebSocket error.

## Web Shell

The concrete Web Shell behavior is specified in [Web Shell](web-shell.md). The
`spec.md` entrypoint keeps the product boundary and lifecycle contract, while
the attachment owns the longer app behavior details for Workbench, runtime
controls, settings, files, status, commands, and browser host interactions.

## Workbench Layout

Workbench layout, navigation, inspector, file review, terminal, settings, and
responsive shell behavior are specified in [Workbench Layout](workbench-layout.md).
The split keeps the concrete UI surface maintainable without changing the
Gateway lifecycle or transport contract.

The Web/Gateway implementation follows the architecture large-file limit from
[001 Architecture](../001-architecture/spec.md). `server.rs` should remain a
thin router/facade over modules for managed binding, launch/auth/static assets,
RPC dispatch, scope/session/source resolution, settings/observability,
downloads, and JSON-RPC helpers. Workbench app entrypoints should likewise be
composition roots over state, session, command, composer, runtime, settings,
and right-workspace modules rather than owning those domains inline.

## Validation

Browser validation uses Playwright against the built Workbench served by
`pevo gateway open --no-browser --print-url`, with isolated config, SQLite
state, and workdir by default. It covers desktop and narrow viewport layout,
Gateway connection, source/thread startup, history management, composer
submission, permission/clarify surfaces, and download flows.

Live model validation is explicit opt-in. When enabled, Playwright uses the
configured live provider/model in an isolated workdir and must not print
tokens or secrets.

Live skill validation is a separate opt-in Playwright path. The reusable
`live-skill` spec runs a configured skill prompt, samples the browser every
three seconds, writes screenshots as test artifacts, and compares rendered DOM
order against the isolated SQLite message-derived transcript. It waits for the
accessible Transcript region and `Ask Psychevo...` composer before submitting,
not legacy status-only chrome. Each screenshot sample prints its sample number,
label, and artifact path to stdout so long live runs expose visible progress.
The sampled transcript rows also print their nonvisual entry id, block id, block
kind, turn id, status, and visible text so a failed screenshot can be tied back
to Gateway projection shape. It must fail immediately if the Workbench render
error boundary is visible, and must fail on stale running reasoning rows that
duplicate committed reasoning, non-monotonic committed row order in the DOM,
tool result JSON in collapsed headers, or evidence header overflow. It must also
fail when an empty assistant update appears after a tool row or when a stale
completion popover remains visible after prompt submission. The default prompt
is `$x-daily`; callers may override the workdir, prompt, interval, timeout, and
model through environment variables.

## Attachments

- [Web Shell](web-shell.md)
- [Workbench Layout](workbench-layout.md)
- [Testing](testing.md) defines managed Gateway and Workbench validation expectations.

## Related Topics

- [021 Gateway](../021-gateway/spec.md) defines source/thread/turn transport behavior.
- [022 UI](../022-ui/spec.md) defines shared frontend package boundaries.
- [200 pevo CLI](../200-pevo-cli/spec.md) defines the `pevo` command product surface.
- [221 pevo Serve](../221-pevo-serve/spec.md) defines the headless API server.
