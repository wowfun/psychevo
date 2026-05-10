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

## Validation

Use deterministic local validation by default. The broad entry point is:

```bash
scripts/validate.sh broad
```

For focused code changes, prefer a narrower command that exercises the changed
behavior, such as:

```bash
cargo test -p psychevo-runtime
cargo test -p psychevo-cli smoke_cli
```

If you create or change a test file, run that test and iterate until it passes.
Do not update snapshots, baselines, inventories, or expected-failure records
unless the behavior change is intentional.

Live-provider, API-key, network, or live-service validation is opt-in only. The
default validation path must use fake providers or deterministic local
harnesses.

## Project Rules

Read [AGENTS.md](AGENTS.md) for repository rules used by human contributors and
coding agents. The short version:

- specs first
- deterministic local validation
- isolated tests
- no real credentials in tests, logs, snapshots, or docs
- report validation blockers instead of hiding them
