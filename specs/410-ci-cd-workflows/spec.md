---
name: 410. CI/CD Workflows
psychevo_self_edit: deny
---

Define Psychevo's concrete local CI/CD workflow runner. This topic implements
the provider-neutral CI/CD foundation from [065 CI/CD](../065-ci-cd/spec.md)
through repo-local `xtask` commands and the minimal hosted workflow that invokes
the same deterministic validation paths.

## Scope

This topic owns:

- local `cargo xtask ci` command behavior
- local `cargo xtask live` command behavior
- named workflow profiles and their planned steps
- repository-owned non-functional baselines and deterministic regression budgets
- scheduled high-risk coverage, finite deterministic boundary matrices, Miri,
  and sanitizer instrumentation
- local artifact root conventions for workflow runs
- live opt-in enforcement for workflow profiles
- lower-level helper scripts used by profile steps
- the required GitHub Actions workflows for pull requests, `main` pushes, and
  explicit or scheduled hosted package validation

Out of scope:

- hosted release, deployment, or registry workflows
- public release publishing, hosted draft releases, deployments, update
  channels, or package registry upload
- user-customizable workflow manifests
- replacing topic-specific testing specs or acceptance matrices

## Runner Interface

The local runner exposes:

- `cargo xtask ci list`
- `cargo xtask ci plan --profile <profile>`
- `cargo xtask ci run --profile <profile>`

`list` prints available profiles. `plan` prints the ordered steps without
executing them. `run` executes the selected profile, reports compact progress,
and preserves step output in logs.

Artifact-only package execution is an explicit opt-in. `cargo xtask ci run
--profile package` must fail before creating an artifact root or starting a
profile step unless the caller also passes `--package`. Planning remains
available without opt-in through `cargo xtask ci plan --profile package`.

During `run`, normal stdout progress is captured to the step log without being
mirrored to the terminal. Stdout warning lines are mirrored to terminal
diagnostics. Stdout error lines are not mirrored by default; errors should
surface through stderr to avoid duplicate diagnostics. Stderr is mirrored to the
terminal and also captured in the step log. When a failed step captured any
output that was not mirrored to the terminal, it reports the log path and the
last 80 log lines to stderr even if the command also emitted a generic stderr
summary. A failure whose captured output was already fully mirrored does not
repeat the log tail. Empty or unreadable logs do not replace the original step
failure.

Runner tests for terminal mirroring use an isolated output sink. Synthetic
warning and error fixture lines remain assertable by the test but must not leak
into the parent Cargo test stderr or a successful profile summary. A passing
profile therefore shows only diagnostics emitted by its real child steps.

Deterministic Rust tests use native host paths, executable formats, and shell
runtimes for platform-neutral behavior. POSIX-only filesystem or process
semantics compile only on Unix, while Windows-only semantics compile only on
Windows. Tests must not pass POSIX fixtures through native Windows path or
process APIs, and cross-platform command fixtures must select an executable
harness for the current host.
Repository Cargo configuration gives Rust test worker threads a host-neutral
8 MiB minimum stack while preserving an explicit caller override. This keeps
large async Gateway contract tests independent of the native Windows default
without adding per-test platform wrappers.

All commands accept `--json` for machine-readable output. JSON output must
include profile ids, profile descriptions, step ids, command arrays, live
flags, artifact roots when available, and per-step status for executed runs.
Executed results also record integer millisecond durations for the whole run
and for every attempted step, including failed or internally errored steps.
Human progress reports each step's final status and elapsed duration without
making wall-clock thresholds part of validation.

The live registry exposes:

- `cargo xtask live list`
- `cargo xtask live plan [--env shared|isolated] [--check <id>]... [--suite <suite>]... [--all]`
- `cargo xtask live run [--env shared|isolated] [--check <id>]... [--suite <suite>]... [--all]`

`cargo xtask live run` is itself explicit live opt-in and does not require an
extra `--live` flag. With no selection, it runs the `smoke` suite. Provider
selection is a repeatable `--provider <id>` command-line argument; v1 supports
`xiaomi-token-plan` and `deepseek`, with `xiaomi-token-plan` as the default.
Live selection must not depend on public live-specific environment variables.
The generic CI profile remains guarded: `cargo xtask ci run --profile live`
must fail before provider work unless the caller also passes `--live`.
`cargo xtask ci plan --profile live` and `cargo xtask ci run --profile live
--live` accept `--live-env shared|isolated` and default to `shared`.

Direct `pnpm exec playwright ...` invocations are debugging commands for
individual Workbench specs. They are not official full visual or full live
entrypoints; use `cargo xtask ci run --profile visual` and
`cargo xtask live run ...` for planned validation and artifacts.

Every provider-required live check expands into one typed plan/run instance per
selected provider. Its instance id, structured result, and artifact path name
that provider; the run-level provider inventory alone is not coverage evidence.
Provider-independent checks execute once. A check that intentionally supports
only a provider subset declares that allowlist in the live registry, and every
unsupported selected-provider instance remains visible as an explicit skip or
rejection rather than silently executing against the first provider. The
Xiaomi-specific ignored Rust live contracts are such allowlisted checks; the
provider-neutral smoke, doctor, Desktop, and Workbench checks run once for each
selected provider.

## Profiles

Initial profiles:

- `changed`: lightweight local confidence for the current checkout; v1 plans
  format checking and lets future work add diff-aware selection.
- `rust-broad`: Rust workspace broad gate; checks generated Gateway protocol
  bindings, compiles `psychevo --no-default-features --all-targets` as an
  independent consumer without workspace feature unification, and checks the
  CLI no-default-features core graph before format, clippy, and tests.
  JavaScript CLIs invoked directly by this profile or its code-generation
  helpers are explicit root workspace development dependencies. The Gateway
  validator generator therefore owns a direct `tsx` dependency rather than
  relying on a binary exposed incidentally by Vite or WebDriverIO.
  Xtask resolves those host commands through the active `PATH` and, on
  Windows, `PATHEXT`; `.cmd` and `.bat` shims run through the captured command
  processor instead of being passed directly to `CreateProcess`.
- `rust-checks` and `rust-tests`: hosted shards of `rust-broad` for parallel
  pull-request execution. `rust-checks` owns every broad step through Clippy;
  `rust-tests` owns the workspace all-target test step. `rust-broad` remains
  the canonical local Rust gate and is exactly the ordered concatenation of
  these shards, with all three profiles sharing the same step definitions.
- `sdk-contracts`: deterministic cross-language SDK and wire-contract checks.
  It owns the Rust SDK surface check, Gateway protocol generation check,
  App Server and generated protocol fixture validation, TypeScript client
  contracts, Python SDK tests, and Python distribution contract tests. It does
  not build release artifacts, access provider credentials, or duplicate the
  package profile's installed-wheel smoke.
- `main-artifact-smoke`: Linux-only mainline evidence that builds the pure
  Python SDK wheel and the no-default-features App Server binary wheel,
  installs those exact artifacts in a clean environment, and completes one
  fake-provider Turn through the installed Python client and installed App
  Server. It reuses the package profile's installed-artifact implementation,
  omits CLI/Desktop artifact work, and is not part of pull-request path
  selection.
- `supply-chain`: Linux-only, non-live CI policy that is locally planable and
  runnable. A repository-owned tool manifest pins scanner versions, release
  URLs, and archive digests; the profile rejects version drift before checking
  the locked Rust graph with cargo-deny advisories, bans, licenses, and sources,
  auditing every pnpm production advisory severity, and scanning all available
  committed Git history with the repository-owned Gitleaks policy. It also
  rejects unused or unlisted Rust and pnpm dependencies using pinned
  cargo-machete and Knip versions. Dynamic package resolution and host-provided
  binaries may be declared only at their exact owning workspace; stale ignore
  declarations fail the check. Hosted execution installs the locked pnpm graph
  before invoking the profile; the profile itself remains a verifier rather
  than a dependency installer. Registry
  failures and unavailable scanners fail the profile. Rust wildcard registry
  requirements are denied while versionless same-workspace path dependencies
  remain valid; the repository-only `psychevo-xtask` crate is explicitly
  non-publishable. Duplicate transitive crate versions remain allowed because
  they are dependency-topology maintenance signals rather than security
  findings. It has no baseline, blanket advisory/secret ignore,
  `continue-on-error`, provider access,
  artifact build, publication, checksum, or provenance step. Package artifact
  checksums remain exclusively owned by `package`; after that complete profile
  succeeds, the hosted package job uses the checksum manifest as the exact
  subject set for one build-provenance attestation. The scanner profile does
  not duplicate or claim that provenance.
- `non-functional`: Linux-only, non-live performance and footprint evidence.
  It measures a clean and immediate no-op Framework check in one
  artifact-owned Cargo target, checks the normal-build dependency graph,
  builds the release CLI, shipped Linux Desktop runtime, and complete Linux
  Python wheel set, measures the first
  isolated `pevo --version` process and the median of nine immediate restarts,
  exercises the Gateway's deterministic first-result, idle-database,
  batched-heartbeat, and retained-event persistence contracts, and runs the
  production Workbench startup-byte journey. It also measures the release
  Desktop executable and all three wheel artifacts, and counts every regular file
  copied into the production `dist/file-viewer/` tree, which is the Vite
  plugin-owned optional-preview asset boundary. The
  Desktop footprint step builds the native release executable with bundling
  disabled; installer/container construction belongs exclusively to the
  `package` profile and must not be repeated merely to measure that executable.
  The
  reviewable `non-functional-budgets.json` records the maximum accepted value
  and, after the first canonical run, the measured baseline. Deterministic
  counts and byte sizes are hard gates. Host-sensitive durations are recorded
  and compared only within the
  same run where a meaningful comparator exists, plus deliberately broad
  runaway ceilings; they must not masquerade as cross-machine nanobenchmarks.
  The first CLI invocation is a first-process sample, not a claim that Linux
  page caches were evicted. A newly added metric may use a null baseline until
  the first complete canonical profile run records it; its maximum still
  applies and the observed value remains mandatory evidence.
  Root-workspace release artifacts use ThinLTO with one code-generation unit
  so executable code is optimized across crate boundaries without stripping
  the symbols used by bounded panic evidence. Release build latency is the
  accepted delivery-time cost; runtime behavior and diagnostic evidence are
  not traded away to satisfy an artifact budget.
- `instrumentation`: Linux-only, non-live high-risk diagnostics. It first runs
  the repository-owned instrumentation harness unit contracts, produces
  targeted Framework/Gateway/protocol coverage artifacts, runs the
  owning crates' finite deterministic boundary matrices for Gateway protocol
  round trips, provider stream fragmentation, split UTF-8, and tool-call
  argument assembly, executes pure protocol contracts under Miri, and runs the
  selected lifecycle test target under AddressSanitizer. The boundary matrices
  use existing fake or in-memory provider seams and the production parsers and
  accumulators; they do not use randomized inputs, mutation runners, sockets,
  custom servers, a separate workspace, or a second parser or accumulator.
  This profile is bounded and scheduled rather than added to every pull
  request. It has no
  provider credentials, soak loop, arbitrary repository-wide coverage
  percentage, baseline update flag, or success-on-tool-failure behavior.
  Coverage and report generation execute with the repository's same
  date-pinned nightly toolchain, whose installed components include
  `llvm-tools-preview`; stable Rust must not be asked to accept the nightly-only
  branch instrumentation flags. Coverage enables LLVM branch instrumentation
  and retains reviewable per-file
  counters for the Framework shutdown/admission lifecycle, Framework Turn
  delivery persistence, Gateway durable-activity persistence, and the App
  Server public-event protocol projection. The profile fails if any named high-risk file is
  absent from the report or has no exercised lines, functions, or branches; it
  does not convert those local counters into a repository-wide percentage
  target.
  Every invocation owns a clean coverage, deterministic-contract, Miri target,
  or sanitizer target directory beneath its artifact root. Reusing an explicit
  artifact root must not let a file from an earlier invocation count as current
  evidence. Every external
  instrumentation command has a finite, deliberately wide timeout that allows
  a cold toolchain build; a timeout identifies the exact command and is
  reported as an instrumentation timeout rather than as a product-test
  failure. The hosted instrumentation job also has a finite outer timeout so a
  wedged installer or runner cannot occupy a worker indefinitely.
- `desktop-rust`: independent Desktop Rust workspace gate; first checks root
  and Desktop manifest parity, then checks formatting, runs clippy with warnings
  denied, and tests all targets using the shipped `native-runtime` feature. It
  does not enable the test-only `wdio-test` feature or duplicate Desktop
  renderer validation from `web`. The tracked Tauri schema artifacts remain the
  `wdio-test` superset so they describe both production capabilities and the
  checked-in test-only `capabilities/wdio.json`; after a production-only build
  rewrites those files, the feature-enabled schema generation is the canonical
  finalization step.
- `web`: all JavaScript workspace unit tests, all workspace typechecks, and all
  workspace production builds, including Workbench and Desktop. Workspace
  tests execute with bounded workspace concurrency so packages that own process
  state or browser-like globals do not race. This profile owns the Browser and
  native Gateway Adapter contract together so reconnect behavior cannot pass
  on only one surface. Its pnpm steps use the same host-command resolver as
  protocol generation and visual workflows, including Windows `PATHEXT` and
  command-script handling. Package test scripts must fail when their configured
  suite discovers no tests instead of using `--passWithNoTests`. After the
  production build, this ordinary gate runs the deterministic desktop-Chromium
  critical first-Turn journey against the real managed Gateway with Native and
  ACP fake runtimes; the broader screenshot and profiling inventory remains in
  `visual`.
- `visual`: deterministic visual diagnostics using fake/local providers. It
  owns the TUI/VHS capture workflow, the complete non-live Workbench
  Playwright inventory in desktop and mobile Chromium, and the native
  Desktop/Floating WebDriverIO smoke on Linux. The Playwright inventory is
  selected by the repository's live-test tag rather than a duplicated spec
  filename allowlist; the global Playwright policy captures a final-state
  screenshot for every opened test page, and the current-run manifest
  inventories manual screenshots together with
  Playwright screenshots, traces, and videos. Native Desktop/Floating visual
  acceptance is explicitly exempt on Windows and macOS, where it remains in
  platform package/live validation, but a missing display or native
  prerequisite is a failure rather than a successful Linux skip. Permanent
  visual filenames, suite labels, request ids, and proof inventory describe
  behavior and contain no planning date or implementation-batch identifier.
  Every visual-owned external command has a finite step-specific timeout. A
  timeout names the command, terminates its complete process tree, and fails
  the step instead of waiting for the hosted job's outer timeout.
- `surface-profile`: deterministic artifact-producing TUI-versus-Workbench
  profiling against one local Native provider fixture. It runs the real
  fullscreen Framework-bound TUI through a pseudo-terminal and the
  Gateway-bound Workbench through desktop Chromium, excludes warmup and traced
  diagnostic samples from percentiles, and writes a validated comparison
  waterfall without screenshots or live credentials. Gateway lifecycle spans
  are required only for Workbench and stop at Gateway-observed public Framework
  events; the harness must not reintroduce an Adapter callback across the
  Application boundary. The shared provider and surface-commit boundaries
  remain the cross-surface control.
- `live`: opt-in live validation using explicit provider credentials.
- `package`: artifact-only CD profile that builds local reviewable artifacts,
  builds the App Server with no default features, discovers both Python SDK
  test directories, installs the real wheels/sdist into a clean environment,
  runs an installed fake-provider stdio smoke, and writes checksums without
  publishing or creating hosted release objects. The hosted matrix job grants
  attestation permissions only to the artifact-building job and, after the
  profile succeeds, signs exactly the subjects named by that checksum file
  with a full-commit-pinned GitHub action; local execution neither requires a
  GitHub identity nor pretends to attest. Running it requires explicit
  `--package` opt-in. An unavailable required install/smoke prerequisite fails
  or reports the profile blocked; it is not a successful skip.
  The profile starts the exact release CLI from its artifact-owned target,
  performs an isolated managed-Gateway ready handshake through that binary,
  and starts the host Desktop artifact through its normal production runtime.
  Desktop acceptance observes the ordered, same-process `window_ready`,
  `managed_gateway_ready`, and Workbench `bridge_connected` startup marks using
  that same release CLI, without a provider, WebDriver, or test-only product
  API. The structured native-artifact report names the exact launched
  paths. Installer/container formats that cannot be safely installed by an
  unprivileged hosted job remain explicit per-artifact `build-only` entries;
  their existence and checksum are not described as execution evidence. Each
  smoke's home, config, database, and generated Gateway token live only in a
  system temporary directory and are deleted after shutdown; retained package
  evidence contains only the structured report, bounded process log, and the
  content-free startup trace.

## Hosted CI

The pull-request workflow has a `Scope` job, five independent Linux execution
jobs (`Rust checks`, `Rust tests`, `SDK contracts`, `Desktop Rust`, and `Web`),
and an always-run `CI Gate`. The execution jobs start in parallel after
successful scope classification. Pushes to `main` additionally run a Linux
`Main artifact smoke` job through `main-artifact-smoke`; pull requests must
skip it. `CI Gate` requires each selected job to succeed, each unselected job
to be skipped, the main-only job to match that event contract, and the scope
job itself to succeed; failure, cancellation, or an unexpected selection
result fails the aggregate check.

Draft pull requests select execution jobs from the complete pull-request diff:

- CI workflow changes, root Cargo configuration or lockfiles, `.cargo/**`, and
  `xtask/**` are common infrastructure and select all five jobs.
- `crates/**`, `scripts/**`, Rust-consumed assets, root pnpm configuration, and
  `packages/protocol/**` select both Rust shards. SDK, Gateway App Server,
  protocol, and Python changes select SDK contracts; protocol and pnpm changes
  also select Web.
- `python/**`, the Python build backends, and package-workflow changes select
  SDK contracts even when they do not select a renderer surface.
- `apps/**`, `packages/**`, assets, and root JavaScript or TypeScript workspace
  configuration select Web.
- `apps/desktop/src-tauri/**` selects Desktop Rust and also matches the Web
  surface so native and renderer integration remain covered together.

A ready-for-review pull request ignores path selection and runs all five jobs
for every head update. The workflow handles `ready_for_review` and
`converted_to_draft` transitions explicitly. Every push to `main` runs all five
deterministic jobs plus the Linux installed-artifact smoke rather than applying
draft path selection. The smoke uploads its exact SDK/App Server artifacts and
structured run evidence from the same successful job. Workflow-level
concurrency is keyed by workflow plus pull request number or pushed ref, and a
newer run cancels an older run for the same pull request or branch.

Before path classification, `Scope` checks out the triggering revision with a
full-commit-pinned checkout action and without persisted credentials. This is
required for the classifier's Git-based `push`/long-lived-branch mode; the same
step also serves pull requests, where classification may use the API. Path
classification uses full-commit-pinned `dorny/paths-filter` v4.0.2 with only
`contents: read` and `pull-requests: read` permissions. Every job that
compiles root or Desktop Rust uses full-commit-pinned `Swatinem/rust-cache`
v2.9.1 with failed-run cache saving enabled. Root Rust shards and Web keep
job-specific root workspace caches; Desktop Rust caches the independent
`apps/desktop/src-tauri` target. Rust tests install Node.js for JavaScript
fixtures but do not install pnpm dependencies; Desktop Rust installs no Node or
pnpm toolchain; Rust checks retains the frozen workspace install required by
generated Gateway protocol verification.

Every third-party `uses:` reference in tracked workflows is pinned to exactly
40 hexadecimal commit characters. A repository test parses workflow `uses:`
values and validates that invariant by value rather than maintaining an action
name or workflow-line inventory; local `./` actions are exempt.

Every hosted workflow that provisions Python uses the full-commit-pinned
`actions/setup-python` v7 action, whose JavaScript action runtime is Node.js 24,
while continuing to install Python 3.12 explicitly. The action implementation
runtime is independent of both the provisioned Python version and the
repository's separately provisioned Node.js toolchain.

The canonical local functional regression remains four explicit commands:
`cargo xtask ci run --profile rust-broad`, `cargo xtask ci run --profile
sdk-contracts`, `cargo xtask ci run --profile desktop-rust`, and `cargo xtask
ci run --profile web`. There is no aggregate `full` profile.
Footprint/performance and toolchain instrumentation remain explicit additional
commands: `cargo xtask ci run --profile non-functional` and `cargo xtask ci
run --profile instrumentation`. Keeping them separate preserves exact evidence
and prerequisites rather than hiding several long-running modes behind one
aggregate command.

Hosted workflows install the workspace minimum Rust toolchain through a
full-commit-pinned `dtolnay/rust-toolchain` action revision whose baked-in
toolchain matches `workspace.package.rust-version`. They must not pass an
unsupported `toolchain` action input that leaves the revision's older baked-in
toolchain active.

The separate artifact-only workflow runs on Linux, macOS, and Windows through
explicit manual dispatch, a weekly Monday 02:00 UTC schedule, version-tag
pushes, and published-release events. Every triggering run builds, tests, and
smokes the directly executable artifacts it uploads, while recording
non-installable installer/container formats as `build-only`; it does not copy
artifacts from another workflow or
publish them to a registry or hosted release. YAML prepares host dependencies,
invokes exactly `cargo xtask ci run --profile package --package --artifact-root
<root>`, and uploads its plan, results, logs, checksums, Python packages,
release CLI, host Desktop bundle, and signed provenance bundle. Build intermediates under the
artifact-owned Cargo target directories are not uploaded. The package profile exclusively
owns the locked standalone Rust SDK check, Python SDK and package tests,
locked release CLI build, Workbench build, host Desktop bundle, native release
artifact smoke, installed Python artifact smoke, and checksums. YAML must not
duplicate those build/test
commands; it may only attest the checksum-owned subjects after those steps
pass. The CLI and Desktop builds use artifact-root-owned Cargo target
directories, and checksum generation covers the CLI, Desktop executable,
Desktop bundle files, and Python artifacts and fails when any category is
absent. A platform is
release-eligible only after its own runner succeeds;
the existence or local validation of the workflow does not claim that another
operating system passed. This workflow never publishes packages, creates a
hosted release, or uses provider credentials. Its artifact upload explicitly
includes hidden paths because package staging lives under `.local`.

The Linux artifact runner executes AppImage build tools in extraction mode so
local, container, WSL, and hosted runners do not require a mounted FUSE device
or the legacy `libfuse.so.2` runtime. This changes only how the packaging tool
starts; the emitted AppImage remains the same host artifact. Hosted Linux also
installs Xvfb and Xauthority explicitly so the release Desktop smoke has a
declared headless display rather than relying on runner-image accident.

The artifact workflow also has one independent `Supply chain` job on
`ubuntu-24.04`. Manual dispatch, the weekly schedule, version tags, and
published releases each invoke `cargo xtask ci run --profile supply-chain`
exactly once; the three-OS artifact matrix does not repeat the scans. Its
checkout contains complete Git history for the committed-secret check, and its
host setup installs only the exact digest-verified scanner binaries declared by
the repository-owned tool manifest.

Package-profile steps must not mutate the workspace `Cargo.lock`. The Rust SDK
package verifier may use temporary extracted packages and temporary
`patch.crates-io` resolution to compile unpublished sibling crates, but patch
metadata from that isolated verification must not enter the workspace lockfile
or make a later `--locked` delivery step fail.

After execution begins, `results.json` records every completed step through the
first failed or internally errored step as well as a fully successful run. It
includes monotonic integer millisecond duration fields for the run and every
attempted step. A step failure must not leave only logs and a plan without the
structured result needed by local review and artifact upload. The hosted upload
step runs after a failed package profile as well as after success so those
diagnostics remain reviewable.

Hosted workflows have no aggregate release job, live provider work, or soak
work. Every package matrix trigger is artifact-only and does not publish. A
separate least-privilege Linux instrumentation workflow runs weekly and by
manual dispatch. It installs the exact locked `cargo-llvm-cov` version and the
pinned nightly components required by coverage,
Miri, and AddressSanitizer, invokes only the `instrumentation` profile, and retains its
structured results plus coverage and deterministic-contract artifacts even on failure.

A second least-privilege Linux extended-validation workflow runs weekly and by
manual dispatch. In one isolated checkout it invokes `visual`,
`surface-profile`, and `non-functional` serially with separate artifact roots,
using only deterministic local fixtures. It installs the declared VHS,
Playwright, Linux native Desktop, virtual-display, Python, Node, and Rust
prerequisites, runs `visual` inside that virtual display, retains structured
and visual evidence even on failure, and excludes disposable Cargo target
directories from upload. The Linux native Desktop/Floating step must execute;
a platform or display skip is not accepted as visual evidence. This scheduled
job is the automatic acceptance boundary for broader visual behavior,
cross-surface regressions, and repository-owned non-functional budgets; it
does not read provider credentials. The `live` profile remains an explicit
manual, artifact-owned entrypoint because live provider access must never be
inferred from hosted secrets.

Non-functional budget changes are review material. A budget may move only with
a new measured baseline from the same checked-in harness and an explanation in
the owning change; deleting a metric, weakening a comparator, or silently
accepting a missing artifact is not a baseline update. Current owned metrics
are Framework direct and reachable normal dependencies in its feature-free
Linux x86_64 graph, clean/no-op check duration and ratio, release CLI and
Desktop executable bytes, Python SDK, App
Server, and CLI wheel bytes,
release CLI first-process and repeated-process startup, Workbench initial
encoded JavaScript bytes and optional file-viewer asset bytes, initialized GUI
pre-provider overhead, idle Gateway SQLite operations, the transaction count
for the full Shell heartbeat batch, and retained-event batch persistence
latency and time per event. Retained-event evidence names and reports p50, p95,
and p99 separately for end-to-end envelope commit latency in microseconds and
for batch commit latency in milliseconds. It also records peak ingress queue
depth and the Store's SQLite busy-operation delta for the measured run; average
microseconds per event remains the commit-throughput indicator. Compilation and
process durations are not compared
across different machines. These executable and wheel baselines are Linux
x86_64 measurements; the cross-platform package workflow records, tests, and
retains real Windows and macOS outputs without comparing them to Linux caps.
The profile does not sample process-wide idle CPU on a shared CI host. The
paused-time Shell contract proves the product-owned scheduler emits no idle
timer work and performs exactly zero SQLite operations, avoiding a noisier
proxy whose result also includes the executor, allocator, kernel scheduler,
and unrelated runner contention.

Workflow definitions are code-owned in `xtask` for v1. Do not add a public
TOML/YAML manifest until there are multiple real adapters or external
customization needs.

Deterministic Workbench harnesses must not acquire extra ACP targets from the
host `PATH`. Unless a test explicitly configures a local shortcut backend, its
isolated config records the known OpenCode and Hermes shortcuts as disabled so
catalog, screenshot, and control assertions cannot depend on developer-machine
executables. Live checks keep normal host discovery semantics.

Rust dependency hygiene is part of the `rust-broad` gate. Workspace-owned
dependencies should use one compatible version line when existing transitive
dependencies already require that line; for HTTP clients this means the
workspace `reqwest` dependency follows the active `0.13.3` line instead of
keeping a separate `0.12` build.

Deterministic provider HTTP fixtures that close a streaming response must
consume the complete request body before sending that response. A fixture must
not leave unread request bytes that can reset the TCP connection and make a
valid terminal SSE event appear to be missing.

## Live Registry

Registered live checks:

- `provider-smoke`: native `xtask` provider smoke with two `pevo run --format
  json --include-reasoning` turns, successful file-inspection tool verification,
  `--continue` thread reuse verification, and token final-answer verification.
  Its verifier consumes the current public item lifecycle: non-empty
  `item.updated(reasoning)` proves streamed reasoning, while a completed normal
  tool result that contains the file's probe token proves inspection. The
  verifier must not require a particular equivalent non-mutating tool choice
  such as `read` versus `exec_command cat`, and it must not silently keep
  accepting a retired `entry.completed.blocks` output shape.
- `pevo-doctor-live`: `pevo doctor --live --json`.
- `runtime-provider-read`: runtime ignored live provider read-tool check.
- `runtime-model-fetch`: runtime ignored Xiaomi `/models` fetch/cache check.
- Runtime live checks owned by the `psychevo` package compile against its named
  semantic Framework interfaces, as required by
  [080 SDK](../080-sdk/spec.md). Their integration-test harness may exercise
  first-party runtime seams without a hidden `product` facade or making raw
  implementation modules public.
- `gateway-automation-live`: gateway automation ignored live check.
- `codex-plugin-broker-live`: read installed Codex plugins through the
  capability broker. A missing Codex executable is a `blocked` result for this
  check, while failure to create its isolated profile is a `failed` result;
  neither condition may abort result generation or prevent later checks.
- `desktop-native-smoke-live`: native Desktop/Floating WebDriverIO smoke
  without provider calls. Every Desktop live check owns a check-local home,
  state database, and managed Gateway even when the surrounding live run uses
  shared mode. It must stop only that check-owned Gateway after WebDriverIO
  exits, on both passing and failing runs. Cleanup failure fails the check;
  isolated live artifacts must not leave listeners that consume later tests'
  managed-Gateway fallback ports.
- `desktop-floating-provider-live`: native Floating provider validation through
  Desktop. This check belongs to the Desktop live suite and is triggered with
  the other Desktop live checks by `cargo xtask live run --suite desktop`; it
  uses the live runner's normal live invocation and credential resolution
  rather than a separate opt-in gate. The selected live provider model must be
  written to a check-local config before Desktop starts. Inference environment
  variables are fallback inputs and must not allow an unrelated model in the
  developer config to make this check claim coverage for the wrong provider.
  The selected-text probe and expected reply must use a benign natural-language
  sentinel rather than a secret-shaped value described as a token; provider
  refusal to reproduce apparent credentials is not a Desktop transport failure.
  A failed Framework Turn rendered in Floating's transcript is immediate
  provider-failure evidence: the native harness must report its bounded
  diagnostic instead of waiting out the successful-response deadline merely
  because the separate capsule error row is absent.
  This provider-specific check records the common Desktop startup evidence and
  then exercises Floating directly. Settings and native bridge inventory belong
  to `desktop-native-smoke-live` and must not be reopened here, where a cold
  model-catalog request can serialize ahead of the provider turn.
- `web-composer-live`: Workbench real-provider composer check. The check must
  use the live context timeout as its test timeout and observe an empty
  `data-composer-state="ready"` draft before sending. The always-visible
  Transcript region is not Composer readiness; a provider response persisted
  by the Gateway but missed by an unbound browser projection is a failed GUI
  check, not a provider timeout.
- `web-composer-draft-open-first-send`: deterministic Workbench/Gateway check
  for the first Composer send while `thread/draft/open` is pending at the
  client protocol boundary. The harness delays only that RPC result while
  forwarding every request to the real Gateway; it must not manufacture the
  pending window by scaling a filesystem inventory or relying on machine
  timing. It uses the check-local cwd, home, executable, and state database
  supplied by the live runner, and retains bounded RPC/provider proof in the
  check artifact so a missing rendered response can be distinguished from a
  missing turn, provider request, or projection.
- `web-automation-live`: Workbench GUI automation live check. After proving
  creation and schedule projection, the check must delete the automation
  through the rendered GUI before teardown so repeated live runs do not leave
  interval jobs that can interfere with later checks.
- `web-subagent-live`: Workbench live subagent GUI check. It proves that at
  least one provider-created child session is rendered and can be opened from
  its parent transcript. The provider's choice to split a request across one
  or several children is not a validation invariant.
  The Workbench web live checks live in
  `apps/workbench/e2e/workbench.live.spec.ts`; the live registry must track
  that file when Workbench deterministic specs are split or renamed.
- `pevo-acp-server-live`: Psychevo ACP server live validation through
  Workbench's Playwright harness. This check lives in
  `apps/workbench/e2e/pevo-acp-server-live.spec.ts` and belongs to the `acp`
  suite so `cargo xtask live run --all --env shared` covers it. It owns a
  finite local provider fixture and is provider-independent: the registry runs
  it once and must not inject or require a selected live provider.
- `web-skill-live`: Workbench live-skill flow.
  The check must wait for Workbench startup to settle, explicitly open a new
  Session, and observe an empty `data-composer-state="ready"` draft before
  sending the skill prompt. A provisional prompt and running Composer without
  a Gateway activity are not evidence that the live turn started.
  If the real skill reaches a Permission interaction, the check may answer only
  through the rendered GUI and must choose the least-persistent `Once` action;
  it must not seed, mutate, or persist trust policy to bypass the interaction.
  If the provider reaches a Clarify interaction, the check may submit only a
  valid default answer already selected by the rendered GUI. It must not
  synthesize an answer, choose `Other`, or call the interaction RPC behind the
  UI.
  Completion checks for live skill flows must scope running/streaming DOM state
  to the active Transcript region so shell, sidebar, or history running
  affordances cannot mask a completed assistant response.
- `opencode-acp-gui-lifecycle-live`: one OpenCode ACP GUI live flow covering
  both the provider-backed Turn and the same test-owned Session's lifecycle;
  those projections are not registered as a second execution of the same
  Playwright test. The check must wait for
  the asynchronous ACP backend inventory before deciding whether OpenCode must
  be configured, and it must tolerate other locally materialized ACP backends;
  an initially empty backend list is not a validation invariant. Before the
  provider-backed turn, the check writes only its temporary Project Runtime
  Profile and selects the OpenCode model whose model id matches the live
  provider model. Because the live harness writes through a protocol-only
  client outside the Workbench state container, it must then bootstrap a fresh
  page and prove that the selected model is visible before sending the turn;
  a stale generated-profile projection is not valid live coverage. After
  page bootstrap, the check must first observe a settled composer state before
  choosing New Session, then observe `data-composer-state="ready"` and zero
  transcript rows before selecting controls or sending; the always-visible
  Transcript region and its hidden empty-state node are not draft readiness.
  Session context fidelity may be `exact` when complete
  prompt usage is available or `partial` when only ACP context/usage evidence
  is available; `unavailable` is a failure after a completed turn. It then
  waits for the authoritative Workbench Turn state to become idle before it
  validates list/resume/fork/close behavior in its unique temporary cwd and
  proves that delete remains unavailable. It never mutates an unrelated
  discovered Agent session.
- `opencode-acp-delegate-live`: `@opencode` delegate live flow. The check proves
  that the child streams before the parent finishes, the child's persisted
  transcript evidence and persisted delegate result contain the requested
  sentinel, and the parent reaches a non-empty normal terminal response. The
  child may emit the sentinel as assistant text or through a read-only tool
  result, and the provider may summarize that result instead of copying the
  child's sentinel verbatim into its final prose; neither exact prose location
  is a validation invariant. The generated
  `@opencode` Agent must resolve the configurable public `opencode` Runtime
  Profile when no Team member supplies another profile. The check configures
  that exact temporary Project Runtime Profile with the same live model used by
  the direct GUI check and fails if delegation bypasses it for an
  unauthenticated generated fallback.
  A provider may recover an incomplete child answer through another child
  attempt. Persistence acceptance therefore aggregates every edge owned by the
  one test parent: all created edges must be closed, at least one linked child
  must contain the requested marker, and the parent Tool result, final answer,
  and authoritative terminal must agree. Selecting an arbitrary joined child
  row is not evidence for or against that invariant.
- `agent-acp-session-lifecycle`: deterministic Workbench lifecycle flow covering
  explicit discovery, import/resume, fork/close/delete capability gating, and
  proof that ordinary Session reads do not initialize ACP processes.
- `codex-acp-session-lifecycle-live`: opt-in Codex ACP list/resume/close/delete
  flow scoped to a unique temporary cwd and a session created by that check.

Suites:

- `smoke`: `provider-smoke`.
- `provider`: `provider-smoke`, runtime provider/catalog checks, and doctor
  live.
- `web`: Workbench composer, automation, and subagent live checks.
- `skill`: live skill check.
- `desktop`: native Desktop/Floating smoke and provider-backed Floating live
  checks.
- `acp`: OpenCode ACP live checks and Psychevo ACP server live validation.
- `automation`: gateway automation and Web automation live checks.
- `all`: all registered checks.

The live runner owns provider/model resolution, dev-home initialization checks,
artifact paths, environment-mode resolution, `PSYCHEVO_HOME`/`PSYCHEVO_CONFIG`/
`PSYCHEVO_DB` injection, and any implementation-only context files passed to
test harnesses. Missing host tools, missing fixture directories, missing config,
or missing credentials are reported as `blocked`, not silent skips.

Live environment modes:

- `shared` is the default. It sets `PSYCHEVO_HOME` to `.local/.psychevo-dev`,
  `PSYCHEVO_CONFIG` to `.local/.psychevo-dev/config.toml`, and `PSYCHEVO_DB` to
  `.local/.psychevo-dev/state.db`.
- `isolated` uses the same dev-home config and `.env`, but sets
  `PSYCHEVO_HOME` and `PSYCHEVO_DB` to per-check paths under
  `.local/.psychevo-dev/ci/<run-id>/live/<check-id>/`.

Plan and run JSON must include the selected environment mode. Run JSON must
also include the effective home, config, and DB paths for each check.

## Artifacts And Isolation

Workflow artifacts live under `.local/.psychevo-dev/ci/<run-id>/` unless the
caller selects an explicit artifact root. The runner creates separate output
paths for plans, step logs, package artifacts, checksums, and live/visual
diagnostics when those workflows run.

A relative explicit `cargo xtask ci run --artifact-root` or
`cargo xtask live run --artifact-root` value resolves once against the xtask
caller's working directory before any step changes its working directory or
creates child paths. Plans, run results, and implementation-only context files must
then carry absolute artifact, home, config, database, and isolated-workspace
paths. Deterministic ACP fixture commands, including the managed Codex ACP
offline launcher, must therefore remain valid when the Gateway launches them
from the isolated workspace rather than the xtask caller's working directory.

The visual profile owns and cleans its complete visual subtree before its first
step, including when an explicit artifact root is reused. Workbench manual
screenshots and Playwright output belong under that CI artifact root, not only
under `.local/playwright` or Playwright's default `test-results` directory.
The runner supplies separate screenshot and Playwright output roots, enables a
final screenshot for every opened deterministic test page, and writes a current-run
manifest that inventories screenshots plus retained failure traces and videos.
Consequently a failed or interrupted earlier invocation cannot satisfy a later
proof check, and `cargo xtask ci run --profile visual` retains reviewable TUI,
Workbench, and Linux native Desktop/Floating evidence in one run directory.

Each deterministic Workbench browser fixture binds its isolated managed
Gateway to an operating-system-assigned loopback port. Visual runs therefore
remain independent of the user-facing managed fallback range and of stale test
instances left by an interrupted earlier worker. Fixture teardown propagates a
failed managed stop instead of reporting successful cleanup.

Cross-surface profiling artifacts belong under
`profile/surface-comparison/`. The runner provides the comparison root, sample
count, built `pevo` binary, and Chromium selection explicitly. A successful
step requires the comparison manifest, Markdown report, TUI content-free JSONL
trace, Workbench trace, raw samples, and recomputed p50/p95/delta data. Failed
runs retain a partial manifest and logs. This profile uses only isolated local
home/config/database paths and never resolves real provider credentials.

The comparison manifest uses schema v2. Its shared presentation boundary is a
surface commit: completed terminal draw for TUI and observed DOM commit for
Workbench. Browser post-frame observations are diagnostic only. Validators
reject comparison v1 data, missing first-non-empty-output marks, cross-clock
subtraction, previous-sample RPC overlap, and screenshot or trace overhead in
measured samples. Workbench startup reports Composer shell commit, transport
connection, editable GUI commit, and draft-context readiness separately; it is
not ratio-compared with TUI process startup.

The initial comparison gate is structural. It requires one accepted request,
one public start, one public completion, no legacy terminal notification, no Web
Review workspace scan in the critical path, a committed optimistic feedback surface,
bounded frame application, and zero hidden-surface auxiliary reads. Clean and
deterministic dirty cohorts still report raw p50/p95 and both GUI-minus-TUI and
dirty-minus-clean deltas, but latency does not fail CI until three stable
canonical-runner baselines are separately reviewed and approved as a ratchet.

After a default-artifact-root run finishes or fails after creating its run
directory, the runner prunes `.local/.psychevo-dev/ci/` to the 10 most recent
numeric run directories. Non-numeric entries are ignored, and explicit
`--artifact-root` paths are not pruned.

The runner must set repo-local paths explicitly for steps that rely on
Psychevo state. Live profiles must not infer credentials from the user's normal
home, and must fail before running provider calls unless live execution is
explicitly allowed.

The `package` profile is artifact-only CD. It may build and checksum local
artifacts, but must not publish, deploy, tag, push, upload release assets,
create hosted draft releases, or mutate package registries.

## Script Adapters

Named CI/CD profiles must not be exposed through shell script adapters. Human,
agent, and future hosted-provider callers use `cargo xtask ci` directly so
there is one source of truth for profile selection, artifact-root reporting,
failure capture, and live opt-in policy.

Scripts that own specialized fixtures may remain callable as lower-level step
implementations when there is no native replacement yet. They are not CI/CD
profile entrypoints. TUI/VHS capture and live provider smoke are runner-owned
and are not exposed through public shell scripts.

Host prerequisite installation is not a CI/CD profile. The `visual` profile may
fail fast when VHS or Playwright host tools are missing and print the manual
commands reported by `cargo xtask doctor deps check`, but it must not install
packages implicitly.

Workbench Playwright validation commands must avoid passing conflicting Node
color controls into Playwright worker processes. The runner may remove
inherited `NO_COLOR` from Playwright test subprocesses because Playwright owns
worker colorization and may set `FORCE_COLOR` for those workers.

## Related Topics

- [065 CI/CD](../065-ci-cd/spec.md) defines the shared CI/CD foundation.
- [060 Automation](../060-automation/spec.md) defines product automation
  foundations, which are separate from CI/CD workflows.
- [200 pevo CLI](../200-pevo-cli/spec.md) defines user-facing CLI behavior.
- [210 pevo TUI](../210-pevo-tui/spec.md) defines terminal visual surfaces used
  by the `visual` profile.
- [240 pevo Web](../240-pevo-web/spec.md) defines Workbench surfaces used by
  the `web` and `visual` profiles.
- [420 Xtask Tools](../420-xtask-tools/spec.md) defines host prerequisite
  diagnostics and explicit installation.
