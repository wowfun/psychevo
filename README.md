<p align="center">
  <img src="assets/psychevo-logo.svg" alt="Psychevo" width="160">
</p>

# Psychevo

Psychevo is a public-alpha Rust coding-agent runtime with local product
surfaces for the shell, a fullscreen terminal UI, and ACP-speaking editors. The
project is focused on observable agent execution, explicit provider
configuration, durable local state, permission-aware tools, and reusable
runtime primitives for agents, skills, and editor integrations.

The main user-facing command is `pevo`. Use it for one-shot coding-agent turns,
interactive TUI work, ACP editor integration, local skills and agents, toolsets,
sessions, model/provider configuration, and usage/context inspection. Use
`peval` for local and live evaluation workflows.

## Current Capabilities

| Area | What exists today |
|------|-------------------|
| CLI turns | `pevo run` executes one coding-agent turn from the shell, using the same runtime as other surfaces. |
| Terminal UI | `pevo tui` provides a fullscreen local workspace with sessions, transcript history, slash commands, model controls, evidence rows, and local shell escapes. |
| ACP editor bridge | `pevo acp` runs a stdio Agent Client Protocol server for editors and other ACP clients. |
| Sessions and evidence | Local SQLite state stores sessions, messages, usage, estimated cost, and compact execution evidence for later inspection. |
| Providers and models | OpenAI Chat-compatible providers are configured through TOML, `.env` files, model metadata, and explicit model selection commands. |
| Skills and bundles | Filesystem-backed skills and bundles can be discovered, viewed, installed, enabled, disabled, and invoked from supported surfaces. |
| Agents and subagents | Agents can be listed, inspected, run, and managed, with subagent flows available through runtime tools and UI surfaces. |
| Runtime tools | Toolsets cover reading, writing, editing, searching, shell execution, web fetch, and MCP-backed tools where configured. |
| Permissions | Runtime permissions combine policy rules, config, and interactive approvals before sensitive actions run. |
| MCP | MCP servers can contribute tools through configured or client-provided sources, while Psychevo keeps runtime permission checks in charge. |
| Usage, context, and compaction | `pevo stats`, `pevo context`, session history, and compaction support help track cost, context pressure, and long-running work. |
| Evaluation | `peval` checks evaluation manifests, runs local and live candidate matrices, writes artifacts, and renders reports. |

## Not Yet

Psychevo does not yet claim product-complete self-evolution, autonomous
evaluation loops, workflow mining, or long-term memory. The current project is
the local execution substrate and product surface that those higher-level
capabilities would need before they can be useful or auditable.

## Install From Source

Psychevo is not documented here as a crates.io or binary release. Install from
the latest source with the helper script:

```bash
curl -fsSL https://raw.githubusercontent.com/wowfun/psychevo/main/scripts/install.sh | sh
```

From a checked-out repository, install the current checkout:

```bash
git clone https://github.com/wowfun/psychevo.git
cd psychevo
sh scripts/install.sh
pevo --help
```

The install script builds with `cargo install --locked --path
crates/psychevo-cli --force`, verifies `pevo --help`, and runs the idempotent
`pevo init` by default. Use `sh scripts/install.sh --no-init` to skip
initialization. Use `sh scripts/install.sh --with-peval` when you also want
the `peval` evaluation CLI installed and verified.

The workspace uses Rust 1.94 and edition 2024. If Rust/Cargo is missing, the
script asks before trying to install Rust. Windows Git Bash/MSYS/MINGW shells
can run the script, but source builds still require a working Rust toolchain and
native C/C++ build tools such as Visual Studio Build Tools or a compatible
MinGW setup.

For development without installing:

```bash
cargo run -p psychevo-cli -- --help
```

## Quick Start

Initialize the global Psychevo home:

```bash
pevo init
```

By default this creates `~/.psychevo/config.toml`, `~/.psychevo/.env`, and
`~/.psychevo/state.db`. The starter config selects DeepSeek. Put credentials in
the generated `.env` file, use project-local `.psychevo/.env`, inherit provider
API-key environment variables, or store a key from stdin:

```bash
pevo auth set deepseek --api-key-stdin
```

Confirm the active model configuration:

```bash
pevo model current
pevo model list
```

Run a one-shot task:

```bash
pevo run "summarize this repository"
```

Open the fullscreen terminal UI:

```bash
pevo tui
```

For editor integration, configure an ACP client to start `pevo acp`; see the
[ACP Configuration Guide](docs/acp-configuration.md).

Select a provider/model for one invocation:

```bash
pevo run -m deepseek/deepseek-chat "inspect the CLI entrypoints"
```

## Commands

| Command | Purpose |
|---------|---------|
| `pevo init` | Create or repair the global Psychevo home, starter config, `.env` template, and SQLite state. |
| `pevo run [message..]` | Run a live coding-agent task from the shell. |
| `pevo tui [message..]` | Start the fullscreen terminal UI, or process scripted stdin line by line. |
| `pevo acp` | Start the Agent Client Protocol stdio server for editor clients. |
| `pevo agent ...` | List, inspect, run, and manage local agents. |
| `pevo skill ...` | List, view, create, install, enable, disable, or scan local skills. |
| `pevo tool ...` | List and configure local toolsets. |
| `pevo session ...` | List, inspect, rename, archive, restore, export, or locally share sessions. |
| `pevo model ...` | Inspect configured models and explicitly fetch provider model catalogs. |
| `pevo config ...` | Inspect config paths and add OpenAI-compatible providers. |
| `pevo auth ...` | Inspect credential status and store provider API keys from stdin. |
| `pevo stats` | Show local token and estimated-cost statistics from SQLite state. |
| `pevo context --session <id\|latest>` | Inspect local context-window usage for a session. |
| `pevo smoke --db <path> --workdir <path>` | Run the deterministic fake-provider validation harness. |
| `peval ...` | Check, run, report, compare, and replay evaluation work. |

Run `pevo <command> --help` or `peval <command> --help` for flags.

## Documentation

- [ACP Configuration Guide](docs/acp-configuration.md)
- [Evaluation Guide](docs/evaluation/README.md)
- [TUI Troubleshooting](docs/troubleshooting/tui.md)

## Workspace

| Crate | Responsibility |
|-------|----------------|
| `psychevo-ai` | Provider protocol normalization and AI transport adapters. |
| `psychevo-agent-core` | Model-agnostic agent loop, tool traits, tool execution hooks, outcomes, and abort handling. |
| `psychevo-runtime` | Coding-agent runtime assembly, provider/model resolution, context, tools, persistence, skills, agents, permissions, and usage accounting. |
| `psychevo-acp` | ACP server packaging and runtime bridge used by `pevo acp`. |
| `psychevo-cli` | The `pevo` command-line entrypoint and fullscreen terminal UI. |
| `psychevo-eval` | The `peval` evaluation CLI, local fixture runner, artifact store, reports, and dataset inventory. |

## Development

Read [AGENTS.md](AGENTS.md) before changing the project.

Default broad validation:

```bash
scripts/validate.sh broad
```

Use narrower validation when it covers the changed behavior. Live-provider,
API-key, and live-service checks are opt-in only.

See [CONTRIBUTING.md](CONTRIBUTING.md) for the local contribution workflow.

## License

Psychevo is licensed under the [MIT License](LICENSE).
