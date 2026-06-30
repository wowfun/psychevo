---
name: 040. Environment - Windows Compatibility
psychevo_self_edit: deny
---

# Windows Compatibility

This attachment defines Psychevo's native Windows compatibility slice for Git
Bash and Windows path parsing. It extends [040 Environment](spec.md) and is the
normative source for Windows shell-family and host-path behavior.

## Scope

This slice covers:

- native Windows local execution through Git Bash
- host path parsing and normalization at host boundaries
- permission, sandbox, workspace, ACP, Gateway, TUI, and Workbench boundaries
  that consume host paths
- deterministic validation plus opt-in real Windows Git Bash smoke validation

Out of scope:

- PowerShell or `cmd.exe` as supported runtime shells
- WSL as a supported runtime host
- long-term compatibility for pre-release SQLite state schemas
- changing model-facing file tool path inputs or workspace-relative output
  paths from strings to objects

## Shell Contract

Native Windows runtime shell execution is Git-Bash-only. `exec_command`, user
shell escape, and Web terminal sessions must resolve and launch Git Bash. They
must not silently fall back to PowerShell or `cmd.exe`.

Git Bash discovery is:

1. `PSYCHEVO_GIT_BASH_PATH`, when set and non-empty
2. Git for Windows paths derived from `git.exe` on `PATH`
3. common Git for Windows install paths
4. `bash.exe` on `PATH`, only if it behaves as Git Bash

The selected Git Bash must be paired with a working `cygpath.exe` for shell
virtual path resolution. If native Windows execution needs Git Bash and no valid
Git Bash is available, Psychevo fails before launching the operation with an
actionable message that names Git for Windows and `PSYCHEVO_GIT_BASH_PATH`.

Git Bash command invocation uses POSIX shell arguments:

- normal command: `bash -c <command>`
- approved login command: `bash -lc <command>`
- interactive terminal: `bash --login -i`

Explicitly requested PowerShell, `cmd.exe`, or another non-Git-Bash shell on
native Windows is unsupported in this slice and must fail closed.

## Path Views

Durable cwd storage stays a canonical native path string. Psychevo does not
persist structured path views for this slice.

Runtime may derive a private path view at a host boundary when a caller needs
both a native path and a display path:

```text
PathView {
  uri: string,
  native: string,
  display: string,
  convention: "posix" | "windows" | "gitBash" | "cygwin" | "wsl" | "fileUri"
}
```

`uri` is a derived identity candidate, not a stored source of truth. It uses a
`file:` URI with Windows drive and UNC paths encoded independently from the
current host platform.

`native` is the string passed to native filesystem and process APIs on the
current host. On native Windows it is a Windows path. On POSIX hosts it is the
POSIX path. For session and automation cwd persistence, this native string is
the stored value.

`display` is the user-facing path. Under native Windows Git Bash it defaults to
Git Bash style such as `/c/Users/alice/project`, because that is copyable into
the supported shell. On POSIX hosts it defaults to the native POSIX string.

`convention` records the input or display convention that most directly
explains the path.

Workspace-relative file paths, diff paths, and model-facing file tool path
results remain strings. Runtime accepts raw string inputs from models and users,
normalizes at the host boundary, and returns compact relative strings when the
result is workspace-relative.

## Path Parsing

The host path parser must recognize these inputs before falling back to
host-native `Path` interpretation:

- Windows drive paths: `C:\repo`, `C:/repo`
- UNC paths: `\\server\share\repo`
- supported verbatim forms: `\\?\C:\repo`, `\\?\UNC\server\share\repo`
- file URIs
- Git Bash/MSYS drive paths: `/c/repo`, `/c:/repo`
- Cygwin drive paths: `/cygdrive/c/repo`
- WSL mount drive paths: `/mnt/c/repo`

Drive-relative paths such as `C:repo` and unsupported device or null namespace
paths must fail closed instead of being joined to cwd.

On native Windows Git Bash, POSIX absolute paths that do not have a drive
prefix, such as `/tmp`, are shell-virtual paths. Runtime resolves them with the
selected Git Bash `cygpath -w -- <path>` before using native filesystem or
permission APIs. If `cygpath` fails, the operation fails closed.

Permission matching, sandbox write checks, ACP local resource paths, Gateway
scopes, and terminal cwd values must normalize user or shell input before using
native filesystem APIs. Session/workspace and automation cwd lookups compare the
stored canonical native cwd string. `/c/repo`, `C:\repo`, and `C:/repo` should
normalize to the same native cwd before storage or lookup on native Windows.

## Storage and Wire

Session and automation state stores cwd as `cwd TEXT NOT NULL`, containing the
canonical native cwd string. The database does not store `cwd_uri`,
`cwd_json`, or any serialized path-view object.

Gateway protocol cwd, project cwd, home, config/database paths, executable
paths, and absolute external roots remain strings. Workbench may derive a
display string at render time, but it must not require durable path-view
metadata to list, filter, resume, or launch sessions.

Because Psychevo is not released yet, this slice does not add compatibility
shims for intermediate pre-release schemas. Older local state schemas are
rejected through the normal reset-state guidance instead of migrated through
unused path metadata columns.

## Validation

Deterministic validation must cover:

- pure parsing for Windows drive, UNC, verbatim, file URI, Git Bash/MSYS,
  Cygwin, WSL mount, and rejected drive-relative/device forms
- Git Bash discovery and missing-Git-Bash hard failure with fakes
- fake `cygpath` resolution for `/tmp` and failure propagation
- permissions and filesystem grants treating Git Bash and native Windows forms
  as the same path
- session and automation state lookups by normalized native cwd string
- Gateway protocol generation and Workbench typechecking after path boundary
  changes

Real Windows Git Bash validation is opt-in. The live smoke entrypoint should be
skippable on non-Windows hosts and cover shell command execution, `/tmp`,
drive-prefix cwd, file tool path normalization, permission matching, and Web
terminal startup.
