---
name: 230. pevo-acp Testing
psychevo_self_edit: deny
---

# 230. pevo-acp Testing

Define deterministic acceptance coverage for the `psychevo-acp` crate,
`psychevo-acp` binary, `pevo acp` product wrapper, process setup, stdio server
packaging, and runtime-call construction.

Automation vocabulary and generic validation boundaries follow
[060 Automation](../060-automation/spec.md).

## Long-Term Acceptance Contract

- `psychevo-acp` exposes a library entrypoint that runs the ACP server over
  stdio. The `psychevo-acp` binary and `pevo acp` command call that same
  entrypoint.
- `pevo acp` remains a product wrapper and does not implement protocol
  behavior in `psychevo-cli`.
- `pevo acp` does not shell out to `pevo run` for prompting, cancellation,
  permissions, MCP, command handling, model selection, configuration updates,
  or session behavior.
- Process help describes `pevo acp` as the Agent Client Protocol stdio server
  for ACP-speaking editors and clients.
- ACP process setup uses `pevo` product path conventions for `PSYCHEVO_HOME`,
  `PSYCHEVO_DB`, `PSYCHEVO_CONFIG`, inherited environment, and cwd-relative
  paths.
- The server may create the home directory before accepting ACP requests and
  must use isolated caller-provided paths in tests.
- `psychevo-acp` constructs runtime calls directly from ACP session state and
  prompt inputs.
- Runtime remains responsible for session coordination, model resolution, tool
  surface assembly, capability source normalization, permission policy, command
  metadata, persistence, and evidence.
- ACP transport-local session and cancellation state is not durable evidence.
- Command availability is sent only after the client receives or can apply the
  ACP session id.

## Deterministic Tests

Required packaging and entrypoint coverage:

- `psychevo-acp` binary startup uses `AcpOptions::from_env()` and the shared
  stdio server entrypoint.
- `pevo acp` dispatches to the same shared entrypoint.
- `pevo acp --help` and top-level command metadata include the ACP stdio server
  description.
- Workspace dependency direction keeps `psychevo-acp` depending on
  `psychevo-runtime` while runtime does not depend on CLI or ACP.
- Product wrapper tests prove `pevo acp` does not call the `pevo run` command
  path.

Required process setup coverage:

- Default `PSYCHEVO_HOME` resolves to the product home convention.
- Explicit `PSYCHEVO_HOME`, `PSYCHEVO_DB`, and `PSYCHEVO_CONFIG` override the
  defaults.
- The server creates the home directory when needed and preserves
  caller-provided isolated config and database paths.
- Relative workdirs and file paths resolve from the server process cwd.
- Inherited environment variables reach runtime provider and auth resolution
  without reading unrelated user config or credential stores.

Required ACP/runtime wiring coverage:

- Session creation and loading produce ACP session actors backed by runtime
  session ids with source `acp`.
- Prompt handling passes cwd, session id, mode, model, image inputs, inherited
  environment, config path, database path, approval handler, and ACP-provided
  MCP servers into normal runtime inputs.
- Cancellation acts on the in-flight runtime control handle for the ACP actor
  without deleting durable session evidence.
- ACP-provided MCP server declarations convert into runtime MCP source inputs
  without granting trust outside normal capability normalization.
- Supported slash-command prompts are handled locally before invoking the
  model-backed runtime path.
- Command advertisements use the shared command catalog and ACP capability
  filter, hide TUI-only commands, and are emitted after the session id is
  usable by the client.
- `/diff` is advertised for ACP sessions, is accepted while an agent turn is
  active, emits a synthetic tool-call update with ACP `ToolCallContent::Diff`,
  stores summary and truncation metadata only in raw tool output, sends no
  assistant text fallback, and leaves runtime model-context messages unchanged.
- Transport-local active-session and cancellation maps are cleared on close or
  cancellation without persisting ACP-only state.

## Validation

Relevant narrow validation:

- `cargo test -p psychevo-acp`
- `cargo test -p psychevo-cli`
- `cargo test -p psychevo-runtime command_registry`

Broad deterministic validation:

- `scripts/validate.sh broad`

## Validation Boundaries

- Tests should use local deterministic JSON-RPC/stdio harnesses and fake or
  test providers.
- Tests must isolate `PSYCHEVO_HOME`, `PSYCHEVO_DB`, `PSYCHEVO_CONFIG`, cwd,
  inherited environment, temporary files, and cancellation state.
- ACP packaging tests should assert product entrypoint behavior and runtime
  input construction, not protocol semantics owned by [027 ACP](../027-acp/spec.md).
- Live editor-client, registry publishing, HTTP/WebSocket transport, and real
  provider validation remain opt-in.

## Opt-In Live ACP Validation

When a change touches ACP peer runtime option handling, run an explicit live
validation path for both directions when local test credentials and binaries
are available:

- Psychevo as ACP client against OpenCode ACP: verify live `configOptions`
  expose OpenCode modes, selecting a mode changes the next turn's peer session,
  and thinking/text updates stream incrementally.
- Psychevo as ACP server with an ACP client harness: verify `mode` is exposed
  as current-runtime mode, setting it updates subsequent runtime input, and
  context usage updates are emitted when runtime accounting is available.

These checks are not part of the default deterministic gate and must report the
exact command, isolated config paths, and any skipped prerequisite.

## Related Topics

- [230 pevo-acp](spec.md) defines the product packaging contract.
- [027 ACP](../027-acp/spec.md) defines protocol mapping and runtime
  boundaries.
- [056 MCP](../056-mcp/spec.md) defines MCP source normalization boundaries.
- [200 pevo CLI Testing](../200-pevo-cli/testing.md) covers shared CLI command
  behavior.
