import {
  Component,
  lazy,
  Suspense,
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type KeyboardEvent as ReactKeyboardEvent,
  type MouseEvent as ReactMouseEvent,
  type ReactNode
} from "react";
import {
  AlertTriangle,
  AppWindow,
  Check,
  ChevronDown,
  ChevronRight,
  Code2,
  Copy,
  Edit3,
  ExternalLink,
  FileText,
  Folder,
  PanelRightClose,
  PanelRightOpen,
  Save,
  Search,
  X
} from "lucide-react";
import { ActionButton, IconButton, MarkdownText, ToggleButton, useConfirmAction } from "@psychevo/components";
import type { GatewayClient } from "@psychevo/client";
import type {
  GatewayRequestScope,
  WorkspaceExternalFileAction,
  WorkspaceFileExternalActionsResult,
  WorkspaceFilePreviewOpenResult
} from "@psychevo/protocol";
import { highlightToHtml, languageForPath } from "../highlight";
import {
  workspaceExternalActionLabel,
  workspaceExternalActionMenuItems
} from "./file-external-actions";
import { WorkspaceFileContextMenu } from "./file-context-menu";
import { HtmlStaticPreview } from "./preview";
import { useWorkspaceFileGatewayAdapter } from "./workspace-file-gateway-adapter";
import type { DelimitedTableLimits } from "./workspace-file-delimited";
import type { WorkspaceExcalidrawScene } from "./workspace-file-excalidraw";
import type { ZipDirectoryEntry } from "./workspace-file-zip";
import type { WorkspaceFileTreeItem } from "../types";

const WHOLE_FILE_LIMIT_BYTES = 32 * 1024 * 1024;
const EXCALIDRAW_LIMIT_BYTES = 5 * 1024 * 1024;

const IMAGE_EXTENSIONS = new Set([
  "png", "jpg", "jpeg", "gif", "webp", "avif", "bmp", "svg", "ico"
]);
const HEIC_EXTENSIONS = new Set(["heic", "heif"]);
const VIDEO_EXTENSIONS = new Set(["mp4", "webm"]);
const AUDIO_EXTENSIONS = new Set([
  "mp3", "wav", "ogg", "oga", "opus", "m4a", "aac", "flac", "weba"
]);
const VENDOR_BLOB_EXTENSIONS = new Set([
  "docx", "docm", "dotx", "dotm",
  "xlsx", "xlsm", "xlsb", "xltx", "xltm",
  "pptx", "pptm", "potx", "potm", "ppsx", "ppsm",
  "rtf", "odt", "ods", "odp", "ofd"
]);
const EXPLICITLY_UNSUPPORTED_EXTENSIONS = new Set([
  "doc", "dot", "xls", "xlt", "ppt",
  "m3u8", "mov", "mkv", "avi",
  "tif", "tiff", "jxl",
  "mid", "midi",
  "xmind", "drawio", "epub",
  "eml", "msg", "mbox",
  "dwg", "dxf", "step", "stp", "iges", "igs",
  "obj", "stl", "fbx", "gltf", "glb", "3mf",
  "geojson", "kml", "kmz", "gpx", "shp",
  "kicad_sch", "kicad_pcb", "sch", "brd", "gbr", "gerber",
  "typ", "typst"
]);

const VendorFilePreview = lazy(() => import("./workspace-file-vendor"));
const ExcalidrawPreview = lazy(async () => ({
  default: (await import("./workspace-file-excalidraw")).ExcalidrawPreview
}));

export type WorkspaceFileSurfaceTarget = {
  scope: GatewayRequestScope;
  path: string;
};

export type WorkspaceFileSurfaceProps = {
  target: WorkspaceFileSurfaceTarget | null;
  active: boolean;
  textEditing: "enabled" | "disabled";
  onDirtyChange(dirty: boolean): void;
  onCompare(path: string): void;
  workspaceRoot?: string | undefined;
  fileTree?: {
    content: ReactNode;
    items: WorkspaceFileTreeItem[];
    open: boolean;
    onOpen(path: string): void;
    onOpenChange(open: boolean): void;
    onReveal(path: string): void;
  } | undefined;
};

export function WorkspaceFileSurface({
  target,
  active,
  textEditing,
  onDirtyChange,
  onCompare,
  workspaceRoot,
  fileTree
}: WorkspaceFileSurfaceProps) {
  const dependencies = useWorkspaceFileGatewayAdapter();
  const [lease, setLease] = useState<WorkspaceFilePreviewOpenResult | null>(null);
  const [phase, setPhase] = useState<"empty" | "loading" | "ready" | "error">(
    target ? "loading" : "empty"
  );
  const [failure, setFailure] = useState<string | null>(null);
  const [materialized, setMaterialized] = useState<MaterializedPreview | null>(null);
  const [progress, setProgress] = useState<{ loaded: number; total: number } | null>(null);
  const [reopenNonce, setReopenNonce] = useState(0);
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState("");
  const [savedText, setSavedText] = useState("");
  const [baseRevision, setBaseRevision] = useState<string | null>(null);
  const [wrap, setWrap] = useState(true);
  const [findText, setFindText] = useState("");
  const [goLine, setGoLine] = useState("");
  const [cursor, setCursor] = useState({ line: 1, column: 1 });
  const [saving, setSaving] = useState(false);
  const [editorError, setEditorError] = useState<string | null>(null);
  const [conflict, setConflict] = useState<string | null>(null);
  const [vendorReady, setVendorReady] = useState(false);
  const [viewingSource, setViewingSource] = useState(false);
  const [externalActions, setExternalActions] = useState<WorkspaceFileExternalActionsResult | null>(null);
  const [externalActionsLoading, setExternalActionsLoading] = useState(false);
  const [externalActionError, setExternalActionError] = useState<string | null>(null);
  const [pendingExternalAction, setPendingExternalAction] = useState<WorkspaceExternalFileAction | null>(null);
  const [copying, setCopying] = useState(false);
  const [copied, setCopied] = useState(false);
  const confirmAction = useConfirmAction();
  const [externalMenu, setExternalMenu] = useState<{
    anchor: HTMLButtonElement;
    x: number;
    y: number;
  } | null>(null);
  const generationRef = useRef(0);
  const externalActionRequestRef = useRef(0);
  const saveRequestRef = useRef(0);
  const copiedTimeoutRef = useRef<ReturnType<typeof globalThis.setTimeout> | null>(null);
  const expirationRetryRef = useRef({ key: "", count: 0 });
  const onDirtyChangeRef = useRef(onDirtyChange);
  const textareaRef = useRef<HTMLTextAreaElement | null>(null);
  const lineNumbersRef = useRef<HTMLPreElement | null>(null);
  const targetKey = target ? `${workspaceScopeKey(target.scope)}\u0000${target.path}` : "";
  const previewIdentityRef = useRef({ resourceId: lease?.resourceId ?? null, targetKey });
  previewIdentityRef.current = { resourceId: lease?.resourceId ?? null, targetKey };
  const previewContent = lease?.content ?? null;
  const dirty = editing && draft !== savedText;
  const lineCount = Math.max(1, draft.split("\n").length);
  const editable = Boolean(
    textEditing === "enabled"
      && lease
      && previewContent !== null
      && lease.editable !== false
      && !lease.binary
      && !lease.truncated
  );
  const canViewSource = Boolean(
    lease
      && previewContent !== null
      && !lease.binary
      && !lease.truncated
      && materialized
      && materialized.kind !== "text"
      && materialized.kind !== "unsupported"
  );
  const copyable = Boolean(
    dependencies.onCopyText
      && lease
      && previewContent !== null
      && !lease.binary
      && !lease.truncated
  );
  const preferredExternalAction = externalActions
    ? selectExternalOpenAction(externalActions)
    : null;
  const handlePreviewStateChange = useCallback((ready: boolean, error: unknown | null) => {
    if (error) {
      const client = dependencies.client;
      const openedLease = lease;
      const identity = { resourceId: openedLease?.resourceId ?? null, targetKey };
      const reportFailure = (message = "The preview renderer could not open this file.") => {
        const current = previewIdentityRef.current;
        if (current.resourceId !== identity.resourceId || current.targetKey !== identity.targetKey) {
          return;
        }
        setFailure(message);
        setPhase("error");
      };
      if (!client || !openedLease) {
        reportFailure();
        return;
      }
      void previewResourceStatus(previewResourceUrl(client, openedLease.resourcePath)).then(
        (status) => {
          const current = previewIdentityRef.current;
          if (current.resourceId !== identity.resourceId || current.targetKey !== identity.targetKey) {
            return;
          }
          if (status === 410) {
            const retry = expirationRetryRef.current;
            if (retry.key === targetKey && retry.count === 0) {
              retry.count += 1;
              setReopenNonce((value) => value + 1);
              return;
            }
          }
          reportFailure(status === 409
            ? "The file changed while its preview was open."
            : undefined);
        },
        () => reportFailure()
      );
      return;
    }
    if (ready) {
      setVendorReady(true);
    }
  }, [dependencies.client, lease, targetKey]);

  useEffect(() => {
    onDirtyChangeRef.current = onDirtyChange;
  }, [onDirtyChange]);

  useEffect(() => {
    onDirtyChangeRef.current(dirty);
  }, [dirty]);

  useEffect(() => () => onDirtyChangeRef.current(false), []);

  useEffect(() => {
    expirationRetryRef.current = { key: targetKey, count: 0 };
    setReopenNonce(0);
    setCopying(false);
    setCopied(false);
    if (copiedTimeoutRef.current) {
      globalThis.clearTimeout(copiedTimeoutRef.current);
      copiedTimeoutRef.current = null;
    }
  }, [targetKey]);

  useEffect(() => () => {
    if (copiedTimeoutRef.current) {
      globalThis.clearTimeout(copiedTimeoutRef.current);
    }
  }, []);

  useEffect(() => {
    const client = dependencies.client;
    const generation = generationRef.current + 1;
    generationRef.current = generation;
    saveRequestRef.current += 1;
    let disposed = false;
    let opened: WorkspaceFilePreviewOpenResult | null = null;

    setLease(null);
    setMaterialized(null);
    setProgress(null);
    setFailure(null);
    setVendorReady(false);
    setViewingSource(false);
    setEditing(false);
    setDraft("");
    setSavedText("");
    setBaseRevision(null);
    setEditorError(null);
    setConflict(null);
    setSaving(false);

    if (!client || !target) {
      setPhase("empty");
      return () => {
        disposed = true;
      };
    }

    setPhase("loading");
    void previewOpen(client, target).then(
      (result) => {
        opened = result;
        if (disposed || generationRef.current !== generation) {
          void previewRelease(client, result.resourceId);
          return;
        }
        setLease(result);
        setDraft(result.content ?? "");
        setSavedText(result.content ?? "");
        setBaseRevision(result.revision ?? null);
        updateCursorFromText(result.content ?? "", 0, setCursor);
      },
      (error) => {
        if (disposed || generationRef.current !== generation) {
          return;
        }
        setFailure(errorMessage(error, "Preview could not be opened."));
        setPhase("error");
      }
    );

    return () => {
      disposed = true;
      if (opened) {
        void previewRelease(client, opened.resourceId);
      }
    };
  }, [dependencies.client, reopenNonce, targetKey]);

  useEffect(() => {
    const client = dependencies.client;
    externalActionRequestRef.current += 1;
    let disposed = false;
    setExternalActions(null);
    setExternalActionError(null);
    setPendingExternalAction(null);
    setExternalMenu(null);
    setExternalActionsLoading(Boolean(client && target));
    if (!client || !target) {
      return () => {
        disposed = true;
      };
    }
    void workspaceFileExternalActions(client, target).then(
      (actions) => {
        if (!disposed) {
          setExternalActions(actions);
          setExternalActionsLoading(false);
        }
      },
      (error) => {
        if (!disposed) {
          setExternalActions(null);
          setExternalActionsLoading(false);
          setExternalActionError(errorMessage(error, "External actions are unavailable."));
        }
      }
    );
    return () => {
      disposed = true;
    };
  }, [dependencies.client, targetKey]);

  useEffect(() => {
    if (!lease) {
      return;
    }
    if (!active && !materialized) {
      return;
    }
    const controller = new AbortController();
    let disposed = false;
    const kind = previewKind(lease.path, lease.content);
    const wholeFileLimit = kind === "excalidraw"
      ? EXCALIDRAW_LIMIT_BYTES
      : WHOLE_FILE_LIMIT_BYTES;

    if (requiresWholeFile(kind) && lease.sizeBytes > wholeFileLimit) {
      setFailure(kind === "excalidraw"
        ? "Excalidraw preview is limited to 5 MiB."
        : "Preview requires the whole file and is limited to 32 MiB.");
      setPhase("error");
      return () => controller.abort();
    }

    if (materialized?.resourceId === lease.resourceId) {
      setPhase("ready");
      return () => controller.abort();
    }

    setFailure(null);
    setProgress(null);
    setPhase("loading");
    void materializePreview({
      kind,
      lease,
      resourceUrl: previewResourceUrl(dependencies.client, lease.resourcePath),
      signal: controller.signal,
      onProgress: (loaded, total) => {
        if (!disposed) {
          setProgress({ loaded, total });
        }
      }
    }).then(
      (next) => {
        if (disposed) {
          return;
        }
        setMaterialized({ ...next, resourceId: lease.resourceId });
        setPhase("ready");
        setProgress(null);
      },
      (error) => {
        if (disposed || isAbortError(error)) {
          return;
        }
        if (error instanceof ExpiredPreviewLeaseError) {
          const retry = expirationRetryRef.current;
          if (retry.key === targetKey && retry.count === 0) {
            retry.count += 1;
            setReopenNonce((value) => value + 1);
            return;
          }
        }
        setFailure(errorMessage(error, "Preview could not be rendered."));
        setPhase("error");
        setProgress(null);
      }
    );
    return () => {
      disposed = true;
      controller.abort();
    };
  }, [active, dependencies.client, lease, materialized, targetKey]);

  async function saveDraft(force: boolean) {
    const client = dependencies.client;
    if (!target || !client || saving) {
      return;
    }
    const requestId = saveRequestRef.current + 1;
    saveRequestRef.current = requestId;
    const savedDraft = draft;
    setSaving(true);
    setEditorError(null);
    setConflict(null);
    try {
      const result = dependencies.onSave
        ? await dependencies.onSave(target.path, savedDraft, baseRevision, force)
        : await workspaceFileWrite(client, target, savedDraft, baseRevision, force);
      if (saveRequestRef.current !== requestId) {
        return;
      }
      setBaseRevision(result.revision);
      setSavedText(savedDraft);
      setDraft(savedDraft);
      setConflict(null);
      setEditorError(null);
      setEditing(false);
      setReopenNonce((value) => value + 1);
    } catch (error) {
      if (saveRequestRef.current !== requestId) {
        return;
      }
      const message = errorMessage(error, "File could not be saved.");
      if (message.includes("changed on disk")) {
        setConflict(message);
      } else {
        setEditorError(message);
      }
    } finally {
      if (saveRequestRef.current === requestId) {
        setSaving(false);
      }
    }
  }

  async function exitEditMode() {
    if (dirty && !await confirmAction({
      confirmLabel: "Discard edits",
      description: "The unsaved file changes will be lost.",
      title: "Discard unsaved file edits?",
      tone: "caution"
    })) {
      return;
    }
    setEditing(false);
    setDraft(savedText);
    setConflict(null);
    setEditorError(null);
    setReopenNonce((value) => value + 1);
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
    for (let lineIndex = 0; lineIndex < Math.min(line - 1, lines.length - 1); lineIndex += 1) {
      index += (lines[lineIndex] ?? "").length + 1;
    }
    textareaRef.current?.focus();
    if (textareaRef.current) {
      textareaRef.current.selectionStart = index;
      textareaRef.current.selectionEnd = index;
    }
    updateCursorFromText(draft, index, setCursor);
  }

  async function openExternally(action: WorkspaceExternalFileAction) {
    if (
      !target
      || !dependencies.client
      || !externalActions?.availableActions.includes(action)
      || pendingExternalAction
    ) {
      return;
    }
    setExternalActionError(null);
    setPendingExternalAction(action);
    const requestId = externalActionRequestRef.current + 1;
    externalActionRequestRef.current = requestId;
    try {
      await workspaceFileOpenExternal(dependencies.client, target, action);
      if (externalActionRequestRef.current === requestId) {
        setExternalMenu(null);
      }
    } catch (error) {
      if (externalActionRequestRef.current === requestId) {
        setExternalActionError(errorMessage(error, "The file could not be opened externally."));
      }
    } finally {
      if (externalActionRequestRef.current === requestId) {
        setPendingExternalAction(null);
      }
    }
  }

  async function copyFileContent() {
    if (!dependencies.onCopyText || !copyable || previewContent === null || copying) {
      return;
    }
    const copyTargetKey = targetKey;
    setCopying(true);
    setCopied(false);
    if (copiedTimeoutRef.current) {
      globalThis.clearTimeout(copiedTimeoutRef.current);
      copiedTimeoutRef.current = null;
    }
    try {
      await dependencies.onCopyText(previewContent);
      if (copyTargetKey !== previewIdentityRef.current.targetKey) {
        return;
      }
      setCopied(true);
      copiedTimeoutRef.current = globalThis.setTimeout(() => {
        setCopied(false);
        copiedTimeoutRef.current = null;
      }, 1_200);
    } catch {
      setCopied(false);
    } finally {
      if (copyTargetKey === previewIdentityRef.current.targetKey) {
        setCopying(false);
      }
    }
  }

  const displayRoot = workspaceRoot ?? target?.scope.cwd ?? "";
  const productPreviewState = phase === "ready"
    && (materialized?.kind === "vendor" || materialized?.kind === "excalidraw")
    && !vendorReady
    ? "loading"
    : phase;
  const externalActionLabel = preferredExternalAction && externalActions
    ? workspaceExternalActionLabel(
      preferredExternalAction,
      externalActions.category,
      externalActions.platform
    )
    : null;
  return (
    <section
      className={`workspaceFileSurface ${fileTree?.open ? "has-fileTree" : ""}`}
      aria-label={target ? `File preview ${target.path}` : "Workspace file preview"}
      data-preview-format={extensionForPath(lease?.path ?? target?.path ?? "") || "unknown"}
      data-preview-state={editing ? "editing" : productPreviewState}
    >
      <header className="workspaceFileToolbar">
        <div className="workspaceFileBreadcrumbRow">
          <WorkspaceFileBreadcrumb
            items={fileTree?.items ?? []}
            onNavigate={fileTree?.onReveal}
            onOpen={fileTree?.onOpen}
            path={target?.path ?? null}
            root={displayRoot}
          />
          {lease?.truncated && <b>truncated</b>}
          {dirty && <b>unsaved</b>}
        </div>
        <div className="workspaceFileToolbarActions">
          {lease && previewContent !== null && !editing && (
            <>
              {canViewSource && (
                <ToggleButton
                  className="workspaceFileTextAction"
                  icon={<FileText size={14} />}
                  label={`Source view for ${lease.path}`}
                  onPressedChange={setViewingSource}
                  pressed={viewingSource}
                  size="compact"
                >
                  {viewingSource ? "View preview" : "View source"}
                </ToggleButton>
              )}
              {isHtmlFile(lease.path) && dependencies.onOpenHtmlPreview && (
                <IconButton
                  icon={<ExternalLink size={14} />}
                  label={`Open HTML preview for ${lease.path}`}
                  onClick={() => dependencies.onOpenHtmlPreview?.(lease.path, previewContent)}
                  size="compact"
                />
              )}
              <ActionButton
                aria-label={`Edit ${lease.path}`}
                className="workspaceFileTextAction"
                disabled={!editable}
                icon={<Edit3 size={14} />}
                onClick={() => {
                  setViewingSource(false);
                  setDraft(previewContent);
                  setSavedText(previewContent);
                  setBaseRevision(lease.revision ?? null);
                  setEditing(true);
                  requestAnimationFrame(() => textareaRef.current?.focus());
                }}
                title={editable ? "Edit" : lease.editableReason ?? "This file cannot be edited"}
                type="button"
                size="compact"
                variant="ghost"
              >
                Edit
              </ActionButton>
            </>
          )}
          {copyable && lease && !editing && (
            <>
              <span aria-live="polite" className="pevo-srOnly" role="status">
                {copied ? `${lease.path} copied` : ""}
              </span>
              <IconButton
                disabled={copying}
                icon={copied ? <Check size={14} /> : <Copy size={14} />}
                label={`Copy ${lease.path}`}
                onClick={() => void copyFileContent()}
                size="compact"
              />
            </>
          )}
          {externalActionsLoading && target && (
            <div className="workspaceFileOpenControl pevo-controlGroup is-loading" aria-label="Detecting external applications">
              <ActionButton icon={<AppWindow size={14} />} pending size="compact" type="button">Open</ActionButton>
            </div>
          )}
          {!externalActionsLoading && externalActions && preferredExternalAction && externalActionLabel && (
            <div className="workspaceFileOpenControl pevo-controlGroup">
              <ActionButton
                aria-label={externalActionLabel}
                disabled={pendingExternalAction !== null}
                icon={preferredExternalAction === "vscode" ? <Code2 size={14} /> : <AppWindow size={14} />}
                onClick={() => void openExternally(preferredExternalAction)}
                title={externalActionLabel}
                type="button"
                size="compact"
                variant="secondary"
              >
                Open
              </ActionButton>
              <IconButton
                aria-expanded={externalMenu !== null}
                aria-haspopup="menu"
                disabled={pendingExternalAction !== null}
                icon={<ChevronDown size={14} />}
                label="Choose external application"
                onClick={(event) => {
                  if (externalMenu) {
                    setExternalMenu(null);
                    return;
                  }
                  const bounds = event.currentTarget.getBoundingClientRect();
                  setExternalMenu({
                    anchor: event.currentTarget,
                    x: Math.max(8, bounds.right - 220),
                    y: bounds.bottom + 4
                  });
                }}
                size="compact"
                type="button"
              />
            </div>
          )}
          {!externalActionsLoading && externalActionError && !externalActions && target && (
            <ActionButton
              className="workspaceFileTextAction"
              disabled
              icon={<AppWindow size={14} />}
              title={externalActionError}
              type="button"
              size="compact"
            >
              Open unavailable
            </ActionButton>
          )}
          {externalActionError && externalActions && (
            <span className="workspaceFileToolbarError" role="alert" title={externalActionError}>
              Open failed
            </span>
          )}
          {fileTree && (
            <ToggleButton
              className={`filesTreeToggle ${fileTree.open ? "is-pressed" : ""}`}
              icon={fileTree.open ? <PanelRightClose size={15} /> : <PanelRightOpen size={15} />}
              label="File tree"
              onPressedChange={fileTree.onOpenChange}
              pressed={fileTree.open}
              size="compact"
            />
          )}
        </div>
      </header>

      <div className="workspaceFileSurfaceSplit">
        <div className="workspaceFileContent">
          {!target ? (
            <p className="workspaceFilePreviewStatus">Select a file to preview.</p>
          ) : editing ? (
            <div className="fileEditor">
              <div className="fileEditorToolbar">
                <label className="pevo-searchField">
                  <Search size={13} aria-hidden />
                  <input
                    aria-label="Find in file"
                    className="pevo-fieldControl pevo-fieldControl--search pevo-fieldControl--compact"
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
                <IconButton icon={<Search size={13} />} label="Find next" onClick={findInDraft} size="compact" />
                <label>
                  <span>:</span>
                  <input
                    aria-label="Go to line"
                    className="pevo-fieldControl pevo-fieldControl--compact"
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
                <IconButton icon={<ChevronRight size={13} />} label="Go to line" onClick={jumpToLine} size="compact" />
                <ToggleButton icon={<FileText size={13} />} label="Word wrap" onPressedChange={setWrap} pressed={wrap} size="compact" />
                <span>{cursor.line}:{cursor.column}</span>
                <IconButton
                  disabled={!dirty || saving}
                  icon={<Save size={13} />}
                  label="Save file"
                  onClick={() => void saveDraft(false)}
                  pending={saving}
                  size="compact"
                />
                <IconButton
                  icon={<X size={13} />}
                  label="Exit edit mode"
                  onClick={() => void exitEditMode()}
                  size="compact"
                />
              </div>
              <div className={`fileEditorBody ${wrap ? "is-wrapped" : "is-unwrapped"}`}>
                <pre className="fileEditorLines" aria-hidden ref={lineNumbersRef}>
                  {Array.from({ length: lineCount }, (_, index) => index + 1).join("\n")}
                </pre>
                <textarea
                  ref={textareaRef}
                  aria-label={`Edit ${target.path}`}
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
                      <ActionButton onClick={() => onCompare(target.path)} size="compact" type="button" variant="ghost">Compare</ActionButton>
                      <ActionButton
                        onClick={() => {
                          setEditing(false);
                          setReopenNonce((value) => value + 1);
                        }}
                        size="compact"
                        type="button"
                        variant="ghost"
                      >Reload</ActionButton>
                      <ActionButton onClick={() => void saveDraft(true)} size="compact" type="button" variant="danger">Force</ActionButton>
                    </>
                  )}
                </div>
              )}
            </div>
          ) : (
            <>
              {phase === "loading" && (
                <p className="workspaceFilePreviewStatus" role="status">
                  {progress
                    ? `Loading preview… ${formatProgress(progress.loaded, progress.total)}`
                    : "Loading preview…"}
                </p>
              )}
              {phase === "error" && (
                <div className="workspaceFilePreviewError" role="alert">
                  <AlertTriangle size={16} />
                  <p>{failure ?? "Preview is not available for this file."}</p>
                  <ActionButton onClick={() => setReopenNonce((value) => value + 1)} size="compact" type="button">Retry</ActionButton>
                </div>
              )}
              {phase === "ready" && materialized && (
                <PreviewErrorBoundary
                  key={materialized.resourceId}
                  onError={(error) => handlePreviewStateChange(false, error)}
                >
                  {viewingSource && canViewSource ? (
                    <HighlightedCodePreview content={previewContent ?? ""} path={lease?.path ?? target.path} />
                  ) : (
                    <PreviewBody
                      active={active}
                      content={previewContent}
                      materialized={materialized}
                      onPreviewStateChange={handlePreviewStateChange}
                      path={lease?.path ?? target.path}
                      vendorReady={vendorReady}
                    />
                  )}
                </PreviewErrorBoundary>
              )}
            </>
          )}
        </div>
        {fileTree?.open && fileTree.content}
      </div>
      {externalMenu && externalActions && (
        <WorkspaceFileContextMenu
          anchor={{ element: externalMenu.anchor, x: externalMenu.x, y: externalMenu.y }}
          ariaLabel={`External actions for ${target?.path ?? externalActions.path}`}
          error={externalActionError}
          items={workspaceExternalActionMenuItems(externalActions, pendingExternalAction !== null)}
          loading={false}
          onClose={() => {
            if (!pendingExternalAction) {
              setExternalMenu(null);
            }
          }}
          onSelect={(action) => void openExternally(action)}
        />
      )}
    </section>
  );
}

function WorkspaceFileBreadcrumb({
  items,
  onNavigate,
  onOpen,
  path,
  root
}: {
  items: WorkspaceFileTreeItem[];
  onNavigate?: ((path: string) => void) | undefined;
  onOpen?: ((path: string) => void) | undefined;
  path: string | null;
  root: string;
}) {
  const [menu, setMenu] = useState<{
    anchor: HTMLButtonElement;
    nodeLabel: string;
    nodePath: string;
    x: number;
    y: number;
  } | null>(null);
  const rootLabel = workspaceRootLabel(root);
  const segments = normalizedWorkspaceSegments(path);
  const current = segments.at(-1) ?? null;
  const directories = segments.slice(0, -1);
  const nodes = [
    { label: rootLabel, path: "" },
    ...directories.map((label, index) => ({
      label,
      path: directories.slice(0, index + 1).join("/")
    }))
  ];
  const menuItems = menu ? immediateWorkspaceChildren(items, menu.nodePath) : [];

  useEffect(() => setMenu(null), [path]);

  function openChildMenu(
    event: ReactMouseEvent<HTMLButtonElement>,
    nodeLabel: string,
    nodePath: string
  ) {
    if (menu?.nodePath === nodePath) {
      setMenu(null);
      return;
    }
    const bounds = event.currentTarget.getBoundingClientRect();
    setMenu({
      anchor: event.currentTarget,
      nodeLabel,
      nodePath,
      x: bounds.left,
      y: bounds.bottom + 4
    });
  }

  return (
    <>
      <nav aria-label="File breadcrumb" className="workspaceFileBreadcrumb">
        {nodes.map((node, index) => {
          const children = immediateWorkspaceChildren(items, node.path);
          return (
            <span className="workspaceFileBreadcrumbNode" key={node.path || "workspace-root"}>
              <span className="workspaceFileBreadcrumbSegment">
                {path && onNavigate ? (
                  <button
                    onClick={() => onNavigate(node.path)}
                    title={node.path ? `Show ${node.path} in file tree` : "Show workspace root in file tree"}
                    type="button"
                  >
                    {node.label}
                  </button>
                ) : (
                  <span aria-current={!path ? "page" : undefined}>{node.label}</span>
                )}
              </span>
              {path && children.length > 0 && onNavigate && onOpen ? (
                <button
                  aria-expanded={menu?.nodePath === node.path}
                  aria-haspopup="menu"
                  aria-label={`Show children of ${node.label}`}
                  className="workspaceFileBreadcrumbDisclosure"
                  onClick={(event) => openChildMenu(event, node.label, node.path)}
                  title={`Show children of ${node.label}`}
                  type="button"
                >
                  <ChevronRight aria-hidden size={13} />
                </button>
              ) : index < nodes.length - 1 || current ? (
                <ChevronRight aria-hidden className="workspaceFileBreadcrumbSeparator" size={13} />
              ) : null}
            </span>
          );
        })}
        {current && (
          <span className="workspaceFileBreadcrumbSegment">
            <span aria-current="page" title={path ?? undefined}>{current}</span>
          </span>
        )}
      </nav>
      {menu && onNavigate && onOpen && (
        <WorkspaceFileContextMenu
          anchor={{ element: menu.anchor, x: menu.x, y: menu.y }}
          ariaLabel={`Children of ${menu.nodeLabel}`}
          items={menuItems.map((item) => ({
            icon: item.kind === "directory"
              ? <Folder aria-hidden size={14} />
              : <FileText aria-hidden size={14} />,
            id: item.path,
            label: item.name
          }))}
          loading={false}
          onClose={() => setMenu(null)}
          onSelect={(selectedPath) => {
            const selected = menuItems.find((item) => item.path === selectedPath);
            setMenu(null);
            if (selected?.kind === "directory") {
              onNavigate(selected.path);
            } else if (selected) {
              onOpen(selected.path);
            }
          }}
        />
      )}
    </>
  );
}

function immediateWorkspaceChildren(
  items: WorkspaceFileTreeItem[],
  parentPath: string
): WorkspaceFileTreeItem[] {
  const normalizedParent = normalizedWorkspaceSegments(parentPath).join("/");
  return items
    .filter((item) => workspaceParentPath(item.path) === normalizedParent)
    .sort((left, right) => (
      left.kind === right.kind
        ? left.name.localeCompare(right.name)
        : left.kind === "directory" ? -1 : 1
    ));
}

function workspaceParentPath(path: string): string {
  const segments = normalizedWorkspaceSegments(path);
  return segments.slice(0, -1).join("/");
}

function normalizedWorkspaceSegments(path: string | null): string[] {
  return (path ?? "").replace(/\\/g, "/").split("/").filter(Boolean);
}

function workspaceRootLabel(root: string): string {
  const segments = root.replace(/[\\/]+$/, "").split(/[\\/]/).filter(Boolean);
  return segments.at(-1) ?? "Workspace";
}

class PreviewErrorBoundary extends Component<{
  children: ReactNode;
  onError(error: Error): void;
}, { failed: boolean }> {
  override state = { failed: false };

  static getDerivedStateFromError() {
    return { failed: true };
  }

  override componentDidCatch(error: Error) {
    this.props.onError(error);
  }

  override render() {
    return this.state.failed
      ? <p className="workspaceFilePreviewStatus" role="status">Preview renderer stopped.</p>
      : this.props.children;
  }
}

function PreviewBody({
  active,
  content,
  materialized,
  onPreviewStateChange,
  path,
  vendorReady
}: {
  active: boolean;
  content: string | null;
  materialized: MaterializedPreview;
  onPreviewStateChange(ready: boolean, error: unknown | null): void;
  path: string;
  vendorReady: boolean;
}) {
  switch (materialized.kind) {
    case "image":
      return (
        <img
          alt={`Preview ${path}`}
          className="workspaceFileImage"
          onError={() => onPreviewStateChange(false, new Error("Image decode failed."))}
          src={materialized.url}
        />
      );
    case "video":
      return (
        <ManagedMedia
          active={active}
          kind="video"
          onError={(error) => onPreviewStateChange(false, error)}
          path={path}
          url={materialized.url}
        />
      );
    case "audio":
      return (
        <ManagedMedia
          active={active}
          kind="audio"
          onError={(error) => onPreviewStateChange(false, error)}
          path={path}
          url={materialized.url}
        />
      );
    case "vendor":
      if (!active && !vendorReady) {
        return null;
      }
      return (
        <Suspense fallback={<p role="status">Loading renderer…</p>}>
          <VendorFilePreview
            active={active}
            filename={path}
            mediaType={materialized.mediaType}
            onStateChange={onPreviewStateChange}
            size={materialized.size}
            {...(materialized.buffer ? { buffer: materialized.buffer } : {})}
            {...(materialized.url ? { url: materialized.url } : {})}
          />
        </Suspense>
      );
    case "table":
      return (
        <DelimitedTable
          delimiter={materialized.delimiter}
          limits={materialized.limits}
          path={path}
          rows={materialized.rows}
          truncated={materialized.truncated}
        />
      );
    case "zip":
      return <ZipDirectory entries={materialized.entries} path={path} />;
    case "excalidraw":
      return (
        <Suspense fallback={<p role="status">Loading drawing…</p>}>
          <ExcalidrawPreview
            active={active}
            onStateChange={onPreviewStateChange}
            path={path}
            scene={materialized.scene}
          />
        </Suspense>
      );
    case "markdown":
      return (
        <div className="fileMarkdownPreview">
          <MarkdownText
            text={content ?? ""}
          />
        </div>
      );
    case "html":
      return (
        <HtmlStaticPreview
          active={active}
          content={content ?? ""}
          documentId={path}
          title={path}
        />
      );
    case "text":
      return <HighlightedCodePreview content={content ?? ""} path={path} />;
    case "unsupported":
      return null;
  }
}

function ManagedMedia({
  active,
  kind,
  onError,
  path,
  url
}: {
  active: boolean;
  kind: "audio" | "video";
  onError(error: Error): void;
  path: string;
  url: string;
}) {
  const ref = useRef<HTMLMediaElement | null>(null);
  useEffect(() => {
    if (!active) {
      ref.current?.pause();
    }
  }, [active]);
  if (kind === "video") {
    return (
      <video
        aria-label={`Preview ${path}`}
        className="workspaceFileVideo"
        controls
        controlsList="nodownload noremoteplayback"
        disablePictureInPicture
        onError={() => onError(new Error("Video decode failed."))}
        preload="metadata"
        ref={(node) => { ref.current = node; }}
        src={url}
      />
    );
  }
  return (
    <audio
      aria-label={`Preview ${path}`}
      className="workspaceFileAudio"
      controls
      controlsList="nodownload noremoteplayback"
      onError={() => onError(new Error("Audio decode failed."))}
      preload="metadata"
      ref={(node) => { ref.current = node; }}
      src={url}
    />
  );
}

function DelimitedTable({
  delimiter,
  limits,
  path,
  rows,
  truncated
}: {
  delimiter: string;
  limits: DelimitedTableLimits;
  path: string;
  rows: string[][];
  truncated: boolean;
}) {
  const [header = [], ...body] = rows;
  return (
    <div className="workspaceFileTableViewport">
      {truncated && (
        <p className="workspaceFilePreviewStatus" role="status">
          Table preview truncated at {limits.maxRows.toLocaleString()} rows, {limits.maxColumns.toLocaleString()} columns, or {limits.maxCells.toLocaleString()} cells.
        </p>
      )}
      <table aria-label={`Preview ${path}`} className="workspaceFileTable" data-delimiter={delimiter}>
        <thead>
          <tr>{header.map((cell, index) => <th key={index} scope="col">{cell}</th>)}</tr>
        </thead>
        <tbody>
          {body.map((row, rowIndex) => (
            <tr key={rowIndex}>
              {row.map((cell, cellIndex) => <td key={cellIndex}>{cell}</td>)}
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

function ZipDirectory({ entries, path }: { entries: ZipDirectoryEntry[]; path: string }) {
  return (
    <div className="workspaceZipDirectory" role="region" aria-label={`Preview ${path}`}>
      <p>{entries.length.toLocaleString()} entries</p>
      <ul>
        {entries.map((entry) => (
          <li key={`${entry.path}:${entry.directory}`}>
            <span>{entry.directory ? "Folder" : "File"}</span>
            <code>{entry.path}</code>
          </li>
        ))}
      </ul>
    </div>
  );
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

type PreviewKind =
  | "image"
  | "video"
  | "audio"
  | "pdf"
  | "vendor-blob"
  | "table"
  | "zip"
  | "excalidraw"
  | "markdown"
  | "html"
  | "text"
  | "unsupported";

type MaterializedPreviewData =
  | { kind: "image" | "video" | "audio"; url: string }
  | { kind: "vendor"; buffer?: ArrayBuffer; mediaType: string; size: number; url?: string }
  | {
    kind: "table";
    delimiter: string;
    limits: DelimitedTableLimits;
    rows: string[][];
    truncated: boolean;
  }
  | { kind: "zip"; entries: ZipDirectoryEntry[] }
  | { kind: "excalidraw"; scene: WorkspaceExcalidrawScene }
  | { kind: "markdown" | "html" | "text" | "unsupported" }
;

type MaterializedPreview = MaterializedPreviewData & { resourceId: string };

async function materializePreview({
  kind,
  lease,
  onProgress,
  resourceUrl,
  signal
}: {
  kind: PreviewKind;
  lease: WorkspaceFilePreviewOpenResult;
  onProgress(loaded: number, total: number): void;
  resourceUrl: string;
  signal: AbortSignal;
}): Promise<MaterializedPreviewData> {
  switch (kind) {
    case "image":
    case "video":
    case "audio":
      return { kind, url: resourceUrl };
    case "pdf":
      return {
        kind: "vendor",
        mediaType: lease.mediaType,
        size: lease.sizeBytes,
        url: resourceUrl
      };
    case "vendor-blob": {
      const bytes = await readPreviewBytes(resourceUrl, lease.sizeBytes, WHOLE_FILE_LIMIT_BYTES, signal, onProgress);
      const extension = extensionForPath(lease.path);
      const safeBytes = HEIC_EXTENSIONS.has(extension)
        ? bytes
        : await import("./workspace-file-parse").then(({ runWorkspaceFileParseTask }) => (
            runWorkspaceFileParseTask({
              bytes,
              filename: lease.path,
              kind: "office"
            }, signal).then((result) => result.bytes)
          ));
      return {
        kind: "vendor",
        buffer: exactArrayBuffer(safeBytes),
        mediaType: lease.mediaType,
        size: safeBytes.byteLength
      };
    }
    case "table": {
      const bytes = await readPreviewBytes(resourceUrl, lease.sizeBytes, WHOLE_FILE_LIMIT_BYTES, signal, onProgress);
      const delimiter = extensionForPath(lease.path) === "tsv" ? "\t" : ",";
      const result = await import("./workspace-file-parse").then(({ runWorkspaceFileParseTask }) => (
        runWorkspaceFileParseTask({ bytes, delimiter, kind: "table" }, signal)
      ));
      return {
        kind: "table",
        delimiter,
        limits: result.limits,
        rows: result.rows,
        truncated: result.truncated
      };
    }
    case "zip": {
      const bytes = await readPreviewBytes(resourceUrl, lease.sizeBytes, WHOLE_FILE_LIMIT_BYTES, signal, onProgress);
      const result = await import("./workspace-file-parse").then(({ runWorkspaceFileParseTask }) => (
        runWorkspaceFileParseTask({ bytes, kind: "zip" }, signal)
      ));
      return { kind: "zip", entries: result.entries };
    }
    case "excalidraw": {
      const bytes = await readPreviewBytes(resourceUrl, lease.sizeBytes, EXCALIDRAW_LIMIT_BYTES, signal, onProgress);
      const result = await import("./workspace-file-parse").then(({ runWorkspaceFileParseTask }) => (
        runWorkspaceFileParseTask({ bytes, kind: "excalidraw" }, signal)
      ));
      return { kind: "excalidraw", scene: result.scene };
    }
    case "markdown":
    case "html":
    case "text":
      return { kind };
    case "unsupported":
      throw new Error("Preview is not available for this file type.");
  }
}

async function readPreviewBytes(
  resourceUrl: string,
  expectedSize: number,
  limit: number,
  signal: AbortSignal,
  onProgress: (loaded: number, total: number) => void
): Promise<Uint8Array> {
  const response = await fetch(resourceUrl, {
    cache: "no-store",
    credentials: "omit",
    mode: "cors",
    referrerPolicy: "no-referrer",
    signal
  });
  if (response.status === 410) {
    throw new ExpiredPreviewLeaseError();
  }
  if (response.status === 409) {
    throw new Error("The file changed while its preview was open.");
  }
  if (!response.ok) {
    throw new Error(`Preview resource failed with HTTP ${response.status}.`);
  }
  const headerSize = Number.parseInt(response.headers.get("content-length") ?? "", 10);
  const total = Number.isFinite(headerSize) && headerSize >= 0 ? headerSize : expectedSize;
  if (total > limit) {
    throw new Error(limit === EXCALIDRAW_LIMIT_BYTES
      ? "Excalidraw preview is limited to 5 MiB."
      : "Preview requires the whole file and is limited to 32 MiB.");
  }
  if (!response.body) {
    const bytes = new Uint8Array(await response.arrayBuffer());
    if (bytes.byteLength > limit) {
      throw new Error("Preview resource exceeded its size limit.");
    }
    onProgress(bytes.byteLength, total || bytes.byteLength);
    return bytes;
  }
  const reader = response.body.getReader();
  const chunks: Uint8Array[] = [];
  let loaded = 0;
  while (true) {
    const next = await reader.read();
    if (next.done) {
      break;
    }
    if (next.value) {
      loaded += next.value.byteLength;
      if (loaded > limit) {
        await reader.cancel();
        throw new Error("Preview resource exceeded its size limit.");
      }
      chunks.push(next.value);
      onProgress(loaded, total || expectedSize || loaded);
    }
  }
  const combined = new Uint8Array(loaded);
  let offset = 0;
  for (const chunk of chunks) {
    combined.set(chunk, offset);
    offset += chunk.byteLength;
  }
  return combined;
}

function previewKind(path: string, content: string | null): PreviewKind {
  if (path.toLowerCase().endsWith(".draw.io")) return "unsupported";
  const extension = extensionForPath(path);
  if (EXPLICITLY_UNSUPPORTED_EXTENSIONS.has(extension)) return "unsupported";
  if (IMAGE_EXTENSIONS.has(extension)) return "image";
  if (HEIC_EXTENSIONS.has(extension)) return "vendor-blob";
  if (VIDEO_EXTENSIONS.has(extension)) return "video";
  if (AUDIO_EXTENSIONS.has(extension)) return "audio";
  if (extension === "pdf") return "pdf";
  if (VENDOR_BLOB_EXTENSIONS.has(extension)) return "vendor-blob";
  if (extension === "csv" || extension === "tsv") return "table";
  if (extension === "zip") return "zip";
  if (extension === "excalidraw") return "excalidraw";
  if (extension === "md" || extension === "markdown") return "markdown";
  if (extension === "html" || extension === "htm") return "html";
  return content !== null ? "text" : "unsupported";
}

function requiresWholeFile(kind: PreviewKind): boolean {
  return kind === "vendor-blob" || kind === "table" || kind === "zip" || kind === "excalidraw";
}

function exactArrayBuffer(bytes: Uint8Array): ArrayBuffer {
  return bytes.buffer.slice(bytes.byteOffset, bytes.byteOffset + bytes.byteLength) as ArrayBuffer;
}

function extensionForPath(path: string): string {
  return path.split(/[\\/]/).pop()?.split(".").pop()?.toLowerCase() ?? "";
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

function isHtmlFile(path: string): boolean {
  const extension = extensionForPath(path);
  return extension === "html" || extension === "htm";
}

function workspaceScopeKey(scope: GatewayRequestScope): string {
  return JSON.stringify(scope);
}

function previewResourceUrl(client: GatewayClient | null, resourcePath: string): string {
  const base = client?.endpoint?.httpBase
    ?? (typeof window !== "undefined" ? window.location.origin : "http://localhost");
  return new URL(resourcePath, `${base.replace(/\/$/, "")}/`).toString();
}

async function previewResourceStatus(resourceUrl: string): Promise<number> {
  const response = await fetch(resourceUrl, {
    cache: "no-store",
    credentials: "omit",
    method: "HEAD",
    mode: "cors",
    referrerPolicy: "no-referrer"
  });
  return response.status;
}

function selectExternalOpenAction(
  actions: WorkspaceFileExternalActionsResult
): WorkspaceExternalFileAction | null {
  const availableActions = Array.isArray(actions.availableActions)
    ? actions.availableActions
    : [];
  if (
    actions.preferredAction !== "reveal"
    && availableActions.includes(actions.preferredAction)
  ) {
    return actions.preferredAction;
  }
  return availableActions.find((candidate) => candidate !== "reveal") ?? null;
}

function formatProgress(loaded: number, total: number): string {
  if (total <= 0) {
    return `${formatBytes(loaded)} loaded`;
  }
  return `${Math.min(100, Math.round((loaded / total) * 100))}%`;
}

function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KiB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MiB`;
}

function errorMessage(error: unknown, fallback: string): string {
  const message = error instanceof Error ? error.message.trim() : String(error).trim();
  return message || fallback;
}

function isAbortError(error: unknown): boolean {
  return error instanceof DOMException && error.name === "AbortError";
}

class ExpiredPreviewLeaseError extends Error {
  constructor() {
    super("Preview lease expired.");
  }
}

function previewOpen(client: GatewayClient, target: WorkspaceFileSurfaceTarget) {
  return client.request("workspace/file/preview/open", target);
}

function previewRelease(client: GatewayClient, resourceId: string) {
  return client.request("workspace/file/preview/release", { resourceId })
    .catch(() => ({ released: false }));
}

function workspaceFileWrite(
  client: GatewayClient,
  target: WorkspaceFileSurfaceTarget,
  content: string,
  expectedRevision: string | null,
  force: boolean
) {
  return client.request("workspace/file/write", {
    scope: target.scope,
    path: target.path,
    content,
    expectedRevision,
    force
  });
}

function workspaceFileExternalActions(client: GatewayClient, target: WorkspaceFileSurfaceTarget) {
  return client.request("workspace/file/externalActions", target);
}

function workspaceFileOpenExternal(
  client: GatewayClient,
  target: WorkspaceFileSurfaceTarget,
  action: WorkspaceExternalFileAction
) {
  return client.request("workspace/file/openExternal", { ...target, action });
}

export const workspaceFilePreviewPolicy = {
  wholeFileLimitBytes: WHOLE_FILE_LIMIT_BYTES,
  excalidrawLimitBytes: EXCALIDRAW_LIMIT_BYTES
} as const;
