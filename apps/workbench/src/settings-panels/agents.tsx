import { Edit3, PlugZap, Plus, Save, Trash2, Wrench, X } from "lucide-react";
import { prettyJson } from "../data";
import type { BackendCommandJson, BackendDraft, WorkbenchBackend, WorkbenchBackendDoctor } from "../types";

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

export function AgentsConfigPanel({
  backendDraft,
  backendDoctor,
  backends,
  disabled,
  onCancelBackendEdit,
  onChangeBackendDraft,
  onDeleteBackend,
  onDoctorBackend,
  onEditBackend,
  onNewBackend,
  onSaveBackendDraft,
  onSetBackendEnabled,
  onSetBackendEntrypoints,
}: {
  backendDraft: BackendDraft | null;
  backendDoctor: Record<string, WorkbenchBackendDoctor>;
  backends: WorkbenchBackend[];
  disabled: boolean;
  onCancelBackendEdit(): void;
  onChangeBackendDraft(draft: BackendDraft): void;
  onDeleteBackend(backend: WorkbenchBackend): void;
  onDoctorBackend(backend: WorkbenchBackend): void;
  onEditBackend(backend: WorkbenchBackend): void;
  onNewBackend(): void;
  onSaveBackendDraft(draft: BackendDraft): void;
  onSetBackendEnabled(backend: WorkbenchBackend, enabled: boolean): void;
  onSetBackendEntrypoints(backend: WorkbenchBackend, entrypoints: string[]): void;
}) {
  const profileBackends = backends.filter((backend) => backend.sourceTargets.includes("profile"));
  return (
    <section className="agentSurfacePanel agentsConfigPanel" aria-label="Agents">
      <header className="agentSurfaceHeaderWithAction">
        <span><PlugZap size={15} /> Profile ACP Backends <b>{profileBackends.length}</b></span>
        {!backendDraft && (
          <button aria-label="Add ACP backend" disabled={disabled} onClick={onNewBackend} title="Add ACP backend" type="button">
            <Plus size={14} />
          </button>
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
      {(!backendDraft || profileBackends.length > 0) && <div className="agentSurfaceList">
        {profileBackends.map((backend) => {
          const doctor = backendDoctor[backend.id] ?? null;
          return (
            <div className="agentSurfaceRow agentBackendRow" key={backend.id}>
	              <div>
	                <strong>{backend.label || backend.id}</strong>
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
                  <label className="backendSwitch">
                    <input
                      aria-label={`${backend.enabled ? "Disable" : "Enable"} ${backend.id}`}
                      checked={backend.enabled}
                      disabled={disabled}
                      onChange={(event) => onSetBackendEnabled(backend, event.currentTarget.checked)}
                      role="switch"
                      type="checkbox"
                    />
                    <span className="backendSwitchTrack" aria-hidden />
                    <span>{backend.enabled ? "Enabled" : "Disabled"}</span>
                  </label>
                  <BackendEntrypointControls
                    backend={backend}
                    disabled={disabled}
                    onChange={(entrypoints) => onSetBackendEntrypoints(backend, entrypoints)}
                  />
                </div>
                <div className="agentBackendActions">
                  <button aria-label={`Edit ${backend.id}`} disabled={disabled} onClick={() => onEditBackend(backend)} title="Edit Profile backend" type="button">
                    <Edit3 size={13} />
                  </button>
                  <button aria-label={`Doctor ${backend.id}`} disabled={disabled} onClick={() => onDoctorBackend(backend)} title="Doctor" type="button">
                    <Wrench size={13} />
                  </button>
                  <button
                    aria-label={`Delete ${backend.id} from Profile`}
                    disabled={disabled}
                    onClick={() => onDeleteBackend(backend)}
                    title="Delete Profile backend"
                    type="button"
                  >
                    <Trash2 size={13} />
                  </button>
                </div>
              </div>
            </div>
          );
        })}
        {profileBackends.length === 0 && <p>No Profile ACP backends configured.</p>}
      </div>}
    </section>
  );
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
      <header>
        <div className="workspaceDialogTitle">
          <PlugZap size={18} aria-hidden />
          <h4>Backend details</h4>
        </div>
        <button aria-label="Close backend editor" onClick={onCancel} title="Close" type="button">
          <X size={15} />
        </button>
      </header>
        <label>
          <span>ID</span>
          <input aria-label="ID" disabled={disabled} onChange={(event) => patch({ id: event.currentTarget.value })} value={draft.id} />
        </label>
        <label>
          <span>Label <em>Optional</em></span>
          <input aria-label="Label" disabled={disabled} onChange={(event) => patch({ label: event.currentTarget.value })} value={draft.label} />
        </label>
        <label>
          <span>Description <em>Optional</em></span>
          <input aria-label="Description" disabled={disabled} onChange={(event) => patch({ description: event.currentTarget.value })} value={draft.description} />
        </label>
        <label>
          <span>Command JSON</span>
          <textarea
            aria-describedby={commandJsonError ? "backend-command-json-error" : undefined}
            aria-invalid={commandJsonError ? true : undefined}
            aria-label="Command JSON"
            className="backendJsonInput"
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
          <textarea aria-label="MCP Servers" disabled={disabled} onChange={(event) => patch({ mcpServersText: event.currentTarget.value })} value={draft.mcpServersText} />
        </label>
        <footer>
          <button disabled={disabled} onClick={onCancel} type="button">
            <X size={14} />
            Cancel
          </button>
          <button disabled={disabled || !canSave} type="submit">
            <Save size={14} />
            Save
          </button>
        </footer>
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
