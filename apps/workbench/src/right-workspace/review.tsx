import { useMemo, useState } from "react";
import { Check, FolderTree, GitPullRequest, RefreshCw, RotateCcw } from "lucide-react";
import { diffLineStats, parseUnifiedDiff } from "@psychevo/components";
import type { ContextReadResult, ThreadSnapshot, WorkspaceChangesResult, WorkspaceDiffResult } from "@psychevo/protocol";
import { SessionObservability } from "./usage";
import { WorkspaceFileTree, absoluteWorkspacePath, changedFileTreeItems, normalizedWorkspacePath } from "./tree";

export function ReviewPanel({
  activity,
  changedFiles,
  changes,
  context,
  diff,
  root,
  sessionId,
  status,
  cwd,
  onAcceptChange,
  onChangedFile,
  onRejectChange,
  onRefresh
}: {
  activity: ThreadSnapshot["activity"];
  changedFiles: WorkspaceDiffResult["files"];
  changes: WorkspaceChangesResult | null;
  context: ContextReadResult | null;
  diff: WorkspaceDiffResult | null;
  root: string;
  sessionId: string | null;
  status: string;
  cwd: string;
  onAcceptChange(turnId: string, path: string): void;
  onChangedFile(path: string): void;
  onRejectChange(turnId: string, path: string): void;
  onRefresh(): void;
}) {
  const [filesOpen, setFilesOpen] = useState(false);
  const contextPercent = typeof context?.percent === "number" ? Math.round(context.percent) : 0;
  const changedTreeItems = useMemo(() => changedFileTreeItems(changedFiles), [changedFiles]);
  const selectedPath = diff?.selectedPath ?? diff?.files[0]?.path ?? null;
  return (
    <section className={`reviewPanel ${filesOpen ? "has-fileTree" : ""}`} aria-label="Review">
      <header>
        <GitPullRequest size={17} />
        <div>
          <h2>Review</h2>
          <p>{cwd || "workspace"}</p>
        </div>
        <div className="rightPanelActions">
          <button
            aria-label={filesOpen ? "Hide changed files" : "Show changed files"}
            aria-pressed={filesOpen}
            className={`reviewFilesToggle ${filesOpen ? "is-pressed" : ""}`}
            onClick={() => setFilesOpen((value) => !value)}
            title="Files"
            type="button"
          >
            <FolderTree size={14} />
            <span>Files</span>
          </button>
          <button aria-label="Refresh Review" onClick={onRefresh} title="Refresh" type="button">
            <RefreshCw size={15} />
          </button>
        </div>
      </header>
      <div className="reviewStatusRows">
        <span>{status}</span>
        <span>{sessionId ? shortSessionId(sessionId) : "draft"}</span>
        <span>{activity.running ? "running" : "idle"}</span>
        <span>{context?.available ? `${contextPercent}% context` : "no context"}</span>
      </div>
      <ReviewChanges
        changes={changes}
        onAcceptChange={onAcceptChange}
        onChangedFile={onChangedFile}
        onRejectChange={onRejectChange}
      />
      <div className="reviewSplit">
        <div className="reviewDiffPane">
          <DiffPreview diff={diff} root={root} />
        </div>
        {filesOpen && (
          <aside className="reviewFilesPane" aria-label="Changed files">
            <WorkspaceFileTree
              emptyLabel="No changed files."
              filterLabel="Filter changed files"
              filterPlaceholder="Filter files..."
              items={changedTreeItems}
              selectedPath={selectedPath}
              onOpen={onChangedFile}
            />
          </aside>
        )}
      </div>
    </section>
  );
}

function ReviewChanges({
  changes,
  onAcceptChange,
  onChangedFile,
  onRejectChange
}: {
  changes: WorkspaceChangesResult | null;
  onAcceptChange(turnId: string, path: string): void;
  onChangedFile(path: string): void;
  onRejectChange(turnId: string, path: string): void;
}) {
  const groups = changes?.groups ?? [];
  if (groups.length === 0) {
    return (
      <div className="reviewChanges is-empty">
        <p>No turn changes.</p>
      </div>
    );
  }
  return (
    <div className="reviewChanges" aria-label="Turn changes">
      {groups.map((group) => (
        <section className="reviewChangeGroup" key={group.turnId}>
          <header>
            <span>{shortSessionId(group.turnId)}</span>
            <b>{group.files.length}</b>
          </header>
          {group.files.map((file) => (
            <div className={`reviewChangeFile is-${file.reviewStatus}`} key={`${group.turnId}:${file.path}`}>
              <button className="reviewChangePath" onClick={() => onChangedFile(file.path)} title={file.path} type="button">
                <span>{file.path}</span>
                <small>{file.status}</small>
              </button>
              <span className="reviewChangeState">{file.reviewStatus}</span>
              <button
                aria-label={`Accept ${file.path}`}
                disabled={file.reviewStatus === "accepted"}
                onClick={() => onAcceptChange(group.turnId, file.path)}
                title="Accept"
                type="button"
              >
                <Check size={13} />
              </button>
              <button
                aria-label={`Reject ${file.path}`}
                disabled={!file.canReject || file.reviewStatus === "rejected"}
                onClick={() => onRejectChange(group.turnId, file.path)}
                title={file.message ?? "Reject"}
                type="button"
              >
                <RotateCcw size={13} />
              </button>
              {file.message && <em title={file.message}>{file.message}</em>}
            </div>
          ))}
        </section>
      ))}
    </div>
  );
}

function DiffPreview({ diff, root }: { diff: WorkspaceDiffResult | null; root: string }) {
  const diffText = useMemo(() => {
    if (!diff) {
      return "";
    }
    if (diff.unifiedDiff.trim()) {
      return diff.unifiedDiff;
    }
    return diff.files
      .map((file) => file.placeholder)
      .filter((value): value is string => Boolean(value?.trim()))
      .join("\n\n");
  }, [diff]);
  const files = useMemo(() => parseUnifiedDiff(diffText), [diffText]);
  const statusByPath = useMemo(() => {
    const map = new Map<string, string>();
    for (const file of diff?.files ?? []) {
      map.set(file.path, file.status);
    }
    return map;
  }, [diff?.files]);

  if (!diff || !diffText.trim()) {
    return (
      <div className="diffPreview is-empty">
        <p>No diff content.</p>
      </div>
    );
  }

  return (
    <div className="diffPreview" aria-label="Diff preview">
      {diff.truncation.truncated && (
        <div className="diffNotice">
          Diff truncated after {diff.truncation.maxLines} lines.
        </div>
      )}
      {files.map((file, fileIndex) => {
        const status = statusByPath.get(file.path) ?? null;
        const statusToken = diffStatusToken(status);
        const stats = diffLineStats(file);
        return (
          <article className="diffFile" key={`${file.path}:${fileIndex}`}>
            <header title={absoluteWorkspacePath(root, file.path)}>
              <span className={`diffFileStatus ${statusToken.className}`} title={statusToken.title}>
                {statusToken.label}
              </span>
              <span className="diffFilePath">{normalizedWorkspacePath(file.path)}</span>
              <span className="diffFileStats" aria-label={`${stats.additions} additions, ${stats.deletions} deletions`}>
                <span className="diffAddStat">+{stats.additions}</span>
                <span className="diffDeleteStat">-{stats.deletions}</span>
              </span>
            </header>
            {file.hunks.length === 0 ? (
              <p className="diffEmptyHunk">No line diff available.</p>
            ) : (
              file.hunks.map((hunk, hunkIndex) => (
                <section className="diffHunk" key={`${hunk.header}:${hunkIndex}`}>
                  <div className="diffHunkHeader">{hunk.header}</div>
                  <div className="diffLines">
                    {hunk.lines.map((line, lineIndex) => (
                      <div className={`diffLine is-${line.kind}`} key={`${line.oldNumber}:${line.newNumber}:${lineIndex}`}>
                        <span className="diffLineNumber">{line.oldNumber ?? ""}</span>
                        <span className="diffLineNumber">{line.newNumber ?? ""}</span>
                        <span className="diffLineMarker">{line.marker}</span>
                        <code>{line.text || " "}</code>
                      </div>
                    ))}
                  </div>
                </section>
              ))
            )}
          </article>
        );
      })}
    </div>
  );
}

function diffStatusToken(status: string | null): { className: string; label: string; title: string } {
  switch (status) {
    case "added":
      return { className: "is-added", label: "A+", title: "Added" };
    case "deleted":
      return { className: "is-deleted", label: "D-", title: "Deleted" };
    case "renamed":
      return { className: "is-renamed", label: "R↷", title: "Renamed" };
    case "untracked":
      return { className: "is-added", label: "U+", title: "Untracked" };
    case "modified":
    default:
      return { className: "is-modified", label: "M↓", title: status ?? "Modified" };
  }
}

function shortSessionId(id: string): string {
  return id.length <= 12 ? id : `${id.slice(0, 8)}...`;
}
