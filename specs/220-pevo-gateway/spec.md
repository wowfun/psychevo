---
name: 220. pevo Gateway
psychevo_self_edit: deny
---

# 220. pevo Gateway

Define the concrete `pevo gateway` product surface for managed local Gateway
lifecycle, browser launch bootstrap, and Web asset mounting.

## Scope

- `pevo gateway open/start/status/stop/restart` lifecycle behavior
- managed local server state and browser launch bootstrap
- static Web Shell asset mounting and launch authorization
- relationship between managed `pevo gateway`, `pevo web`, and foreground
  `pevo serve`

Out of scope:

- concrete Web Shell, Workbench layout, browser host, and frontend package
  behavior; these belong to [240 pevo Web](../240-pevo-web/spec.md)
- public LAN, relay, TLS, account, or hosted service behavior
- native desktop or mobile shell packaging
- provider secret storage in the browser, and arbitrary host-file editing
  outside the active project root
- headless API contract, which belongs to [221 pevo Serve](../221-pevo-serve/spec.md)

## Lifecycle

`pevo gateway` with no subcommand is equivalent to `pevo gateway open`.
Lifecycle commands emit exactly one JSON object to stdout so tests, desktop
shells, and automation can parse them without scraping human text.

`pevo web` is a top-level convenience alias for managed Web lifecycle commands.
With no subcommand it is equivalent to `pevo gateway open`, keeps the same
JSON-only stdout contract, and defaults to opening the current working
directory. `pevo web start [--bind <ADDR>]`, `pevo web stop`, and
`pevo web restart [--bind <ADDR>]` are aliases for the matching
`pevo gateway` lifecycle commands. `pevo web restart` stops the current
profile's managed server when one is running, then starts it; if no server is
running, it starts one. GUI or desktop-shell no-project entrypoints may request
the default workspace cwd instead of the launcher cwd.

Managed state lives under `$PSYCHEVO_HOME/gateway/`:

- `server.json`: non-secret instance id, pid, address, version, executable
  fingerprint, static asset directory, asset mode, and timestamps
- `token`: the managed server bearer token, owner-readable only
- `lock`: lifecycle transaction lock; mutating lifecycle commands hold it
  exclusively from the first state read through launch or shutdown completion,
  while `status` holds a shared lock
- `instance.lock`: the managed `serve` instance lease, held exclusively by the
  child from before binding until process exit
- `server.log`: appended stdout/stderr from the background server

The directory is owner-only. `server.json` must not contain the token.
`$PSYCHEVO_HOME` is the resolved active profile home from
[057 Profiles](../057-profiles/spec.md). One managed Gateway server belongs to
one active profile; lifecycle commands do not start, stop, or reuse managed
servers from other profiles. Resetting that profile's state with
`pevo init --reset-state` stops the profile-local managed server before the
SQLite state files are backed up and recreated.

`open` and `start` reuse the same server implementation as `pevo serve`.
Managed mode passes internal flags to mount Web Shell assets, generated token
state, and launch bootstrap state. The public `pevo serve` command remains
headless.

Managed `open`, `start`, and `restart` spawn the `serve` child as an
independent long-lived process. The child must keep running after the opener
command exits, so a ready `server.json` cannot immediately become stale because
the caller's shell, terminal, or test harness closed its process group.
If that child does not become ready, the invoking command exits non-zero and
writes a bounded excerpt of the stdout/stderr produced by that startup attempt
to terminal stderr together with the full `server.log` path. Because
`server.log` is append-only across launches, the excerpt must start at the
current attempt rather than replaying output from older managed servers. If no
new output can be read, the failure still reports the full log path.
Managed mode exposes a bearer-authenticated internal control plane that is not
registered by public `pevo serve`: identity returns the instance id, pid, and
version, while shutdown accepts the expected instance id and rejects a
mismatch without stopping the process. `open` validates that identity before
launching a workspace. A recoverable launch connect, timeout, or authentication
failure may trigger one ownership recheck, safe replacement, and one retry; it
must not enter an unbounded restart loop.

`stop` first requests authenticated managed shutdown and waits for the bounded
signal-aware cleanup before reporting success and removing managed state. That
cleanup gracefully shuts down the Agent Session Host and its resident ACP
process pool with a forced fallback, so managed Agent adapter children cannot
survive as orphans. If the managed child still
does not exit after the complete bounded cleanup window, `stop` forcibly
terminates that exact managed Unix process group or platform-equivalent process
tree, then reports an error if it cannot prove the managed pid exited. It never
uses a name- or command-pattern kill. The first `gateway stop` or `web stop`
invocation against an owned running server completes the shutdown and state
cleanup on every supported host; Windows callers must not need to repeat the
command after an access-denied wait on the verified Job Object.
When no `--bind` is provided, managed commands prefer `127.0.0.1:58080` and may
fall back through `127.0.0.1:58099` when a lower port is already in use. The
actual bound address is persisted in `server.json` and reported through
`baseUrl`/`readyzUrl`. An explicit nonzero `--bind` disables fallback and must
either reuse a matching managed server or start exactly on the requested
address. Port `0` requests an operating-system-assigned ephemeral port on the
specified interface; reuse compares the interface while accepting the persisted
nonzero assigned port.

Managed server reuse must prove that the running process is the same owned
instance, local build, and asset set that the caller would start now. Every new
managed state includes an `instanceId`; old state without one is stale and its
recorded pid is never terminated unless ownership can be proven independently.
`open` and `start` may reuse an existing server only when the instance lease is
held, the OS-specific process identity matches the recorded pid and instance,
the authenticated identity endpoint agrees, `server.json` includes an
executable fingerprint, that fingerprint matches the current `pevo` executable,
the running process executable is not a deleted Unix inode, and the recorded
static asset directory matches the directory resolved for the current command.
Default-bind callers may reuse only a server bound inside the managed fallback
range. Explicit-bind callers may reuse only a server whose recorded address
matches the requested address.
On Windows, ownership additionally requires a live process handle and membership
in the named Job Object `Local\\PsychevoGateway-<instanceId>`; the handle remains
open and wait-capable through inspection, shutdown, and termination so PID
reuse cannot redirect a signal and exit can be proven without a second
lifecycle command.
The managed child creates that Job Object, enables
`JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`, and assigns itself before binding. Failure
to create or join the Job is a startup failure, never a fallback to `taskkill`.
On Unix, ownership combines the lease, pid/process group, and executable
identity, and forced shutdown targets only the verified process group.

Token and `server.json` replacement is atomic. Old-style or invalid state,
missing leases, exited owned processes, build/static/bind mismatches, and
unhealthy identity endpoints are reported stale. Ownership that is proven but
stale may be stopped precisely and replaced. If the lease is held but ownership
cannot be tied to state and OS identity, or the OS denies inspection, lifecycle
commands fail closed: they preserve state and token, start no second server, and
send no signal. `gateway status` sets `running` only when OS ownership and
liveness are proven; an owned process with an unhealthy identity endpoint may
be both `running: true` and `stale: true`.

Lifecycle JSON for `open`, `start`, `status`, and `restart` includes
`instanceId`. Machine-readable stale reasons include `invalid_state`,
`missing_instance_id`, `instance_lease_missing`, `process_identity_mismatch`,
`process_identity_unavailable`, `gateway_identity_mismatch`, and
`gateway_identity_unavailable`, in addition to executable/static/bind and
`pid_not_running` reasons.

## Launch Bootstrap

`pevo gateway open -C <DIR>` (or `--cd <DIR>`) canonicalizes the cwd, ensures
the managed server is running, records a launch entry, and opens the browser unless
`--no-browser` is set. The removed `--dir` spelling is rejected for both
`gateway open` and its `pevo web` alias. `pevo gateway open --default-workspace` resolves the
configured workspace root, creates `<root>/general` on demand, and launches it
as an ordinary cwd. `--print-url` prints the one-time launch URL and expiry
metadata in the JSON response for Playwright and desktop shells.

The launch URL carries only opaque launch material. It must not contain the raw
absolute cwd. Launch entries are in-memory, single-use, and expire after 30
seconds. A successful launch sets an HttpOnly SameSite=Lax browser-session
cookie and redirects to a clean Web Shell URL. Reopening a consumed launch URL
with a valid browser-session cookie redirects to the clean shell. Reopening it
without a valid browser-session cookie returns a launch-expired diagnostic page
with the recovery command.

Creating or consuming a launch opportunistically removes other expired launch
entries from the in-memory map. This bounded housekeeping requires no periodic
task and does not add a TTL, logout flow, or background reaper to authenticated
browser sessions.

The managed cookie authorizes cwds granted by a launch/open flow in the
current server process, cwds created by browser workspace-management RPCs,
and cwds explicitly adopted from human-visible global session groups. A
browser session may adopt another cwd by resuming a stored session or by
starting a new draft from that cwd group in the Sessions browser, but it
may not request arbitrary cwds that have no visible stored session. Direct
Bearer API clients may request any local cwd accessible to the Psychevo
process.

Direct browser visits to the managed base URL without a valid browser-session
cookie are not authorized Web Shell launches. They should return a local
launch-required diagnostic page with the recovery command, rather than mounting
the Workbench SPA and letting it fail later with a generic WebSocket error.

## Web Asset Mounting

Managed mode mounts the Web Shell assets defined by
[240 pevo Web](../240-pevo-web/spec.md), but this topic owns only lifecycle,
launch, authorization, and static-asset serving concerns. The concrete
Workbench product surface, browser host behavior, source binding, panels,
commands, settings, files, and browser validation belong to `240`.

Static files are read without blocking the async server executor. Fingerprinted
files below `/assets/` return
`Cache-Control: public, max-age=31536000, immutable`; HTML, SPA fallbacks, and
non-fingerprinted files return `Cache-Control: no-store` so an updated managed
server cannot reuse a stale shell. The local server does not add on-the-fly
compression; Workbench's lazy initial graph and immutable repeat-load cache are
the primary startup controls.

Static request paths are resolved beneath the canonical static asset root.
Absolute paths, parent traversal, platform path prefixes, and existing files
whose canonical target escapes through a symlink or junction are rejected with
`404` before SPA fallback or file reads. This containment does not change the
authorization or cache behavior of valid assets and shell routes.

The Web/Gateway implementation follows the architecture large-file limit from
[001 Architecture](../001-architecture/spec.md). `server.rs` should remain a
thin router/facade over modules for managed binding, launch/auth/static assets,
RPC dispatch, scope/session/source resolution, settings/observability,
downloads, and JSON-RPC helpers.

## Validation

Managed Gateway validation is specified in [Testing](testing.md). Browser and
Workbench validation belongs to [240 pevo Web Testing](../240-pevo-web/testing.md).

## Attachments

- [Testing](testing.md) defines managed Gateway validation expectations.

## Related Topics

- [021 Gateway](../021-gateway/spec.md) defines source/thread/turn transport behavior.
- [022 UI](../022-ui/spec.md) defines shared UI foundation.
- [240 pevo Web](../240-pevo-web/spec.md) defines concrete Web Shell and
  Workbench behavior.
- [200 pevo CLI](../200-pevo-cli/spec.md) defines the `pevo` command product surface.
- [221 pevo Serve](../221-pevo-serve/spec.md) defines the headless API server.
