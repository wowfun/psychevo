import { useEffect, useMemo, useState, type CSSProperties } from "react";
import { ChevronDown, ChevronRight, FileText, FolderTree, Search } from "lucide-react";
import type { WorkspaceDiffResult } from "@psychevo/protocol";
import { fileBasename } from "../right-workspace-model";
import type { WorkspaceFileTreeItem } from "../types";

export function WorkspaceFileTree({
  emptyLabel,
  filterLabel,
  filterPlaceholder,
  items,
  selectedPath,
  onOpen
}: {
  emptyLabel: string;
  filterLabel: string;
  filterPlaceholder: string;
  items: WorkspaceFileTreeItem[];
  selectedPath: string | null;
  onOpen(path: string): void;
}) {
  const [collapsedDirs, setCollapsedDirs] = useState<Set<string>>(() => new Set());
  const [filter, setFilter] = useState("");
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

  return (
    <div className="workspaceFileTree">
      <label className="workspaceFileTreeFilter">
        <Search size={14} aria-hidden />
        <input
          aria-label={filterLabel}
          onChange={(event) => setFilter(event.currentTarget.value)}
          placeholder={filterPlaceholder}
          type="search"
          value={filter}
        />
      </label>
      <div className="fileTree" role="tree">
        {visibleItems.map((item) => {
          const directory = item.kind === "directory";
          const collapsed = directory && collapsedDirs.has(item.path);
          const selected = !directory && selectedPath === item.path;
          const badge = item.badge ?? item.status ?? null;
          return (
            <button
              aria-expanded={directory ? !collapsed : undefined}
              aria-selected={selected || undefined}
              className={[
                directory ? "is-directory" : "is-file",
                selected ? "is-selected" : "",
                item.status ? `is-${item.status}` : ""
              ].filter(Boolean).join(" ")}
              disabled={item.disabled}
              key={`${item.kind}:${item.path}`}
              onClick={() => directory ? toggleDirectory(item.path) : onOpen(item.path)}
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
