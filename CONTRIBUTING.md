# Contributing

Psychevo is spec-first. Before changing behavior, public docs, tests, or
developer workflow, read the best-fit `specs/<topic>/spec.md`. If no existing
topic owns the change, create or update the right spec before implementation.

## Local Workflow

1. Read the owning spec and nearby code.
2. Keep the change scoped to the behavior you are changing.
3. Update or add the closest meaningful test for code logic changes.
4. Run validation that covers the changed surface.
5. Update `CHANGELOG.md` after implementation lands.

Documentation-only and changelog-only changes do not need code tests unless
they change executable examples, generated artifacts, or validation
instructions.

## Contribution Placement

Put a contribution in the narrowest surface that can do the job:

- Use core runtime code when the change affects invocation assembly, accepted
  tools, context projection, evidence, permissions, provider behavior, storage,
  or UI/API contracts.
- Use a skill for reusable model instructions and supporting files that do not
  need new runtime authority.
- Use an agent definition for a reusable execution identity with instructions,
  model preference, tool policy, skills, hooks, MCP scope, or child-agent use.
- Use a hook for event-scoped observation, review, feedback, or bounded effects
  around existing runtime events.
- Use a plugin package when distribution or package policy matters, or when one
  package must bundle multiple declarations such as skills, agents, hooks, MCP
  servers, worker tools, providers, commands, or toolsets.

Do not introduce a plugin just to ship one local skill, agent, or hook unless
the package lifecycle is part of the requirement. Do not add a registry or
shared abstraction until multiple existing surfaces need the same host-owned
interface.

## Validation

Use deterministic local validation by default. The Rust workspace broad entry
point is:

```bash
cargo xtask ci run --profile rust-broad
```

For focused code changes, prefer a narrower or subsystem-specific command that
exercises the changed behavior, such as:

```bash
cargo test -p psychevo-runtime
cargo test -p psychevo-cli smoke_cli
```

If you create or change a test file, run that test and iterate until it passes.
Do not update snapshots, baselines, inventories, or expected-failure records
unless the behavior change is intentional.

Live-provider, API-key, network, or live-service validation is opt-in only. The
default validation path must use fake providers or deterministic local
harnesses. Use `cargo xtask init dev-env` to prepare the repo-local live home
and `cargo xtask live run [--env shared|isolated] [--suite <suite>|--check
<id>]` for explicit live validation. Live validation defaults to the shared
repo-local dev home so persisted-home issues are visible.

## Project Rules

Read [AGENTS.md](AGENTS.md) for repository rules used by human contributors and
coding agents. The short version:

- specs first
- deterministic local validation
- isolated tests
- no real credentials in tests, logs, snapshots, or docs
- report validation blockers instead of hiding them
