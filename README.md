# Psychevo

Psychevo is a public-alpha Rust agent kernel and coding CLI/TUI. The project is
building toward self-evolving agents, but the current focus is the execution
substrate: observable agent runs, replay-ready evidence, local persistence,
provider configuration, and a small coding tool surface.

The user-facing entry point is `pevo`. It can run one-shot coding-agent tasks,
open a fullscreen terminal UI, manage local skills, and report local usage
statistics from its SQLite state.

## Current Shape

| Area | What exists today |
|------|-------------------|
| Agent kernel | A Rust workspace split across AI protocol, agent loop, runtime, and CLI crates. |
| Coding agent | `pevo run` and `pevo tui` route work through runtime-owned tools for reading, writing, editing, searching, listing, and shell commands. |
| Terminal UI | Fullscreen `pevo tui` with sessions, transcript history, model and variant selection, tool evidence rows, stats, and local shell escapes. |
| Providers | OpenAI Chat-compatible provider configuration with built-in provider ids, JSONC config, `.env` credentials, and model metadata enrichment. |
| Skills | Filesystem-backed skills that can be discovered, viewed, installed, enabled, disabled, and explicitly invoked. |
| State | Local SQLite state for sessions, messages, usage accounting, and estimated costs. |
| Development model | Specs are the source of truth for behavior before implementation changes land. |

Psychevo does not yet claim product-complete self-evolution, auto-skill
generation, workflow mining, long-term memory, or autonomous evaluation loops.
Those capabilities need the execution substrate first.

## Source Install

Psychevo is not documented here as a crates.io or binary release. Install from a
checked-out repository:

```bash
git clone https://github.com/wowfun/psychevo.git
cd psychevo
cargo install --path crates/psychevo-cli
pevo --help
```

The workspace uses Rust 1.94 and edition 2024.

For development without installing:

```bash
cargo run -p psychevo-cli -- --help
```

## Quick Start

Initialize the global Psychevo home:

```bash
pevo init
```

By default this creates `~/.psychevo/config.jsonc`, `~/.psychevo/.env`, and
`~/.psychevo/state.db`. The starter config selects DeepSeek. Put credentials in
the generated `.env` file, for example:

```bash
DEEPSEEK_API_KEY=...
```

Run a one-shot task:

```bash
pevo run "summarize this repository"
```

Open the fullscreen terminal UI:

```bash
pevo tui
```

Select a provider/model for one invocation:

```bash
pevo run -m deepseek/deepseek-chat "inspect the CLI entrypoints"
```

## Commands

| Command | Purpose |
|---------|---------|
| `pevo init` | Create the global Psychevo home, starter config, `.env` template, and SQLite state. |
| `pevo run [message..]` | Run a live coding-agent task from the shell. |
| `pevo tui [message..]` | Start the fullscreen terminal UI, or process scripted stdin line by line. |
| `pevo skills ...` | List, view, create, patch, remove, enable, disable, install, or scan local skills. |
| `pevo stats` | Show local token and estimated-cost statistics from SQLite state. |
| `pevo smoke --db <path> --workdir <path>` | Run the deterministic fake-provider validation harness. |

Run `pevo <command> --help` for flags.

## Workspace

| Crate | Responsibility |
|-------|----------------|
| `psychevo-ai` | Provider protocol normalization and AI transport adapters. |
| `psychevo-agent-core` | Model-agnostic agent loop, tool traits, tool execution hooks, outcomes, and abort handling. |
| `psychevo-runtime` | Coding-agent runtime assembly, provider/model resolution, context, tools, persistence, skills, and usage accounting. |
| `psychevo-cli` | The `pevo` command-line and terminal UI transport. |

## Development

Read [AGENTS.md](AGENTS.md) before changing the project. Specs live under
[`specs/`](specs/) and should be read, updated, or created before implementation
changes.

Default broad validation:

```bash
scripts/validate.sh broad
```

Use narrower validation when it covers the changed behavior. Live-provider,
API-key, and live-service checks are opt-in only.

See [CONTRIBUTING.md](CONTRIBUTING.md) for the local contribution workflow.

## License

Psychevo is licensed under the [MIT License](LICENSE).
