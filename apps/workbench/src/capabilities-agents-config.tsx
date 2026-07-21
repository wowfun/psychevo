import { Edit3, PlugZap, Plus, RefreshCw, Save, Trash2, Wrench, X } from "lucide-react";
import { ActionButton, CreatePanel, IconButton, Switch } from "@psychevo/components";
import { backendDisplayLabel, prettyJson } from "./data";
import type { BackendCommandJson, BackendDraft, WorkbenchBackend, WorkbenchBackendDoctor } from "./types";

const BACKEND_ENTRYPOINTS = ["peer", "subagent"] as const;
const BACKEND_CLIENT_CAPABILITIES = ["fs.read", "fs.write", "terminal"] as const;
const BACKEND_COMMAND_JSON_TEMPLATE = `{
  "command": "opencode",
  "args": ["acp"],
  "env": {}
}`;
export const EMPTY_BACKEND_DRAFT: BackendDraft = {
  id: "",
  enabled: true,
  label: "",
  description: "",
  commandJsonText: BACKEND_COMMAND_JSON_TEMPLATE,
  cwd: "",
  entrypoints: ["peer", "subagent"],
  clientCapabilities: ["fs.read", "fs.write", "terminal"],
  mcpServersText: ""
};

export type ManagedBackendAction = "install" | "repair" | "upgrade";

export function AgentsConfigPanel({
  backendDraft,
  backendDoctor,
  backends,
  disabled,
  runtimeReadiness,
  onCancelBackendEdit,
  onChangeBackendDraft,
  onDeleteBackend,
  onDoctorBackend,
  onEditBackend,
  onManageBackend,
  onNewBackend,
  onSaveBackendDraft,
  onSetBackendEnabled,
  onSetBackendEntrypoints,
}: {
  backendDraft: BackendDraft | null;
  backendDoctor: Record<string, WorkbenchBackendDoctor>;
  backends: WorkbenchBackend[];
  disabled: boolean;
  runtimeReadiness: Record<string, string>;
  onCancelBackendEdit(): void;
  onChangeBackendDraft(draft: BackendDraft): void;
  onDeleteBackend(backend: WorkbenchBackend): void;
  onDoctorBackend(backend: WorkbenchBackend): void;
  onEditBackend(backend: WorkbenchBackend): void;
  onManageBackend(backend: WorkbenchBackend, action: ManagedBackendAction): void;
  onNewBackend(): void;
  onSaveBackendDraft(draft: BackendDraft): void;
  onSetBackendEnabled(backend: WorkbenchBackend, enabled: boolean): void;
  onSetBackendEntrypoints(backend: WorkbenchBackend, entrypoints: string[]): void;
}) {
  const catalogBackends = backends;
  return (
    <section className="agentSurfacePanel agentsConfigPanel" aria-label="Agents">
      <header className="agentSurfaceHeaderWithAction">
        <span><PlugZap size={15} /> ACP Backends <b>{catalogBackends.length}</b></span>
        {!backendDraft && (
          <ActionButton disabled={disabled} icon={<Plus size={14} />} onClick={onNewBackend} size="compact" variant="primary">
            Add backend
          </ActionButton>
        )}
      </header>
      {backendDraft && (
        <BackendEditorForm
          draft={backendDraft}
          disabled={disabled}
          onCancel={onCancelBackendEdit}
          onChange={onChangeBackendDraft}
          onSave={onSaveBackendDraft}
        />
      )}
      {(!backendDraft || catalogBackends.length > 0) && <div className="agentSurfaceList">
        {catalogBackends.map((backend) => {
          const doctor = backendDoctor[backend.id] ?? null;
          const managedState = managedCodexState(backend, doctor, runtimeReadiness);
          const profileMutable = backend.sourceTargets.includes("profile");
          const managedAction = managedBackendAction(backend, managedState);
          const managedRecovery = managedRecoveryAction(backend, managedState);
          const backendReady = managedState === "ready";
          return (
            <div className="agentSurfaceRow agentBackendRow" key={backend.id}>
	              <div>
	                <strong>{backendDisplayLabel(backend)}</strong>
	                <span>{backend.command ? [backend.command, ...backend.args].join(" ") : backend.description || backend.kind}</span>
	                {backend.diagnostics.length > 0 && (
	                  <small className="agentSurfaceWarning">{backend.diagnostics.map((diagnostic) => diagnostic.message).join(" · ")}</small>
	                )}
                {doctor && (
                  <small className={doctor.ok ? "agentSurfaceOk" : "agentSurfaceWarning"}>
                    {doctor.checks.map((check) => `${check.name}: ${check.ok ? "ok" : check.message}`).join(" · ")}
                  </small>
                )}
              </div>
              <div className="agentBackendSide">
                <div className="agentBackendControls">
                  {managedRecovery ? (
                    <ActionButton
                      aria-label={`${managedActionLabel(managedRecovery)} ${backend.label || backend.id} ACP`}
                      disabled={disabled}
                      icon={managedRecovery === "repair" ? <Wrench size={13} /> : <Plus size={13} />}
                      onClick={() => onManageBackend(backend, managedRecovery)}
                      size="compact"
                      variant={managedRecovery === "repair" ? "danger" : "primary"}
                    >
                      {managedActionLabel(managedRecovery)}
                    </ActionButton>
                  ) : (
                    <Switch
                      checked={backend.enabled}
                      disabled={disabled || !profileMutable || !backendReady}
                      label={`${backend.id} enabled`}
                      onCheckedChange={(enabled) => onSetBackendEnabled(backend, enabled)}
                      showLabel={false}
                      size="compact"
                    />
                  )}
                  <BackendEntrypointControls
                    backend={backend}
                    disabled={disabled || !profileMutable || !backendReady}
                    onChange={(entrypoints) => onSetBackendEntrypoints(backend, entrypoints)}
                  />
                </div>
                <div className="agentBackendActions">
                  {managedAction && (
                    <IconButton
                      disabled={disabled}
                      icon={managedAction === "upgrade" ? <RefreshCw size={13} /> : managedAction === "repair" ? <Wrench size={13} /> : <Plus size={13} />}
                      label={`${managedActionLabel(managedAction)} managed ${backend.id}`}
                      onClick={() => onManageBackend(backend, managedAction)}
                      size="compact"
                      variant={managedAction === "repair" ? "danger" : "ghost"}
                    />
                  )}
                  <IconButton disabled={disabled || !profileMutable} icon={<Edit3 size={13} />} label={`Edit ${backend.id}`} onClick={() => onEditBackend(backend)} size="compact" />
                  <IconButton disabled={disabled} icon={<Wrench size={13} />} label={`Doctor ${backend.id}`} onClick={() => onDoctorBackend(backend)} size="compact" />
                  <IconButton
                    disabled={disabled || !profileMutable}
                    icon={<Trash2 size={13} />}
                    label={`Delete ${backend.id} from Profile`}
                    onClick={() => onDeleteBackend(backend)}
                    size="compact"
                    variant="danger"
                  />
                </div>
              </div>
            </div>
          );
        })}
        {catalogBackends.length === 0 && <p>No ACP backends configured.</p>}
      </div>}
    </section>
  );
}

function managedBackendAction(
  backend: WorkbenchBackend,
  state: ManagedCodexState
): ManagedBackendAction | null {
  if (backend.id !== "codex") return null;
  return state === "ready" ? "upgrade" : null;
}

type ManagedCodexState = "missing" | "invalid" | "ready";
type ManagedRecoveryAction = "install" | "repair";

function managedCodexState(
  backend: WorkbenchBackend,
  doctor: WorkbenchBackendDoctor | null,
  runtimeReadiness: Record<string, string>
): ManagedCodexState {
  if (backend.id !== "codex") return "ready";
  const managedCheck = doctor?.checks.find((check) => check.name === "managedAdapter") ?? null;
  if (managedCheck) {
    if (managedCheck.ok) return "ready";
    return managedCheck.message.toLowerCase().includes("repair") ? "invalid" : "missing";
  }
  const readiness = runtimeReadiness[backend.id];
  if (readiness === "ready" || readiness === "needsAuth" || readiness === "disabled") return "ready";
  if (readiness === "error" || readiness === "unsupported") return "invalid";
  return "missing";
}

function managedRecoveryAction(
  backend: WorkbenchBackend,
  state: ManagedCodexState
): ManagedRecoveryAction | null {
  if (backend.id !== "codex" || state === "ready") return null;
  return state === "invalid" ? "repair" : "install";
}

function managedActionLabel(action: ManagedBackendAction | ManagedRecoveryAction): string {
  return `${action.charAt(0).toUpperCase()}${action.slice(1)}`;
}

function BackendEntrypointControls({
  backend,
  disabled,
  onChange
}: {
  backend: WorkbenchBackend;
  disabled: boolean;
  onChange(entrypoints: string[]): void;
}) {
  const selected = backend.entrypoints.length > 0 ? backend.entrypoints : ["peer", "subagent"];
  return (
    <div className="backendEntrypointControls" aria-label={`${backend.id} entrypoints`}>
      {BACKEND_ENTRYPOINTS.map((entrypoint) => {
        const checked = selected.includes(entrypoint);
        const isLastSelected = checked && selected.length === 1;
        return (
          <label key={entrypoint}>
            <input
              aria-label={`${backend.id} ${entrypoint} entrypoint`}
              checked={checked}
              className="pevo-choiceControl"
              disabled={disabled || isLastSelected}
              onChange={(event) => {
                const next = event.currentTarget.checked
                  ? [...selected, entrypoint]
                  : selected.filter((item) => item !== entrypoint);
                onChange(BACKEND_ENTRYPOINTS.filter((item) => next.includes(item)));
              }}
              type="checkbox"
            />
            <span>{entrypoint}</span>
          </label>
        );
      })}
    </div>
  );
}

function BackendEditorForm({
  draft,
  disabled,
  onCancel,
  onChange,
  onSave
}: {
  draft: BackendDraft;
  disabled: boolean;
  onCancel(): void;
  onChange(draft: BackendDraft): void;
  onSave(draft: BackendDraft): void;
}) {
  const commandConfig = parseBackendCommandJson(draft.commandJsonText);
  const commandJsonError = draft.commandJsonText.trim() ? commandConfig.error : null;
  const canSave = Boolean(draft.id.trim() && commandConfig.command.trim() && !commandConfig.error);
  function patch(patch: Partial<BackendDraft>) {
    onChange({ ...draft, ...patch });
  }
  function toggleClientCapability(value: string) {
    const current = draft.clientCapabilities;
    patch({
      clientCapabilities: current.includes(value)
        ? current.filter((item) => item !== value)
        : [...current, value]
    });
  }
  return (
    <form
      aria-label="Profile ACP backend"
      className="backendEditor"
      onSubmit={(event) => {
        event.preventDefault();
        if (canSave && !disabled) {
          onSave(draft);
        }
      }}
    >
      <CreatePanel
        description="Configure a Profile-level ACP backend for peer or subagent execution."
        icon={<PlugZap size={18} />}
        layout="side"
        onClose={onCancel}
        title={draft.id.trim() ? "Edit backend" : "Add backend"}
        footer={
          <>
            <ActionButton disabled={disabled} icon={<X size={14} />} onClick={onCancel} variant="ghost">
              Cancel
            </ActionButton>
            <ActionButton disabled={disabled || !canSave} icon={<Save size={14} />} type="submit" variant="primary">
              Save
            </ActionButton>
          </>
        }
      >
        <label>
          <span>ID</span>
          <input aria-label="ID" className="pevo-fieldControl" disabled={disabled} onChange={(event) => patch({ id: event.currentTarget.value })} value={draft.id} />
        </label>
        <label>
          <span>Label <em>Optional</em></span>
          <input aria-label="Label" className="pevo-fieldControl" disabled={disabled} onChange={(event) => patch({ label: event.currentTarget.value })} value={draft.label} />
        </label>
        <label>
          <span>Description <em>Optional</em></span>
          <input aria-label="Description" className="pevo-fieldControl" disabled={disabled} onChange={(event) => patch({ description: event.currentTarget.value })} value={draft.description} />
        </label>
        <label>
          <span>Command JSON</span>
          <textarea
            aria-describedby={commandJsonError ? "backend-command-json-error" : undefined}
            aria-invalid={commandJsonError ? true : undefined}
            aria-label="Command JSON"
            className="backendJsonInput pevo-fieldControl pevo-fieldControl--code"
            disabled={disabled}
            onChange={(event) => patch({ commandJsonText: event.currentTarget.value })}
            spellCheck={false}
            value={draft.commandJsonText}
          />
          {commandJsonError && <small className="backendFieldError" id="backend-command-json-error">{commandJsonError}</small>}
        </label>
        <label>
          <span>Workspace</span>
          <input
            aria-label="Backend workspace"
            className="pevo-fieldControl"
            disabled={disabled}
            onChange={(event) => patch({ cwd: event.currentTarget.value })}
            placeholder="Defaults to workspace"
            value={draft.cwd}
          />
        </label>
        <fieldset className="backendDialogChecks">
          <legend>Client Capabilities</legend>
          {BACKEND_CLIENT_CAPABILITIES.map((capability) => (
            <label key={capability}>
              <input
                checked={draft.clientCapabilities.includes(capability)}
                className="pevo-choiceControl"
                disabled={disabled}
                onChange={() => toggleClientCapability(capability)}
                type="checkbox"
              />
              <span>{capability}</span>
            </label>
          ))}
        </fieldset>
        <label>
          <span>MCP Servers</span>
          <textarea aria-label="MCP Servers" className="pevo-fieldControl pevo-fieldControl--code pevo-fieldControl--compact" disabled={disabled} onChange={(event) => patch({ mcpServersText: event.currentTarget.value })} value={draft.mcpServersText} />
        </label>
      </CreatePanel>
    </form>
  );
}

export function backendDraftFromBackend(backend: WorkbenchBackend): BackendDraft {
  return {
    id: backend.id,
    enabled: backend.enabled,
    label: backend.label && backend.label !== backend.id ? backend.label : "",
    description: backend.description ?? "",
    commandJsonText: backend.command ? formatBackendCommandJson({
      command: backend.command,
      args: backend.args,
      env: {}
    }) : "",
    cwd: backend.cwd && backend.cwd !== "invocation" ? backend.cwd : "",
    entrypoints: backend.entrypoints.length > 0 ? backend.entrypoints : ["peer", "subagent"],
    clientCapabilities: backend.clientCapabilities.length > 0
      ? backend.clientCapabilities
      : ["fs.read", "fs.write", "terminal"],
    mcpServersText: backend.mcpServers.join("\n")
  };
}

function formatBackendCommandJson(config: BackendCommandJson): string {
  return prettyJson({
    command: config.command,
    args: config.args,
    env: config.env
  });
}

export function parseBackendCommandJson(value: string): BackendCommandJson & { error: string | null } {
  const trimmed = value.trim();
  if (!trimmed) {
    return { command: "", args: [], env: {}, error: null };
  }
  let parsed: unknown;
  try {
    parsed = JSON.parse(trimmed);
  } catch {
    return { command: "", args: [], env: {}, error: "Command JSON must be valid JSON." };
  }
  if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) {
    return { command: "", args: [], env: {}, error: "Command JSON must be an object." };
  }
  const record = parsed as Record<string, unknown>;
  if (typeof record.command !== "string") {
    return { command: "", args: [], env: {}, error: "Command JSON must include a string command." };
  }
  const args = record.args === undefined ? [] : record.args;
  if (!Array.isArray(args) || !args.every((item) => typeof item === "string")) {
    return { command: "", args: [], env: {}, error: "Command JSON args must be an array of strings." };
  }
  const envValue = record.env === undefined ? {} : record.env;
  if (!envValue || typeof envValue !== "object" || Array.isArray(envValue)) {
    return { command: "", args: [], env: {}, error: "Command JSON env must be an object." };
  }
  const env: Record<string, string> = {};
  for (const [key, envItem] of Object.entries(envValue as Record<string, unknown>)) {
    if (typeof envItem !== "string") {
      return { command: "", args: [], env: {}, error: "Command JSON env values must be strings." };
    }
    if (key.trim()) {
      env[key.trim()] = envItem;
    }
  }
  return {
    command: record.command,
    args,
    env,
    error: null
  };
}
