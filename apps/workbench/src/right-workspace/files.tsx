import { useEffect, useLayoutEffect, useMemo, useRef, useState, type KeyboardEvent as ReactKeyboardEvent } from "react";
import {
  AlertTriangle,
  ChevronRight,
  Edit3,
  ExternalLink,
  FileText,
  FolderTree,
  PanelRightClose,
  PanelRightOpen,
  Save,
  Search,
  X
} from "lucide-react";
import { MarkdownText } from "@psychevo/components";
import type { GatewayClient } from "@psychevo/client";
import type {
  GatewayRequestScope,
  WorkspaceExternalFileAction,
  WorkspaceFileEntry,
  WorkspaceFileExternalActionsResult,
  WorkspaceFileReadResult,
  WorkspaceFileWriteResult
} from "@psychevo/protocol";
import { highlightToHtml, languageForPath } from "../highlight";
import { isUnsupportedPreviewFile } from "../right-workspace-model";
import type { WorkspaceFileTreeItem } from "../types";
import { workspaceExternalActionMenuItems } from "./file-external-actions";
import { WorkspaceFileContextMenu } from "./file-context-menu";
import { HtmlStaticPreview } from "./preview";
import {
  WorkspaceFileTree,
  absoluteWorkspacePath,
  type WorkspaceFileContextMenuRequest
} from "./tree";

export function FilesPanel({
  client,
  files,
  preview,
  previewMessage,
  root,
  scope,
  selectedPath,
  tabId,
  truncated,
  onCompare,
  onCopyText,
  onDirtyChange,
  onFileTreeOpenChange,
  onOpen,
  onOpenHtmlPreview,
  htmlExecutionActive,
  fileTreeOpen,
  onSave
}: {
  client: GatewayClient | null;
  files: WorkspaceFileEntry[];
  preview: WorkspaceFileReadResult | null;
  previewMessage: string | null;
  root: string;
  scope: GatewayRequestScope | null;
  selectedPath: string | null;
  tabId: string;
  truncated: boolean;
  onCompare(path: string): void;
  onCopyText?: ((text: string) => void | Promise<void>) | undefined;
  onDirtyChange(tabId: string, dirty: boolean): void;
  onOpen(path: string): void;
  onOpenHtmlPreview(path: string, content: string): void;
  htmlExecutionActive: boolean;
  fileTreeOpen: boolean;
  onFileTreeOpenChange(open: boolean): void;
  onSave(path: string, content: string, expectedRevision: string | null, force: boolean): Promise<WorkspaceFileWriteResult>;
}) {
  const treeItems = useMemo(() => workspaceFileTreeItems(files), [files]);
  const previewPath = preview?.path ?? selectedPath ?? "";
  const previewLabel = previewPath ? absoluteWorkspacePath(root, previewPath) : "Preview";
  const previewContent = typeof preview?.content === "string" ? preview.content : null;
  const editable = Boolean(previewContent !== null && preview && preview.editable !== false && !preview.binary && !preview.truncated);
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState("");
  const [baseRevision, setBaseRevision] = useState<string | null>(null);
  const [wrap, setWrap] = useState(true);
  const [findText, setFindText] = useState("");
  const [goLine, setGoLine] = useState("");
  const [cursor, setCursor] = useState({ line: 1, column: 1 });
  const [saving, setSaving] = useState(false);
  const [editorError, setEditorError] = useState<string | null>(null);
  const [conflict, setConflict] = useState<string | null>(null);
  const textareaRef = useRef<HTMLTextAreaElement | null>(null);
  const lineNumbersRef = useRef<HTMLPreElement | null>(null);
  const fileMenuRequestRef = useRef(0);
  const [fileMenu, setFileMenu] = useState<WorkspaceFileMenuState | null>(null);
  const fileMenuScopeKey = workspaceScopeIdentity(scope);
  const fileMenuContextRef = useRef({ client, root, scopeKey: fileMenuScopeKey });
  const dirty = editing && draft !== (previewContent ?? "");
  const lineCount = Math.max(1, draft.split("\n").length);

  useEffect(() => {
    setEditing(false);
    setDraft(previewContent ?? "");
    setBaseRevision(preview?.revision ?? null);
    setEditorError(null);
    setConflict(null);
    updateCursorFromText(previewContent ?? "", 0, setCursor);
  }, [preview?.path, preview?.revision, previewContent]);

  useEffect(() => {
    onDirtyChange(tabId, dirty);
  }, [dirty, onDirtyChange, tabId]);

  useLayoutEffect(() => {
    const previous = fileMenuContextRef.current;
    fileMenuContextRef.current = { client, root, scopeKey: fileMenuScopeKey };
    if (previous.client !== client || previous.root !== root || previous.scopeKey !== fileMenuScopeKey) {
      fileMenuRequestRef.current += 1;
      setFileMenu(null);
    }
  }, [client, fileMenuScopeKey, root]);

  function confirmDiscard(): boolean {
    return !dirty || window.confirm("Discard unsaved file edits?");
  }

  function openTreePath(path: string) {
    if (!confirmDiscard()) {
      return;
    }
    setEditing(false);
    onOpen(path);
  }

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

  function exitEditMode() {
    if (!confirmDiscard()) {
      return;
    }
    setEditing(false);
    setDraft(previewContent ?? "");
    setEditorError(null);
    setConflict(null);
  }

  async function saveDraft(force = false) {
    if (!previewPath || saving) {
      return;
    }
    setSaving(true);
    setEditorError(null);
    setConflict(null);
    try {
      const result = await onSave(previewPath, draft, baseRevision, force);
      setBaseRevision(result.revision);
      setDraft(draft);
      setEditing(true);
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      if (message.includes("changed on disk")) {
        setConflict(message);
      } else {
        setEditorError(message);
      }
    } finally {
      setSaving(false);
    }
  }

  function handleTextareaKeyDown(event: ReactKeyboardEvent<HTMLTextAreaElement>) {
    if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === "s") {
      event.preventDefault();
      void saveDraft(false);
      return;
    }
    if (event.key === "Tab") {
      event.preventDefault();
      const input = event.currentTarget;
      const start = input.selectionStart;
      const end = input.selectionEnd;
      const next = `${draft.slice(0, start)}  ${draft.slice(end)}`;
      setDraft(next);
      requestAnimationFrame(() => {
        input.selectionStart = start + 2;
        input.selectionEnd = start + 2;
        updateCursorFromText(next, start + 2, setCursor);
      });
    }
  }

  function findInDraft() {
    if (!findText || !textareaRef.current) {
      return;
    }
    const start = textareaRef.current.selectionEnd;
    let index = draft.indexOf(findText, start);
    if (index < 0) {
      index = draft.indexOf(findText);
    }
    if (index >= 0) {
      textareaRef.current.focus();
      textareaRef.current.selectionStart = index;
      textareaRef.current.selectionEnd = index + findText.length;
      updateCursorFromText(draft, index, setCursor);
    }
  }

  function jumpToLine() {
    const line = Math.max(1, Number.parseInt(goLine, 10) || 1);
    const lines = draft.split("\n");
    let index = 0;
    for (let i = 0; i < Math.min(line - 1, lines.length - 1); i += 1) {
      index += (lines[i] ?? "").length + 1;
    }
    textareaRef.current?.focus();
    if (textareaRef.current) {
      textareaRef.current.selectionStart = index;
      textareaRef.current.selectionEnd = index;
    }
    updateCursorFromText(draft, index, setCursor);
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
          <div className="rightSectionLabel filePreviewPath">
            <span>{previewLabel}</span>
            {preview?.truncated && <b>truncated</b>}
            {dirty && <b>unsaved</b>}
            {previewContent !== null && !editing && (
              <div className="filePreviewActions">
                {isHtmlFile(previewPath) && (
                  <button
                    aria-label={`Open HTML preview for ${previewPath}`}
                    onClick={() => onOpenHtmlPreview(previewPath, previewContent)}
                    title="Open HTML preview"
                    type="button"
                  >
                    <ExternalLink size={13} />
                  </button>
                )}
                <button
                  aria-label={`Edit ${previewPath}`}
                  disabled={!editable}
                  onClick={() => {
                    setDraft(previewContent);
                    setBaseRevision(preview?.revision ?? null);
                    setEditing(true);
                    requestAnimationFrame(() => textareaRef.current?.focus());
                  }}
                  title={editable ? "Edit" : preview?.editableReason ?? "This file cannot be edited"}
                  type="button"
                >
                  <Edit3 size={13} />
                </button>
              </div>
            )}
          </div>
          {previewContent !== null ? (
            editing ? (
              <div className="fileEditor">
                <div className="fileEditorToolbar">
                  <label>
                    <Search size={13} aria-hidden />
                    <input
                      aria-label="Find in file"
                      onChange={(event) => setFindText(event.currentTarget.value)}
                      onKeyDown={(event) => {
                        if (event.key === "Enter") {
                          event.preventDefault();
                          findInDraft();
                        }
                      }}
                      placeholder="Find"
                      type="search"
                      value={findText}
                    />
                  </label>
                  <button aria-label="Find next" onClick={findInDraft} title="Find next" type="button">
                    <Search size={13} />
                  </button>
                  <label>
                    <span>:</span>
                    <input
                      aria-label="Go to line"
                      inputMode="numeric"
                      onChange={(event) => setGoLine(event.currentTarget.value)}
                      onKeyDown={(event) => {
                        if (event.key === "Enter") {
                          event.preventDefault();
                          jumpToLine();
                        }
                      }}
                      placeholder="Line"
                      value={goLine}
                    />
                  </label>
                  <button aria-label="Go to line" onClick={jumpToLine} title="Go to line" type="button">
                    <ChevronRight size={13} />
                  </button>
                  <button
                    aria-label="Toggle word wrap"
                    aria-pressed={wrap}
                    onClick={() => setWrap((value) => !value)}
                    title="Toggle word wrap"
                    type="button"
                  >
                    <FileText size={13} />
                  </button>
                  <span>{cursor.line}:{cursor.column}</span>
                  <button aria-label="Save file" disabled={!dirty || saving} onClick={() => void saveDraft(false)} title="Save" type="button">
                    <Save size={13} />
                  </button>
                  <button aria-label="Exit edit mode" onClick={exitEditMode} title="Exit edit mode" type="button">
                    <X size={13} />
                  </button>
                </div>
                <div className={`fileEditorBody ${wrap ? "is-wrapped" : "is-unwrapped"}`}>
                  <pre className="fileEditorLines" aria-hidden ref={lineNumbersRef}>
                    {Array.from({ length: lineCount }, (_, index) => index + 1).join("\n")}
                  </pre>
                  <textarea
                    ref={textareaRef}
                    aria-label={`Edit ${previewPath}`}
                    onChange={(event) => {
                      setDraft(event.currentTarget.value);
                      updateCursorFromText(event.currentTarget.value, event.currentTarget.selectionStart, setCursor);
                    }}
                    onClick={(event) => updateCursorFromText(draft, event.currentTarget.selectionStart, setCursor)}
                    onKeyDown={handleTextareaKeyDown}
                    onKeyUp={(event) => updateCursorFromText(draft, event.currentTarget.selectionStart, setCursor)}
                    onScroll={(event) => {
                      if (lineNumbersRef.current) {
                        lineNumbersRef.current.scrollTop = event.currentTarget.scrollTop;
                      }
                    }}
                    spellCheck={false}
                    value={draft}
                    wrap={wrap ? "soft" : "off"}
                  />
                </div>
                {(editorError || conflict) && (
                  <div className="fileEditorNotice">
                    <AlertTriangle size={14} />
                    <span>{conflict ?? editorError}</span>
                    {conflict && (
                      <>
                        <button onClick={() => onCompare(previewPath)} type="button">Compare</button>
                        <button onClick={() => openTreePath(previewPath)} type="button">Reload</button>
                        <button onClick={() => void saveDraft(true)} type="button">Force</button>
                      </>
                    )}
                  </div>
                )}
              </div>
            ) : isMarkdownFile(previewPath) ? (
              <div className="fileMarkdownPreview">
                <MarkdownText
                  copyLabel="Copy Markdown file"
                  copyText={previewContent}
                  onCopyText={onCopyText}
                  text={previewContent}
                />
              </div>
            ) : isHtmlFile(previewPath) ? (
              <HtmlStaticPreview
                active={htmlExecutionActive}
                content={previewContent}
                documentId={previewPath}
                title={previewPath}
              />
            ) : (
              <HighlightedCodePreview content={previewContent} path={previewPath} />
            )
          ) : (
            <p>{previewMessage ?? "Select a text file to preview."}</p>
          )}
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
              onOpen={openTreePath}
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

function updateCursorFromText(
  text: string,
  position: number,
  setCursor: (cursor: { line: number; column: number }) => void
) {
  const bounded = Math.max(0, Math.min(position, text.length));
  const before = text.slice(0, bounded);
  const lines = before.split("\n");
  setCursor({
    line: lines.length,
    column: (lines.at(-1)?.length ?? 0) + 1
  });
}

function HighlightedCodePreview({ content, path }: { content: string; path: string }) {
  const language = useMemo(() => languageForPath(path), [path]);
  const html = useMemo(() => highlightToHtml(content, language), [content, language]);
  return (
    <pre className="rightCodePreview hljs" data-lang={language || undefined}>
      <code dangerouslySetInnerHTML={{ __html: html }} />
    </pre>
  );
}

function workspaceFileTreeItems(files: WorkspaceFileEntry[]): WorkspaceFileTreeItem[] {
  return files.map((file) => ({
    kind: file.kind,
    name: file.name,
    path: file.path,
    depth: file.depth,
    previewDisabled: file.kind === "file" && isUnsupportedPreviewFile(file.path)
  }));
}

function isMarkdownFile(path: string): boolean {
  const extension = path.split(/[\\/]/).pop()?.split(".").pop()?.toLowerCase();
  return extension === "md" || extension === "markdown";
}

function isHtmlFile(path: string): boolean {
  const extension = path.split(/[\\/]/).pop()?.split(".").pop()?.toLowerCase();
  return extension === "html" || extension === "htm";
}

export { isUnsupportedPreviewFile } from "../right-workspace-model";
