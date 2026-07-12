<p align="center">
  <img src="assets/psychevo-logo.svg" alt="Psychevo" width="160">
</p>

English | [简体中文](README.zh-CN.md)

# Psychevo

Psychevo is a local coding agent for work in an existing codebase. Set up a
provider, then run tasks from the terminal, local Workbench, or a supported ACP
editor. Configuration, permission policy, and session history stay local and
inspectable.

## What you can do

- Work where you prefer: run a one-shot task with `pevo run`, use the full-screen
  terminal UI, open local Workbench, launch Desktop from a source checkout, or
  connect a supported ACP editor.
- Choose a provider, model, and agent: configure an OpenAI-compatible provider,
  then use local agents or managed Codex, OpenCode, and Hermes ACP backends when
  they are available.
- Extend and delegate: combine filesystem-backed skills, plugins, hooks,
  MCP-backed tools, local agents, and subagent flows.
- Connect approved WeChat, Telegram, Feishu, and Lark conversations through the
  managed Gateway when chat is the right interface.

Psychevo focuses on dependable local execution today. It does not claim
autonomous evaluation loops, workflow mining, or long-term memory.

## Install From Source

Psychevo currently installs from source. Install Git, Rust/Cargo, a native
compiler, Node.js, and pnpm, then run the checkout-local installer:

```bash
git clone https://github.com/wowfun/psychevo.git
cd psychevo
sh scripts/install.sh --check
sh scripts/install.sh
```

The installer builds the local `pevo` binary and Workbench Web UI assets. See
the [Installation Guide](docs/install.md) for prerequisites, diagnostics,
Windows Git Bash notes, enterprise network guidance, and development commands.

For a CLI-only install, use the underlying Cargo command:

```bash
cargo install --locked --path crates/psychevo-cli --force
```

For development without installing:

```bash
cargo run -p psychevo-cli -- --help
pnpm --filter @psychevo/workbench dev
```

## Quick Start

Set up Psychevo once for your provider and model:

```bash
pevo setup
```

The wizard initializes Psychevo home, configures a provider and model, stores
or references an API key, checks Web UI assets, and finishes with a doctor
summary. Confirm the local setup without contacting a provider:

```bash
pevo doctor
```

Run your first task from the project you want to work on:

```bash
pevo run "summarize this repository"
```

Open an interactive workspace when you need one:

```bash
pevo tui
pevo web
```

For editor integration, configure an ACP client to start `pevo acp`; see the
[ACP Configuration Guide](docs/acp-configuration.md).

Select a provider/model for one invocation:

```bash
pevo run -m deepseek/deepseek-chat "inspect the CLI entrypoints"
```

## Documentation

- [Installation Guide](docs/install.md)
- [ACP Configuration Guide](docs/acp-configuration.md)
- [Channels Guide](docs/channels/README.md)
- [TUI Troubleshooting](docs/troubleshooting/tui.md)
- [Contributing Guide](CONTRIBUTING.md)

## More ways to work

| When you need to... | Use Psychevo to... |
|---------------------|-------------------|
| Work from a shell | Run a coding-agent turn with `pevo run`, including local tools in the selected cwd. |
| Stay in an interactive workspace | Use `pevo tui`, `pevo web`, or `pevo desktop` from a source checkout. |
| Work with editors or compatible agents | Run the `pevo acp` bridge for ACP-speaking editors, or choose an available managed Codex, OpenCode, or Hermes ACP agent in Workbench. |
| Configure local models and history | Use profiles, provider and auth commands, and local SQLite-backed sessions with usage, estimated cost, and execution evidence. |
| Extend or delegate a task | Manage skills, plugins, hooks, toolsets, local agents, and subagent flows. |
| Serve tools or connect chat | Run the loopback Gateway API, expose the MCP stdio server, or configure approved Channels through the managed Gateway. |
| Check what is ready | Run `pevo doctor` without live provider calls unless you explicitly request them. |

## Commands

| Command | Purpose |
|---------|---------|
| `pevo init` | Create or repair the active Psychevo profile home, starter config, `.env` template, and SQLite state. |
| `pevo setup` | Run the interactive first-run setup wizard and finish with local diagnostics. |
| `pevo doctor` | Run deterministic local diagnostics; use `--live` only when provider network checks are intended. |
| `pevo run [message..]` | Run one coding-agent turn from the shell. |
| `pevo tui [message..]` | Start the fullscreen terminal UI, or process scripted stdin line by line. |
| `pevo web` | Open the managed local Workbench Web UI for the current cwd. |
| `pevo desktop` | Open the native Desktop app from a source checkout. |
| `pevo gateway ...` | Open, start, inspect, stop, or restart the managed local Gateway Web Shell. |
| `pevo serve` | Run the strict headless local Gateway API server on loopback. |
| `pevo acp` | Start the Agent Client Protocol stdio server for editor clients. |
| `pevo mcp serve` | Start the Model Context Protocol stdio server. |
| `pevo profile ...` | List, inspect, create, switch, alias, rename, and delete local profiles. |
| `pevo agent ...` | List, inspect, run, and manage local agents. |
| `pevo skill ...` | List, view, create, install, configure, audit, and manage local skills and bundles. |
| `pevo plugin ...` | List, inspect, install, and enable local plugins. |
| `pevo hooks ...` | List, trust, enable, and disable local hooks. |
| `pevo tool ...` | List and configure local toolsets. |
| `pevo session ...` | List, inspect, rename, archive, restore, export, or locally share sessions. |
| `pevo model ...` | Inspect configured models and explicitly fetch provider model catalogs. |
| `pevo config ...` | Inspect config paths and add OpenAI-compatible providers. |
| `pevo auth ...` | Inspect credential status, run provider setup, and store provider API keys. |
| `pevo stats` | Show local token and estimated-cost statistics from SQLite state. |
| `pevo context --session <id\|latest>` | Inspect local context-window usage for a session. |

Run `pevo <command> --help` for flags and subcommands.

## Development

Read [AGENTS.md](AGENTS.md) before changing the project. Psychevo is spec-first:
before behavior, public docs, tests, or workflow changes, read and update the
best-fit `specs/<topic>/spec.md`.

Rust workspace broad gate:

```bash
cargo xtask ci run --profile rust-broad
```

Use narrower validation when it covers the changed behavior. Live-provider,
API-key, and live-service checks are opt-in only.

Repo-local live validation is xtask-owned:

```bash
cargo xtask init dev-env
cargo xtask live run
cargo xtask live run --env isolated
cargo xtask live run --suite provider
```

Useful local commands:

```bash
cargo run -p psychevo-cli -- --help
cargo test -p psychevo-cli smoke_cli
pnpm --filter @psychevo/workbench build
pnpm --filter @psychevo/workbench dev
```

## Workspace for contributors

| Crate or package | Responsibility |
|------------------|----------------|
| `psychevo-ai` | Provider protocol normalization and AI transport adapters. |
| `psychevo-agent-core` | Model-agnostic agent loop, tool traits, tool execution hooks, outcomes, and abort handling. |
| `psychevo-runtime` | Coding-agent runtime assembly, provider/model resolution, context, tools, persistence, skills, agents, permissions, and usage accounting. |
| `psychevo-gateway` | Local Gateway API and WebSocket server used by Web and CLI surfaces. |
| `psychevo-acp` | ACP server packaging and runtime bridge used by `pevo acp`. |
| `psychevo-cli` | The `pevo` command-line entrypoint, TUI, managed Web/Gateway commands, setup, and diagnostics. |
| `apps/workbench` | The Vite/React Workbench Web UI served by managed `pevo web` flows. |
| `packages/*` | Shared TypeScript protocol, client, host, component, and asset packages. |

## License

Psychevo is licensed under the [MIT License](LICENSE).
