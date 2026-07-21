import { useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { Check, FolderOpen, GitBranch, Pin, Plus, X } from "lucide-react";
import type {
  InitializeResult,
  ThreadControlDescriptorView,
  WorkspaceFolderListResult,
  WorkspaceGitBranchesResult
} from "@psychevo/protocol";
import { RuntimeControlFields } from "./runtime-controls";
import { usePopoverDismiss } from "./popover-dismiss";
import { WorkspacePickerDialog } from "./workspace-picker-dialog";

export function ComposerEnvironment({
  branch,
  branchDisabled,
  controlValues,
  controls,
  cwd,
  disabled,
  draft,
  path,
  profile,
  runtimeSafetyLabel,
  workspaces,
  onBranchChange,
  onOpenFiles,
  onReadBranches,
  onReadFolders,
  onRuntimeControlChange,
  onWorkspaceChange
}: {
  branch: string | null;
  branchDisabled: boolean;
  controlValues: Record<string, unknown>;
  controls: ThreadControlDescriptorView[];
  cwd: string;
  disabled: boolean;
  draft: boolean;
  path: string;
  profile: InitializeResult["profile"] | null;
  runtimeSafetyLabel?: string | null;
  workspaces: Array<{ cwd: string; displayPath?: string }>;
  onBranchChange(branch: string, create: boolean): Promise<WorkspaceGitBranchesResult>;
  onOpenFiles(): void;
  onReadBranches(): Promise<WorkspaceGitBranchesResult>;
  onReadFolders(path: string | null): Promise<WorkspaceFolderListResult>;
  onRuntimeControlChange(control: ThreadControlDescriptorView, value: unknown): void;
  onWorkspaceChange(cwd: string): Promise<unknown>;
}) {
  const workspaceRootRef = useRef<HTMLDivElement | null>(null);
  const branchRootRef = useRef<HTMLDivElement | null>(null);
  const workspaceTriggerRef = useRef<HTMLButtonElement | null>(null);
  const branchTriggerRef = useRef<HTMLButtonElement | null>(null);
  const [workspaceMenuOpen, setWorkspaceMenuOpen] = useState(false);
  const [branchMenuOpen, setBranchMenuOpen] = useState(false);
  const [branchState, setBranchState] = useState<WorkspaceGitBranchesResult | null>(null);
  const [menuError, setMenuError] = useState<string | null>(null);
  const [loadingBranches, setLoadingBranches] = useState(false);
  const [folderDialogOpen, setFolderDialogOpen] = useState(false);
  const [newBranchOpen, setNewBranchOpen] = useState(false);
  const visibleBranch = branch?.trim() || null;
  const permissionControls = controls.filter((control) => control.id === "permissionMode");
  const profileLabel = profile && !profile.default ? profile.name : null;
  const knownWorkspaces = useMemo(
    () => {
      const byCwd = new Map<string, { cwd: string; displayPath: string }>();
      for (const workspace of [{ cwd, displayPath: path }, ...workspaces]) {
        const canonicalCwd = workspace.cwd.trim();
        if (!canonicalCwd) continue;
        byCwd.set(canonicalCwd, {
          cwd: canonicalCwd,
          displayPath: workspace.displayPath?.trim() || canonicalCwd
        });
      }
      return Array.from(byCwd.values());
    },
    [cwd, path, workspaces]
  );

  usePopoverDismiss(workspaceMenuOpen, workspaceRootRef, workspaceTriggerRef, () => setWorkspaceMenuOpen(false));
  usePopoverDismiss(branchMenuOpen, branchRootRef, branchTriggerRef, () => setBranchMenuOpen(false));

  useEffect(() => {
    const escape = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        setFolderDialogOpen(false);
        setNewBranchOpen(false);
      }
    };
    document.addEventListener("keydown", escape);
    return () => {
      document.removeEventListener("keydown", escape);
    };
  }, []);

  async function openBranchMenu() {
    setWorkspaceMenuOpen(false);
    setBranchMenuOpen(true);
    setLoadingBranches(true);
    setMenuError(null);
    try {
      setBranchState(await onReadBranches());
    } catch (error) {
      setMenuError(errorMessage(error));
    } finally {
      setLoadingBranches(false);
    }
  }

  async function changeBranch(nextBranch: string, create: boolean) {
    setMenuError(null);
    try {
      const result = await onBranchChange(nextBranch, create);
      setBranchState(result);
      setBranchMenuOpen(false);
      setNewBranchOpen(false);
    } catch (error) {
      setMenuError(errorMessage(error));
    }
  }

  async function changeWorkspace(cwd: string) {
    setMenuError(null);
    setWorkspaceMenuOpen(false);
    try {
      await onWorkspaceChange(cwd);
    } catch (error) {
      setMenuError(errorMessage(error));
      setWorkspaceMenuOpen(true);
    }
  }

  return (
    <>
      <div className="composerStatusLine" aria-label="Composer environment">
        {runtimeSafetyLabel ? (
          <span className="profileStatusPill" aria-label="Runtime Profile safety policy" title={runtimeSafetyLabel}>
            <span>{runtimeSafetyLabel}</span>
          </span>
        ) : null}
        {profileLabel ? (
          <span className="profileStatusPill" title={profile?.home || profileLabel}>
            <Pin size={12} />
            <span>{profileLabel}</span>
          </span>
        ) : null}
        <div className="composerEnvironmentControl is-workspace" ref={workspaceRootRef}>
          <button
            aria-expanded={draft ? workspaceMenuOpen : undefined}
            aria-haspopup={draft ? "menu" : undefined}
            aria-label="Workspace"
            className="pathStatusButton"
            disabled={disabled}
            onClick={() => {
              if (!draft) {
                onOpenFiles();
                return;
              }
              setBranchMenuOpen(false);
              setMenuError(null);
              setWorkspaceMenuOpen((open) => !open);
            }}
            ref={workspaceTriggerRef}
            title={cwd}
            type="button"
          >
            {path || "workspace"}
          </button>
          {draft && workspaceMenuOpen ? (
            <EnvironmentMenu ariaLabel="Workspace">
              {knownWorkspaces.map((candidate) => (
                <button
                  aria-current={candidate.cwd === cwd ? "true" : undefined}
                  key={candidate.cwd}
                  onClick={() => void changeWorkspace(candidate.cwd)}
                  role="menuitem"
                  title={candidate.cwd}
                  type="button"
                >
                  <span>{candidate.displayPath}</span>
                  {candidate.cwd === cwd ? <Check size={13} aria-hidden /> : null}
                </button>
              ))}
              {menuError ? <div className="composerEnvironmentMenuState is-error" role="alert">{menuError}</div> : null}
              <div className="composerEnvironmentMenuDivider" />
              <button
                onClick={() => {
                  setWorkspaceMenuOpen(false);
                  setFolderDialogOpen(true);
                }}
                role="menuitem"
                type="button"
              >
                <FolderOpen size={14} aria-hidden />
                <span>Open workspace...</span>
              </button>
            </EnvironmentMenu>
          ) : null}
        </div>
        {draft || visibleBranch ? (
          <div className="composerEnvironmentControl is-branch" ref={branchRootRef}>
            <button
              aria-expanded={branchMenuOpen}
              aria-haspopup="menu"
              aria-label="Git branch"
              className="branchStatusButton"
              disabled={disabled || branchDisabled}
              onClick={() => void openBranchMenu()}
              ref={branchTriggerRef}
              title={visibleBranch ?? "Git branch"}
              type="button"
            >
              <GitBranch size={13} />
              <span>{visibleBranch ?? "Git branch"}</span>
            </button>
            {branchMenuOpen ? (
              <EnvironmentMenu ariaLabel="Git branch">
                {loadingBranches ? <div className="composerEnvironmentMenuState">Loading branches...</div> : null}
                {branchState?.branches.map((candidate) => (
                  <button
                    aria-current={candidate === branchState.current ? "true" : undefined}
                    disabled={candidate === branchState.current}
                    key={candidate}
                    onClick={() => void changeBranch(candidate, false)}
                    role="menuitem"
                    title={candidate}
                    type="button"
                  >
                    <span>{candidate}</span>
                    {candidate === branchState.current ? <Check size={13} aria-hidden /> : null}
                  </button>
                ))}
                {menuError ? <div className="composerEnvironmentMenuState is-error" role="alert">{menuError}</div> : null}
                <div className="composerEnvironmentMenuDivider" />
                <button
                  disabled={disabled || branchDisabled}
                  onClick={() => {
                    setBranchMenuOpen(false);
                    setNewBranchOpen(true);
                  }}
                  role="menuitem"
                  type="button"
                >
                  <Plus size={14} aria-hidden />
                  <span>New branch...</span>
                </button>
              </EnvironmentMenu>
            ) : null}
          </div>
        ) : null}
        <RuntimeControlFields
          controls={permissionControls}
          dependencyControls={controls}
          disabled={disabled}
          values={controlValues}
          onChange={onRuntimeControlChange}
        />
      </div>
      {folderDialogOpen ? (
        <WorkspacePickerDialog
          disabled={disabled}
          onCancel={() => setFolderDialogOpen(false)}
          onOpen={async (nextCwd) => {
            await onWorkspaceChange(nextCwd);
            setFolderDialogOpen(false);
          }}
          onReadFolders={onReadFolders}
        />
      ) : null}
      {newBranchOpen ? (
        <NewBranchDialog
          disabled={disabled || branchDisabled}
          error={menuError}
          onCancel={() => setNewBranchOpen(false)}
          onCreate={(name) => void changeBranch(name, true)}
        />
      ) : null}
    </>
  );
}

function EnvironmentMenu({ ariaLabel, children }: { ariaLabel: string; children: ReactNode }) {
  return <div aria-label={ariaLabel} className="composerEnvironmentMenu pevo-controlPopover" role="menu">{children}</div>;
}

function NewBranchDialog({
  disabled,
  error,
  onCancel,
  onCreate
}: {
  disabled: boolean;
  error: string | null;
  onCancel(): void;
  onCreate(name: string): void;
}) {
  const [name, setName] = useState("");
  const trimmed = name.trim();
  return (
    <div className="modalBackdrop" role="presentation">
      <form
        aria-label="New branch"
        className="workspaceDialog composerBranchDialog"
        onSubmit={(event) => {
          event.preventDefault();
          if (trimmed && !disabled) onCreate(trimmed);
        }}
        role="dialog"
      >
        <header>
          <div className="workspaceDialogTitle"><GitBranch size={18} /><h2>New branch</h2></div>
          <button aria-label="Close new branch dialog" onClick={onCancel} type="button"><X size={16} /></button>
        </header>
        <label>
          Branch name
          <input className="pevo-fieldControl" autoFocus disabled={disabled} onChange={(event) => setName(event.target.value)} value={name} />
        </label>
        {error ? <p className="composerDialogError" role="alert">{error}</p> : null}
        <footer>
          <button disabled={disabled} onClick={onCancel} type="button">Cancel</button>
          <button disabled={disabled || !trimmed} type="submit">Create branch</button>
        </footer>
      </form>
    </div>
  );
}

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}
