---
psychevo_self_edit: deny
---

- Specs first: before implementing code, always READ/UPDATE/CREATE `specs/<topic>/spec.md`.
- The product experience MUST minimize cognitive load by omitting anything that does not clarify intent, enable action, or add incremental value, unless required for correctness, safety, or explicit user confirmation.
- After implementation lands, update `CHANGELOG.md`.

## Tests
- After code changes, run the relevant validation path. When a unified default validation entrypoint exists, prefer it for broad validation.
- For code logic changes, prefer adding or updating the closest meaningful test. If no narrow test exists, fall back to higher-level validation.
- Narrow validation is not permission to ignore plausibly related failures; fix them or report them explicitly.
- If a test file is created or changed, run that test and iterate until it passes.
- Documentation-only or changelog-only changes do not require code tests unless executable examples, generated artifacts, or validation instructions changed.
- Default validation must use deterministic local harnesses and fake or test providers.
- Real provider, API-key, or live-service validation is opt-in only.
- Keep tests isolated from real user config, credentials, environment, temp state, global mocks, timers, sockets, and persistent host state; restore or clean up any such state touched by a test.
- Avoid brittle change-detector tests over volatile inventories, generated lists, workflow text, or model and provider catalogs.
- Prefer behavior and invariant assertions with structured comparisons over field-by-field checks or string-grep checks.
- Update snapshots, baselines, inventories, ignore lists, or expected-failure records only for intentional behavior changes or with explicit approval; treat those diffs as review material.
- Do not rerun the exact same validation only for formality; report the validation most relevant to the changed surface.
- Do not run multiple broad test commands concurrently in the same worktree unless the test infrastructure explicitly supports isolation.
- If validation cannot be run, report the attempted validation path and the blocker.
