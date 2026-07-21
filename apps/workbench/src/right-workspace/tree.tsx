import { useEffect, useId, useLayoutEffect, useMemo, useRef, useState, type CSSProperties, type KeyboardEvent, type MouseEvent } from "react";
import { ChevronDown, ChevronRight, FileText, FolderTree, Search } from "lucide-react";
import type { WorkspaceDiffResult } from "@psychevo/protocol";
import { fileBasename } from "../right-workspace-model";
import type { WorkspaceFileTreeItem } from "../types";

export function WorkspaceFileTree({
  emptyLabel,
  filterLabel,
  filterPlaceholder,
  items,
  revealRequest,
  selectedPath,
  onOpen,
  onFileContextMenu
}: {
  emptyLabel: string;
  filterLabel: string;
  filterPlaceholder: string;
  items: WorkspaceFileTreeItem[];
  revealRequest?: { id: number; path: string } | null | undefined;
  selectedPath: string | null;
  onOpen(path: string): void;
  onFileContextMenu?: ((request: WorkspaceFileContextMenuRequest) => void) | undefined;
}) {
  const previewUnavailableDescriptionId = useId();
  const filterRef = useRef<HTMLInputElement | null>(null);
  const itemRefs = useRef(new Map<string, HTMLButtonElement>());
  const [collapsedDirs, setCollapsedDirs] = useState<Set<string>>(() => new Set());
  const [filter, setFilter] = useState("");
  const [pendingRevealPath, setPendingRevealPath] = useState<string | null>(null);
  const directoryPaths = useMemo(
    () => new Set(items.filter((item) => item.kind === "directory").map((item) => item.path)),
    [items]
  );
  const visibleItems = useMemo(
    () => visibleWorkspaceTreeItems(items, collapsedDirs, filter),
    [collapsedDirs, filter, items]
  );

  useEffect(() => {
    setCollapsedDirs((current) => {
      const next = new Set([...current].filter((path) => directoryPaths.has(path)));
      return next.size === current.size ? current : next;
    });
  }, [directoryPaths]);

  useEffect(() => {
    if (!revealRequest) {
      return;
    }
    setFilter("");
    setCollapsedDirs((current) => {
      const next = new Set(current);
      for (const directory of [...ancestorDirectoryPaths(revealRequest.path), revealRequest.path]) {
        next.delete(directory);
      }
      return next;
    });
    setPendingRevealPath(revealRequest.path);
  }, [revealRequest]);

  useLayoutEffect(() => {
    if (pendingRevealPath === null) {
      return;
    }
    const target = pendingRevealPath
      ? itemRefs.current.get(pendingRevealPath) ?? null
      : filterRef.current;
    if (!target) {
      return;
    }
    target.focus();
    target.scrollIntoView?.({ block: "nearest" });
    setPendingRevealPath(null);
  }, [pendingRevealPath, visibleItems]);

  function toggleDirectory(path: string) {
    setCollapsedDirs((current) => {
      const next = new Set(current);
      if (next.has(path)) {
        next.delete(path);
      } else {
        next.add(path);
      }
      return next;
    });
  }

  function openFileContextMenu(
    item: WorkspaceFileTreeItem,
    anchor: HTMLButtonElement,
    clientX: number,
    clientY: number
  ) {
    if (item.kind !== "file" || !onFileContextMenu) {
      return;
    }
    onFileContextMenu({
      anchor,
      clientX,
      clientY,
      path: item.path
    });
  }

  function handleContextMenu(event: MouseEvent<HTMLButtonElement>, item: WorkspaceFileTreeItem) {
    if (item.kind !== "file" || !onFileContextMenu) {
      return;
    }
    event.preventDefault();
    openFileContextMenu(item, event.currentTarget, event.clientX, event.clientY);
  }

  function handleTreeItemKeyDown(event: KeyboardEvent<HTMLButtonElement>, item: WorkspaceFileTreeItem) {
    if (
      item.kind !== "file"
      || !onFileContextMenu
      || (event.key !== "ContextMenu" && !(event.shiftKey && event.key === "F10"))
    ) {
      return;
    }
    event.preventDefault();
    const bounds = event.currentTarget.getBoundingClientRect();
    openFileContextMenu(
      item,
      event.currentTarget,
      Math.min(bounds.right, bounds.left + 24),
      bounds.bottom
    );
  }

  return (
    <div className="workspaceFileTree">
      <label className="workspaceFileTreeFilter pevo-searchField">
        <Search size={14} aria-hidden />
        <input
          aria-label={filterLabel}
          className="pevo-fieldControl pevo-fieldControl--search"
          onChange={(event) => setFilter(event.currentTarget.value)}
          placeholder={filterPlaceholder}
          ref={filterRef}
          type="search"
          value={filter}
        />
      </label>
      <span className="pevo-srOnly" id={previewUnavailableDescriptionId}>
        Only the built-in preview is unavailable. External file actions remain available from the context menu.
      </span>
      <div className="fileTree" role="tree">
        {visibleItems.map((item) => {
          const directory = item.kind === "directory";
          const collapsed = directory && collapsedDirs.has(item.path);
          const selected = !directory && selectedPath === item.path;
          const badge = item.badge ?? item.status ?? null;
          return (
            <button
              aria-describedby={item.previewDisabled ? previewUnavailableDescriptionId : undefined}
              aria-expanded={directory ? !collapsed : undefined}
              aria-selected={selected || undefined}
              className={[
                directory ? "is-directory" : "is-file",
                selected ? "is-selected" : "",
                item.previewDisabled ? "is-preview-disabled" : "",
                item.status ? `is-${item.status}` : ""
              ].filter(Boolean).join(" ")}
              key={`${item.kind}:${item.path}`}
              onClick={() => {
                if (directory) {
                  toggleDirectory(item.path);
                } else if (!item.previewDisabled) {
                  onOpen(item.path);
                }
              }}
              onContextMenu={(event) => handleContextMenu(event, item)}
              onKeyDown={(event) => handleTreeItemKeyDown(event, item)}
              ref={(node) => {
                if (node) {
                  itemRefs.current.set(item.path, node);
                } else {
                  itemRefs.current.delete(item.path);
                }
              }}
              role="treeitem"
              style={{ "--depth": item.depth } as CSSProperties}
              title={item.path}
              type="button"
            >
              <span className="fileTreeDisclosure" aria-hidden>
                {directory ? (collapsed ? <ChevronRight size={13} /> : <ChevronDown size={13} />) : null}
              </span>
              {directory ? <FolderTree size={14} /> : <FileText size={14} />}
              <span>{item.name}</span>
              {badge && <small>{badge}</small>}
            </button>
          );
        })}
        {visibleItems.length === 0 && <p>{emptyLabel}</p>}
      </div>
    </div>
  );
}

export type WorkspaceFileContextMenuRequest = {
  anchor: HTMLButtonElement;
  clientX: number;
  clientY: number;
  path: string;
};

function hasCollapsedDirectoryAncestor(path: string, collapsedDirs: Set<string>): boolean {
  for (const directory of collapsedDirs) {
    if (path !== directory && path.startsWith(`${directory}/`)) {
      return true;
    }
  }
  return false;
}

export function changedFileTreeItems(files: WorkspaceDiffResult["files"]): WorkspaceFileTreeItem[] {
  const items = new Map<string, WorkspaceFileTreeItem>();
  for (const file of files) {
    for (const directory of ancestorDirectoryPaths(file.path)) {
      items.set(`directory:${directory}`, {
        kind: "directory",
        name: fileBasename(directory),
        path: directory,
        depth: workspacePathDepth(directory)
      });
    }
    items.set(`file:${file.path}`, {
      badge: file.status,
      kind: "file",
      name: fileBasename(file.path),
      path: file.path,
      depth: workspacePathDepth(file.path),
      status: file.status
    });
  }
  return [...items.values()].sort(compareTreeItems);
}

function visibleWorkspaceTreeItems(
  items: WorkspaceFileTreeItem[],
  collapsedDirs: Set<string>,
  filter: string
): WorkspaceFileTreeItem[] {
  const normalizedFilter = filter.trim().toLowerCase();
  if (!normalizedFilter) {
    return items.filter((item) => !hasCollapsedDirectoryAncestor(item.path, collapsedDirs));
  }
  const matchingPaths = new Set<string>();
  const visibleAncestorPaths = new Set<string>();
  const matchingDirectoryPaths = new Set<string>();
  for (const item of items) {
    if (!treeItemMatches(item, normalizedFilter)) {
      continue;
    }
    matchingPaths.add(item.path);
    if (item.kind === "directory") {
      matchingDirectoryPaths.add(item.path);
    }
    for (const ancestor of ancestorDirectoryPaths(item.path)) {
      visibleAncestorPaths.add(ancestor);
    }
  }
  return items.filter((item) => {
    if (matchingPaths.has(item.path) || visibleAncestorPaths.has(item.path)) {
      return true;
    }
    for (const directory of matchingDirectoryPaths) {
      if (item.path !== directory && item.path.startsWith(`${directory}/`)) {
        return true;
      }
    }
    return false;
  });
}

function treeItemMatches(item: WorkspaceFileTreeItem, normalizedFilter: string): boolean {
  return item.path.toLowerCase().includes(normalizedFilter) || item.name.toLowerCase().includes(normalizedFilter);
}

function ancestorDirectoryPaths(path: string): string[] {
  const segments = normalizedWorkspacePath(path).split("/").filter(Boolean);
  const directories: string[] = [];
  for (let index = 1; index < segments.length; index += 1) {
    directories.push(segments.slice(0, index).join("/"));
  }
  return directories;
}

function compareTreeItems(left: WorkspaceFileTreeItem, right: WorkspaceFileTreeItem): number {
  const leftSegments = left.path.split("/");
  const rightSegments = right.path.split("/");
  const length = Math.min(leftSegments.length, rightSegments.length);
  for (let index = 0; index < length; index += 1) {
    const leftSegment = leftSegments[index] ?? "";
    const rightSegment = rightSegments[index] ?? "";
    if (leftSegment !== rightSegment) {
      return leftSegment.localeCompare(rightSegment);
    }
  }
  if (leftSegments.length !== rightSegments.length) {
    return leftSegments.length - rightSegments.length;
  }
  if (left.kind !== right.kind) {
    return left.kind === "directory" ? -1 : 1;
  }
  return left.path.localeCompare(right.path);
}

function workspacePathDepth(path: string): number {
  return Math.max(0, normalizedWorkspacePath(path).split("/").filter(Boolean).length - 1);
}

export function normalizedWorkspacePath(path: string): string {
  return path.replace(/\\/g, "/").replace(/^\/+/, "").replace(/\/+$/, "");
}

export function absoluteWorkspacePath(root: string, path: string): string {
  const trimmedPath = path.trim();
  if (!trimmedPath) {
    return root || "";
  }
  if (/^(?:[a-zA-Z]:[\\/]|\/)/.test(trimmedPath)) {
    return trimmedPath;
  }
  const trimmedRoot = root.trim().replace(/[\\/]+$/, "");
  if (!trimmedRoot) {
    return trimmedPath;
  }
  return `${trimmedRoot}/${normalizedWorkspacePath(trimmedPath)}`;
}

export { fileBasename } from "../right-workspace-model";
