import { useEffect, useRef, useState } from "react";
import { ArrowUp, Folder, FolderOpen, FolderPlus, X } from "lucide-react";
import type { WorkspaceFolderListResult } from "@psychevo/protocol";

export function WorkspacePickerDialog({
  ariaLabel = "Choose workspace folder",
  disabled,
  onCancel,
  onCreate,
  onOpen,
  onReadFolders,
  title = "Choose workspace folder"
}: {
  ariaLabel?: string;
  disabled: boolean;
  onCancel(): void;
  onCreate?(parent: string, name: string): Promise<unknown>;
  onOpen(path: string): Promise<unknown>;
  onReadFolders(path: string | null): Promise<WorkspaceFolderListResult>;
  title?: string;
}) {
  const [state, setState] = useState<WorkspaceFolderListResult | null>(null);
  const [loading, setLoading] = useState(true);
  const [pending, setPending] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [creating, setCreating] = useState(false);
  const [name, setName] = useState("");
  const initialReadFolders = useRef(onReadFolders);
  const trimmedName = name.trim();

  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    setError(null);
    void initialReadFolders.current(null).then((result) => {
      if (!cancelled) setState(result);
    }).catch((cause: unknown) => {
      if (!cancelled) setError(errorMessage(cause));
    }).finally(() => {
      if (!cancelled) setLoading(false);
    });
    return () => {
      cancelled = true;
    };
  }, []);

  async function navigate(path: string) {
    setLoading(true);
    setError(null);
    setCreating(false);
    try {
      setState(await onReadFolders(path));
    } catch (cause) {
      setError(errorMessage(cause));
    } finally {
      setLoading(false);
    }
  }

  async function openWorkspace() {
    if (!state) return;
    setPending(true);
    setError(null);
    try {
      await onOpen(state.current);
    } catch (cause) {
      setError(errorMessage(cause));
    } finally {
      setPending(false);
    }
  }

  async function createWorkspace() {
    if (!state || !onCreate || !trimmedName) return;
    setPending(true);
    setError(null);
    try {
      await onCreate(state.current, trimmedName);
    } catch (cause) {
      setError(errorMessage(cause));
    } finally {
      setPending(false);
    }
  }

  const interactionDisabled = disabled || loading || pending;
  return (
    <div className="modalBackdrop" onMouseDown={(event) => event.target === event.currentTarget && !pending && onCancel()} role="presentation">
      <div aria-label={ariaLabel} className="workspaceDialog composerFolderDialog" role="dialog">
        <header>
          <div className="workspaceDialogTitle"><FolderOpen size={18} /><h2>{title}</h2></div>
          <button aria-label="Close folder picker" disabled={pending} onClick={onCancel} type="button"><X size={16} /></button>
        </header>
        <div className="composerFolderLocation">
          <button
            aria-label="Parent folder"
            disabled={interactionDisabled || !state?.parent}
            onClick={() => state?.parent && void navigate(state.parent)}
            title="Parent folder"
            type="button"
          >
            <ArrowUp size={15} />
          </button>
          <span title={state?.current ?? ""}>{state?.current ?? "Loading..."}</span>
        </div>
        <div className="composerFolderList" aria-busy={loading}>
          {state?.folders.map((folder) => (
            <button disabled={interactionDisabled} key={folder.path} onClick={() => void navigate(folder.path)} type="button">
              <Folder size={15} />
              <span>{folder.name}</span>
            </button>
          ))}
          {!loading && state && state.folders.length === 0 ? <p>No subfolders</p> : null}
          {error ? <p className="is-error" role="alert">{error}</p> : null}
        </div>
        {creating ? (
          <form
            className="workspacePickerCreate"
            onSubmit={(event) => {
              event.preventDefault();
              void createWorkspace();
            }}
          >
            <label>
              New workspace in <span title={state?.current}>{state?.current}</span>
              <input
                autoFocus
                disabled={disabled || pending}
                onChange={(event) => setName(event.target.value)}
                placeholder="Workspace name"
                value={name}
              />
            </label>
          </form>
        ) : null}
        <footer>
          <button disabled={pending} onClick={creating ? () => setCreating(false) : onCancel} type="button">
            {creating ? "Back" : "Cancel"}
          </button>
          {onCreate && !creating ? (
            <button disabled={interactionDisabled || !state} onClick={() => setCreating(true)} type="button">
              <FolderPlus size={14} aria-hidden />
              New workspace...
            </button>
          ) : null}
          <button
            disabled={disabled || pending || (creating ? !trimmedName : loading || !state)}
            onClick={() => void (creating ? createWorkspace() : openWorkspace())}
            type="submit"
          >
            {pending ? "Working..." : creating ? "Create workspace" : "Open folder"}
          </button>
        </footer>
      </div>
    </div>
  );
}

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}
