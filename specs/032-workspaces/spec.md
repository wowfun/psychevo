---
name: 032. Workspaces
psychevo_self_edit: deny
---

# 032. Workspaces

Define Psychevo workspace identity, directory membership, Thread execution
environment, and product navigation placement.

This topic is the source of truth for workspace semantics. Environment facts,
permission policy, sandbox enforcement, and interface rendering remain owned by
their existing topics.

## Workspace Catalog

A workspace is a stable profile-scoped entity with:

- an opaque `WorkspaceId`;
- an independently editable non-empty display name;
- one or more ordered canonical existing directories;
- a revision used for atomic editor updates.

A Workspace contains at most 256 directories. This keeps every root
representable within bounded Files and completion work rather than accepting a
catalog shape whose later roots can never be visited under the global budget.

The first directory is the primary directory. Reordering a directory to the
first position changes the primary directory. There is no separate directory
identifier or primary-directory field. Renaming a workspace never renames or
moves a directory.

One canonical directory belongs to at most one workspace. Nested directories
may belong to different workspaces. When a direct cwd must be associated for
navigation, an existing explicit Thread binding wins, followed by the workspace
whose root is the longest containing path. If none matches, Framework creates a
new single-directory workspace as part of Thread admission.

Workspace updates atomically replace the display name and complete ordered
directory list. They reject an empty name, an empty or duplicate directory
list, a directory owned by another workspace, a missing/non-directory path, or
a stale expected revision. A stale update returns the latest workspace view and
does not partially apply.

Psychevo does not support migration from pre-v33 state databases. Opening a
v29-v32 database fails with the normal reset/new-state guidance. No historical
cwd grouping or client-local pin state is imported.

## Thread Workspace Context

Every durable Thread stores one workspace binding and a root-source mode. The
Thread cwd is the first runtime root.

Starting from an explicit workspace captures its current primary directory as
cwd and resolves the workspace's current ordered directories at each execution
boundary. Starting from a direct cwd captures only `[cwd]`, even when the Thread
is grouped under a containing workspace. Catalog membership is therefore not
an implicit authority grant.

For a detached explicit-workspace draft, the Workspace id is authoritative at
first Turn or user-shell admission. Gateway resolves the current primary and
ordered roots before target, configuration, permission, sandbox, or Agent
preparation; a stale draft scope is never allowed to prepare one directory and
admit another. An explicit Workspace target also overrides any inferred source
binding: admission either creates a new Thread from one captured Workspace
snapshot or rejects an explicit conflicting Thread target. Framework consumes
that captured snapshot for Thread creation, shell validation, sandbox
construction, and the accepted Turn invocation without re-reading the mutable
catalog during the same admission. A captured snapshot is an opaque Framework
value: admission revalidates its id, revision, canonical ordered roots, and
primary cwd against the catalog before granting filesystem authority, and the
creation transaction rejects a catalog revision that changed after that
validation. Callers cannot construct or widen a snapshot from public fields.
Fallible root and sandbox validation occurs
before a first persistent shell creates its Thread, so a rejected shell leaves
no durable empty Thread behind. An already-created Thread retains its captured cwd when the
Workspace is reordered, while its next-boundary runtime roots use the latest
catalog order with that cwd retained as the execution root.

Editing an explicit workspace affects its bound Threads on their next start,
resume, fork, or Turn; it never mutates an already running Turn. Direct-cwd
Threads remain fixed to `[cwd]` until an explicit future rebind operation. This
matches the reference lifecycle rule that workspace roots can be replaced at a
Turn boundary and that resume without an override restores the latest roots.

Side children and native or Agent forks inherit the parent Thread's binding and
root-source mode and the parent Turn's captured runtime-root snapshot for both
permission evaluation and sandboxing. An imported ACP session enters through direct-cwd admission
unless the import surface supplies an explicit Workspace target. Framework
persists the binding and direct-cwd root snapshot in the same admission or fork
transaction that creates the durable Thread fact.

Runtime roots are environment facts, not permissions. Permission profiles and
sandbox mode decide which roots are readable or writable. Runtime Profile
configured roots remain a separate policy input and never appear as workspace
directories, Thread roots, or Files roots.

The canonical filesystem identity of every captured runtime root is immutable
for the lifetime of that Turn. Permission and sandbox checks compare targets
against those captured identities; they never reinterpret a stored root
pathname as new authority. If a root pathname is renamed, replaced, or starts
resolving to a different identity, the action fails closed.
Canonical pathname equality is insufficient identity: admission records the
existing directory object's stable device/inode identity on Unix and volume/file
identity on Windows. A missing or recreated directory at the same pathname is a
different object. On hosts with directory-handle support, the capture retains an
open handle for its complete lifetime; this prevents a deleted object's stable
number from being recycled into an indistinguishable replacement and lets
permission, sandbox, and ACP operations remain relative to the accepted object.
Windows obtains the volume and file identity from that opened handle through a
stable operating-system API; workspace admission must compile on the workspace's
declared stable Rust toolchain and must not depend on unstable standard-library
filesystem extensions. A Windows reparse-point root, or a volume that reports no
stable nonzero file identity (including unsupported FAT/exFAT configurations),
is not admitted because it cannot satisfy the immutable-root contract. This is
an explicit current limitation for a selected OneDrive directory when that
directory itself is represented as a reparse point; selecting a non-reparse
ancestor or local materialized directory is required.
Revalidation requires the pathname to resolve to the same opened directory and
rejects a missing root, a non-directory replacement, or a different object. A
shell sandbox either installs its rule from that verified handle or revalidates
immediately before rule installation.
Every capture/revalidation failure at this boundary uses the
`path_identity_changed` error code while retaining the underlying diagnostic;
raw platform I/O errors never escape as an unclassified authorization failure.
Identity capture is part of Turn admission and completes before asynchronous
Agent preparation, the durable accepted receipt, or a wait in the Thread lane.
Preparation, Native and ACP dispatch, and delegated child admission consume the
same opaque pre-acceptance capture; none may reconstruct authority from root
path strings or establish a newer baseline after a queued Turn starts running.
Admission performs directory canonicalization and filesystem-identity reads on
a blocking worker rather than a Tokio runtime worker. Standalone child-agent
and existing-Thread shell entrypoints establish this capture before loading
project configuration, extension catalogs, or other asynchronous preparation.
ACP validates that capture before every session handshake and before prompt
dispatch. A platform sandbox that can
express Workspace write access only through mutable pathnames fails closed for
that mode rather than claiming the immutable-root guarantee.

## Navigation

Workspace and Thread pinning are profile-scoped Gateway product state, not
Framework Thread semantics. Gateway stores two independently ordered lists:

- pinned Thread ids;
- pinned Workspace ids.

New pins are inserted at the start of their own list. Pin and unpin are
single-target, idempotent mutations. Product surfaces may render both lists in
one `Pinned` section, with Threads before Workspaces, but there is no cross-kind
order.

One pin mutation changes only the target row and the navigation revision. It
does not rewrite unaffected pins or maintain unused wall-clock metadata; the
ordered representation keeps writer work bounded independently of list length.

A pinned item is removed from its ordinary placement. A pinned Workspace may
render its unpinned Threads; a separately pinned Thread is omitted from that
Workspace so each visible entity has one placement. Pin and unpin execute
immediately as idempotent single-target mutations.
Navigation revision and both ordered pin lists are read from one SQLite
snapshot, so the revision always identifies the returned lists. A cascading
Thread deletion that removes a pin advances the revision exactly as an
explicit unpin does.

## Files and Execution

The browser projection exposes stable Workspace identity and ordered roots;
Framework execution context resolves the bound Thread's `workspaceId`, `cwd`,
and runtime roots. Files presentation and `@` completion consume those roots. A
detached explicit-workspace draft uses the current catalog directories; a
direct-cwd draft uses only its cwd.

An explicit-Workspace Thread persists only its stable Workspace binding; its
runtime roots are read from the live catalog at a Turn boundary. Only a
direct-cwd Thread persists its single captured root. Forking preserves that
root-source distinction and does not copy redundant catalog-root rows.

Workbench sends the selected Files root as the existing request scope cwd and
sends paths relative to that root. Gateway applies the existing path identity
and traversal checks below that scope. Absolute local-file references remain
authoritative; relative references resolve from the Thread cwd.

The selected Files root is local presentation state. Changing it updates only
the Files tree and preview. It never changes Thread cwd, Terminal cwd, Changes,
Diff, Git state, or Transcript link discovery. The selection is keyed by the
authoritative Thread/draft root set returned by Thread or draft open, rather
than a separately cached catalog projection, and is clamped to the primary root
whenever that authority changes, including switches between Threads with the
same cwd. Every such authority transition invalidates in-flight Files reads,
even when the already committed Files inventory happens to use the new primary
root. Transcript-relative file actions run the same dirty-editor transition as
ordinary Files navigation before changing either the selected root or preview
target; declining confirmation leaves both the root and draft text unchanged.
Opening another file in the already selected root still runs that transition;
the caller may bypass the ordinary tab guard only when the same navigation has
already completed an authoritative commit-time dirty confirmation.
They carry the Thread-cwd inventory root into preview and save operations
instead of inheriting the currently selected Files root. Multi-root selectors
expose a unique accessible path label for every root, including roots with the
same basename. Completion deduplicates candidates by absolute target identity,
ranks matches from every eligible root before applying its global bound, and
reserves representation across roots without making catalog order a ranking
input. An exact match in a later root cannot be excluded by weaker matches in
the first 50 roots, and nested roots do not emit duplicate rows or React keys
for one target.
Root selection is committed only by the newest successful `workspace/files`
response. A failed or superseded read leaves the previously rendered and
selected root unchanged. Callers stop the dependent preview intent when a root
read is superseded, avoid rereading an already selected current result, and
fence late file-write refreshes so they cannot restore an obsolete root or
patch a tab now displaying another root. Completion retains its per-root
reservation through the public limit and Agent-result merge, and filesystem
traversal has a global visited-entry budget while running outside the async RPC
worker. Superseding or cancelling a completion request cooperatively stops its
blocking traversal and releases global scan admission; obsolete scans cannot
retain all permits until a slow filesystem walk naturally finishes. Unused scan
budget from a sparse root is redistributed to remaining
roots, and each root retains only a bounded best-candidate set before the final
fair merge; an empty query never materializes the entire visited inventory.
Composer completion owns a cancellable Gateway request. Replacing, closing, or
blurring the completion surface aborts the prior request so cancellation reaches
the server traversal rather than merely hiding its response. While an explicit
Workspace draft is opening, completion is addressed to that destination
Workspace and scope; it never falls back to the previously rendered draft.
An explicit Files-root selection owns one single-flight intent. Background
refreshes join that intent instead of superseding it, and selecting the currently
rendered root cancels a pending switch to another root. Because the old preview
remains editable while inventory I/O is pending, the dirty-editor guard runs at
commit time; a draft that became dirty after the request began cannot be cleared
without confirmation. Identical Thread-cwd inventory reads needed by Files and
Transcript link discovery share one in-flight filesystem scan.
A Transcript link that depends on a pending root switch joins that exact intent,
waits for its committed inventory, and runs its own commit-time dirty guard
before opening the preview. It never treats an uncommitted root intent as the
currently rendered Files authority. One dependent navigation produces at most
one discard confirmation: joiners share the authoritative transition result
rather than appending equivalent guards. Starting a newer explicit root intent
aborts the superseded `workspace/files` RPC so obsolete inventory scans do not
continue consuming Gateway request and filesystem capacity.
Changing Thread or draft authority detaches every obsolete pending Files read,
and the visible Files effect keys refreshes by the complete authority identity,
not cwd alone, so same-cwd direct and explicit-Workspace transitions reload the
new primary inventory.

Browser preview-root installation and use are ordered against Workspace catalog
edits. A grant records the durable Workspace revision that supplied its roots;
every preview authorization verifies that revision against the shared catalog.
This check is process-independent, so an edit through one Gateway revokes stale
preview authority held by another Gateway sharing the state database.
An asynchronous draft or Thread-scope grant may publish roots only while the
Workspace authority generation it observed remains current; a stale writer
cannot reinstall roots after catalog invalidation. Detached explicit-Workspace
preparation compares its complete root snapshot immediately before and after
ACP preparation. If the catalog changes, Gateway discards the preparation and
does not publish a stale draft or preview grant.

Native `workspace-write` execution admits non-cwd runtime roots as additional
writable roots; other sandbox modes retain their existing policy. Outbound ACP
new, load, resume, fork, Turn, and detached-draft preparation requests send all
non-cwd roots as `additional_directories`; a rejected multi-root handshake fails
before a prompt is delivered rather than silently narrowing it.
Peers must advertise the ACP `session.additionalDirectories` capability before
Gateway sends a non-empty set. When an explicit Workspace changes between
Turns, Gateway reattaches the same native session with `session/load` and the
new roots before delivering the next prompt; if the peer cannot safely reload,
the Turn remains not-delivered. Before that rejection Gateway revokes the stale
callback context and terminates session-owned terminals. Reload, session-ready,
configuration, input preparation, and other pre-prompt waits observe Turn
cancellation; an already-cancelled Turn never dispatches a prompt. Prepared ACP
promotion identity includes cwd and the complete additional-directory set.
Cancellation during a new or reloaded ACP session is owned by that lifecycle
operation until the protocol response is fenced and the provisional process,
callback context, and terminals have been reaped; Turn cancellation may request
peer cancellation or force process teardown, but no outer wait may drop the
cleanup future. Every ACP filesystem callback is authorized by the
same captured runtime permission policy as an equivalent built-in filesystem
tool before client capability and Workspace containment checks permit local I/O.
Every not-delivered failure after ACP attachment and before prompt dispatch
revokes that attachment and reaps its terminals. Forced actor teardown revokes
all cloned attachment guards before clearing the context map, so callbacks that
outlive map removal cannot commit later.
The persistence callback that binds a newly attached native session is part of
that provisional lifecycle. Cancellation or failure there removes the resident
session and callback context and waits for all session-owned terminal process
trees. Generation teardown is not complete until those terminal tasks have
reported exit. Workspace-root reload never relaxes the immutable MCP declaration
binding.

Filesystem callback authorization returns an identity-bound open handle, or
owns the operation itself; it never returns a pathname for a later independent
open. Terminal callback cwd installation is likewise anchored to the captured
directory object. Platforms without a safe handle-relative implementation fail
closed for those delegated operations.
Every installed ACP attachment owns a revocation signal. Replacing or removing
its context revokes cloned callback contexts, and each callback rechecks that
signal immediately before filesystem mutation, process spawn, and terminal
publication. Session terminals run in owned process groups; teardown waits for
the complete process tree and terminal reader tasks before reporting revocation
or generation completion.

## Presentation

Pinned and ordinary placements render the same shared Thread row and Workspace
row Modules. Placement may change grouping or indentation, but not row DOM,
height, color, hover/active/running/focus state, or available action menu.
Ordinary Thread rows remain single-line; workspace context belongs in accessible
hover/title content rather than a pinned-only subtitle.

When navigation projects a Thread into a Workspace, explicit `sessionIds`
membership is resolved across the complete Workspace set before any cwd/root
containment fallback. A retained cwd that was later adopted by another
Workspace never overrides the Thread's explicit binding.
Drafts without durable membership use the longest containing Workspace root,
with path-component boundaries, so a nested draft does not jump groups when its
first Thread is admitted.

The Workspace editor updates name and ordered directories in one save. It marks
the first directory as primary, lets another directory move to the first
position, and prevents removal of the last directory. Workspace deletion,
arbitrary workspace merge, pin drag-reordering, and Thread rebind controls are
out of scope.

Direct-cwd and explicit-Workspace composer choices remain distinct even when
they share the same primary cwd. Selecting or visually marking one never
converts catalog membership into authority for the other.

Before a root edit rebinds an active draft, Workbench runs the Files dirty-state
transition; declining leaves both the editor and draft authority unchanged. A
name-only edit does not rebind the draft. After an editor save commits, the modal remains mounted and busy until the
active draft is rebound and navigation refresh completes. Rebinding preserves
the currently selected Agent and Runtime Profile even when a primary-directory
reorder changes cwd; a post-commit failure remains visible in the editor.
Editing the Workspace bound to the current durable Thread also refreshes that
Thread before the editor closes. A root edit performs the dirty Files transition
before commit, then adopts the returned authoritative roots so removed roots do
not remain selectable and added roots become available immediately.

On an optimistic-revision conflict the editor loads and displays the complete
latest Workspace returned by Gateway before another ordinary save is possible;
updating only the revision while retaining unseen stale roots is not an
informed overwrite.

The editor is a modal keyboard surface: focus enters it, Tab and Shift+Tab stay
inside it, idle Escape closes it, and close restores the invoking control. Its
body scrolls within the viewport while header and footer remain reachable.
While save work disables ordinary controls, the dialog itself remains a focused
Tab stop and contains keyboard focus until the operation settles.

Workspace storage has no implicit/explicit presentation status: authority is
defined by each Thread binding's `root_source`. A catalog edit changes name,
ordered roots, and revision only. Maximum-size root validation and insertion use
set-based SQLite statements, not one awaited statement per root. Cwd-scoped
initial browsing selects eligible Workspace ids and roots in SQL, using an index
that covers the retained `sessions.cwd` membership lookup rather than scanning
the complete Thread history. Once a stable
Workspace cursor exists, later pages ignore the initial cwd filter. Pin
mutations commit their bounded target/revision write before reading the complete
navigation projection, so pin-list size does not extend the writer lock.
