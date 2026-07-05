from __future__ import annotations

import sqlite3

from peval_py.state.artifacts import StateArtifactMixin
from peval_py.state.ingest import StateIngestMixin
from peval_py.state.mutations import StateMutationMixin
from peval_py.state.paths import WorkspacePaths, resolve_workspace_root, workspace_paths
from peval_py.state.queries import StateQueryMixin
from peval_py.state.schema import StateSchemaMixin


class ServeStateStore(
    StateSchemaMixin,
    StateIngestMixin,
    StateQueryMixin,
    StateMutationMixin,
    StateArtifactMixin,
):
    def __init__(
        self,
        paths: WorkspacePaths,
        *,
        initialize: bool = True,
        readonly: bool = False,
    ) -> None:
        self.paths = paths
        if readonly:
            uri = self.paths.state_db_path.resolve().as_uri() + "?mode=ro"
            self.conn = sqlite3.connect(uri, uri=True, check_same_thread=False)
        else:
            self.paths.state_db_path.parent.mkdir(parents=True, exist_ok=True)
            self.conn = sqlite3.connect(self.paths.state_db_path, check_same_thread=False)
        self.conn.row_factory = sqlite3.Row
        if initialize:
            self.initialize_schema()

    def close(self) -> None:
        self.conn.close()



def open_workspace_state(root: str | None = None) -> ServeStateStore:
    resolved = resolve_workspace_root(root)
    return ServeStateStore(workspace_paths(resolved))


def open_workspace_state_readonly(root: str | None = None) -> ServeStateStore:
    resolved = resolve_workspace_root(root)
    return ServeStateStore(
        workspace_paths(resolved),
        initialize=False,
        readonly=True,
    )
