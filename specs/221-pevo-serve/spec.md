---
name: 221. pevo Serve
psychevo_self_edit: deny
---

# 221. pevo Serve

Define the concrete `pevo serve` headless local API server.

## Scope

- foreground headless Gateway API server behavior
- loopback binding, readiness, authentication, and stdout contract
- strict WebSocket JSON-RPC 2.0 transport contract
- request-scoped source/cwd inputs for multi-cwd clients

Out of scope:

- managed Web launch behavior, owned by
  [220 pevo Gateway](../220-pevo-gateway/spec.md), and concrete Web Shell
  behavior, owned by [240 pevo Web](../240-pevo-web/spec.md)
- public LAN, relay, TLS, account, or hosted service behavior
- installer service or OS login-item daemon behavior

## Process Contract

`pevo serve` starts a foreground headless API server. It binds loopback by
default and emits exactly one ready JSON object to stdout after binding. Server
logs go to stderr. The ready JSON includes non-secret address and endpoint
metadata; it does not include tokens.

The foreground CLI installs explicit SIGINT/SIGTERM handling. On either signal
it stops accepting new HTTP/WebSocket work, asks the Gateway-owned
`AgentSessionHost` to shut down its resident Agent adapters gracefully within a
fixed deadline, and falls back to a separately bounded forced shutdown when
graceful shutdown times out or fails. The server process does not exit until
this cleanup path and its bounded connection drain have settled. This applies
equally to direct `pevo serve` and the managed `serve` child started by
`pevo gateway`.

`BoundGatewayWebServer` keeps signal ownership explicit for library callers.
Its ordinary `run` method does not install process-global signal handlers; an
embedding host may retain its own lifecycle policy. The CLI uses the server's
signal-aware run entrypoint and supplies the OS shutdown future.

Direct `pevo serve` requires an explicit token from `PSYCHEVO_SERVE_TOKEN` or
`--token-file`. There is no `--token` flag, and query string tokens are not
accepted. Managed `pevo gateway` may start `pevo serve` with internal flags and
a generated token file. Managed-only internal flags may request fallback across
a finite loopback port range; the public `pevo serve` command keeps its
single-address bind behavior.

## HTTP And WebSocket

`/readyz` is public and returns only non-sensitive readiness/version data.
WebSocket, downloads, and detailed status routes require authentication.
Non-browser API clients use `Authorization: Bearer <token>`.

The WebSocket transport is strict JSON-RPC 2.0:

- request: `{ "jsonrpc": "2.0", "id": ..., "method": ..., "params": ... }`
- response: `{ "jsonrpc": "2.0", "id": ..., "result": ... }`
- error: `{ "jsonrpc": "2.0", "id": ..., "error": { "code": ..., "message": ... } }`
- notification: `{ "jsonrpc": "2.0", "method": ..., "params": ... }`

Fields are camelCase. Methods use singular resource-oriented names such as
`thread/draft/open`, `turn/start`, `thread/action/run`, and
`thread/interaction/respond`.

## Request Scope

Source-selecting methods carry `params.scope`, which includes `cwd` and
source intent. `source.kind` is an open namespace string. `rawId` may be
omitted; the server derives it from kind plus canonical cwd. Derived source
keys avoid exposing raw local paths.

`thread/draft/open` requires `params.origin`; source-default `thread/resume` and
`turn/start` require `params.scope`. Thread-id anchored read/write/control methods authorize through
the stored thread/cwd binding. `thread/list` uses an explicit cwd
filter.

## Attachments

- [Testing](testing.md) defines headless server validation expectations.

## Related Topics

- [021 Gateway](../021-gateway/spec.md) defines the transport-neutral Gateway.
- [200 pevo CLI](../200-pevo-cli/spec.md) defines the CLI surface.
- [220 pevo Gateway](../220-pevo-gateway/spec.md) defines managed Web launch
  lifecycle.
