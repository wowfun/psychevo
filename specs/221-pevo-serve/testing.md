---
name: 221. pevo Serve Testing
psychevo_self_edit: deny
---

Define acceptance expectations and validation scenarios for the foreground
headless `pevo serve` API server.

## Long-Term Acceptance Contract

- `pevo serve` starts a foreground headless API server and emits exactly one
  ready JSON object to stdout after binding.
- Server logs go to stderr, and ready metadata never includes bearer tokens.
- Loopback binding is the default; public managed fallback behavior belongs to
  managed Gateway internals, not the public `pevo serve` surface.
- Direct `pevo serve` requires a bearer token from `PSYCHEVO_SERVE_TOKEN` or
  `--token-file`.
- There is no public `--token` flag, and query-string tokens are rejected.
- `/readyz` is public and contains only non-sensitive readiness/version data.
- WebSocket, downloads, and detailed status routes require authentication.
- WebSocket transport follows strict JSON-RPC 2.0 request, response, error, and
  notification shapes with camelCase fields.
- Source-selecting requests carry explicit scope, and thread-id anchored
  methods authorize through stored thread/workdir binding.
- Derived source keys avoid exposing raw local paths.

## Current Implementation Slice

Automation vocabulary and generic validation boundaries follow
[060 Automation](../060-automation/spec.md).

The default validation path should use local loopback sockets, temporary config
and database paths, temporary workdirs, and fake or test providers. It should
not read browser-managed Gateway state or user credentials.

Live provider validation is not part of this topic's default path. If a serve
change requires live model verification, use the managed Gateway live paths
with isolated config and database state.

## Scenario Matrix

- Startup succeeds with `PSYCHEVO_SERVE_TOKEN` and emits one ready JSON object.
- Startup succeeds with `--token-file` and does not echo the token.
- Startup rejects when no token source is provided.
- Public `--token` and query-string token attempts are rejected.
- `/readyz` succeeds without auth and omits secrets.
- Authenticated WebSocket requests return strict JSON-RPC responses.
- Unauthenticated WebSocket, download, or detailed status requests are rejected.
- Invalid JSON, invalid `jsonrpc`, unknown methods, and malformed params return
  bounded structured errors.
- `thread/start`, source-default `thread/resume`, `turn/start`, `thread/list`,
  and thread-id anchored operations enforce the documented scope rules.
- Source keys in responses are stable while avoiding raw local path exposure.

## Validation Boundaries

- Tests should assert transport and authorization semantics, not private router
  implementation details.
- Server tests must isolate sockets, temp state, environment variables, and
  provider configuration.
- Browser launch cookies, managed server reuse, and Workbench UI behavior
  belong to [220 pevo Gateway Testing](../220-pevo-gateway/testing.md).
