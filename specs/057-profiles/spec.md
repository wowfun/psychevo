---
name: 057. Profiles
psychevo_self_edit: deny
---

# 057. Profiles

Define Psychevo profiles: isolated local homes for configuration, credentials,
state, sessions, caches, skills, agents, and managed processes.

## Scope

- active profile home resolution
- default and named profile directory layout
- profile metadata
- `pevo -p/--profile`
- `pevo profile` management commands
- clone and alias behavior
- interaction with workdirs, project `.psychevo`, Gateway, TUI, and Workbench

Out of scope:

- request-level profile selection in Gateway or runtime protocols
- cross-profile session browsing or aggregation
- switching profiles inside one running Gateway/Workbench process
- profile import/export archives
- copying runtime state when cloning profiles
- automatic bundled skill or agent seeding beyond existing `pevo init` behavior

## Concepts

A profile is the active Psychevo home. It is not a workspace protocol field.
Interactive context is represented by the current workdir and the existing
`GatewayRequestScope { workdir, source }` shape. A workdir may be a code
project, a plain directory, or a GUI-created workspace; runtime, Gateway, and
session storage do not persist a project/workspace type distinction.

The default profile home is:

- `~/.psychevo`

Named profile homes live under the default profile root:

- `~/.psychevo/profiles/<name>`

The root `~/.psychevo` also acts as the profile registry. It may contain
registry-level files such as `active_profile` and the `profiles/` directory in
addition to the default profile's own `config.toml`, `.env`, `state.db`,
`sessions/`, `logs/`, `cache/`, `skills/`, `agents/`, and `gateway/` data.

A workdir-local `<workdir>/.psychevo` remains an overlay for config, agents,
skills, and other directory-scoped resources. It does not select, own, or
override the active profile.

GUI and desktop shells may create user-facing workdirs under a configurable
workspace root. The default root is `~/workspaces`; the default no-project GUI
workdir is `<workspace-root>/general`. This root is profile configuration, not
profile-owned data: multiple profiles may use the same root while keeping
credentials, sessions, and profile-level configuration isolated.

## Resolution

The active profile is resolved before command execution and before any child
Gateway/ACP/TUI process is launched.

Precedence:

1. Global CLI `-p, --profile <name>`
2. Registry file `<registry-root>/active_profile`
3. Default profile

`PSYCHEVO_HOME` remains supported. If it points at the registry root, the
resolution rules above apply inside that root. If it points at a named profile
home under `<registry-root>/profiles/<name>`, that profile home is treated as an
already-resolved active profile. If it points somewhere else, that directory is
treated as a custom default-style active home.

After resolution, commands pass the resolved active profile home to existing
runtime layers as `PSYCHEVO_HOME`. `PSYCHEVO_DB` and `PSYCHEVO_CONFIG` keep
their existing override semantics and may bypass the home for the specific
database or config path they name.

Selecting a missing named profile fails with a clear local error that points to
`pevo profile create <name>`.

## Initialization

`pevo init` initializes the active profile home. A newly created named profile
is immediately usable and contains at least:

- `config.toml`
- `.env`
- `state.db`
- `sessions/`
- `logs/`
- `cache/`
- `skills/`
- `agents/`
- `profile.toml`

`profile.toml` stores small profile metadata. The first slice supports
`description` and `description_auto`. Missing or unreadable metadata must not
prevent listing or selecting a profile.

## Commands

`pevo profile` owns profile management:

- `pevo profile list`
- `pevo profile show [name]`
- `pevo profile create <name> [--description <text>] [--clone] [--clone-from <name>] [--alias[=<command>]]`
- `pevo profile use <name|default>`
- `pevo profile delete <name> --yes`
- `pevo profile rename <old> <new>`
- `pevo profile alias <name> [--name <command>] [--remove]`

Profile names are local identifiers, not paths. The accepted v1 grammar is
lowercase ASCII alphanumeric plus `-` and `_`, starting with an alphanumeric
character. `default` is reserved as the built-in default profile selector.

`profile list` marks the active profile and reports the resolved home path.
`profile show` reports metadata, home path, and whether the profile is active.

`profile use` writes the sticky registry selection. Using `default` removes or
clears the sticky selection.

`profile delete` never deletes the default profile and never deletes the active
profile. It removes the named profile home only after explicit confirmation or
`--yes`.

`profile rename` moves one named profile directory to another name and updates
`active_profile` when the renamed profile was sticky-active.

## Clone

`pevo profile create <name> --clone` copies shareable setup from another
profile. The source defaults to the currently active profile and may be
overridden by `--clone-from <profile>`.

Clone copies:

- `config.toml`
- `.env`
- `skills/`
- `agents/`

Clone does not copy:

- `state.db`, `state.db-wal`, or `state.db-shm`
- `sessions/`
- `gateway/`
- `logs/`
- `cache/`
- snapshots or trace artifacts

## Alias

`--alias` and `profile alias` create a local shell wrapper for the profile. The
default alias command is the bare profile name, for example `coder` wrapping
`pevo -p coder "$@"`.

Alias creation must reject reserved command names and commands already found on
`PATH` unless the existing file is a Psychevo-managed wrapper for the same
profile. The first slice writes wrappers into `~/.local/bin`.

## Runtime And UI Behavior

One managed Gateway server belongs to exactly one active profile. Managed state
lives under that profile home at `$PSYCHEVO_HOME/gateway/`. Starting, opening,
stopping, and checking status only affect the active profile's managed server.
Managed child processes receive the resolved active profile home through
`PSYCHEVO_HOME`.

Gateway request schemas do not include a profile selector. Workbench may display
the profile returned by `initialize`, but v1 does not switch profiles inside the
browser. Starting a different profile requires launching a different `pevo`
process or using `pevo -p <name> web`.

TUI and CLI surfaces should show the current profile when it is not the default
profile. The default profile should not be repeated in normal status chrome.

Session history is profile-local in v1. A profile's `state.db` is the boundary
for history, usage, Gateway source bindings, and runtime session state.
