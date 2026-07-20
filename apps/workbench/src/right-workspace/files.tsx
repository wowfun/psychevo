import { useLayoutEffect, useMemo, useRef, useState } from "react";
import {
  FolderTree,
  PanelRightClose,
  PanelRightOpen
} from "lucide-react";
import type { GatewayClient } from "@psychevo/client";
import type {
  GatewayRequestScope,
  WorkspaceExternalFileAction,
  WorkspaceFileEntry,
  WorkspaceFileExternalActionsResult
} from "@psychevo/protocol";
import type { WorkspaceFileTreeItem } from "../types";
import { workspaceExternalActionMenuItems } from "./file-external-actions";
import { WorkspaceFileContextMenu } from "./file-context-menu";
import { WorkspaceFileSurface } from "./workspace-file-surface";
import {
  WorkspaceFileTree,
  type WorkspaceFileContextMenuRequest
} from "./tree";

export function FilesPanel({
  client,
  files,
  root,
  scope,
  selectedPath,
  tabId,
  truncated,
  onCompare,
  onDirtyChange,
  onFileTreeOpenChange,
  onOpen,
  htmlExecutionActive,
  fileTreeOpen
}: {
  client: GatewayClient | null;
  files: WorkspaceFileEntry[];
  root: string;
  scope: GatewayRequestScope | null;
  selectedPath: string | null;
  tabId: string;
  truncated: boolean;
  onCompare(path: string): void;
  onDirtyChange(tabId: string, dirty: boolean): void;
  onOpen(path: string): void;
  htmlExecutionActive: boolean;
  fileTreeOpen: boolean;
  onFileTreeOpenChange(open: boolean): void;
}) {
  const treeItems = useMemo(() => workspaceFileTreeItems(files), [files]);
  const fileMenuRequestRef = useRef(0);
  const [fileMenu, setFileMenu] = useState<WorkspaceFileMenuState | null>(null);
  const fileMenuScopeKey = workspaceScopeIdentity(scope);
  const fileMenuContextRef = useRef({ client, root, scopeKey: fileMenuScopeKey });

  useLayoutEffect(() => {
    const previous = fileMenuContextRef.current;
    fileMenuContextRef.current = { client, root, scopeKey: fileMenuScopeKey };
    if (previous.client !== client || previous.root !== root || previous.scopeKey !== fileMenuScopeKey) {
      fileMenuRequestRef.current += 1;
      setFileMenu(null);
    }
  }, [client, fileMenuScopeKey, root]);

  function closeFileMenu() {
    fileMenuRequestRef.current += 1;
    setFileMenu(null);
  }

  function openFileMenu(request: WorkspaceFileContextMenuRequest) {
    const requestId = fileMenuRequestRef.current + 1;
    fileMenuRequestRef.current = requestId;
    const nextMenu: WorkspaceFileMenuState = {
      actions: null,
      anchor: request.anchor,
      error: null,
      loading: true,
      path: request.path,
      pendingAction: null,
      requestId,
      x: request.clientX,
      y: request.clientY
    };
    setFileMenu(nextMenu);
    if (!client || !scope) {
      setFileMenu({
        ...nextMenu,
        error: "Connect to the workspace Gateway to use external file actions.",
        loading: false
      });
      return;
    }
    void client.request("workspace/file/externalActions", { path: request.path, scope }).then(
      (actions) => {
        setFileMenu((current) => (
          current?.requestId === requestId
            ? { ...current, actions, loading: false }
            : current
        ));
      },
      (error) => {
        setFileMenu((current) => (
          current?.requestId === requestId
            ? { ...current, error: fileActionErrorMessage(error), loading: false }
            : current
        ));
      }
    );
  }

  async function runFileMenuAction(action: WorkspaceExternalFileAction) {
    const current = fileMenu;
    if (
      !current
      || !current.actions?.availableActions.includes(action)
      || !client
      || !scope
      || current.pendingAction
    ) {
      return;
    }
    setFileMenu({ ...current, error: null, pendingAction: action });
    try {
      await client.request("workspace/file/openExternal", {
        action,
        path: current.path,
        scope
      });
      if (fileMenuRequestRef.current === current.requestId) {
        closeFileMenu();
      }
    } catch (error) {
      setFileMenu((latest) => (
        latest?.requestId === current.requestId
          ? { ...latest, error: fileActionErrorMessage(error), pendingAction: null }
          : latest
      ));
    }
  }

  return (
    <section className={`filesPanel ${fileTreeOpen ? "has-fileTree" : ""}`} aria-label="Workspace files">
      <header>
        <div className="filesPanelTitle">
          <FolderTree size={17} />
          <h2>Files</h2>
        </div>
        <div className="rightPanelActions">
          <button
            aria-label={fileTreeOpen ? "Hide file tree" : "Show file tree"}
            aria-pressed={fileTreeOpen}
            className={`filesTreeToggle ${fileTreeOpen ? "is-pressed" : ""}`}
            onClick={() => onFileTreeOpenChange(!fileTreeOpen)}
            title={fileTreeOpen ? "Hide file tree" : "Show file tree"}
            type="button"
          >
            {fileTreeOpen ? <PanelRightClose size={15} /> : <PanelRightOpen size={15} />}
          </button>
        </div>
      </header>
      <div className="filesSplit">
        <div className="filePreview">
          <WorkspaceFileSurface
            active={htmlExecutionActive}
            onCompare={onCompare}
            onDirtyChange={(nextDirty) => onDirtyChange(tabId, nextDirty)}
            target={scope && selectedPath ? { path: selectedPath, scope } : null}
            textEditing="enabled"
          />
        </div>
        {fileTreeOpen && (
          <aside className="filesTreePane" aria-label="Workspace file tree">
            <WorkspaceFileTree
              emptyLabel="No workspace files."
              filterLabel="Filter workspace files"
              filterPlaceholder="Filter files..."
              items={treeItems}
              selectedPath={selectedPath}
              onFileContextMenu={openFileMenu}
              onOpen={onOpen}
            />
            {truncated && <footer>File tree truncated.</footer>}
          </aside>
        )}
      </div>
      {fileMenu && (
        <WorkspaceFileContextMenu
          anchor={{ element: fileMenu.anchor, x: fileMenu.x, y: fileMenu.y }}
          ariaLabel={`Actions for ${fileMenu.path}`}
          error={fileMenu.error}
          items={fileMenu.actions
            ? workspaceExternalActionMenuItems(fileMenu.actions, fileMenu.pendingAction !== null)
            : []}
          loading={fileMenu.loading}
          onClose={closeFileMenu}
          onSelect={(action) => void runFileMenuAction(action)}
        />
      )}
    </section>
  );
}

type WorkspaceFileMenuState = {
  actions: WorkspaceFileExternalActionsResult | null;
  anchor: HTMLButtonElement;
  error: string | null;
  loading: boolean;
  path: string;
  pendingAction: WorkspaceExternalFileAction | null;
  requestId: number;
  x: number;
  y: number;
};

function fileActionErrorMessage(error: unknown): string {
  const message = (error instanceof Error ? error.message : String(error)).trim()
    || "External file action failed.";
  return message.length <= 240 ? message : `${message.slice(0, 239)}…`;
}

function workspaceScopeIdentity(scope: GatewayRequestScope | null): string {
  return scope ? JSON.stringify(scope) : "";
}

function workspaceFileTreeItems(files: WorkspaceFileEntry[]): WorkspaceFileTreeItem[] {
  return files.map((file) => ({
    kind: file.kind,
    name: file.name,
    path: file.path,
    depth: file.depth
  }));
}
