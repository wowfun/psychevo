---
name: 031. Workspace Snapshot Cache
psychevo_self_edit: deny
---

Define the local workspace snapshot cache used by undo/redo.

## Cache Boundary

Workspace snapshots are local temporary cache material. They are not durable
evidence, session continuity facts, or audit records. Durable sessions and
messages may reference snapshot tree hashes for local undo/redo, but those
hashes are best-effort handles into the cache.

The cache is scoped by canonical workspace path, not by session. A workspace id
is a stable filesystem-safe hash derived from the canonical workspace path. The
default cache location is:

`<profile-home>/snapshots/workspaces/<workspace-id>`

The workspace id must not embed the raw workspace path. Implementations should
use a collision-resistant stable hash with a short filesystem-safe prefix.

## Snapshot Operations

New snapshots for all sessions in the same canonical workspace write to the
same workspace Git store. Snapshot restore reads from that workspace store.
Session ids must not define the primary snapshot storage path.

Because the workspace Git store is shared across sessions and processes,
snapshot track and restore operations must hold a cross-process operation lock
for the workspace store. Lock contention should produce bounded waiting or a
clear snapshot-unavailable failure instead of corrupting the shared store.

## Retention

Snapshot retention is best-effort cache maintenance. Runtime may prune
unreachable snapshot objects after seven days, matching the temporary nature of
undo/redo cache data. Cleanup should be throttled across processes so ordinary
snapshot use does not run expensive maintenance repeatedly.

Cleanup failure must not fail an otherwise valid user turn. If retention later
removes material needed by an old undo/redo operation, the interface may report
that the Git snapshot is unavailable.
