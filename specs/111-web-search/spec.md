# 111. Web Search

## Purpose

Define one provider-neutral `web_search` capability with two mutually exclusive
execution lanes. `web_fetch` remains the tool for a known URL. A generation
request exposes at most one provider-visible tool named `web_search`.

## Configuration

The merged runtime configuration is:

```toml
[web.search]
execution = "auto"              # auto | local | hosted
backend = "auto"                # auto | searxng | brave | exa | parallel
external_access = "live"        # live | cached
context_size = "medium"         # low | medium | high
return_token_budget = "default" # default | unlimited
content_types = ["text"]        # text and/or image
allowed_domains = []
blocked_domains = []
background_storage_acknowledged = false

[web.search.location]
country = ""
region = ""
city = ""
timezone = ""

[web.search.image]
max_results = 3
caption = true
```

Defaults are `auto`, `auto`, `live`, `medium`, `default`, and text-only.
Unknown keys and invalid enum values reject configuration. `limit` is not a
persistent setting: local calls accept 1 through 20 and default to 8.

`EXA_API_KEY`, `PARALLEL_API_KEY`, `BRAVE_SEARCH_API_KEY`, and `SEARXNG_URL`
are read from the effective profile environment. Raw credentials and endpoint
userinfo never enter TOML, public configuration values, RPC responses, events,
traces, tool results, or errors. Settings reports only `present` or `missing`.

`return_token_budget = "unlimited"` is invalid unless
`background_storage_acknowledged = true`. The acknowledgement records explicit
acceptance of provider-side temporary storage; it is not inferred and the
runtime does not silently downgrade the request.

## Lane Resolution

`local` selects a runtime execution binding. `hosted` selects a
provider-executed declaration and has no `ToolRouter` binding. `auto` selects
hosted only when all of the following are true:

- the selected provider is built-in `openai`;
- model metadata explicitly says `web_search = true`, or the model belongs to
  the confirmed `gpt-5*`, `gpt-4.1*`, or `o4-mini*` families;
- the effective permission configuration statically proves an unconditional
  allow for every search query;
- the hosted adapter supports the selected controls.

Otherwise `auto` selects local. Unknown model capability selects local. An
explicit hosted request that fails one of these preconditions rejects before
provider invocation with a precise configuration error.

Local `backend = "auto"` selects the first configured backend in this order:
Exa, Parallel, SearXNG, Brave. Exa and Parallel are configured when their key is
present or their public no-key MCP route is available. SearXNG requires
`SEARXNG_URL`; Brave requires `BRAVE_SEARCH_API_KEY`. An explicit unavailable
backend fails with its missing environment variable or unavailable route.

Tool-surface evidence records the selected lane, resolved backend, provider
capability input, permission decision basis, and omitted or unavailable reason.

## Local Tool Contract

The declaration is generated after backend resolution. Every backend exposes
required non-empty `query` and integer `limit` (default 8, range 1 through 20).
Exa additionally exposes only `type = auto|fast|deep`,
`livecrawl = fallback|preferred`, and `context_max_characters` up to 50,000.

The local result is one tagged envelope:

```json
{
  "query": "...",
  "provider": "searxng",
  "execution_owner": "runtime",
  "payload": {
    "type": "results",
    "items": [
      {"title": "...", "url": "...", "description": "...", "position": 1}
    ]
  },
  "truncated": false,
  "error": null
}
```

SearXNG and Brave return `payload.type = "results"`. Exa and Parallel MCP
return `payload.type = "context"` with bounded `text`; arbitrary provider text
must not be parsed into invented result rows. Model projection marks all search
material as external and untrusted.

SearXNG and Brave use bounded JSON HTTP. Exa and Parallel use JSON-RPC
`tools/call`, accept JSON or SSE responses, cap response bytes at 256 KiB, time
out after 25 seconds, and propagate cancellation. Adapter errors are classified
without leaking credentials.

## Hosted OpenAI Contract

Built-in `openai` generation uses the Responses API. User-defined and other
OpenAI-compatible providers remain on Chat Completions. The Responses adapter
preserves existing text, reasoning, function calls, images, usage, terminal,
and error semantics while adding one hosted `web_search` declaration with the
selected controls.

Provider-executed search, open, and find actions are normalized as hosted tool
lifecycle blocks. They retain provider call identity and action but never enter
local tool execution. Provider-neutral sources include URL citations with URL,
title, and text indices, and image results with image URL, thumbnail URL,
source-site URL, and optional caption. Complete source metadata is retained.

`unlimited` sets `background = true` and `store = true`. Runtime polls every two
seconds until a terminal status, maps failure and incomplete states precisely,
and sends the provider cancel request when the local abort signal fires.

## Persistence And Presentation

Provider-executed blocks and source blocks persist with the assistant message
and survive replay and reload. URL citations are visible and clickable in
Workbench and render as compact links in TUI. Image results remain remote
metadata, are not downloaded as artifacts, and show caption/source first;
Workbench loads a thumbnail only after explicit expansion.

The existing transcript `Web` kind owns both web tools. `web_search` uses
`Searching the web` while active and `Searched the web` when terminal.
Workbench Settings writes global-profile search configuration and profile
environment variables. Project TOML may override non-secret fields.

## Permissions

`WebSearch(pattern)` matches the actual query string. Local execution evaluates
permission immediately before transport and supports deny, ask, allow, and
persistent grants. Hosted exposure requires static proof of unconditional
allow. Query-specific ask or deny rules force local in `auto`; explicit hosted
rejects before generation.

## URL Policy

`WebUrlPolicy` is shared by model-provided web URLs. After percent-decoding, it
rejects token-like content and sensitive query parameter names. It accepts only
HTTP(S) and always rejects localhost, private, link-local, metadata, multicast,
unspecified, and reserved IP targets, including DNS resolutions to those
ranges. Redirects are manual, each hop is revalidated, and the limit is ten.
Transport connects to the validated address so validation cannot be bypassed by
a second DNS resolution.

A user-configured SearXNG endpoint is trusted control-plane configuration and
may intentionally be local; model-provided URLs never receive this exception.

## Scope Boundary

Browser automation, provider-backed extraction, image-result downloading,
separate evidence artifacts, and generic third-party hosted-search adapters are
out of scope.

## Evidence Basis

- Local adapter shape follows
  `.references/hermes-agent/agent/web_search_provider.py`; backend precedence
  follows `.references/hermes-agent/tools/web_tools.py`.
- MCP transport follows
  `.references/opencode/packages/core/src/tool/websearch.ts`.
- Hosted behavior follows the OpenAI Web Search and Background mode guides.
- Repo gap analysis lives at
  `.local/notes/0713-web-search/web-search-tool-research.md`.

## Related Topics

- [003 AI Protocol](../003-ai-protocol/spec.md)
- [007 Tool Surface](../007-tool-surface/spec.md)
- [035 Event Stream](../035-event-stream/spec.md)
- [041 Permissions](../041-permissions/spec.md)
- [110 Coding Core Tools](../110-coding-core-tools/spec.md)
- [120 Provider Registry](../120-provider-registry/spec.md)
- [240 pevo Web](../240-pevo-web/spec.md)
- [250 UI Display Model](../250-ui-display-model/spec.md)
