---
name: 0001. Rust Pi-style Agent Kernel Architecture
status: archived
date: 2026-05-01
psychevo_self_edit: deny
---

## Context

Psychevo is intended to become a self-evolving agent, but the first milestone should not attempt self-evolution directly. A self-evolving system needs a reliable execution substrate before it can safely evaluate, modify, and preserve improvements.

The immediate goal is a minimal agent kernel that is observable, replayable, and extensible. The design should follow Pi's useful separation of concerns while avoiding Pi's product-level surface area in the first version.

Pi's `coding-agent` package is not just a CLI package. It bundles product runtime, coding tools, sessions, compaction, CLI modes, SDK, and UI/RPC/print entry points. Psychevo follows Pi's layering idea, not its package bundling.

## Decision

Implement the first version as a Rust workspace with four crates:

- `psychevo-ai`: model and provider protocol layer. This replaces the narrower `psychevo-llm` name because future model integrations may include language models, embedding models, rerankers, vision models, or other AI capabilities. The first implementation will only include an OpenAI-compatible language model provider plus a fake provider for tests.
- `psychevo-agent-core`: model-agnostic agent loop layer. It owns the ReAct loop, agent events, tool traits, tool execution hooks, stop conditions, max turns, and abort handling. It must not know about coding-agent tools, sessions, memory, evaluation, or self-evolution.
- `psychevo-runtime`: coding-agent runtime layer. It wires the agent core to workspace policy, built-in tools, JSONL traces, context construction, system prompts, and session replay.
- `psychevo-cli`: thin command-line entry point. It parses user input, constructs the runtime, and renders events. Business logic belongs in the library crates, not the CLI.

`psychevo-runtime` is the stable library surface for future evaluator, replay runner, evolution manager, daemon, and other non-CLI entry points. `psychevo-cli` stays separate because argument parsing, terminal rendering, stdin/stdout, exit codes, and environment handling are transport concerns.

The first built-in tools are:

- `read`
- `write`
- `edit`
- `bash`

The first durable record format is append-only JSONL trace data. Trace events are the foundation for future replay, evaluation, skill extraction, and evolution loops.

## Non-Goals

The first milestone will not include:

- long-term memory
- skill generation
- self-modifying code
- workflow search
- multi-agent orchestration
- vector databases
- TUI
- extension/package system
- session tree branching
- multiple real providers

These capabilities can be added later after the kernel has a stable event, trace, and runtime boundary.

## Consequences

This design favors a small, explicit kernel over a broad first release. The main cost is more workspace structure up front. The benefit is that future evaluator and evolution systems can depend on stable lower layers without contaminating the core agent loop.

Keeping `psychevo-cli` separate prevents CLI dependencies and terminal semantics from leaking into `psychevo-runtime`. Future tools such as evaluators, replay runners, daemons, and evolution managers can depend directly on `psychevo-runtime` without going through a command-line transport. The tradeoff is one extra crate compared with a bundled product package, but that cost is low in a Rust workspace and keeps the CLI replaceable.

The crate boundary rule is:

- `psychevo-ai` only knows model protocols.
- `psychevo-agent-core` only knows agent execution and tool traits.
- `psychevo-runtime` knows coding-agent behavior and persistence.
- `psychevo-cli` is transport only.
