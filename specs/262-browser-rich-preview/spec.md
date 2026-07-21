---
name: 262. Browser and Rich Preview
psychevo_self_edit: deny
---

# 262. Browser and Rich Preview

Define Psychevo's right-workspace rich preview, shared Markdown diagram
rendering, and Browser surface.

## Scope

- right-workspace preview destinations for Markdown documents, local HTML, and
  Browser state
- authorized workspace-file previews for images, PDF, media, modern Office,
  compatible documents, delimited tables, Excalidraw, and ZIP directories
- shared Markdown rendering for fenced Mermaid diagrams
- Workbench Browser pane product behavior and Web/PWA fallback
- Desktop-owned managed Browser host contract
- built-in Browser plugin projection in Capabilities
- Browser annotation context serialization

Out of scope:

- replacing `web_fetch` or Codex-style `web.run` search/open/find semantics
- arbitrary `file://` loading from Workbench
- per-origin Browser permission prompts in the first slice
- screenshot-backed annotation context
- legacy binary Office (`.doc`, `.dot`, `.xls`, `.xlt`, `.ppt`), streaming
  playlists, MOV/MKV/AVI, TIFF/JXL, MIDI, XMind, draw.io, EPUB, email,
  CAD/3D/Geo/EDA, and Typst rendering
- remote document conversion or browser access to raw `file://` resources

## Product Model

The right workspace is the only user-facing Browser and rich preview home. It
hosts Markdown preview, local HTML preview, and one Browser pane per thread as
peers to Review, Terminal, Files, Side chat, Team, and child-agent tabs. It must
not create a separate product mode or nested card layout inside the right
workspace.

`Open in` actions are allowed only for explicit typed refs that identify a real
workspace file or a structured artifact reference. Workbench must not attach
`Open in` to arbitrary pasted code blocks, untrusted raw HTML, or strings that
only look like paths.

Assistant answer Markdown may promote a path-like span into a workspace-file
link only after it matches an exact file entry from the current
`workspace/files` result. Matching accepts workspace-relative spellings and
equivalent absolute POSIX, native Windows, Git Bash/MSYS, or UNC spellings, but
the link target is always the entry's compact workspace-relative path. Missing
or excluded files, directories, paths outside the workspace, and entries absent
because the current file result is truncated remain ordinary text.

This promotion applies to ordinary assistant-answer text, complete inline-code
paths, and existing Markdown links. It does not apply to user messages,
reasoning, tool details, fenced code, Mermaid source, raw HTML, images, or
external URLs. While an answer is streaming, a candidate at the unfinished end
of the visible text remains plain until a stable boundary or block completion
proves the whole path. Clicking a promoted path routes the canonical relative
target through `workspace/file/read` and opens the existing Files preview; it
must not navigate the browser, construct a raw `file://` URL, or create a second
artifact-card surface. Completion of a main, side-conversation, or child-agent
turn reevaluates workspace-file demand across the visible primary transcript
and the turn's committed entries. When either contains demand, Workbench
refreshes the workspace-scoped file inventory even if Files is hidden or the
completion belongs to another thread, so created and deleted files do not leave
actions stale. When Workbench defers the initial `workspace/files` read, its
cheap transcript demand check is a conservative superset of these supported
Markdown forms, including root-level filenames, inline-code paths with line
suffixes, and Markdown link destinations with line fragments. A false-to-true
demand from a completed supported file-tool entry immediately refreshes a
same-workspace cached inventory rather than being suppressed by the matching
root. Turn completion then performs the demand-driven final revalidation needed
for later mutations such as deletion. Exact inventory matching remains the
authority for promotion. If Files is visible, its ordinary completion refresh
shares the same single-flight inventory request instead of issuing a duplicate
read.

Completed `read`, `edit`, and `write` transcript tool calls expose a clickable
workspace-file target when their structured arguments contain a string `path`
that resolves to a current file entry. The target reuses the same canonical
workspace-path matching and Files-preview navigation as assistant-answer links;
it does not make raw tool details generally linkable, and pending, running,
failed, missing, directory, or outside-workspace targets remain non-actionable.

Markdown preview uses the shared `@psychevo/components` Markdown renderer
everywhere Workbench previews Markdown: transcript, Files, Review, capability
definition previews, and future artifact previews. Raw HTML remains escaped.
Fenced Mermaid diagrams are rendered by the shared renderer only after the
closing fence exists. During streaming or incomplete input, the Mermaid block is
ordinary code. Mermaid is lazy-loaded only when a complete Mermaid block is
present. Render failures are inline, preserve the raw source for copy, and do
not break the rest of the Markdown document. Rendered diagrams provide a compact
tool surface for copying Mermaid source, fitting to available width, viewing at
original size, zooming in/out, resetting the view, and opening a larger diagram
viewer without leaving the transcript or preview surface.

Local HTML preview is read-only and constrained to content that Gateway has
already authorized as a workspace file or artifact. Workbench must not use raw
`file://` URLs for local preview. Files and Preview run authorized HTML
immediately in an interactive opaque-origin iframe with `allow-scripts`.
Scripts, network requests, pointer input, keyboard input, and document scrolling
are available without a trust prompt or locked mode. The injected Content
Security Policy still denies base rewriting, forms, nested frames, and objects;
the iframe still withholds form, popup, same-origin, top-navigation, and
download sandbox capabilities. Selecting a different document remounts the
execution surface, and changing its content reloads `srcDoc`.

Files and Preview are two views over one HTML execution surface. At most one
iframe for a selected HTML document may be mounted at a time. Activating Preview
must suspend the Files iframe, and returning to Files must suspend the Preview
iframe, without unmounting unrelated inactive tabs such as Terminal or Side
chat.

The Files pane has one toolbar rather than a panel title followed by a second
absolute-path row. Its left side is a breadcrumb rooted at the workspace name;
all following segments come from the selected workspace-relative path and must
not disclose or repeat the absolute host path. Activating a directory breadcrumb
restores the file tree, expands the relevant ancestors, and moves focus to that
directory; activating the root breadcrumb focuses the tree filter. The current
file is the non-interactive final breadcrumb.

Every breadcrumb node before the current file has a separate disclosure button
after its label, matching a desktop file explorer address bar. Its menu contains
only that node's immediate children from the current Gateway-authorized Files
inventory, with directories before files and each group ordered by name.
Selecting a directory restores the file tree and reveals that directory;
selecting a file opens it in the same Files tab. The menu is keyboard navigable,
closes on Escape, outside interaction, scroll, selection, or target change, and
does not query or expose paths outside the existing inventory. A node with no
known children has no disclosure button.

The toolbar's right side contains the selected-file actions followed by the
persistent toggle for the right-hand file tree. Text-backed rich previews expose
`View source`, which switches between the product preview and a read-only
highlighted source view without entering edit mode. While source is visible the
same control reads `View preview`. `Edit` remains revision-aware and is disabled
when the selected file is not editable. `Open HTML preview`, when applicable,
remains adjacent to those file actions.

When the selected lease contains complete non-binary text and the host provides
clipboard writing, a file-level icon-only `Copy` action copies the canonical raw
file content. It replaces the Markdown renderer's nested copy affordance and
sits immediately to the left of the external `Open` control. Pending and copied
states remain visible and announced without changing the copied payload when
the user switches between preview and source.

When Gateway advertises external actions, the toolbar exposes a compact split
open control. Its primary icon, accessible label, tooltip, and action use
Gateway's `preferredAction`; the adjacent text remains the generic verb `Open`
and application names stay in the menu rather than consuming toolbar width. The
menu preserves the ordered `availableActions`,
including reveal. Choosing a different action executes that action once without
changing Gateway preference or storing executable paths in Workbench. The
control sends only the existing closed semantic action, scope, and
workspace-relative path through
`workspace/file/openExternal`. Browser hosts with no advertised action omit the
control. Loading, pending, failure, keyboard focus, and menu dismissal remain
explicit states rather than falling back to an untrusted local launch.

Hiding the tree expands the selected-file preview to the full pane while keeping
the toolbar toggle available to restore it. The selected content renderer also
uses that full width instead of retaining the tree-open reading-width cap or an
equivalent empty gutter. Panel-level tree visibility remains visually separate
from the preceding file-level actions.

Opening a file preview from an assistant workspace-file link or from the
`Open` action on a completed `read`, `edit`, or `write` tool call starts in the
preview-focused layout with the file tree hidden. Opening Files as a workspace
browser, including the Composer Workspace entry and the Files host command,
explicitly restores the tree even when reusing a preview-focused Files tab.
Selecting another file from that tree preserves the current tree visibility.
The persistent header toggle remains the explicit way to override either
default.

## Workspace File Surface

Files owns one deep `WorkspaceFileSurface` module. Its public interface is:

```ts
WorkspaceFileSurface({
  target: { scope, path } | null,
  active,
  textEditing: "enabled" | "disabled",
  onDirtyChange,
  onCompare,
  workspaceRoot,
  fileTree: { open, content, items, onOpen, onOpenChange, onReveal }
})
```

Callers supply the selected target, activation state, editing capability,
file-level callbacks, and the Files-owned authorized tree items/content/state
needed to compose the single toolbar, breadcrumb child menus, and split body.
Format recognition, toolbar file-action
state, external-action discovery and execution, source/preview selection,
loading, renderer selection, progress and error presentation, lease renewal,
parse cancellation, workers, object URLs, and media lifecycle remain private to
the module. The renderer catalog is not a public plugin API and vendor renderer
names never cross the wire protocol. A Files tab retains navigation, selected
path, tree visibility, and dirty state; it does not retain file bodies, blobs,
object URLs, preview leases, application paths, or a duplicate preferred-opener
preference.

All ordinary files enter this surface. Files must not reject a target merely
from its extension before Gateway authorization and media classification. An
unsupported or oversized target receives the same compact error state as a
failed renderer, including a system external-open action when the host exposes
one. No additional product mode, nested preview card, or `file://` path is
introduced. Desktop, Web, and PWA use the same Gateway preview adapter. After
Gateway opens a target, the returned canonical `path` is authoritative for
format classification and renderer selection; the originally selected target
continues to identify editing, comparison, and external-file actions.

The supported read-only preview matrix is:

| Category | Extensions |
| --- | --- |
| Images | `png`, `jpg`, `jpeg`, `gif`, `webp`, `avif`, `bmp`, `svg`, `ico`, `heic`, `heif` |
| PDF | `pdf` |
| Video | `mp4`, `webm` |
| Audio | `mp3`, `wav`, `ogg`, `oga`, `opus`, `m4a`, `aac`, `flac`, `weba` |
| Modern Office | `docx`, `xlsx`, `pptx` |
| Office companions | `docm`, `dotx`, `dotm`, `xlsm`, `xlsb`, `xltx`, `xltm`, `pptm`, `potx`, `potm`, `ppsx`, `ppsm` |
| Compatible documents | `rtf`, `odt`, `ods`, `odp`, `ofd` |
| Delimited tables | `csv`, `tsv` |
| Other | `excalidraw`, `zip` |

MP4 support is validated against H.264/AAC and WebM against VP9/Opus. Browser
codec availability remains authoritative for a user's host. Complex Office
layout is best effort and does not promise Microsoft Office pixel parity.
Macros, scripts, OLE objects, and external document relationships are never
executed. External links are inert by default and document rendering must not
initiate external network requests.

Native images, video, and audio consume the authorized Range URL directly.
PDF uses the renderer's streaming-URL path. Office, delimited tables, HEIC,
OFD, ZIP, and Excalidraw use a bounded whole-file blob. Whole-file parsing is
limited to 32 MiB. ZIP preview lists at most 5,000 sanitized directory entries
without extracting content. Excalidraw preview is an export-only Adapter over
the exactly pinned official `@excalidraw/excalidraw` `0.18.1` package, limited
to 5 MiB and 5,000 elements. The Adapter restores the bounded scene and exports
an inert read-only SVG; it does not mount the Excalidraw editor. Element links
and embeddables are removed, binary files are restricted to embedded image
data, and the exported SVG is scrubbed of executable or externally addressable
content before it enters the DOM. Required fonts are self-hosted and rendering
must not fall back to the Excalidraw CDN. Development asset requests resolve
only to strict descendants of the configured font root: empty, parent,
absolute-relative, cross-root, and cross-volume results are rejected before
file access on every host platform. CSV and TSV render as read-only tables
bounded to 2,000 total rows, 100 columns per row, and 20,000 rendered cells. Parsing
retains no cells beyond those structural limits and stops early once no further
row or cell can be retained. The table displays a truncation notice instead of
materializing the remainder into DOM nodes. SVG, CSV, TSV, and Excalidraw may
still switch to source editing when the existing 1 MiB text limit and revision
rules permit it.

Renderer code is loaded only after classification selects it. The initial
Workbench JavaScript request graph and budget must not grow with preview
renderers. The Vite build copies required workers and assets and creates
renderer-specific deferred chunks. File Viewer integration is locked exactly
to the compatible `@file-viewer/*` `2.2.2` React/core, PDF, Word, Spreadsheet,
Presentation, OFD, Image, and Vite-plugin packages. Excalidraw integration is
locked exactly to `@excalidraw/excalidraw` `0.18.1`; ZIP parsing uses a direct
`jszip` dependency. Workbench and Desktop production builds resolve the bare
`jszip` package entry to its pinned official browser distribution so the
package's Node stream compatibility adapter does not enter either browser
module graph. Preview code must not import `jszip` subpaths or add a general
Node stream polyfill. Preset packages and vendor download, export, or edit
actions are not part of the surface.

Office sanitization, ZIP directory parsing, CSV/TSV parsing, and Excalidraw
validation and security projection run in a task-scoped Vite module Worker
whenever the host exposes the Worker API. Each task owns one Worker and
terminates it after success, failure, or abort; aborting the task must terminate
the Worker immediately rather than only ignoring a later result. The official
restore/export step runs only after Excalidraw becomes the active renderer. Its
non-abortable work may finish after deactivation, but stale output must never be
committed to the DOM. Non-browser and deterministic test hosts without Worker
support may execute the same pure parser functions in-process. The parser
Worker remains deferred with the selected preview chunk and must not enter the
initial Workbench request graph.

Only controls needed to inspect the selected content remain: page, slide, or
sheet navigation; search; zoom and fit; and native media controls. When
`active` becomes false, unfinished reads and parsing are aborted and media is
paused. The one completed Files surface may keep its DOM while inactive.
Changing target or workspace, closing the tab, or unmounting the surface
terminates workers, revokes object URLs, and releases the active lease.

## Workspace Preview Transport

Gateway exposes two typed RPC methods:

- `workspace/file/preview/open`
- `workspace/file/preview/release`

Open accepts a normal scoped workspace-file target. Its result reuses the
text snapshot, binary flag, revision, truncation, and editing metadata of
`WorkspaceFileReadResult`, adds Gateway-classified `mediaType`, and adds an
opaque resource lease:

```ts
{
  resourceId: string;
  resourcePath: string;
  expiresAtMs: number;
}
```

`resourcePath` is a Gateway-relative path and is the only URL callers may use.
The lease ID contains at least 128 bits of cryptographically secure randomness
and is bound to one canonical file path, its opened-file identity, revision,
byte size, and MIME. Gateway reuses the existing workspace scope resolution,
canonical containment, and symlink escape protections both when opening and
serving a resource. Lease creation derives the returned text snapshot,
binary/truncation/editing metadata, identity, size, and revision from one opened
handle, then verifies that the current workspace path still resolves to that
identity. Each GET or HEAD reopens the canonical workspace path and validates
identity, revision, and size on that same handle before serving it. Windows also
validates final-handle path containment. A path or parent-directory symlink swap
between validation and I/O therefore fails closed.

`GET`, `HEAD`, and `OPTIONS` on
`/_gateway/workspace-preview/{resourceId}` serve the lease. GET supports a
complete response or one RFC byte range and returns `206` or `416` as
appropriate. HEAD returns the same representation headers without a body.
Responses include a stable lease `ETag`, correct `Content-Type` and
`Content-Length`, `Accept-Ranges: bytes`, `Cache-Control: no-store`,
`X-Content-Type-Options: nosniff`, `Referrer-Policy: no-referrer`, and a
sandboxing Content Security Policy. Controlled CORS permits only the Gateway's
configured Workbench origins, product-owned Desktop development and packaged
webview origins, and the methods and Range-related headers required by remote
Web/PWA or Desktop PDF and media clients. Gateway lifecycle configuration
registers `http://127.0.0.1:5175`, `http://tauri.localhost`, and
`tauri://localhost` without requiring users to set
`PSYCHEVO_WORKBENCH_ORIGINS`; other origins remain explicit. This never turns a
workspace path into public authority.

Leases expire after 30 minutes without a successful resource access and after
8 hours absolutely. Successful GET or HEAD access advances only the idle
deadline. An expired or explicitly released ID returns `410`; a file whose
canonical target, revision, or byte size has changed returns `409`. Unknown or
malformed IDs do not disclose file existence. Opening a replacement target and
unmounting proactively release the prior lease. If an active surface observes
lease expiration, it may reopen the same target automatically once; repeated
expiration is shown as an ordinary preview error.

`workspace/file/read` and `workspace/file/write` remain the authority for text
editing and revision-conflict behavior. The opaque resource URL grants only
read access to the bound representation and does not authorize listing,
writing, another workspace file, or a changed version of the same path.

The Browser pane has compact toolbar controls for navigation, reload, address,
annotation, and external open. Web and managed-Web hosts show ordinary
`http://` and `https://` pages in an unsandboxed preview-only iframe so page
scripts, forms, popups, and same-origin behavior follow normal browser rules.
Browser automation remains Desktop-only. Non-Desktop Browser control attempts
return a clear `Desktop required` failure instead of silently opening an
external browser. A remote site's own embedding headers may still prevent it
from loading in the iframe.

Browser tab identity, navigation state, and visibility are thread-scoped. Each
thread may reuse exactly one Browser tab, different threads never share a tab or
URL, and switching A -> B -> A restores A's Browser state. Address input accepts
explicit `http://` and `https://` URLs plus host/port shorthand. Public hosts
without a scheme default to HTTPS; `localhost`, IPv4 loopback, and IPv6 loopback
default to HTTP. Inputs with any other explicit scheme are rejected.

Desktop owns the managed Browser host. Workbench receives typed Browser state
and events through host/Gateway boundaries; it must not own native browser
handles or CDP connections. Desktop Browser profile storage is workspace-scoped
under the Browser plugin `data_root`.

## Browser Plugin

Browser is a built-in plugin surfaced in `Capabilities > Plugins`. It is
enabled by default unless the profile or project policy explicitly disables it.
The plugin row exposes its built-in source, Desktop requirement, data root, and
contributions. Disabling the plugin removes Browser automation from discovery
and disables Browser pane automation, while safe static previews remain
available.

`Capabilities > Tools` may reflect Browser toolset contributions only to the
extent they are backed by an implemented executor. Psychevo must not expose
model-visible `browser_*` tools that pretend to have CDP semantics before the
Desktop Browser host can execute those semantics. `web_fetch` remains the
read-only URL fetch toolset and is not the Browser.

## Browser Tools

Browser tools are stateful page-control actions against the thread-bound
Desktop Browser session. V1 tool names are:

- navigation/state: `browser_navigate`, `browser_snapshot`,
  `browser_wait_for`, `browser_resize`
- interaction: `browser_click`, `browser_click_coordinates`, `browser_type`,
  `browser_fill_form`, `browser_press_key`, `browser_hover`,
  `browser_scroll`, `browser_scroll_into_view`, `browser_select_option`,
  `browser_drag`
- inspection: `browser_take_screenshot`, `browser_evaluate`
- tabs: `browser_tab_list`, `browser_tab_new`, `browser_tab_select`,
  `browser_tab_close`

Tool outputs are text-first observations with structured errors. Screenshots
may be diagnostic tool outputs, but screenshots are not annotation context.

## Annotation Context

Annotation mode is entered from the Browser pane toolbar. Desktop injects the
page overlay through the Browser host/CDP boundary so the user can select an
element and write a comment inside the visible page.

Submitted comments are serialized as text appended to the user prompt before
`turn/start`:

```xml
<workspace_comment_context>
  <browser_annotation>
    <url>...</url>
    <title>...</title>
    <element>...</element>
    <comment>...</comment>
    <created_at>...</created_at>
  </browser_annotation>
</workspace_comment_context>
```

All fields are XML-escaped. Annotation context contains no screenshots and no
image input parts. Workbench shows pending annotation context as removable
composer chips before submit.

## Validation

Default validation uses deterministic local harnesses and fake providers.

- Shared Markdown tests cover complete Mermaid rendering, incomplete-fence code
  fallback, render errors, source copy, and existing GFM/frontmatter behavior.
- Shared Markdown and transcript tests cover exact current-inventory file-link
  promotion across relative, POSIX, Windows, Git Bash/MSYS, and UNC spellings;
  stable streaming boundaries; line suffixes; and the excluded message, block,
  URL, image, directory, missing-file, and outside-workspace cases. Transcript
  tests also cover completed `read`, `edit`, and `write` path targets plus their
  pending, failed, missing, and unrelated-tool exclusions.
- Workbench demand-detection tests cover plain root-level filenames, inline-code
  paths with line suffixes, and relative Markdown link destinations so lazy
  inventory loading cannot make supported promotion forms unreachable.
- Workbench tests cover per-thread Browser tab creation/reuse and A-B-A state
  restoration, Web preview-only automation messaging, public and loopback
  host/port normalization, explicit scheme rejection, unsandboxed Browser
  fallback, immediate HTML script and pointer interaction, document/content
  reload, the retained HTML sandbox/CSP restrictions, single active HTML
  execution, preview-focused hidden-tree defaults for workspace links and file
  tool actions, completion-driven same-workspace inventory refresh while Files
  is hidden, browser-entry restoration of a previously hidden file tree,
  file-preview expansion after hiding the tree, and right workspace navigation
  without text overlap.
- Gateway preview tests cover scoped authorization, traversal, pre-lookup
  symlink swaps, opened-handle text projection after path rebinding, and a
  deterministic lookup-to-open same-size/same-mtime swap rejection,
  unpredictable and distinct lease IDs, explicit release, idle and absolute
  expiration, changed-file conflicts, complete GET, HEAD, OPTIONS, open,
  closed, suffix, and invalid single ranges, rejected multi-range input,
  controlled CORS, and every security response header.
- Workspace file-surface tests exercise the public component seam and cover
  latest-target-wins, stale-lease release, inactive parsing cancellation and
  media pause, one automatic expired-lease reopen, loading progress, parser
  limits, common error presentation, workspace-relative breadcrumbs,
  direct-child breadcrumb menus, preview/source switching, file-level copy,
  preferred and alternate external actions, and the existing text editing and
  dirty guard behavior. File-tree tests cover
  breadcrumb-driven ancestor expansion and focus.
- Deterministically generated, traceable fixtures cover PNG, SVG, HEIC, PDF,
  H.264/AAC MP4, VP9/Opus WebM, MP3, DOCX, XLSX, PPTX, ODF, RTF, OFD, CSV, TSV,
  ZIP, and Excalidraw by asserting visible content through real renderers rather
  than mocking vendor internals. Security fixtures prove SVG scripts, Office
  macros and external relationships, hostile ZIP paths, and Excalidraw links or
  embeddables cannot execute, fetch, or write. Excalidraw fixtures cover shapes,
  arrows, free draw, frames, bound text, and embedded images.
- Playwright validates PDF Range traffic, media play and seek, Office worker and
  asset delivery without 404s, Excalidraw self-hosted font delivery without
  external requests, narrow and wide layout, and external-open fallback. Its
  startup-performance assertion verifies that preview chunks are absent from
  the initial Workbench request graph.
- Pure path-containment tests exercise both POSIX and Windows path semantics,
  including different drive letters, without requiring a Windows CI host.
- Workbench and Desktop production builds complete without a Node built-in
  externalization warning from `jszip`.
- Gateway/runtime tests cover the built-in Browser plugin list row, default
  enabled policy, explicit disable policy, and secret-free projection.
- Desktop tests cover Browser host command routing once the native host lands.
- Visual validation includes right-workspace Markdown/Mermaid, HTML preview,
  Browser empty state, Browser preview fallback, and Capabilities Browser
  plugin rows at desktop and mobile widths.
