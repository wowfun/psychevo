# 111. Web Search Testing

Tests use fake provider, HTTP, DNS, clock, and abort boundaries by default.
They must not discover or consume real user credentials.

## Runtime And Local Adapters

- parse, merge, default, reject invalid configuration, and enforce the
  unlimited-storage acknowledgement;
- cover the execution/capability/permission lane matrix and Exa, Parallel,
  SearXNG, Brave automatic order;
- assert backend-specific schemas and the 1..20 limit;
- assert tagged result/context envelopes, no-results, truncation, and untrusted
  framing;
- cover all four adapters, JSON/SSE MCP, timeout, 256-KiB ceiling, abort, error
  classification, and credential redaction.

## Permissions And URL Security

- cover `WebSearch(*)` allow, ask, deny, and persistent grants;
- prove hosted static allow and rejection/fallback for query-specific rules;
- reject raw/encoded secrets, sensitive query parameters, private DNS/IP,
  rebinding, unsafe redirects, and the eleventh redirect;
- prove transport uses the validated address and configured local SearXNG works.

## AI And Agent

- cover Responses translation, ordinary streaming, reasoning, function tools,
  hosted controls, and coexistence without duplicate names;
- normalize provider-executed lifecycle, citations, sources, images, usage,
  errors, and terminal states;
- cover background create/poll, `store=true`, abort cancellation, and storage
  acknowledgement;
- replay/reload hosted blocks and prove they never enter `ToolRouter`.

## Gateway And UI

- check protocol schema/code generation;
- cover Settings update, credential presence projection, secret non-return,
  project override, and unlimited acknowledgement;
- replay simultaneous positioned local searches with empty initial arguments and
  prove every live snapshot keeps both calls, query-bearing titles, and states;
- render parallel live searches at desktop and mobile widths, keeping provider
  metadata out of the title summary while preserving title hover disclosure;
- reconcile hosted lifecycle/sources and cover citations, TUI links, query
  wrapping, and image lazy-load/failure.

## Validation Order

1. affected narrow tests;
2. `cargo xtask gateway-protocol generate --check`;
3. `cargo xtask ci run --profile rust-broad`;
4. `cargo xtask ci run --profile web`;
5. `cargo xtask ci run --profile visual`.

Real provider checks are opt-in and never automatically use user credentials.
