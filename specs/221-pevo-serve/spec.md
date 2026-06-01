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
- request-scoped source/workdir inputs for multi-workdir clients

Out of scope:

- Web Shell lifecycle and browser launch behavior, owned by
  [220 pevo Gateway](../220-pevo-gateway/spec.md)
- public LAN, relay, TLS, account, or hosted service behavior
- installer service or OS login-item daemon behavior

## Process Contract

`pevo serve` starts a foreground headless API server. It binds loopback by
default and emits exactly one ready JSON object to stdout after binding. Server
logs go to stderr. The ready JSON includes non-secret address and endpoint
metadata; it does not include tokens.

Direct `pevo serve` requires an explicit token from `PSYCHEVO_SERVE_TOKEN` or
`--token-file`. There is no `--token` flag, and query string tokens are not
accepted. Managed `pevo gateway` may start `pevo serve` with internal flags and
a generated token file.

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
`thread/start`, `turn/start`, `permission/respond`, and `clarify/respond`.

## Request Scope

Source-selecting methods carry `params.scope`, which includes `workdir` and
source intent. `source.kind` is an open namespace string. `rawId` may be
omitted; the server derives it from kind plus canonical workdir. Derived source
keys avoid exposing raw local paths.

`thread/start`, source-default `thread/resume`, and `turn/start` require
`params.scope`. Thread-id anchored read/write/control methods authorize through
the stored thread/workdir binding. `thread/list` uses an explicit workdir
filter.

## Related Topics

- [021 Gateway](../021-gateway/spec.md) defines the transport-neutral Gateway.
- [200 pevo CLI](../200-pevo-cli/spec.md) defines the CLI surface.
- [220 pevo Gateway](../220-pevo-gateway/spec.md) defines managed Web Shell lifecycle.
