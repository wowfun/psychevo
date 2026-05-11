---
name: 025. CLI
psychevo_self_edit: deny
---

Define Psychevo's command-line interface foundation semantics.

This topic specializes [020 Interfaces](../020-interfaces/spec.md) for
process-oriented command-line entrypoints. It does not define concrete `pevo`
commands or flags; shared command discovery, naming, argument, alias, and
output-contract conventions belong to [026 Commands](../026-commands/spec.md).

## Scope

- CLI invocation semantics
- argv, cwd, stdin, stdout, and stderr boundaries
- human and machine output modes
- exit status expectations
- CLI-owned environment and config boundary rules

Out of scope:

- concrete product commands, flags, aliases, or help text
- provider, model, tool, storage, or session schemas
- terminal UI rendering, interactive approval UX, or background services
- stable JSON event payload schemas or error-code taxonomies

## Invocation

A CLI invocation starts with process argv, process cwd, inherited process
environment, and standard streams. A CLI product entrypoint may translate these
process inputs into runtime invocation inputs, but it must route accepted agent
work through `psychevo-runtime`.

CLI argument parsing failures happen before runtime invocation. Runtime
configuration failures, session-start rejection, and before-agent-start
rejection must remain distinguishable from failed agent execution.

When a CLI command accepts stdin as user input, stdin is part of the caller
input for that command. Commands intended for automation should define whether
stdin is ignored, exclusive with argv input, or appended to argv input.

## Streams

Human-readable command results should use stdout when they are the command's
primary result. Human-readable diagnostics, progress, and errors should use
stderr unless a command's selected machine format explicitly defines structured
error output on stdout.

Machine-readable output modes should avoid mixing human prose into the same
stream. If a command emits newline-delimited JSON, each stdout line must be one
complete JSON object.

## Exit Status

CLI commands exit successfully only when the requested command completed under
that command's success criteria. Usage errors, invalid configuration,
session-start rejection, before-agent-start rejection, provider failures, and
failed terminal outcomes exit non-zero.

## Environment

Environment variables may provide command-level configuration, config
locations, and isolated test or automation controls. A CLI command must not
mutate global process environment to implement per-invocation `.env` loading.

Product CLI specs own concrete environment variable names. Provider and model
resolution semantics remain owned by [120 Provider Registry](../120-provider-registry/spec.md).

## Related Topics

- [020 Interfaces](../020-interfaces/spec.md) defines caller-facing invocation,
  observation, completion, and control semantics.
- [026 Commands](../026-commands/spec.md) defines shared command contract
  conventions across product command surfaces.
- [200 pevo CLI](../200-pevo-cli/spec.md) defines the concrete `pevo` product
  command line.
- [120 Provider Registry](../120-provider-registry/spec.md) defines
  provider/model configuration and resolution.
