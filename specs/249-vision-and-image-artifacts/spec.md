---
name: 249. Vision and Image Artifacts
psychevo_self_edit: deny
---

# 249. Vision and Image Artifacts

Define Psychevo multimodal image input, image inspection, image generation, and
rich generated-image rendering across Workbench, Floating, Gateway, runtime
providers, and transcript projection.

Vision is a shared media capability, not a mode switch. Users attach, paste,
drop, or reference images; agents can inspect explicit image sources and
generate images; the runtime resolves sources safely, persists generated media,
and projects stable artifact rows that rich clients can render without storing
raster bytes inside transcript JSON.

## Scope

- shared image source resolution for local paths, `file:`, `data:image`, explicit
  remote image URLs, and Psychevo media artifact references
- model-visible multimodal Q&A with text-only degradation rather than composer
  blocking
- `view_image` tool for local/media image inspection
- `image_generation.generate` and provider-visible `image_generate` tool names
- independent `[image_generation]` config and deterministic fake provider
- OpenAI-first image generation provider using configurable `gpt-image-2`
- persisted generated image files under the Psychevo state media root
- authenticated Gateway media reads for local media artifacts
- Workbench and Floating attachment thumbnails, generated-image cards,
  lightbox/download controls, and text/path dedupe

Out of scope:

- auxiliary `vision_analyze` provider orchestration
- automatic extraction of arbitrary `http(s)` prompt URLs as image attachments
- embedding generated image bytes in transcript snapshots
- default live validation that requires API keys or real image providers
- generic non-image artifact rendering changes

## Evidence Basis

Current Psychevo already has `GatewayInputPart.Image`,
`UserContentBlock::ImageUrl`, `UserContentBlock::LocalImage`, provider image
degradation/retry, `ToolAttachment::ImageUrl`, and
`TranscriptBlockKind::Artifact`. The missing pieces are the shared safe resolver,
agent-facing image tools, persisted media artifacts, media serving, and rich
generated-image rendering.

The local Codex reference models image input as structured turn content, exposes
`view_image`, and treats image generation as a tool/thread item with status,
result, revised prompt, and saved path. The local Hermes reference centralizes
image source resolution, stores host/agent-visible image identities separately,
and renders pending/loaded/failed generated-image cards with lightbox/download
controls and generated-path prose dedupe.

## Image Source Resolution

Runtime owns a shared image resolver. It accepts only explicit image sources:

- local relative or absolute paths, resolved against the request cwd
- `file:` URLs
- `data:image/*;base64,...` URLs
- explicit `http://` and `https://` image URLs passed as tool or attachment
  fields
- `psychevo-media://<artifactId>` references minted by Psychevo

Prompt extraction remains conservative. Leading or embedded local/file/data
sources may become image inputs; arbitrary remote URLs mentioned in prompt text
remain text unless a structured attachment or tool argument explicitly carries
them as image sources.

The resolver enforces bounded size, MIME/magic-byte validation, redirect and
timeout limits for remote images, and unsafe local/network guardrails. Text-only
models receive visible source text plus the user prompt instead of a failed
composer submission.

## Configuration

Image generation configuration lives under Settings > Models and profile/project
TOML. There is no separate Images page.

```toml
[image_generation]
provider = "openai"
model = "gpt-image-2"
size = "1024x1024"
format = "png"
```

The provider defaults to OpenAI. Credentials resolve through the existing
provider environment rules; raw API keys are never accepted inside
`[image_generation]`. Tests may use `provider = "fake"` without credentials.

## Provider Contracts

`psychevo-ai` owns image generation request/result types and provider traits.
The V1 request carries:

- `provider`
- `model`
- `prompt`
- optional `aspect_ratio`
- optional edit/source image
- optional reference images, capped together with recent images to five total
- requested output format

The V1 result carries one generated image, MIME type, provider/model, optional
revised prompt, and bounded provider metadata. Runtime persists the returned
bytes before projecting transcript artifact metadata.

## Agent Tools

`view_image` resolves one image source and returns text metadata. If the active
model can receive images, the tool result also attaches a model-visible image;
otherwise it returns an explicit text-only degraded result.

`image_generation.generate` is canonical. `image_generate` is exposed as a
provider-visible alias for systems that expect the shorter name. Arguments:

- `prompt`
- optional `aspect_ratio`
- optional `image_url`
- optional `reference_image_urls`
- optional `num_recent_images`

The runtime caps the total of `image_url`, `reference_image_urls`, and recent
thread images to five. Missing provider credentials and unsupported providers
produce bounded tool errors.

## Media Artifacts

Generated images are stored under the Psychevo state media root and referenced
by stable artifact ids. Transcript projection uses
`TranscriptBlockKind::Artifact` with metadata:

- `mediaKind: "generated_image"`
- `artifactId`
- `status`
- `mimeType`
- `prompt`
- `provider`
- `model`
- `savedPath`
- `displayUrl`
- `agentVisibleSource`
- optional `revisedPrompt`
- optional dimensions when known

TUI/export/share stay text-first: they show the generated-image row and saved
artifact path, not raster previews.

## Gateway Media

Gateway exposes authenticated media reads and URL construction for Psychevo media
artifacts. The endpoint validates artifact ids, prevents path traversal, returns
correct image MIME types, and reports missing files with bounded errors.
Workbench and Floating use the media URL instead of embedding image bytes in
thread snapshots.

## Workbench and Floating UX

Workbench accepts image file picker, paste, and drop where the host allows it.
Attachments render as compact thumbnails with remove/error states. Text-only
models do not block submission; unsupported image input degrades in the runtime.

Generated-image transcript cards reserve the final aspect ratio, render pending,
loaded, and failed states, fade in loaded media, provide lightbox and download
actions, and hide duplicate saved-path prose that immediately follows the
artifact block. Floating renders the same artifact metadata in its compact
history surface.

Controls do not explain implementation details. Errors state the failed action
and the next available action.

## Acceptance Criteria

- `[image_generation]` TOML parsing accepts documented fields, rejects raw API
  keys, and resolves provider credentials through existing provider config.
- Shared resolver tests cover local, `file:`, `data:image`, remote image, media
  reference, unsafe host/network, size limit, MIME/magic-byte, and no HTTP prompt
  inference behavior.
- `view_image` returns image metadata and model-visible image attachment when
  possible, and explicit text-only degradation when not.
- Image generation uses deterministic fake provider in tests and OpenAI request
  shaping without real provider calls by default.
- Generated images persist under the state media root and project as
  `artifact` blocks with `mediaKind: "generated_image"`.
- Gateway media reads require normal Gateway auth, validate artifact ids, set
  image MIME types, and fail cleanly for missing files.
- Workbench and Floating render image attachments and generated-image artifacts
  with pending/loaded/failed states, lightbox/download controls, and path/prose
  dedupe.
- Full visual and shared live validation pass without API keys. Real OpenAI
  image generation validation remains opt-in.

## Attachments

- [Testing](testing.md) defines deterministic and opt-in live validation.

## Related Topics

- [003 AI Protocol](../003-ai-protocol/spec.md) defines provider request
  translation ownership.
- [021 Gateway](../021-gateway/spec.md) defines Gateway thread, turn, and media
  read ownership.
- [027 ACP](../027-acp/spec.md) defines multimodal ACP input boundaries.
- [120 Provider Registry](../120-provider-registry/spec.md) defines provider
  configuration and credential resolution.
- [125 Model Config](../125-model-config/spec.md) defines Settings > Models.
- [240 pevo Web](../240-pevo-web/spec.md) defines Workbench composer and
  transcript layout.
- [245 pevo Floating](../245-pevo-floating/spec.md) defines Floating history
  rendering.
- [250 UI Display Model](../250-ui-display-model/spec.md) defines transcript
  projection contracts.
