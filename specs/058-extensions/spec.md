---
name: 058. Extensions
psychevo_self_edit: deny
---

Define Psychevo's executable Extension package, host protocol, lifecycle,
installation, and product-surface contribution contract.

An Extension is executable capability for Psychevo hosts. A Plugin is a
declarative distribution package. They are separate user concepts with
separate policy and trust, even when one Extension distribution also carries a
Plugin manifest.

## Scope

- Extension identity, manifest, install scopes, and sources
- precompiled target artifact acquisition and local in-place development
- the versioned stdio host protocol
- lazy sidecar ownership and leases
- direct `pevo <command>` registration
- native host-rendered and MCP Apps UI contributions
- first-party Channel Extension packaging
- Extension management command semantics

Out of scope:

- declarative Plugin package and marketplace policy, owned by
  [054 Plugins](../054-plugins/spec.md)
- owning semantics for tools, hooks, MCP, skills, agents, commands, Channels,
  permissions, or durable Thread state
- an in-process Rust, JavaScript, Python, or frontend module ABI
- compiling remotely acquired source or running package-manager lifecycle
  scripts
- a general mutable runtime registry

## Extension And Plugin Boundary

An Extension is a directory with one root `psychevo.extension.json` and one
executable sidecar selected for the current target. Its executable capability
is available only through the host-owned protocol in this spec. Installing an
Extension immediately enables and trusts that exact installed Extension
fingerprint in the selected scope; every effect still passes through its owning
permission and runtime policy.

A Plugin is a manifest-first declarative package distributed through a
marketplace. Adding a Plugin materializes it disabled. Plugin enablement and
trust remain explicit and independent of Extension policy.

An Extension root may contain at most one recognized co-root Plugin base
manifest and its optional Psychevo companion overlay. The Extension installer
reports that Plugin but neither adds nor enables it implicitly. A Plugin root
must not contain `psychevo.extension.json`; Plugin installation fails closed
with guidance to use `pevo install` when it does. This one-way containment
prevents a declarative Plugin from gaining executable authority through an
unexpected nested package.

Extension installation never grants hook trust, MCP approval, provider
credentials, durable permissions, or arbitrary filesystem access. Plugin and
Extension fingerprints, enablement, and trust records never imply one another.

## Manifest

`psychevo.extension.json` is the only recognized Extension manifest path. It
is strict, versioned JSON with these required fields:

- `schemaVersion`: integer `1`
- `id`: stable reverse-domain or first-party identifier
- `version`: semantic version for materialized releases, or `local` for an
  in-place development root
- `runtime.protocol`: exactly `psychevo-extension/1`
- `runtime.executable`: an explicit package-relative executable path

It may declare `displayName`, `description`, `homepage`, literal
`runtime.args`, and static `contributions` descriptors. Package-relative paths
must start with `./`, contain no `..`, and remain within the Extension root.
Arguments are literal argv and never pass through a shell.

Static contribution descriptors let the host discover and conflict-check an
Extension without starting it. Supported descriptor families are:

- `commands`: direct CLI/TUI/GUI command name, usage, summary, argument shape,
  surface hints, and required capabilities
- `channels`: Channel kinds and delivery capabilities
- `displays`: native structured-display schemas and fallback text behavior
- `mcpApps`: MCP resource URI, presentation metadata, and fallback behavior
- `tools` and `hooks`: identities and owning-surface metadata only; executable
  bindings are confirmed by `contributions/list`

Unknown manifest fields are retained for inspection and reported as
unsupported. A malformed recognized manifest fails closed. The manifest does
not contain shell snippets, host callbacks, permission grants, embedded source
compilation instructions, or frontend module imports.

Every direct command has one first path segment matching
`[a-z][a-z0-9-]*`. Built-in `pevo` commands and options always win. Two enabled
Extensions that claim the same direct command make the later activation fail
with both source identities; installation order never silently selects a
winner. Subcommands below an Extension-owned first segment are routed as raw
arguments to the same owner.

## Sources And Materialization

`pevo install <source>` accepts exactly these source classes:

- a first-party Extension id from Psychevo's compiled release index
- an HTTPS release descriptor URL
- a local directory containing `psychevo.extension.json`

A remote release descriptor identifies the Extension, version, and one or more
target artifacts. Each target record carries a Rust target triple, HTTPS
archive URL, SHA-256 digest, archive format, executable path, and optional
size. The installer selects the current target before download, rejects an
absent target, downloads into a bounded staging root, verifies SHA-256 before
extraction, applies the shared bounded archive/path policy, validates the
materialized manifest and executable, and atomically replaces the selected
install. The initial install is enabled; replacing an existing record preserves
that record's enabled state throughout conflict validation and publication.
Failure leaves the previous install and policy intact.

Remote sources are precompiled artifacts. Psychevo does not run Cargo, npm,
Python package installation, build scripts, or lifecycle hooks for them. An
HTTPS descriptor or artifact redirect must remain HTTPS at every hop; checking
only the final response URL does not protect an HTTPS-to-HTTP-to-HTTPS chain.
Source identity and diagnostics omit credentials and sensitive query values.

A local directory is an in-place development source. It is not copied, and its
manifest declares `version = "local"`. `pevo -e <local-path> [command...]`
loads it temporarily for one invocation without writing installation or policy
state. A temporary Extension participates in the same manifest validation,
command conflicts, host protocol, permission, and shutdown rules as an
installed one. With a trailing direct command the invocation is one-shot.
Without a trailing command, `-e` opens the TUI (including its deterministic
line-oriented mode) and registers TUI-capable commands as `/name`; the
sidecar still starts only when that command is invoked and is shut down when
the TUI exits.

Profile Extensions live under:

```text
$PSYCHEVO_HOME/extensions/{cache,data}
```

Project-local Extensions live under:

```text
<cwd>/.psychevo/extensions/{cache,data}
```

Remote packages are immutable cache content. An Extension receives only its
identity-qualified data root as writable Extension state. Local development
roots are code and assets, not an implicit writable data root.

## Host Protocol

The sidecar speaks newline-delimited JSON-RPC 2.0 over stdio. The negotiated
protocol name is `psychevo-extension/1`. Stdout is protocol-only; stderr is a
bounded diagnostic stream. The host owns process-tree termination, deadlines,
maximum message size, redaction, and cancellation.

The required methods are:

- `initialize`: negotiate protocol, Extension identity, host capabilities,
  selected scope, package root, and data root
- `contributions/list`: return runtime-confirmed descriptors and availability
- `command/run`: execute an Extension-owned direct or interactive command
- `shutdown`: stop accepting work and release resources

An Extension that declares a Channel also implements the optional Channel
transport methods `channel/start`, `channel/poll`, `channel/send`, and
`channel/stop`. The host passes one connection id plus the already-resolved
connection configuration; the sidecar returns normalized inbound envelopes
and accepts normalized outbound envelopes. Polling is a bounded compatibility
path for transports whose upstream API is pull-based. A push-capable sidecar
may additionally emit `channel/ingress` and `channel/status` notifications,
but those notifications never bypass the same host routing, allowlist,
durable-outbox, and source-lane owners used by `channel/poll`.

Channel RPC is multiplexed by request id. A long-running `channel/poll` must
not stop the sidecar from accepting `channel/send`, `channel/stop`, or control
requests, and responses may arrive out of request order. The host gives poll a
deadline longer than the longest declared transport poll, supports cancellation
without corrupting the response stream, and invalidates a terminated or timed-
out session so the next call starts and initializes a fresh process.

The WeChat QR control response is a typed status union. Waiting and scanned
states require a message and base URL, `scaned_but_redirect` additionally
updates the active base URL, expired requires a message, and confirmed requires
account id, token, and base URL but no message.

Sidecars may emit progress, log, display, Channel ingress, and availability
notifications declared by the negotiated capabilities. Unknown methods or
capabilities are not inferred. Requests carry typed context and opaque handles,
not unrestricted host objects, credentials, arbitrary callbacks, or mutable
runtime registries.

`command/run` receives the canonical command identity, literal argv, cwd,
surface kind, terminal/interactivity facts, and host capability descriptors.
It returns host-applied effects: bounded text, structured display, artifact,
prompt submission, or an owning-surface request. The host validates effects
before applying them. A command manifest never authorizes direct execution of a
different program; the registered sidecar is the sole executable entrypoint.

`contributions/list` may refine availability but must not invent a direct
command absent from the static manifest. This preserves deterministic startup
conflict checking and help without eagerly starting sidecars.

## CLI Management And Dispatch

Extension lifecycle is a Pi-style top-level product surface:

```text
pevo install <source> [-l|--local]
pevo remove <selector> [-l|--local]
pevo list [--local] [--json]
pevo update
pevo update <selector>
pevo update --extensions
pevo update --all
pevo config extension [selector] [--local]
pevo -e <local-path> [command...]
```

The default scope is the active profile; `-l/--local` selects the current
workspace. `list --local` selects project records rather than overlaying both
scopes. Selectors are scope-qualified when ambiguous. `remove` deletes the
install record and materialized cache but retains the data root.

Bare `pevo update` updates the `pevo` product when its installation method has
a supported updater; a source checkout or unknown installation returns exact
reinstall guidance and does not mutate Extensions. `update <selector>` updates
one remote Extension, `--extensions` updates all remote Extensions, and
`--all` updates the product followed by Extensions. Local in-place Extensions
are reported unchanged.

`pevo config extension` lists effective Extension policy and source precedence;
with a selector it edits that Extension through the normal interactive config
flow. Noninteractive callers receive explicit flags or guidance and are never
silently prompted.

After built-in parsing, an otherwise unknown top-level word is looked up in the
effective static Extension command catalog. A match starts its sidecar lazily
and calls `command/run`; an unknown word remains a normal CLI parse error.
Built-ins cannot be shadowed. Help lists enabled direct Extension commands in a
separate `Extensions` group without nesting them under `pevo extension` or
`pevo ext`.

For a known first-party Extension command that is not installed, an interactive
terminal may show its source, requested action, and exact install command, then
ask once whether to install. Declining changes nothing. Noninteractive use
fails immediately and prints the exact `pevo install <id>` command. Third-party
unknown commands never trigger discovery or installation.

## Lifecycle And Ownership

Extension sidecars are lazy. Listing, help, static validation, and conflict
checking read manifests without spawning a process.

A scripted CLI dispatch owns a one-shot lease: it initializes at first use,
runs the request, sends `shutdown`, and waits for process exit before the CLI
returns. TUI, Desktop/Web, and Gateway hosts retain a process-lifetime runtime
pool keyed by effective Extension identity and fingerprint, and reuse its one
initialized sidecar across concurrent App, display, and Channel leases.

The host owns a lease for every active call, display, MCP App bridge, or
Channel runner. Releasing the final lease starts one cancelable five-minute
idle timer. A new lease cancels that timer without restarting the process. If
the timer expires with zero leases, the host sends `shutdown` once, waits a
bounded interval, then terminates and reaps the process tree. Host exit cancels
work and terminates all remaining sidecars. Drop is only an abnormal
best-effort kill fallback, not the normal teardown path.

Every active runtime lease holds a shared `.activity.lock` in the Extension's
scope-specific data directory. Installation, removal, replacement, and
enablement changes take the corresponding exclusive lock before publishing a
new record or effective catalog. A mutation fails with an actionable error
while any process holds a runtime lease; after the caller closes or stops that
runtime, retrying performs the mutation. A runtime invocation likewise retries
if an installation mutation already owns the lock. This rule applies to the
first installation as well as later changes so separate CLI and Gateway
processes cannot race record publication.

An active Channel keeps its Extension lease and shared activity lock until the
Channel stops; it is never detached by an idle timeout. A leased but idle
sidecar retains the lock until its cancelable idle deadline shuts the process
down. A one-shot host releases the lock after deterministic shutdown.

## UI Contributions

Extensions enhance CLI, TUI, Web, and Desktop through two bounded paths:

1. Native contributions return host-owned command effects and structured
   display values. Each host renders these using its existing components,
   themes, accessibility model, navigation, and permission UI.
2. Rich interactive UI uses MCP Apps resources. Web/Desktop renders the
   resource in a sandboxed iframe behind an AppBridge-compatible message
   boundary. The host validates resource origin, content type, CSP connect and
   resource domains, message shape, size, tool identity, and active lease.

An MCP App cannot import code into the Workbench bundle, access parent DOM,
inherit host credentials, bypass normal MCP tool approval, navigate the host,
or persist permission. The host proxies only negotiated resource reads, tool
calls, elicitation, display-mode changes, and size updates. Inline, fullscreen,
and picture-in-picture modes remain host choices.

CLI and TUI never attempt to render arbitrary HTML. Every MCP App descriptor
must provide text or structured native fallback, or it is unavailable on those
surfaces with bounded guidance. Extension management UI shows source,
fingerprint, scope, enablement, trust, protocol compatibility, permissions,
sidecar state, lease reason, and CSP/App readiness without exposing secrets.
Trust and lifecycle evidence is grouped into readable, neutral evidence cards;
only changed fingerprints and actual diagnostics use warning color. Evidence
labels, values, and explanations remain distinct at narrow mobile widths.

Web/Desktop exposes Extensions as a first-class Capabilities domain. It reads
that domain through `extension/list` and `extension/read`, changes explicit
policy through `extension/setEnabled`, and removes an installed record through
`extension/remove`. These management reads are static: they validate the
installed record and manifest but never acquire a lease or start a sidecar.
Consequently `sidecarState = "not_started"` means only that the management
read performed no execution; it must not be presented as an observation of a
sidecar owned by a different Gateway or CLI process. The view reports
`leaseReason = null` for the same reason.

Opening a declared App uses `extension/app/open`; the Gateway validates the
selected Extension fingerprint, starts the sidecar, confirms the static App
descriptor through `contributions/list`, and returns a connection-owned lease
id plus the bounded resource policy. `extension/app/close` releases that lease.
A disconnected Web/Desktop client releases every App lease it owns. The first
hosted App slice is display-only: an App declaring tool ids is reported as
`fallback_only` until its tool calls can be routed through an active
permission-owning Thread surface; the host must not substitute a direct
sidecar call or silently weaken approval.

The App surface keys its open state by Extension row and resolved scope
generation. Switching either immediately clears the prior descriptor and
closes its lease. A successful open response from an invalidated generation is
closed immediately and can never become visible or retain a hidden lease.

The MCP Apps frame is a host component, not Extension JavaScript. It accepts
only an HTTPS resource URL whose origin is in the descriptor's declared
resource domains. The host fetches it without ambient credentials, bounds the
response, requires an HTML content type, rejects a redirect outside the same
allowlist, parses the response as an HTML document, removes conflicting policy
metadata, and serializes a host CSP as the first element of the document head.
Executable content that appeared textually before an explicit source `head`
therefore cannot precede the policy in the resulting `allow-scripts` `srcdoc`
iframe. The opaque-origin frame receives a per-mount random bridge token. Every
`postMessage` must match the exact `event.source`, opaque `null` origin, and
token before the host parses its bounded AppBridge envelope. The
initial bridge exposes only the negotiated resource URI, display mode, and
allowed tool ids; tool calls are forwarded through the normal approval-owning
host callback. Unknown message methods, over-size envelopes, undeclared tool
ids, non-finite or out-of-range size requests, and messages after lease release
fail closed.

## First-Party Channel Extensions

All first-party messaging adapters are precompiled Extensions:

- `psychevo.channel.wechat`
- `psychevo.channel.telegram`
- `psychevo.channel.feishu-lark`

One Extension-store resolver owns Channel contribution selection for both CLI
setup and Gateway runtime use. It accepts exactly one enabled, fingerprint-
trusted Extension declaring the requested Channel, rejects ambiguity, and
returns the same first-party `pevo install <id>` guidance when none is
installed. Ordinary setup validates this static manifest without starting the
sidecar; only a transport operation such as WeChat QR acquires a runtime lease.

The Gateway core retains Channel configuration, secret references, normalized
source and delivery contracts, ingress routing, Thread binding, outbox state,
diagnostics, and lifecycle supervision. Each Extension owns only its platform
SDK/transport, platform payload conversion, attachment transport, and platform
delivery implementation.

Channel adapter and sidecar crates are standalone protocol consumers; they do
not depend on the Psychevo Framework crate. Framework consumers such as CLI and
Gateway reach the wire contract through the public
`psychevo::extensions::protocol` facade instead of depending on a Framework
implementation dependency directly.

Feishu and Lark share one artifact because they use the same SDK and adapter
family; credentials, domains, tenants, and source identity remain isolated.
No `native-channels` default feature or platform SDK dependency belongs in the
Gateway or CLI default dependency graph. Installing or starting a configured
Channel obtains a long-lived Extension lease. A missing first-party Channel
Extension is an actionable unavailable state with its exact install command,
not a Gateway startup failure.

The package workflow builds each first-party Channel sidecar on native Linux,
macOS, and Windows hosts. Each host emits a deterministic archive plus one
single-target descriptor fragment. A final release-index job requires exactly
one artifact per expected target and Extension, verifies archive names and
digests while merging, emits one three-target descriptor per Extension, and
attests the resulting checksum manifest before release upload. Upload is the
bounded hosted promotion defined by
[410 CI/CD Workflows](../410-ci-cd-workflows/spec.md): it runs only for a
published-release event after both the supply-chain and every native host job
succeed. The CLI's
compiled first-party index points only at these version-qualified descriptors.
Deterministic archive and merge-policy unit tests run in the package profile
before the release sidecars are built. The host package checksum manifest fails
closed unless it includes at least one built Channel Extension archive and its
single-target descriptor evidence alongside the CLI, Desktop, and Python
artifacts.

Deterministic live checks that exercise a Channel run in an isolated Psychevo
home and must explicitly build the release-form sidecar, materialize it as a
local package, and install the real Channel Extension into that home before
Gateway starts. Writing a Channel connection alone is not activation evidence,
and the validation harness has no private in-process transport fallback.

## Acceptance Criteria

- malformed manifests, unsafe paths, unsupported targets, digest mismatch,
  duplicate commands, and built-in command conflicts fail before sidecar start
- remote failure cannot replace a working install or enable unverified bytes
- listing/help/config and Plugin inspection do not spawn Extension processes
- direct commands invoke only `command/run` and preserve literal argv
- one-shot and leased hosts deterministically shut down and reap process trees
- the five-minute idle timer is cancelable and active Channels hold a lease
- Extension and co-root Plugin policy/trust remain independent
- Web/Desktop rich UI is sandboxed; CLI/TUI use declared fallback
- Gateway and minimal CLI builds contain no first-party platform SDK
  dependencies
- the feature-free Channel adapter test target compiles without provider test
  fixtures or Rust warnings; each single-provider feature compiles only its own
  adapter tests and the shared fixtures that those tests actually consume
- tests use fake sidecars, fake HTTPS release endpoints, fake Channel services,
  and deterministic clocks by default; live validation is explicit
- the deterministic Workbench visual inventory installs a fake display-only
  Extension through the public CLI and captures both static management evidence
  and its sandboxed MCP App on desktop and mobile; it also proves an active App
  lease blocks policy mutation until the App closes

## Related Topics

- [026 Commands](../026-commands/spec.md) defines shared interactive command
  metadata and effects.
- [028 Channels](../028-channels/spec.md) defines the host-owned Channel model.
- [050 Capability Extensions](../050-capability-extensions/spec.md) defines
  source acceptance and immutable runtime assembly.
- [054 Plugins](../054-plugins/spec.md) defines declarative Plugin packages.
- [150 Plugin Runtime](../150-plugin-runtime/spec.md) defines Plugin storage and
  marketplace management.
- [155 Plugin Manifest](../155-plugin-manifest/spec.md) defines Plugin
  manifests.
- [200 pevo CLI](../200-pevo-cli/spec.md) defines concrete command spelling.
