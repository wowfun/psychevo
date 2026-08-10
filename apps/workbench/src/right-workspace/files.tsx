import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { Check, ChevronDown } from "lucide-react";
import type { GatewayClient } from "@psychevo/client";
import { useConfirmAction } from "@psychevo/components";
import type {
  GatewayRequestScope,
  WorkspaceExternalFileAction,
  WorkspaceFileEntry,
  WorkspaceFileExternalActionsResult
} from "@psychevo/protocol";
import type { WorkspaceFileTreeItem } from "../types";
import { usePopoverDismiss } from "../popover-dismiss";
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
  roots,
  scope,
  selectedPath,
  tabId,
  truncated,
  onCompare,
  onDirtyChange,
  onFileTreeOpenChange,
  onOpen,
  onRootChange,
  htmlExecutionActive,
  fileTreeOpen
}: {
  client: GatewayClient | null;
  files: WorkspaceFileEntry[];
  root: string;
  roots: string[];
  scope: GatewayRequestScope | null;
  selectedPath: string | null;
  tabId: string;
  truncated: boolean;
  onCompare(path: string): void;
  onDirtyChange(tabId: string, dirty: boolean): void;
  onOpen(path: string): void;
  onRootChange(root: string, beforeCommit?: () => boolean | Promise<boolean>): Promise<boolean>;
  htmlExecutionActive: boolean;
  fileTreeOpen: boolean;
  onFileTreeOpenChange(open: boolean): void;
}) {
  const treeItems = useMemo(() => workspaceFileTreeItems(files), [files]);
  const fileMenuRequestRef = useRef(0);
  const [fileMenu, setFileMenu] = useState<WorkspaceFileMenuState | null>(null);
  const dirtyRef = useRef(false);
  const confirmAction = useConfirmAction();
  const [treeRevealRequest, setTreeRevealRequest] = useState<{ id: number; path: string } | null>(null);
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

  useEffect(() => {
    if (!fileTreeOpen) {
      setTreeRevealRequest(null);
    }
  }, [fileTreeOpen]);

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

  function revealBreadcrumbInTree(path: string) {
    onFileTreeOpenChange(true);
    setTreeRevealRequest((current) => ({ id: (current?.id ?? 0) + 1, path }));
  }

  async function selectRoot(nextRoot: string) {
    await requestFilesRootChange({
      currentRoot: root,
      nextRoot,
      onRootChange,
      confirmDirty: async () => (
        !dirtyRef.current || confirmAction({
          confirmLabel: "Discard edits",
          description: "The unsaved file changes will be lost.",
          title: "Discard unsaved file edits?",
          tone: "caution"
        })
      )
    });
  }

  return (
    <section className={`filesPanel ${fileTreeOpen ? "has-fileTree" : ""}`} aria-label="Workspace files">
      <WorkspaceFileSurface
        active={htmlExecutionActive}
        fileTree={{
          content: (
            <aside className="filesTreePane" aria-label="Workspace file tree">
              {roots.length > 1 && (
                <FilesRootSelector root={root} roots={roots} onSelect={selectRoot} />
              )}
              <WorkspaceFileTree
                emptyLabel="No workspace files."
                filterLabel="Filter workspace files"
                filterPlaceholder="Filter files..."
                items={treeItems}
                revealRequest={treeRevealRequest}
                selectedPath={selectedPath}
                onFileContextMenu={openFileMenu}
                onOpen={onOpen}
              />
              {truncated && <footer>File tree truncated.</footer>}
            </aside>
          ),
          items: treeItems,
          onOpen,
          onOpenChange: onFileTreeOpenChange,
          onReveal: revealBreadcrumbInTree,
          open: fileTreeOpen
        }}
        onCompare={onCompare}
        onDirtyChange={(nextDirty) => {
          dirtyRef.current = nextDirty;
          onDirtyChange(tabId, nextDirty);
        }}
        target={scope && selectedPath ? { path: selectedPath, scope } : null}
        textEditing="enabled"
        workspaceRoot={root || scope?.cwd || ""}
      />
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

function FilesRootSelector({
  root,
  roots,
  onSelect
}: {
  root: string;
  roots: string[];
  onSelect(root: string): Promise<void>;
}) {
  const [open, setOpen] = useState(false);
  const rootRef = useRef<HTMLDivElement>(null);
  const triggerRef = useRef<HTMLButtonElement>(null);
  usePopoverDismiss(open, rootRef, triggerRef, () => setOpen(false));

  return (
    <div className="filesRootSelector" ref={rootRef}>
      <button
        aria-expanded={open}
        aria-haspopup="menu"
        aria-label="Workspace directory"
        className="pevo-fieldControl pevo-fieldControl--compact filesRootSelectorTrigger"
        onClick={() => setOpen((current) => !current)}
        ref={triggerRef}
        title={root}
        type="button"
      >
        <span>{workspaceRootOptionLabel(root)}</span>
        <ChevronDown aria-hidden size={14} />
      </button>
      {open ? (
        <div aria-label="Workspace directory" className="filesRootMenu pevo-controlPopover" role="menu">
          {roots.map((candidate) => {
            const selected = candidate === root;
            return (
              <button
                aria-checked={selected}
                className={selected ? "is-selected" : undefined}
                key={candidate}
                onClick={() => {
                  setOpen(false);
                  void onSelect(candidate);
                }}
                role="menuitemradio"
                title={candidate}
                type="button"
              >
                <span>{workspaceRootOptionLabel(candidate)}</span>
                {selected ? <Check aria-hidden size={14} /> : null}
              </button>
            );
          })}
        </div>
      ) : null}
    </div>
  );
}

export function workspaceRootOptionLabel(root: string): string {
  return root;
}

export async function requestFilesRootChange({
  confirmDirty,
  currentRoot,
  nextRoot,
  onRootChange
}: {
  confirmDirty(): boolean | Promise<boolean>;
  currentRoot: string;
  nextRoot: string;
  onRootChange(root: string, beforeCommit?: () => boolean | Promise<boolean>): Promise<boolean>;
}): Promise<void> {
  if (nextRoot === currentRoot) {
    await onRootChange(nextRoot);
    return;
  }
  await onRootChange(nextRoot, confirmDirty);
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
