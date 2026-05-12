---
name: 110. Coding Core Tools
psychevo_self_edit: deny
---

Define the required `coding-core` toolset for the built-in coding-agent capability.

## Scope

- `coding-core` toolset semantics
- four core coding tools: `read`, `edit`, `write`, and `bash`
- model-visible JSON result contracts for those four tools
- working-context and resource-boundary expectations for core tools
- observable failure, truncation, timeout, abort, and conflict behavior

Out of scope:
- tool declaration schemas, parameter names, JSON Schema shapes, Rust APIs, or handler signatures
- provider-specific tool-call fields or wire formats
- CLI commands, terminal rendering, interactive PTY behavior, background process management, or process registry behavior
- approval UX, sandbox behavior, deny lists, dangerous-command policy, or concrete resource policy
- binary/image reading, append/delete/rename tools, search/list tools, memory tools, skill adjunct tools, or self-evolution tools
- storage schemas, evidence record shapes, or replay formats

## Toolset Contract

`coding-core` is the default toolset required by [100 Coding Agent](../100-coding-agent/spec.md). It directly contains exactly these tools:
- `read`
- `edit`
- `write`
- `bash`

`coding-core` does not include search, list, grep, find, background process, memory, skill, or project-discovery tools. A model may use `bash` for command-line search or listing when the runtime resource boundary allows it. Optional skill tools are adjacent runtime tools defined by [055 Skills](../055-skills/spec.md), not members of `coding-core`.

Each `coding-core` tool operates through the runtime-resolved working context accepted for the coding-agent invocation. Tools must not independently choose a different project, filesystem, process environment, or resource boundary.

Runtime must expose a model-visible tool declaration only when the matching execution binding is available for the same agent invocation and generation-request tool declaration snapshot. [007 Tool Surface](../007-tool-surface/spec.md) owns snapshot visibility semantics.

## JSON Result Contract

Each `coding-core` tool result is a model-visible JSON object. This is a `110` contract for these four tools only; [007 Tool Surface](../007-tool-surface/spec.md) does not require all tools to use this result shape.

Each tool uses a top-level `error` field for failure explanation. A missing, null, or empty `error` means the result does not report a failure through that field. Tool-specific failure rules may also use other fields such as `exit_code` or `success`.

Resource denial, resource deferral, timeout, size bound, truncation failure, abort, ambiguity, not-found, and conflict conditions must be observable in the JSON result or as before-agent-start rejection. They must not be silently hidden.

Output that may grow large must be bounded. When material is truncated, the result must make truncation observable through a stable field or a clear adjacent result field. This spec does not define concrete byte, line, or time limits.

## `read`

`read` reads text from the working context. It is text-only in this slice.

Successful text reads return a JSON object with stable fields:
- `content`: model-visible text content
- `total_lines`: total line count when known
- `file_size`: file size when known
- `truncated`: whether the returned content is incomplete
- `hint`: optional guidance for reading the next or narrower range
- `error`: failure explanation when the read fails
- `similar_files`: optional candidate paths when the target cannot be found

Binary files and images are not inlined by `read`. They must return an error or clear hint instead of binary or base64 content.

Read ranges, pagination, and truncation are stable behaviors, but this spec does not freeze parameter names or numeric limits.

## `write`

`write` creates or completely replaces one target's text content in the working context.

`write` is not append, delete, rename, or patch. It writes the complete intended content for its target.

`write` creates missing parent directories when the resource boundary allows it.

Successful writes return a JSON object with stable fields:
- `path`: target path or equivalent target identifier
- `bytes_written`: number of bytes written when known
- `dirs_created`: whether parent directories were created
- `error`: failure explanation when the write fails

## `edit`

`edit` modifies existing text content in the working context.

`edit` supports two semantic modes:
- `replace`: replace target text in an existing file or resource
- `patch`: apply a patch to existing files or resources

The `patch` mode may update existing files or resources, including multiple existing targets. It must not create or delete files in this slice. New files belong to `write`; deletion belongs to a later capability or to allowed `bash` use.

Successful edits return a JSON object with stable fields:
- `success`: true when the edit was applied
- `diff`: unified diff or equivalent change text
- `files_modified`: modified paths or equivalent target identifiers
- `error`: failure explanation when the edit fails

Ambiguous matches, not-found targets, no-change edits, stale content conflicts, and resource-denied writes must be observable in the JSON result. Same-resource mutations must be ordered or conflicts must be visible.

This spec defines semantic modes and result material, not concrete patch syntax or parameter names.

## `bash`

`bash` executes a foreground bounded command through the runtime-bound process or shell resource for the working context.

`bash` does not provide background process management, PTY interaction, process registry operations, or long-lived server orchestration in this slice.
The foreground command must run without consuming caller stdin. When runtime
aborts or times out the command, it must interrupt the foreground command tree
as far as the local platform permits instead of only marking the direct shell
wrapper as cancelled. Output collection must also settle promptly when a killed
command leaves inherited stdout or stderr descriptors open.

`bash` returns a JSON object with stable fields:
- `output`: bounded command output
- `exit_code`: process exit code or equivalent terminal status when known
- `error`: failure explanation when execution fails or the command is treated as failed
- `exit_code_meaning`: optional explanation for recognized non-zero exit codes

Timeout, abort, command-start failure, resource denial, command-tree cleanup
limits, and output truncation must be observable.

Exit code `0` is success unless another runtime boundary reports failure. Non-zero exit codes are failed tool results by default.

The first built-in non-zero explanation table covers:
- `grep`, `rg`, `ag`, and `ack` exit code `1`
- `diff` exit code `1`
- `test` and `[` exit code `1`

When the table matches, the result remains a failed tool result but includes `exit_code_meaning` so the model can interpret the non-zero status without guessing.

## Related Topics

- [100 Coding Agent](../100-coding-agent/spec.md) requires the `coding-core` toolset for default coding-agent invocations.
- [110 Tool I/O](tool-io.md) defines the first implementation slice parameter and JSON result contract.
- [110 Testing](testing.md) defines acceptance scenarios and validation expectations.
- [004 Runtime Contract](../004-runtime-contract/spec.md) defines agent-invocation assembly and tool surface wiring.
- [007 Tool Surface](../007-tool-surface/spec.md) defines agent-invocation scoped tool declarations, generation-request tool declaration snapshots, execution bindings, and toolset expansion.
- [009 Resource Surface](../009-resource-surface/spec.md) defines resource decisions that may affect tool execution.
- [005 Durable Evidence](../005-durable-evidence/spec.md) defines durable linkage for tool requests, outcomes, and result artifacts.
- [055 Skills](../055-skills/spec.md) defines optional skill adjunct tools.
