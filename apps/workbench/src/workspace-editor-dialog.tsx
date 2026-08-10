import { GatewayClientError } from "@psychevo/client";
import { useEffect, useRef, useState } from "react";
import { ArrowUp, FolderOpen, Plus, Trash2, X } from "lucide-react";
import type { SessionBrowserWorkspaceState } from "./types";

export function WorkspaceEditorDialog({
  disabled,
  onCancel,
  onSave,
  workspace
}: {
  disabled: boolean;
  onCancel(): void;
  onSave(name: string, roots: string[], expectedRevision: number): Promise<boolean | void>;
  workspace: SessionBrowserWorkspaceState;
}) {
  const [name, setName] = useState(workspace.name);
  const nextRootId = useRef(workspace.roots.length);
  const [roots, setRoots] = useState(() => workspace.roots.map((value, index) => ({
    id: `initial-${index}`,
    value
  })));
  const [pending, setPending] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [expectedRevision, setExpectedRevision] = useState(workspace.revision);
  const [returnFocus] = useState<HTMLElement | null>(() => (
    document.activeElement instanceof HTMLElement ? document.activeElement : null
  ));
  const dialogRef = useRef<HTMLDivElement>(null);
  const valid = Boolean(name.trim()) && roots.length > 0 && roots.every((root) => root.value.trim());

  useEffect(() => {
    dialogRef.current?.querySelector<HTMLElement>("input:not(:disabled), button:not(:disabled)")?.focus();
    return () => returnFocus?.focus();
  }, [returnFocus]);

  useEffect(() => {
    setName(workspace.name);
    setRoots(workspace.roots.map((value, index) => ({
      id: `workspace-${workspace.revision}-${index}`,
      value
    })));
    nextRootId.current = workspace.roots.length;
    setExpectedRevision(workspace.revision);
  }, [workspace.id, workspace.name, workspace.revision, workspace.roots]);

  useEffect(() => {
    if (pending) dialogRef.current?.focus();
  }, [pending]);

  async function submit() {
    if (!valid) return;
    setPending(true);
    setError(null);
    try {
      const saved = await onSave(
        name.trim(),
        roots.map((root) => root.value.trim()),
        expectedRevision
      );
      if (saved === false) setPending(false);
    } catch (cause) {
      const latest = workspaceConflictSnapshot(cause, workspace.id);
      if (latest) {
        setName(latest.name);
        setRoots(latest.roots.map((value, index) => ({
          id: `conflict-${latest.revision}-${index}`,
          value
        })));
        nextRootId.current = latest.roots.length;
        setExpectedRevision(latest.revision);
        setError("Workspace changed elsewhere. Latest values loaded; review and save again.");
      } else {
        setError(cause instanceof Error ? cause.message : String(cause));
      }
      setPending(false);
    }
  }

  return (
    <div className="modalBackdrop" role="presentation">
      <div
        aria-label={`Edit workspace ${workspace.name}`}
        aria-modal="true"
        className="workspaceDialog workspaceEditorDialog"
        onKeyDownCapture={(event) => {
          if (event.key === "Escape" && !pending) {
            event.preventDefault();
            onCancel();
            return;
          }
          if (event.key !== "Tab") return;
          const focusable = [...event.currentTarget.querySelectorAll<HTMLElement>(
            "button, input, select, textarea, [href]"
          )].filter((element) => (
            !element.hasAttribute("disabled") && !element.closest("fieldset[disabled]")
          ));
          if (focusable.length === 0) {
            event.preventDefault();
            event.currentTarget.focus();
            return;
          }
          const first = focusable[0]!;
          const last = focusable[focusable.length - 1]!;
          const active = event.target instanceof HTMLElement ? event.target : document.activeElement;
          if (event.shiftKey && active === first) {
            event.preventDefault();
            last.focus();
          } else if (!event.shiftKey && active === last) {
            event.preventDefault();
            first.focus();
          }
        }}
        ref={dialogRef}
        role="dialog"
        tabIndex={-1}
      >
        <header>
          <div className="workspaceDialogTitle"><FolderOpen size={18} /><h2>Edit workspace</h2></div>
          <button aria-label="Close workspace editor" disabled={pending} onClick={onCancel} type="button"><X size={16} /></button>
        </header>
        <div className="workspaceEditorBody">
          <label>
            <span>Name</span>
            <input
              autoFocus
              className="pevo-fieldControl"
              disabled={disabled || pending}
              onChange={(event) => setName(event.target.value)}
              value={name}
            />
          </label>
          <fieldset disabled={disabled || pending}>
            <legend>Working directories</legend>
            {roots.map((root, index) => (
              <div className="workspaceEditorRoot" key={root.id}>
                <input
                  aria-label={index === 0 ? "Primary working directory" : `Working directory ${index + 1}`}
                  className="pevo-fieldControl"
                  onChange={(event) => setRoots((current) => current.map((value, rootIndex) => (
                    rootIndex === index ? { ...value, value: event.target.value } : value
                  )))}
                  value={root.value}
                />
                {index === 0 ? <span className="workspaceEditorPrimary">Primary</span> : (
                  <button
                    aria-label={`Make ${root.value || `directory ${index + 1}`} primary`}
                    onClick={() => setRoots((current) => [current[index]!, ...current.filter((_, rootIndex) => rootIndex !== index)])}
                    title="Make primary"
                    type="button"
                  ><ArrowUp size={15} /></button>
                )}
                <button
                  aria-label={`Remove ${root.value || `directory ${index + 1}`}`}
                  disabled={roots.length === 1}
                  onClick={() => setRoots((current) => current.filter((_, rootIndex) => rootIndex !== index))}
                  title="Remove directory"
                  type="button"
                ><Trash2 size={15} /></button>
              </div>
            ))}
            <button className="workspaceEditorAdd" onClick={() => {
              const id = `added-${nextRootId.current}`;
              nextRootId.current += 1;
              setRoots((current) => [...current, { id, value: "" }]);
            }} type="button">
              <Plus size={15} /> Add directory
            </button>
          </fieldset>
          {error ? <p className="composerDialogError" role="alert">{error}</p> : null}
        </div>
        <footer>
          <button disabled={pending} onClick={onCancel} type="button">Cancel</button>
          <button disabled={disabled || pending || !valid} onClick={() => void submit()} type="button">
            {pending ? "Saving..." : "Save workspace"}
          </button>
        </footer>
      </div>
    </div>
  );
}

function workspaceConflictSnapshot(
  cause: unknown,
  workspaceId: string
): { name: string; revision: number; roots: string[] } | null {
  if (!(cause instanceof GatewayClientError) || !cause.data || typeof cause.data !== "object") {
    return null;
  }
  const data = cause.data as Record<string, unknown>;
  const workspace = data.workspace;
  if (!workspace || typeof workspace !== "object") return null;
  const record = workspace as Record<string, unknown>;
  if (
    record.id !== workspaceId
    || typeof record.name !== "string"
    || !Number.isSafeInteger(record.revision)
    || !Array.isArray(record.roots)
    || record.roots.length === 0
    || !record.roots.every((root) => typeof root === "string")
  ) return null;
  return {
    name: record.name,
    revision: record.revision as number,
    roots: record.roots as string[]
  };
}
