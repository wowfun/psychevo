---
name: 249. Vision and Image Artifacts Testing
psychevo_self_edit: deny
---

# 249. Vision and Image Artifacts Testing

Default validation is deterministic and uses local fake providers and local
fixtures. Real provider calls are opt-in only.

## Narrow Validation

- runtime resolver and prompt extraction unit tests
- config parsing and credential resolution unit tests
- `psychevo-ai` fake/OpenAI image generation request-shaping tests
- Gateway RPC/media endpoint tests with fake provider output
- Workbench/Floating component tests for attachment thumbnails and generated
  image artifact cards

## Visual Validation

Run the visual profile for deterministic Workbench, Floating, and TUI artifact
evidence:

```bash
cargo xtask ci plan --profile visual --json
cargo xtask ci run --profile visual
```

Visual fixtures must use local media files or deterministic fake image
generation. They must not require `OPENAI_API_KEY` or a real provider endpoint.

## Live Validation

Run the shared live plan and sweep:

```bash
cargo xtask live plan --all --env shared
cargo xtask live run --all --env shared
```

If full live validation is requested, also run the direct ACP browser spec using
the latest passing ACP server live context.

## Opt-In Provider Validation

OpenAI image generation live validation is opt-in. It requires an explicit
environment with credentials and must be reported separately from default CI.
Default validation must skip or fake provider image generation rather than
silently calling a live service.
