# Visual Direction

The first Workbench visual direction is a dark precision ledger: quiet, dense,
local-agent workspace chrome with transcript rows as the primary surface,
evidence-oriented status details in secondary panes, and only black, white, or
transparent button/logo backgrounds. It is an app shell, not a landing page;
the first viewport orients the user, shows current work state, and enables the
next turn without hero copy or decorative backgrounds.

Surface hierarchy uses compact spacing, fine dividers, ledger rows, and
restrained shadow. Cards are reserved for bounded repeated items, requests,
drawers, explicit previews, and code surfaces; ordinary page sections should
read as continuous panes or rows rather than generic floating cards. Buttons use
a consistent radius scale and press feedback without resizing their layout
footprint. Workbench controls prefer icon-first expression for familiar actions
such as navigation, close, add, refresh, scroll, copy, and panel toggles; these
controls expose clear `aria-label` text and hover tooltips rather than permanent
long labels. Text buttons are reserved for commands whose wording is the
primary affordance or where an icon would be ambiguous. The transcript
jump-to-latest affordance is therefore an icon-only control with an accessible
label and hover tooltip. The left
navigation/sidebar reads as one continuous navigation
surface: Actions, Pinned, Sessions, and Settings use spacing, typography, and
soft selection indicators rather than prominent boxed outlines, heavy divider
lines, left rails, underline rails, or resting card-like row fills. Section
headers in that sidebar align their icons and labels to the same grid as the
action rows above them, with matching text scale and weight. Active
navigation rows, session rows, tabs, and segmented controls use a shallow tonal
shadow with a quiet surface step instead of inset rail effects. Logo containers
are transparent when the mark itself is visible against the dark chrome.
Settings and Status surfaces follow the same rule: setting rows, status metric
groups, context usage, and changed-file lists are list-like content on the
ordinary pane background. They should not render permanent row cards, heavy
outer panel borders, or filled containers just to separate adjacent controls.

Desktop uses a persistent left history/workdir pane, center
transcript/composer, and a resizable right workspace. On Web startup, Workbench
creates a local detached draft for the launch scope, falling back to the most
recent project scope and then the initialized default scope. The draft is
selected so the composer is ready immediately, but it is hidden from the left
Sessions browser until the first accepted prompt or shell command creates a
durable session. Startup does not proactively open the right workspace; users
reveal it from the right-column control or by taking an explicit file/diff
action. If a user navigates to another primary surface while startup is still
finishing, startup must not force the main surface back to Transcript.
Starting a new session may reveal Transcript immediately so the composer is
ready, but delayed startup, scope adoption, or history refresh work must not
override a later user-selected mobile Workbench panel such as Status.

When revealed without an active tab, the right workspace shows a
status/navigation home. The home summarizes current connection, session,
workdir, context, and changed-file state, then offers compact bordered rows for
Review, Terminal, and Files. Those rows use icon plus label only, with no
right-side explanatory copy. Selecting a row creates and activates a tab of
that type. After one tab exists, the right workspace shows a compact tab strip
plus a `+` menu for opening additional Review, Terminal, or Files tabs. Browser
is not part of this slice. Debug remains a developer-only tab when the local
Debug preference is enabled, but is not shown as a normal home navigation
target.

Review tabs own changed-file review and structured unified diff display. A
Review tab exposes a top-right Files toggle; when pressed, the tab splits into
left diff preview and right changed-files tree. Files tabs split into left file
preview and right workspace tree. This left-preview/right-tree structure is the
desktop information architecture for both tab types and must not be inverted;
only narrow responsive layouts may stack the tree below the preview. Review and
Files share the same filterable tree behavior: case-insensitive path filtering,
preserved ancestor folders, local folder expand/collapse, selected row state,
and compact status/count metadata where available. Their preview and tree
regions are immersive panes on the right-workspace background, separated by
subtle dividers rather than card frames, filled panel backgrounds, or rounded
outer containers. Selecting a changed file or a workspace file activates an
existing compatible tab when practical, otherwise it creates a new
right-workspace tab.

Review tabs may show Gateway-provided review groups in addition to the current
workspace diff. Review groups are ordered by turn, list changed files, and
allow only file-level Accept or Reject actions. Accept marks the file reviewed
without changing disk content. Reject asks Gateway to restore that file to the
baseline captured before the selected turn while preserving user changes that
already existed before that turn. If the file no longer matches the turn's
post-change revision, the row enters a conflict state and the user must reload
or inspect before retrying.

File and diff previews no longer open an inline center split; the transcript
surface remains the center workspace. Diff previews render parsed file headers,
hunks, line-number gutters, and addition/deletion/context rows instead of raw
plain `<pre>` output. Diff file headers use a compact Codex-style identifier:
status marker, workspace-relative path, and addition/deletion counts. They do
not show the raw `diff --git`, `index`, `---`, or `+++` metadata block as
visible header copy, and absolute paths are reserved for title/tooltip text
when the active workdir can be joined with the changed file path. Files
previews render text files as syntax-highlighted code and Markdown files
through the shared transcript Markdown renderer, with raw HTML escaped. The
Files tab header keeps only the tab title; the selected file absolute path
appears above the preview. Code highlighting uses the Workbench-local
`highlight.js` core integration with a hand-picked language set and app-token
`.hljs-*` colors, not a stock theme stylesheet.
Files tabs support bounded manual text editing in Workbench/Web. A text file
preview exposes an explicit Edit action. Edit mode uses a plain textarea with
line numbers, current line/column, Tab indentation, wrap toggle, find, go to
line, a dirty state, and `Cmd`/`Ctrl+S` save. Saves are explicit manual user
edits and do not enter the Review changed-file queue. Workbench blocks
navigation, file switching, and edit-mode exit while unsaved edits exist unless
the user confirms discarding them. If the file revision has changed since it
was opened, saving is blocked and the user can inspect the conflict, reload, or
force overwrite.
Terminal tabs are real
interactive local terminal sessions scoped to the active project workdir.
Terminal output is UI-only and is not transcript history or model-visible
context. Terminal tabs keep the xterm viewport primary and do not render a
persistent project title, path, or running badge above it. Apart from the
shared tab strip, a Terminal tab is a full-height terminal canvas; it must not
leave ordinary right-workspace background visible below a shorter xterm
viewport. Diff and code panes must remain readable in both dark and light
appearances; light appearance uses light diff and Markdown code surfaces rather
than retaining dark diff panels or dark Markdown code blocks, while dark code
surfaces use dedicated code text tokens rather than inheriting ordinary page
ink. Permission approval and clarify requests render in the composer area,
where TUI-style bottom interaction lives, and must not be displaced into
Review, Terminal, Files, Debug, or passive metrics.

The right workspace width is a Workbench host preference. Desktop users can
resize it from the left edge of the right column; the chosen width defaults to
about `520px` so Review and Files can open directly into their split working
layout, is clamped to a broad desktop range up to about `1200px` with a
viewport cap that protects the center transcript, persists on drag end, and is
restored on the next launch. Narrow/mobile layouts ignore the persisted desktop
width and keep one active surface visible at a time.

The Workbench web build keeps large, stable browser dependencies in named
vendor chunks instead of one monolithic vendor asset. React runtime,
Markdown/remark processing, syntax highlighting, terminal rendering, icons,
schema validation, generated protocol schemas, and local workspace packages may
be split independently so production builds stay inspectable without raising
the default chunk-size warning threshold.
Workdir-group ordering in the Sessions pane is based on actual session or
local draft recency, with label as a deterministic tie-breaker. Selecting or
resuming a session in a lower workdir marks that row active but must not lift
the workdir group to the top of the Sessions pane. Collapsed workdir groups
remain a compact top-stacked list with stable row spacing; empty available space
belongs below the list and must not be distributed between collapsed projects.
Workdir group labels set the hierarchy for the Sessions list; session titles
must not render larger or visually heavier than their workdir label. Active
session rows use a quiet background step for selection instead of oversized
typography. Session titles are visually nested under their workdir group label
with a minimal child-row indent instead of starting to the left of the group
label; the indent should clarify ownership without making the list feel
stair-stepped.
The Sessions and Transcript scrollers reserve a stable gutter so surrounding
layout does not shift, but their scrollbar thumbs stay hidden until the
scrolling surface itself is hovered, keyboard-focused, or actively scrolling.
Persisted session rows keep the row body focused on the session title. Time
metadata appears as compact relative days such as `0d` or `3d`, and the time
metadata plus More affordance appear on row hover or keyboard-visible focus
instead of staying visible in the resting list. Pointer-only focus must not keep
those affordances visible after the pointer leaves the row, and leaving the
hover/focus-visible area hides them immediately rather than fading them out.
Pin, rename, export, share, archive/restore, and
delete controls live behind that secondary More menu instead of rendering as a
permanent action strip under the session name. Local draft rows do not expose
session management actions until they become persisted sessions. Secondary
More and `+` menus in Workbench chrome must close when the user clicks outside
the menu, close on Escape with focus restored to the trigger, and remain open
when the user clicks inert space inside the menu. This outside-click behavior
applies to menu popovers, not slash, skill, or file completion listboxes.
Workbench chrome uses `Psychevo` as the visible product name. Project identity
belongs in the workdir/session grouping and settings detail surfaces, not as a
subtitle under the product brand. The Workbench browser tab title is
`Psychevo`, and the tab favicon uses the shared Psychevo logo mark rather than
a generic browser or globe icon. GUI-created workspaces and opened projects
are both ordinary workdirs; UI may show project affordances such as Git branch
only when the current workdir supports them. Creating a GUI workspace is an
icon-only Sessions header action immediately to the left of the
expand/collapse-all Sessions control, not a standalone primary left-nav item.
The Settings center page exposes an explicit return control at the top of its
own left navigation, followed by a settings search field. It does not show a
separate top Settings header, top-right close button, or current
project/workdir path. Settings is a compact app-level configuration center
rather than a single-column preference list or an embedded session panel. When
Settings is active, it replaces the Workbench session shell: the session list,
composer, mobile Workbench panel tabs, and right inspector are hidden. The
internal Settings navigation lists app-level settings directly in the left
column: `Appearance`, `Debug`, `Agents`, and bottom-aligned
`Archived sessions`. The right side renders only the selected item.
`Appearance` owns the light/dark Workbench preference, `Debug` owns the local
developer-diagnostics switch, `Agents` owns embedded Profile-level ACP backend
management, and `Archived sessions` directly displays archived sessions with
restore/delete actions. Outside Settings, the Workbench left sidebar always
shows active sessions; archived sessions are not a sidebar filter state.
Session-scoped controls such as Agent,
Model, Variant, and Permission mode do not appear in Settings; the
current-session agent can only be chosen through the composer agent selector.
Command catalog browsing is a transient transcript overlay, not a Settings
section, and MCP/integration or observability placeholders do not appear in
Settings until they become app-level configuration surfaces. The internal
Settings navigation becomes horizontal tabs on narrow layouts and follows the
same low-emphasis selected-row treatment as the left sidebar. The left sidebar collapse
control sits in the
same brand row as the logo/name and is icon-only; it must align to the right
edge of the session column. When the left sidebar is collapsed, the same
control becomes the expand affordance and uses a scaled Psychevo logo mark
instead of the generic panel icon. Collapsed sidebar chrome keeps the primary
action icons, such as New Session and Search, visible directly below
the logo toggle while hiding their text labels, and keeps the Settings utility
icon in the bottom utility rail at its normal vertical position. It must not
keep Pinned or Sessions list components mounted. The transcript surface starts
directly with conversation content rather than a redundant `Transcript` title row, and the
right inspector starts directly with Status/Files/Debug tabs instead of a
separate connection endpoint header. The right inspector expand/collapse
control is fixed to the top-right edge of the transcript column, above the
transcript surface, so inspector tabs remain only tab choices and collapsed
inspector state does not reserve a separate right-side rail.
The Status inspector treats the full, unshortened session id as the header
subtitle directly under `Status`; it does not repeat the project path there and
does not render a separate Session/Connection/Turn/Queued metric grid. Context
window, session-token, cache-read, cost, reasoning, and other useful usage
totals are compact Status facts, not transcript rows. They sit directly below
the session id as a single observability group instead of being duplicated in a
primary metric grid. The compact usage grid must not repeat Messages, Provider,
or Model once the context label/status already identifies the active provider
usage source.

Appearance is a frontend/host preference, not a provider or secret setting.
In light appearance, Workbench uses a warm reading-paper palette rather than a
cool blue or icy gray shell. The canvas is ivory, panels are warm paper,
borders are taupe, primary text is warm charcoal, and selected controls,
status accents, and active UI state use low-chroma amber/taupe tokens so they
read as quiet application chrome instead of a saturated brand color.
The bottom Settings utility entry is a location marker, not a primary action;
when Settings is active it uses the ordinary sidebar selected surface instead
of an accent fill.
Workbench defaults to the dark ledger appearance, and Settings provides a
light/dark appearance toggle. The choice may be persisted by the host storage
adapter and applied before ordinary panel rendering when available. Theme
switching must preserve the same layout, density, button background rules, and
status/diff preview behavior.
In dark appearance, primary shell labels such as `New Session`, `Search`,
`Pinned`, `Sessions`, `Settings`, and transcript state labels such
as `Thinking` must use readable foreground tokens rather than the faintest
muted text color. Dark surfaces use neutral warm-black tokens rather than a
cold blue sidebar hue, and primary text should sit in a higher luminance range
so navigation labels, empty transcript states, and transcript copy remain
legible while preserving the dark ledger structure. Filled user bubbles and
selected navigation rows must remain visibly separated from the page background
without becoming saturated accent surfaces.

Settings Debug provides a local Debug switch. When enabled, the right
inspector adds a `Debug` tab after `Files`; when disabled, the tab is absent.
The Debug tab shows the current Workbench event stream and Gateway notifications
as developer diagnostics, separate from ordinary transcript content and hidden
by default.

Mobile uses the same component tree with compact chrome: top status must not
crowd the composer or tab rail, collapsed sidebars must keep fixed-size icon
buttons, and the active panel owns the viewport.
