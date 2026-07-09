<p align="center">
  <img src="assets/psychevo-logo.svg" alt="Psychevo" width="160">
</p>

# Psychevo

Psychevo is a public-alpha local coding-agent runtime written in Rust. The main
product command is `pevo`, which runs one-shot agent turns, opens terminal and
browser workspaces, manages local profiles/configuration/credentials, and stores
durable session evidence in local SQLite state.

The project is intentionally local-first: provider credentials stay in explicit
config or `.env` locations, tool execution is permission-aware, and the CLI,
TUI, Web UI, ACP bridge, and Gateway surfaces share the same runtime foundation.

## Current Surfaces

| Area | What exists today |
|------|-------------------|
| CLI turns | `pevo run` executes a coding-agent turn from the shell using the same runtime as the interactive surfaces. |
| Terminal UI | `pevo tui` opens a fullscreen workspace with sessions, transcripts, slash commands, model controls, evidence rows, and local shell escapes. |
| Web UI | `pevo web` and `pevo gateway open` launch the managed local Workbench for the current cwd. |
| Gateway API | `pevo serve` starts a strict loopback API server for headless or managed local clients. |
| ACP editors | `pevo acp` runs a stdio Agent Client Protocol bridge for ACP-speaking editors. |
| Profiles | `pevo profile` manages named Psychevo homes for separate config, credentials, skills, agents, and Gateway state. |
| Sessions and evidence | Local SQLite state stores sessions, messages, usage, estimated cost, and compact execution evidence for later inspection. |
| Providers and auth | `pevo model`, `pevo config`, and `pevo auth` inspect and configure OpenAI-compatible provider backends and credentials. |
| Skills and agents | Filesystem-backed skills, bundles, local agents, generated peer agents, and subagent flows are available from supported surfaces. |
| Permissions and tools | Runtime tools cover reading, writing, editing, searching, shell execution, web fetch, and MCP-backed tools under permission policy. |
| Diagnostics | `pevo doctor` checks local config, auth/model selection, Web assets, Gateway state, and required tools without live provider calls unless requested. |

Psychevo does not yet claim product-complete self-evolution, autonomous
evaluation loops, workflow mining, or long-term memory. The current project is
the local execution substrate and product surface those higher-level systems
would need before they can be useful or auditable.

## Install From Source

Psychevo is not documented here as a crates.io package or binary release. Source
installs build the local `pevo` binary with Cargo, build Workbench Web UI
assets, and copy those assets beside the installed binary.

Install Git, Rust/Cargo, a native compiler, Node.js, and pnpm first. Then
install from a checkout:

```bash
git clone https://github.com/wowfun/psychevo.git
cd psychevo
sh scripts/install.sh --check
sh scripts/install.sh
```

See the [Installation Guide](docs/install.md) for prerequisites, diagnostics,
Windows Git Bash notes, enterprise network guidance, and development commands.

For CLI-only installs, use the underlying Cargo command:

```bash
cargo install --locked --path crates/psychevo-cli --force
```

For development without installing:

```bash
cargo run -p psychevo-cli -- --help
pnpm --filter @psychevo/workbench dev
```

## Quick Start

Run the first-run wizard:

```bash
pevo setup
```

The wizard can initialize Psychevo home, configure a provider/model, store or
reference an API key, check Web UI assets, and finish with a doctor summary.

For a lower-level idempotent initializer:

```bash
pevo init
```

By default this creates or repairs `~/.psychevo/config.toml`,
`~/.psychevo/.env`, `~/.psychevo/state.db`, and supporting local directories.
Put credentials in the generated `.env`, a project-local `.psychevo/.env`,
inherited provider API-key environment variables, or store a key from stdin:

```bash
pevo auth set deepseek --api-key-stdin
```

Check local setup without contacting providers:

```bash
pevo doctor
pevo model current
pevo model list
```

Run a one-shot task:

```bash
pevo run "summarize this repository"
```

Open interactive surfaces:

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

## Commands

| Command | Purpose |
|---------|---------|
| `pevo init` | Create or repair the active Psychevo profile home, starter config, `.env` template, and SQLite state. |
| `pevo setup` | Run the interactive first-run setup wizard and finish with local diagnostics. |
| `pevo doctor` | Run deterministic local diagnostics; use `--live` only when provider network checks are intended. |
| `pevo run [message..]` | Run one coding-agent turn from the shell. |
| `pevo tui [message..]` | Start the fullscreen terminal UI, or process scripted stdin line by line. |
| `pevo web` | Open the managed local Workbench Web UI for the current cwd. |
| `pevo gateway ...` | Open, start, inspect, stop, or restart the managed local Gateway Web Shell. |
| `pevo serve` | Run the strict headless local Gateway API server on loopback. |
| `pevo acp` | Start the Agent Client Protocol stdio server for editor clients. |
| `pevo profile ...` | List, inspect, create, switch, alias, rename, and delete local profiles. |
| `pevo agent ...` | List, inspect, run, and manage local agents. |
| `pevo skill ...` | List, view, create, install, configure, audit, and manage local skills and bundles. |
| `pevo tool ...` | List and configure local toolsets. |
| `pevo session ...` | List, inspect, rename, archive, restore, export, or locally share sessions. |
| `pevo model ...` | Inspect configured models and explicitly fetch provider model catalogs. |
| `pevo config ...` | Inspect config paths and add OpenAI-compatible providers. |
| `pevo auth ...` | Inspect credential status, run provider setup, and store provider API keys. |
| `pevo stats` | Show local token and estimated-cost statistics from SQLite state. |
| `pevo context --session <id\|latest>` | Inspect local context-window usage for a session. |

Run `pevo <command> --help` for flags and subcommands.

## Documentation

- [Installation Guide](docs/install.md)
- [ACP Configuration Guide](docs/acp-configuration.md)
- [TUI Troubleshooting](docs/troubleshooting/tui.md)
- [Contributing Guide](CONTRIBUTING.md)

## Workspace

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

## License

Psychevo is licensed under the [MIT License](LICENSE).
