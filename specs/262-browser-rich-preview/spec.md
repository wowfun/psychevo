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
turn refreshes the workspace-scoped file inventory independently of which
thread owns the visible primary transcript, so newly created files can be
promoted when the transcript rerenders.

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
`file://` URLs for local preview. Workspace authorization grants read access; it
does not make the document trusted code.

The default preview is a locked, scriptless offline sandbox. Inline styles and
data/blob-backed local assets may render, but scripts do not run. Workbench
injects a restrictive Content Security Policy that denies scripts and automatic
network side effects from connections, parser-loaded subresources, forms, and
nested browsing by default, including `default-src`, `connect-src`,
`form-action`, frames, objects, workers, and remote media. The iframe does not
grant script, same-origin, form, popup, top-navigation, or download sandbox
capabilities. This locked mode is the state entered when a file is selected or
opened in Preview.

Locked mode is non-interactive as well as scriptless. Workbench makes the iframe
inert, removes it from keyboard focus, and blocks pointer delivery at the outer
iframe boundary so links or other disguised click targets cannot navigate or
issue requests before trust is granted. This intentionally disables scrolling
inside the locked document; the user must enter the explicit trusted run to
interact with or scroll the document itself.

`Run interactive preview` is a visible, explicit user action that starts a
trusted run for the currently displayed document. Only that mode grants
`allow-scripts`; network activity from the document is then inside the explicit
trust boundary. Form, popup, same-origin, top-navigation, and download sandbox
capabilities remain withheld. The trusted run also restores iframe pointer and
keyboard interaction. Trust is scoped to the exact path and content in that
preview surface and is revoked immediately when either changes. Returning to
previously viewed content must not silently resurrect an earlier trusted run.
The locked/trusted status and action row is Workbench chrome outside the iframe
content area. It must reserve its own layout space and may wrap at narrow
widths, but must never overlay or obscure the preview document.

Files and Preview are two views over one HTML execution surface. At most one
iframe for a selected HTML document may be mounted at a time. Activating Preview
must suspend the Files iframe, and returning to Files must suspend the Preview
iframe, without unmounting unrelated inactive tabs such as Terminal or Side
chat.

The Browser pane has compact toolbar controls for navigation, reload, address,
annotation, and external open. Web and managed-Web hosts may show a preview-only
iframe fallback for ordinary `http://` and `https://` pages, but Browser
automation is Desktop-only. Non-Desktop Browser control attempts return a clear
`Desktop required` failure instead of silently opening an external browser.

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
  URL, image, directory, missing-file, and outside-workspace cases.
- Workbench tests cover per-thread Browser tab creation/reuse and A-B-A state
  restoration, Web preview-only automation messaging, public and loopback
  host/port normalization, explicit scheme rejection, locked scriptless HTML
  CSP and sandbox behavior, explicit trusted runs, trust reset on path/content
  changes, locked link-click suppression with zero requests, restored trusted
  pointer interaction, single active HTML execution, non-overlapping HTML
  preview chrome at desktop and mobile widths, and right workspace navigation
  without text overlap.
- Gateway/runtime tests cover the built-in Browser plugin list row, default
  enabled policy, explicit disable policy, and secret-free projection.
- Desktop tests cover Browser host command routing once the native host lands.
- Visual validation includes right-workspace Markdown/Mermaid, HTML preview,
  Browser empty state, Browser preview fallback, and Capabilities Browser
  plugin rows at desktop and mobile widths.
