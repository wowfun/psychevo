import { useEffect, useMemo, useRef, useState, type FormEvent } from "react";
import type { GatewayClient } from "@psychevo/client";
import { ActionButton, CreatePanel, MarkdownText, Switch } from "@psychevo/components";
import type {
  GatewayRequestScope,
  RuntimeProfileView,
  TeamMemberInput
} from "@psychevo/protocol";
import { Edit3, LogIn, LogOut, Play, Plus, RefreshCw, Save, Search, Trash2, Wrench, X } from "lucide-react";
import { AgentsConfigPanel, type ManagedBackendAction } from "./capabilities-agents-config";
import { parseBackendList } from "./data";
import { formatRuntimeCheckedAt } from "./runtime-context";
import type { BackendConfigTarget, BackendDraft, CapabilityTab, WorkbenchBackend, WorkbenchBackendDoctor } from "./types";

type JsonObject = Record<string, unknown>;

type CapabilityRow = {
  id: string;
  name: string;
  description: string;
  enabled: boolean;
  status: string;
  badges: string[];
  raw: JsonObject;
};

type SkillInstallDraft = { source: string; name: string; target: "profile" | "project"; force: boolean };
type PluginInstallDraft = {
  source: string;
  kind: "local" | "git" | "npm";
  npmVersion: string;
  npmRegistry: string;
  adapterMode: "manifest_only" | "adapter_host" | "disabled";
  force: boolean;
  inspection: JsonObject | null;
};
type MutationOptions = { notice?: string; refresh?: boolean };
type AgentDefinitionState = "active" | "shadowed" | "disabled";
type AgentsSegment = "definitions" | "teams" | "runtimes" | "backends";

type AgentDefinitionRow = {
  id: string;
  name: string;
  description: string;
  enabled: boolean;
  source: string;
  sourceLabel: string;
  target: BackendConfigTarget | null;
  mutable: boolean;
  path: string | null;
  entrypoints: string[];
  tools: string[];
  mcpServers: string[];
  diagnostics: string[];
  backendRef: string;
  state: AgentDefinitionState;
  raw: JsonObject;
};

type AgentDraft = {
  mode: "form" | "markdown";
  target: BackendConfigTarget;
  name: string;
  description: string;
  enabled: boolean;
  instructions: string;
  backendRef: string;
  entrypointsText: string;
  toolsText: string;
  mcpServersText: string;
  rawMarkdown: string;
};

type AgentTeamRow = {
  id: string;
  name: string;
  description: string;
  enabled: boolean;
  source: string;
  sourceLabel: string;
  target: BackendConfigTarget | null;
  mutable: boolean;
  path: string | null;
  leader: string;
  members: JsonObject[];
  maxParallelAgents: number;
  diagnostics: string[];
  state: AgentDefinitionState;
  raw: JsonObject;
};

type RuntimeProfileRow = {
  id: string;
  label: string;
  runtime: string;
  enabled: boolean;
  generated: boolean;
  configured: boolean;
  backendRef: string;
  provenance: string;
  profileRevision: string;
  capabilityRevision: string;
  defaultModel: string;
  defaultMode: string;
  defaultAgent: string;
  approvalMode: string;
  sandbox: string;
  workspaceRoots: string[];
  healthStatus: string;
  healthSummary: string;
  checkedAtMs: number | null;
  readinessStages: Array<{ id: string; status: string; summary: string; observedAtMs: number | null }>;
  sourceTargets: BackendConfigTarget[];
  diagnostics: string[];
  raw: JsonObject;
};

type RuntimeProfileKind = "native" | "acp";

type RuntimeProfileDraft = {
  target: BackendConfigTarget;
  id: string;
  runtime: RuntimeProfileKind;
  enabled: boolean;
  label: string;
  backendRef: string;
  defaultModel: string;
  defaultMode: string;
  defaultAgent: string;
  approvalMode: string;
  sandbox: string;
  workspaceRootsText: string;
  optionsText: string;
};

type TeamDraft = {
  mode: "form" | "markdown";
  target: BackendConfigTarget;
  name: string;
  description: string;
  enabled: boolean;
  leader: string;
  membersText: string;
  maxParallelAgents: string;
  instructions: string;
  rawMarkdown: string;
};

type AgentDetailState = {
  id: string;
  loading: boolean;
  value: JsonObject | null;
  instructions: string;
  rawMarkdown: string;
  error: string | null;
};

type TeamDetailState = {
  id: string;
  loading: boolean;
  value: JsonObject | null;
  instructions: string;
  rawMarkdown: string;
  error: string | null;
};

type SkillRow = CapabilityRow & {
  collisionGroup: string[];
  issues: string[];
  location: string;
  missingCredentialFiles: string[];
  missingEnvVars: string[];
  promptVisible: boolean;
  readiness: string;
  requiredTools: string[];
  requiredToolsets: string[];
  skillDir: string;
  source: string;
  sourceLabel: string;
  supported: boolean;
  tags: string[];
};

const TABS: Array<{ id: CapabilityTab; label: string }> = [
  { id: "agents", label: "Agents" },
  { id: "skills", label: "Skills" },
  { id: "plugins", label: "Plugins" },
  { id: "mcp", label: "MCP" },
  { id: "tools", label: "Tools" }
];

export function CapabilitiesPage({
  activeTab,
  backendDraft,
  backendDoctor,
  backends,
  client,
  cwd,
  disabled,
  onActiveTabChange,
  onAgentSurfaceChanged,
  onCancelBackendEdit,
  onChangeBackendDraft,
  onCopyText,
  onDeleteBackend,
  onDoctorBackend,
  onEditBackend,
  onNewBackend,
  onSaveBackendDraft,
  onSetBackendEnabled,
  onSetBackendEntrypoints,
  scope
}: {
  activeTab: CapabilityTab;
  backendDraft: BackendDraft | null;
  backendDoctor: Record<string, WorkbenchBackendDoctor>;
  backends: WorkbenchBackend[];
  client: GatewayClient | null;
  cwd: string;
  disabled: boolean;
  onActiveTabChange(value: CapabilityTab): void;
  onAgentSurfaceChanged?: (() => Promise<void> | void) | undefined;
  onCancelBackendEdit(): void;
  onChangeBackendDraft(draft: BackendDraft): void;
  onCopyText?: ((text: string) => void | Promise<void>) | undefined;
  onDeleteBackend(backend: WorkbenchBackend): void;
  onDoctorBackend(backend: WorkbenchBackend): void;
  onEditBackend(backend: WorkbenchBackend): void;
  onNewBackend(): void;
  onOpenSession(threadId: string, readOnly?: boolean): void;
  onSaveBackendDraft(draft: BackendDraft): void;
  onSetBackendEnabled(backend: WorkbenchBackend, enabled: boolean): void;
  onSetBackendEntrypoints(backend: WorkbenchBackend, entrypoints: string[]): void;
  scope: GatewayRequestScope | null;
}) {
  const [query, setQuery] = useState("");
  const [data, setData] = useState<Record<CapabilityTab, JsonObject | null>>({
    agents: null,
    skills: null,
    plugins: null,
    mcp: null,
    tools: null
  });
  const [selected, setSelected] = useState<Record<CapabilityTab, string | null>>({
    agents: null,
    skills: null,
    plugins: null,
    mcp: null,
    tools: null
  });
  const [loading, setLoading] = useState(false);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [refreshToken, setRefreshToken] = useState(0);
  const [createPanel, setCreatePanel] = useState<CapabilityTab | null>(null);
  const [skillInstall, setSkillInstall] = useState<SkillInstallDraft>({ source: "", name: "", target: "profile", force: false });
  const [pluginInstall, setPluginInstall] = useState<PluginInstallDraft>({ source: "", kind: "local", npmVersion: "", npmRegistry: "", adapterMode: "manifest_only", force: false, inspection: null });
  const [toolDraft, setToolDraft] = useState({ name: "", description: "", tools: "", includes: "", force: false });
  const [mcpDraft, setMcpDraft] = useState({
    name: "",
    transport: "stdio",
    command: "",
    url: "",
    bearerTokenEnvVar: "",
    oauthClientId: ""
  });
  const [toolPolicyDraft, setToolPolicyDraft] = useState({ enabledTools: "", disabledTools: "" });
  const [oauthSession, setOauthSession] = useState<string | null>(null);
  const [pluginConnectSession, setPluginConnectSession] = useState<string | null>(null);
  const [pluginDetail, setPluginDetail] = useState<{ id: string; loading: boolean; value: JsonObject | null; error: string | null } | null>(null);
  const [pluginOperation, setPluginOperation] = useState<JsonObject | null>(null);

  const requestScope = scope ?? (cwd ? { cwd, source: { kind: "web", rawId: null, lifetime: "persistent", rawIdentity: null, visibleName: null } } as GatewayRequestScope : null);

  useEffect(() => {
    if (!client || !requestScope) return;
    const activeClient = client;
    const activeScope = requestScope;
    let cancelled = false;
    async function load() {
      setLoading(true);
      setError(null);
      try {
        const result = await requestTab(activeClient, activeTab, activeScope);
        if (cancelled) return;
        setData((current) => ({ ...current, [activeTab]: objectValue(result) }));
      } catch (err) {
        if (!cancelled) setError(errorMessage(err));
      } finally {
        if (!cancelled) setLoading(false);
      }
    }
    void load();
    return () => {
      cancelled = true;
    };
  }, [activeTab, client, refreshToken, requestScope?.cwd]);

  useEffect(() => {
    if (!client || !requestScope || !oauthSession) return;
    let stopped = false;
    const timer = window.setInterval(() => {
      void client.request("mcp/oauth/status", { sessionId: oauthSession, scope: requestScope }).then((result) => {
        const status = stringField(result, "status");
        if (stopped || status === "pending") return;
        window.clearInterval(timer);
        setOauthSession(null);
        setNotice(status === "succeeded" ? "OAuth login saved. Changes apply to the next run/session." : stringField(result, "error") || "OAuth login failed.");
        setRefreshToken((value) => value + 1);
      }).catch((err) => {
        if (!stopped) {
          window.clearInterval(timer);
          setOauthSession(null);
          setError(errorMessage(err));
        }
      });
    }, 1200);
    return () => {
      stopped = true;
      window.clearInterval(timer);
    };
  }, [client, oauthSession, requestScope?.cwd]);

  useEffect(() => {
    if (!client || !requestScope || !pluginConnectSession) return;
    let stopped = false;
    const timer = window.setInterval(() => {
      void client.request("plugin/connect/status", { sessionId: pluginConnectSession, scope: requestScope }).then((result) => {
        const status = stringField(result, "status");
        if (stopped || status === "pending") return;
        window.clearInterval(timer);
        setPluginConnectSession(null);
        setNotice(status === "succeeded" ? "Plugin connection is ready." : stringField(result, "reason") || "Plugin connection failed.");
        setRefreshToken((value) => value + 1);
      }).catch((err) => {
        if (!stopped) {
          window.clearInterval(timer);
          setPluginConnectSession(null);
          setError(errorMessage(err));
        }
      });
    }, 1200);
    return () => {
      stopped = true;
      window.clearInterval(timer);
    };
  }, [client, pluginConnectSession, requestScope?.cwd]);

  const rows = useMemo(() => {
    const source = data[activeTab];
    const all = rowsForTab(activeTab, source);
    const needle = query.trim().toLowerCase();
    if (!needle) return all;
    return all.filter((row) => `${row.name} ${row.description}`.toLowerCase().includes(needle));
  }, [activeTab, data, query]);

  const selectedId = selected[activeTab] ?? rows[0]?.id ?? null;
  const selectedRow = rows.find((row) => row.id === selectedId) ?? rows[0] ?? null;
  const listEntries = capabilityListEntries(activeTab, rows);
  const visibleBackends = useMemo(
    () => backends.length > 0
      ? backends
      : data.agents
        ? parseBackendList(data.agents)
        : [],
    [backends, data.agents]
  );

  useEffect(() => {
    if (!client || !requestScope || activeTab !== "plugins" || !selectedRow) {
      setPluginDetail(null);
      return;
    }
    let cancelled = false;
    setPluginDetail({ id: selectedRow.id, loading: true, value: null, error: null });
    void client.request("plugin/read", { selector: pluginSelector(selectedRow), scope: requestScope }).then((value) => {
      if (!cancelled) setPluginDetail({ id: selectedRow.id, loading: false, value: objectValue(value), error: null });
    }).catch((err) => {
      if (!cancelled) setPluginDetail({ id: selectedRow.id, loading: false, value: null, error: errorMessage(err) });
    });
    return () => {
      cancelled = true;
    };
  }, [activeTab, client, refreshToken, requestScope?.cwd, selectedRow?.id]);

  async function mutate(action: () => Promise<unknown>, options: MutationOptions = {}): Promise<boolean> {
    if (!client || !requestScope) return false;
    setSaving(true);
    setError(null);
    setPluginOperation(null);
    try {
      const result = await action();
      const resultObject = objectValue(result);
      const partial = resultObject.partial === true
        || (resultObject.success === false && typeof resultObject.failedStep === "string");
      if (partial) {
        setPluginOperation(resultObject);
        setNotice(null);
      } else {
        setNotice(options.notice ?? "Saved. Changes apply to the next run/session; current sessions may differ.");
      }
      if (options.refresh !== false) setRefreshToken((value) => value + 1);
      return true;
    } catch (err) {
      setError(errorMessage(err));
      return false;
    } finally {
      setSaving(false);
    }
  }

  const busy = disabled || loading || saving || !client || !requestScope;

  return (
    <section aria-label="Capabilities" className="capabilitiesPage">
      <header className="capabilitiesHeader">
        <div>
          <h2>Capabilities</h2>
          <span>{cwd}</span>
        </div>
        <ActionButton ariaLabel="Refresh" disabled={busy} icon={<RefreshCw size={15} />} iconOnly onClick={() => setRefreshToken((value) => value + 1)} tooltip="Refresh" variant="ghost">
          Refresh
        </ActionButton>
      </header>

      <div className="capabilityTabs" role="tablist" aria-label="Capability types">
        {TABS.map((tab) => (
          <button
            aria-selected={activeTab === tab.id}
            className={activeTab === tab.id ? "is-selected" : ""}
            key={tab.id}
            onClick={() => {
              onActiveTabChange(tab.id);
              setCreatePanel(null);
              setQuery("");
            }}
            role="tab"
            type="button"
          >
            {tab.label}
          </button>
        ))}
      </div>

      {activeTab !== "agents" && (
        <div className="capabilitiesToolbar">
          <label>
            <Search size={15} />
            <input aria-label={`Search ${tabLabel(activeTab)}`} onChange={(event) => setQuery(event.target.value)} placeholder="Search" value={query} />
          </label>
          <ActionButton
            active={createPanel === activeTab}
            disabled={busy}
            icon={<Plus size={14} />}
            onClick={() => setCreatePanel((current) => current === activeTab ? null : activeTab)}
            variant={createPanel === activeTab ? "neutral" : "primary"}
          >
            {createActionLabel(activeTab)}
          </ActionButton>
        </div>
      )}

      {(error || notice || oauthSession || pluginConnectSession) && (
        <div className={`capabilityBanner ${error ? "is-error" : ""}`}>
          {error ?? (oauthSession ? "OAuth login pending" : pluginConnectSession ? "Plugin connection pending" : notice)}
        </div>
      )}

      {pluginOperation && <PluginOperationResult value={pluginOperation} />}

      {activeTab === "agents" ? (
        <AgentsCapabilityPanel
          backendDraft={backendDraft}
          backendDoctor={backendDoctor}
          backends={visibleBackends}
          busy={busy}
          client={client}
          data={data.agents}
          disabled={disabled}
          loading={loading}
          mutate={mutate}
          onAgentSurfaceChanged={onAgentSurfaceChanged}
          onCancelBackendEdit={onCancelBackendEdit}
          onChangeBackendDraft={onChangeBackendDraft}
          onCopyText={onCopyText}
          onDeleteBackend={onDeleteBackend}
          onDoctorBackend={onDoctorBackend}
          onEditBackend={onEditBackend}
          onManageBackend={(backend, action) => {
            if (!client || !requestScope) return;
            void mutate(async () => {
              const result = await manageAcpBackend(client, requestScope, backend.id, action);
              await onAgentSurfaceChanged?.();
              return result;
            }, { notice: `Managed ACP backend ${action} completed.` });
          }}
          onNewBackend={onNewBackend}
          onSaveBackendDraft={onSaveBackendDraft}
          onSetBackendEnabled={onSetBackendEnabled}
          onSetBackendEntrypoints={onSetBackendEntrypoints}
          scope={requestScope}
        />
      ) : activeTab === "skills" ? (
        <SkillsPanel
          busy={busy}
          client={client}
          data={data.skills}
          loading={loading}
          mutate={mutate}
          onCopyText={onCopyText}
          query={query}
          refreshToken={refreshToken}
          scope={requestScope}
          createOpen={createPanel === "skills"}
          selectedId={selected.skills}
          setSkillInstall={setSkillInstall}
          skillInstall={skillInstall}
          onCloseCreate={() => setCreatePanel(null)}
          onSelect={(id) => setSelected((current) => ({ ...current, skills: id }))}
        />
      ) : (
        <>
          {activeTab === "plugins" && (
            <CodexAuthorityCard
              authority={objectField(data.plugins, "codex_authority")}
              busy={busy}
              client={client}
              key={stringField(objectField(data.plugins, "codex_authority"), "resolvedBinary") || "codex"}
              mutate={mutate}
              scope={requestScope}
            />
          )}
          <CapabilityForms
            busy={busy}
            client={client}
            scope={requestScope}
            tab={activeTab}
            pluginInstall={pluginInstall}
            setPluginInstall={setPluginInstall}
            toolDraft={toolDraft}
            setToolDraft={setToolDraft}
            mcpDraft={mcpDraft}
            setMcpDraft={setMcpDraft}
            mutate={mutate}
            open={createPanel === activeTab}
            onClose={() => setCreatePanel(null)}
          />

          <div className="capabilitiesGrid">
            <div className="capabilityList" role="list">
              {loading && <div className="capabilityEmpty">Loading</div>}
              {!loading && rows.length === 0 && <div className="capabilityEmpty">No matches</div>}
              {listEntries.map((entry) => {
                if (entry.kind === "section") {
                  return (
                    <div className="capabilityRowMain" key={`section:${entry.label}`} role="heading" aria-level={3}>
                      <strong>{entry.label}</strong>
                      {entry.description && <span>{entry.description}</span>}
                    </div>
                  );
                }
                const row = entry.row;
                const selectedClass = row.id === selectedRow?.id ? " is-selected" : "";
                if (hasCapabilityRowSwitch(activeTab)) {
                  return (
                    <div className={`capabilityRow capabilityRowWithSwitch${selectedClass}`} key={row.id} role="listitem">
                      <button
                        aria-label={`${rowKindLabel(activeTab)} ${row.name}`}
                        className="capabilityRowSelect"
                        onClick={() => setSelected((current) => ({ ...current, [activeTab]: row.id }))}
                        type="button"
                      >
                        <span className="capabilityRowMain">
                          <strong>{row.name}</strong>
                          <RowDescription fallback={row.status} value={row.description} />
                        </span>
                        <CapabilityBadges row={row} />
                      </button>
                      <Switch
                        ariaLabel={row.enabled ? `Disable ${row.name}` : `Enable ${row.name}`}
                        checked={row.enabled}
                        className="capabilityRowSwitch"
                        disabled={busy || (activeTab === "plugins" && !pluginEnablementMutable(row))}
                        label={row.enabled ? "Enabled" : "Disabled"}
                        onCheckedChange={(enabled) => void mutate(() => setCapabilityEnabled(client, requestScope, activeTab, row, enabled))}
                        showLabel={false}
                        size="compact"
                      />
                    </div>
                  );
                }
                return (
                  <button
                    className={`capabilityRow${selectedClass}`}
                    key={row.id}
                    onClick={() => setSelected((current) => ({ ...current, [activeTab]: row.id }))}
                    type="button"
                  >
                    <span className="capabilityRowMain">
                      <strong>{row.name}</strong>
                      <RowDescription fallback={row.status} value={row.description} />
                    </span>
                    <span className="capabilityRowMeta">
                      <span className={row.enabled ? "capabilityChip is-on" : "capabilityChip"}>{row.enabled ? "On" : "Off"}</span>
                      {row.badges.slice(0, 2).map((badge) => <span className="capabilityChip" key={badge}>{badge}</span>)}
                    </span>
                  </button>
                );
              })}
            </div>

            <aside className="capabilityDetail" aria-label={`${tabLabel(activeTab)} detail`}>
              {selectedRow ? (
                <>
                  <div className="capabilityDetailHeader">
                    <div>
                      <h3>{selectedRow.name}</h3>
                      <span>{selectedRow.status}</span>
                    </div>
                  </div>
                  <CapabilityActions
                    busy={busy}
                    client={client}
                    row={selectedRow}
                    scope={requestScope}
                    tab={activeTab}
                    toolPolicyDraft={toolPolicyDraft}
                    setToolPolicyDraft={setToolPolicyDraft}
                    mutate={mutate}
                    onOAuthSession={setOauthSession}
                    onPluginConnectSession={setPluginConnectSession}
                    pluginDetail={pluginDetail?.id === selectedRow.id ? pluginDetail.value : null}
                  />
                  {activeTab === "plugins" && (
                    <PluginComponentStatuses
                      detail={pluginDetail?.id === selectedRow.id ? pluginDetail : null}
                      row={selectedRow}
                    />
                  )}
                  <KeyValueView value={selectedRow.raw} />
                </>
              ) : (
                <div className="capabilityEmpty">Select an item</div>
              )}
            </aside>
          </div>
        </>
      )}
    </section>
  );
}

function AgentsCapabilityPanel({
  backendDraft,
  backendDoctor,
  backends,
  busy,
  client,
  data,
  disabled,
  loading,
  mutate,
  onAgentSurfaceChanged,
  onCancelBackendEdit,
  onChangeBackendDraft,
  onCopyText,
  onDeleteBackend,
  onDoctorBackend,
  onEditBackend,
  onManageBackend,
  onNewBackend,
  onSaveBackendDraft,
  onSetBackendEnabled,
  onSetBackendEntrypoints,
  scope
}: {
  backendDraft: BackendDraft | null;
  backendDoctor: Record<string, WorkbenchBackendDoctor>;
  backends: WorkbenchBackend[];
  busy: boolean;
  client: GatewayClient | null;
  data: JsonObject | null;
  disabled: boolean;
  loading: boolean;
  mutate(action: () => Promise<unknown>, options?: MutationOptions): Promise<boolean>;
  onAgentSurfaceChanged?: (() => Promise<void> | void) | undefined;
  onCancelBackendEdit(): void;
  onChangeBackendDraft(draft: BackendDraft): void;
  onCopyText?: ((text: string) => void | Promise<void>) | undefined;
  onDeleteBackend(backend: WorkbenchBackend): void;
  onDoctorBackend(backend: WorkbenchBackend): void;
  onEditBackend(backend: WorkbenchBackend): void;
  onManageBackend(backend: WorkbenchBackend, action: ManagedBackendAction): void;
  onNewBackend(): void;
  onSaveBackendDraft(draft: BackendDraft): void;
  onSetBackendEnabled(backend: WorkbenchBackend, enabled: boolean): void;
  onSetBackendEntrypoints(backend: WorkbenchBackend, entrypoints: string[]): void;
  scope: GatewayRequestScope | null;
}) {
  const [segment, setSegment] = useState<AgentsSegment>("definitions");
  const [query, setQuery] = useState("");
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [selectedTeamId, setSelectedTeamId] = useState<string | null>(null);
  const [draft, setDraft] = useState<AgentDraft | null>(null);
  const [teamDraft, setTeamDraft] = useState<TeamDraft | null>(null);
  const [editing, setEditing] = useState<AgentDefinitionRow | null>(null);
  const [editingTeam, setEditingTeam] = useState<AgentTeamRow | null>(null);
  const [detail, setDetail] = useState<AgentDetailState | null>(null);
  const [teamDetail, setTeamDetail] = useState<TeamDetailState | null>(null);
  const [pendingSelection, setPendingSelection] = useState<{ name: string; target: BackendConfigTarget } | null>(null);
  const [pendingTeamSelection, setPendingTeamSelection] = useState<{ name: string; target: BackendConfigTarget } | null>(null);
  const [panelError, setPanelError] = useState<string | null>(null);

  const allRows = useMemo(() => agentRowsFromData(data), [data]);
  const allTeamRows = useMemo(() => agentTeamRowsFromData(data), [data]);
  const runtimeRows = useMemo(() => runtimeProfileRowsFromData(data), [data]);
  const runtimeReadiness = useMemo(
    () => Object.fromEntries(
      runtimeRows
        .filter((row) => row.backendRef)
        .map((row) => [row.backendRef, row.healthStatus])
    ),
    [runtimeRows]
  );
  const rows = useMemo(() => {
    const needle = query.trim().toLowerCase();
    return allRows.filter((row) => !needle || `${row.name} ${row.description} ${row.sourceLabel}`.toLowerCase().includes(needle));
  }, [allRows, query]);
  const teamRows = useMemo(() => {
    const needle = query.trim().toLowerCase();
    return allTeamRows.filter((row) => !needle || `${row.name} ${row.description} ${row.sourceLabel} ${row.leader}`.toLowerCase().includes(needle));
  }, [allTeamRows, query]);
  const filteredRuntimeRows = useMemo(() => {
    const needle = query.trim().toLowerCase();
    return runtimeRows.filter((row) => !needle || `${row.id} ${row.label} ${row.runtime}`.toLowerCase().includes(needle));
  }, [runtimeRows, query]);
  const selected = rows.find((row) => row.id === selectedId) ?? rows[0] ?? null;
  const selectedTeam = teamRows.find((row) => row.id === selectedTeamId) ?? teamRows[0] ?? null;
  const selectedRuntime = filteredRuntimeRows.find((row) => row.id === selectedId) ?? filteredRuntimeRows[0] ?? null;
  const selectedDetail = selected && detail?.id === selected.id ? detail : null;
  const selectedTeamDetail = selectedTeam && teamDetail?.id === selectedTeam.id ? teamDetail : null;

  useEffect(() => {
    if (!pendingSelection) return;
    const next = allRows.find((row) => row.name === pendingSelection.name && row.target === pendingSelection.target);
    if (!next) return;
    setSelectedId(next.id);
    setPendingSelection(null);
  }, [allRows, pendingSelection]);

  useEffect(() => {
    if (!pendingTeamSelection) return;
    const next = allTeamRows.find((row) => row.name === pendingTeamSelection.name && row.target === pendingTeamSelection.target);
    if (!next) return;
    setSelectedTeamId(next.id);
    setPendingTeamSelection(null);
  }, [allTeamRows, pendingTeamSelection]);

  useEffect(() => {
    if (!client || !scope || !selected || !selected.target || draft) {
      if (!selected || draft) setDetail(null);
      return;
    }
    let cancelled = false;
    setDetail({
      id: selected.id,
      loading: true,
      value: null,
      instructions: "",
      rawMarkdown: "",
      error: null
    });
    void client.request("agent/read", { name: selected.name, target: selected.target, scope }).then((value) => {
      const result = objectValue(value);
      if (cancelled) return;
      setDetail({
        id: selected.id,
        loading: false,
        value: objectField(result, "agent"),
        instructions: stringField(result, "instructions"),
        rawMarkdown: stringField(result, "rawMarkdown"),
        error: null
      });
    }).catch((error) => {
      if (!cancelled) {
        setDetail({
          id: selected.id,
          loading: false,
          value: null,
          instructions: "",
          rawMarkdown: "",
          error: errorMessage(error)
        });
      }
    });
    return () => {
      cancelled = true;
    };
  }, [client, draft, scope?.cwd, selected?.id, selected?.target]);

  useEffect(() => {
    if (!client || !scope || segment !== "teams" || !selectedTeam || !selectedTeam.target || teamDraft) {
      if (!selectedTeam || teamDraft) setTeamDetail(null);
      return;
    }
    let cancelled = false;
    setTeamDetail({
      id: selectedTeam.id,
      loading: true,
      value: null,
      instructions: "",
      rawMarkdown: "",
      error: null
    });
    void client.request("team/read", { name: selectedTeam.name, target: selectedTeam.target, scope }).then((value) => {
      const result = objectValue(value);
      if (cancelled) return;
      setTeamDetail({
        id: selectedTeam.id,
        loading: false,
        value: objectField(result, "team"),
        instructions: stringField(result, "instructions"),
        rawMarkdown: stringField(result, "rawMarkdown"),
        error: null
      });
    }).catch((error) => {
      if (!cancelled) {
        setTeamDetail({
          id: selectedTeam.id,
          loading: false,
          value: null,
          instructions: "",
          rawMarkdown: "",
          error: errorMessage(error)
        });
      }
    });
    return () => {
      cancelled = true;
    };
  }, [client, scope?.cwd, segment, selectedTeam?.id, selectedTeam?.target, teamDraft]);

  if (!client || !scope) {
    return <div className="capabilityEmpty">Gateway unavailable</div>;
  }

  function openCreate() {
    setEditing(null);
    setPanelError(null);
    setSelectedId(null);
    setDraft(emptyAgentDraft());
  }

  function openCreateTeam() {
    setEditingTeam(null);
    setPanelError(null);
    setSelectedTeamId(null);
    setTeamDraft(emptyTeamDraft());
  }

  function closeDraft() {
    setDraft(null);
    setEditing(null);
    setPanelError(null);
  }

  function closeTeamDraft() {
    setTeamDraft(null);
    setEditingTeam(null);
    setPanelError(null);
  }

  async function openEdit(row: AgentDefinitionRow) {
    if (!row.target) return;
    setPanelError(null);
    try {
      const result = detail?.id === row.id && detail.value
        ? {
            agent: detail.value,
            instructions: detail.instructions,
            rawMarkdown: detail.rawMarkdown
          }
        : objectValue(await client!.request("agent/read", { name: row.name, target: row.target, scope }));
      const agent = objectField(result, "agent");
      setEditing(row);
      setDraft(agentDraftFromRead(row, agent, stringField(result, "instructions"), stringField(result, "rawMarkdown")));
    } catch (error) {
      setPanelError(errorMessage(error));
    }
  }

  async function openEditTeam(row: AgentTeamRow) {
    if (!row.target) return;
    setPanelError(null);
    try {
      const result = teamDetail?.id === row.id && teamDetail.value
        ? {
            team: teamDetail.value,
            instructions: teamDetail.instructions,
            rawMarkdown: teamDetail.rawMarkdown
          }
        : objectValue(await client!.request("team/read", { name: row.name, target: row.target, scope }));
      const team = objectField(result, "team");
      setEditingTeam(row);
      setTeamDraft(teamDraftFromRead(row, team, stringField(result, "instructions"), stringField(result, "rawMarkdown")));
    } catch (error) {
      setPanelError(errorMessage(error));
    }
  }

  async function saveDraft(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (!draft) return;
    if (!client) return;
    const activeClient = client;
    const name = draft.name.trim();
    if (!name) return;
    const ok = await mutate(async () => {
      const result = await activeClient.request("agent/write", {
        name,
        description: draft.description.trim(),
        target: draft.target,
        enabled: draft.enabled,
        instructions: draft.instructions,
        backend: draft.backendRef.trim() ? { ref: draft.backendRef.trim() } : null,
        entrypoints: splitList(draft.entrypointsText) ?? [],
        tools: splitList(draft.toolsText) ?? [],
        mcpServers: splitList(draft.mcpServersText) ?? [],
        rawMarkdown: draft.mode === "markdown" ? draft.rawMarkdown : null,
        scope
      });
      await onAgentSurfaceChanged?.();
      return result;
    }, { notice: "Agent saved." });
    if (ok) {
      setPendingSelection({ name, target: draft.target });
      closeDraft();
    }
  }

  async function saveTeamDraft(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (!teamDraft || !client) return;
    const name = teamDraft.name.trim();
    if (!name) return;
    const members = parseTeamMembersText(teamDraft.membersText);
    if (teamDraft.mode === "form" && members.length === 0) {
      setPanelError("Team requires at least one member.");
      return;
    }
    const ok = await mutate(async () => client.request("team/write", {
      name,
      description: teamDraft.description.trim(),
      target: teamDraft.target,
      enabled: teamDraft.enabled,
      leader: teamDraft.leader.trim(),
      members,
      maxParallelAgents: Number(teamDraft.maxParallelAgents) || null,
      instructions: teamDraft.instructions,
      rawMarkdown: teamDraft.mode === "markdown" ? teamDraft.rawMarkdown : null,
      scope
    }), { notice: "Team saved." });
    if (ok) {
      setPendingTeamSelection({ name, target: teamDraft.target });
      closeTeamDraft();
    }
  }

  function setDraftMode(mode: AgentDraft["mode"]) {
    if (!draft) return;
    setDraft({
      ...draft,
      mode,
      rawMarkdown: draft.rawMarkdown.trim() ? draft.rawMarkdown : renderAgentDraftMarkdown(draft)
    });
  }

  function setTeamDraftMode(mode: TeamDraft["mode"]) {
    if (!teamDraft) return;
    setTeamDraft({
      ...teamDraft,
      mode,
      rawMarkdown: teamDraft.rawMarkdown.trim() ? teamDraft.rawMarkdown : renderTeamDraftMarkdown(teamDraft)
    });
  }

  return (
    <div className="agentsCapability">
      <div className="agentCapabilitySegments" role="tablist" aria-label="Agent management">
        <button className={segment === "definitions" ? "is-selected" : ""} onClick={() => setSegment("definitions")} role="tab" aria-selected={segment === "definitions"} type="button">
          Definitions
        </button>
        <button className={segment === "teams" ? "is-selected" : ""} onClick={() => setSegment("teams")} role="tab" aria-selected={segment === "teams"} type="button">
          Teams
        </button>
        <button className={segment === "runtimes" ? "is-selected" : ""} onClick={() => setSegment("runtimes")} role="tab" aria-selected={segment === "runtimes"} type="button">
          Runtime Profiles
        </button>
        <button className={segment === "backends" ? "is-selected" : ""} onClick={() => setSegment("backends")} role="tab" aria-selected={segment === "backends"} type="button">
          ACP Backends
        </button>
      </div>

      {segment === "backends" ? (
        <AgentsConfigPanel
          backendDraft={backendDraft}
          backendDoctor={backendDoctor}
          backends={backends}
          disabled={disabled}
          runtimeReadiness={runtimeReadiness}
          onCancelBackendEdit={onCancelBackendEdit}
          onChangeBackendDraft={onChangeBackendDraft}
          onDeleteBackend={onDeleteBackend}
          onDoctorBackend={onDoctorBackend}
          onEditBackend={onEditBackend}
          onManageBackend={onManageBackend}
          onNewBackend={onNewBackend}
          onSaveBackendDraft={onSaveBackendDraft}
          onSetBackendEnabled={onSetBackendEnabled}
          onSetBackendEntrypoints={onSetBackendEntrypoints}
        />
      ) : segment === "runtimes" ? (
        <RuntimeProfilesPanel
          backendDoctor={backendDoctor}
          backends={backends}
          busy={busy}
          client={client}
          loading={loading}
          mutate={mutate}
          query={query}
          rows={filteredRuntimeRows}
          scope={scope}
          selected={selectedRuntime}
          onDoctorBackend={onDoctorBackend}
          onQueryChange={setQuery}
          onSelect={setSelectedId}
        />
      ) : segment === "teams" ? (
        <>
          <div className="capabilitiesToolbar agentDefinitionsToolbar">
            <label>
              <Search size={15} />
              <input aria-label="Search Teams" onChange={(event) => setQuery(event.target.value)} placeholder="Search" value={query} />
            </label>
            <ActionButton disabled={busy} icon={<Plus size={14} />} onClick={openCreateTeam} variant="primary">
              Create team
            </ActionButton>
          </div>

          {panelError && <div className="capabilityBanner is-error">{panelError}</div>}

          <div className="capabilitiesGrid agentsDefinitionsGrid">
            <div className="capabilityList" role="list">
              {loading && <div className="capabilityEmpty">Loading</div>}
              {!loading && teamRows.length === 0 && <div className="capabilityEmpty">No teams</div>}
              {teamRows.map((row) => (
                <div className={row.id === selectedTeam?.id ? "capabilityRow capabilityRowWithSwitch agentDefinitionRow is-selected" : "capabilityRow capabilityRowWithSwitch agentDefinitionRow"} key={row.id} role="listitem">
                  <button aria-label={`Team ${row.name}`} className="capabilityRowSelect" onClick={() => setSelectedTeamId(row.id)} type="button">
                    <span className="capabilityRowMain">
                      <strong>{row.name}</strong>
                      <RowDescription fallback={teamStateLabel(row.state)} value={row.description} />
                      <span className="skillRowMetadata">{teamRowMetadata(row)}</span>
                    </span>
                    <CapabilityBadges row={{
                      ...row,
                      badges: [targetLabel(row.target), teamStateLabel(row.state)].filter(Boolean),
                      status: teamStateLabel(row.state)
                    }} />
                  </button>
                  <Switch
                    ariaLabel={row.enabled ? `Disable ${row.name}` : `Enable ${row.name}`}
                    checked={row.enabled}
                    className="capabilityRowSwitch"
                    disabled={busy || !row.mutable || !row.target}
                    label={row.enabled ? "Enabled" : "Disabled"}
                    onCheckedChange={(enabled) => {
                      if (!row.target) return;
                      void mutate(() => client.request("team/setEnabled", { name: row.name, target: row.target, enabled, scope }), { notice: enabled ? "Team enabled." : "Team disabled." });
                    }}
                    showLabel={false}
                    size="compact"
                  />
                </div>
              ))}
            </div>

            <aside className="capabilityDetail agentDefinitionDetail" aria-label="Team definition detail">
              {teamDraft ? (
                <>
                  <div className="capabilityDetailHeader">
                    <div>
                      <h3>{editingTeam ? editingTeam.name : "Create team"}</h3>
                      <span>{editingTeam ? [targetLabel(editingTeam.target), editingTeam.sourceLabel].filter(Boolean).join(" · ") : "Project/Profile Markdown definition"}</span>
                    </div>
                  </div>
                  <TeamDefinitionEditorForm
                    busy={busy}
                    draft={teamDraft}
                    editing={Boolean(editingTeam)}
                    onCancel={closeTeamDraft}
                    onChange={setTeamDraft}
                    onModeChange={setTeamDraftMode}
                    onSubmit={saveTeamDraft}
                  />
                </>
              ) : selectedTeam ? (
                <>
                  <div className="capabilityDetailHeader">
                    <div>
                      <h3>{selectedTeam.name}</h3>
                      <span>{[targetLabel(selectedTeam.target), selectedTeam.sourceLabel, teamStateLabel(selectedTeam.state)].filter(Boolean).join(" · ")}</span>
                    </div>
                    <div className="capabilityDetailHeaderActions">
                      <button
                        disabled={busy || !selectedTeam.mutable || !selectedTeam.target}
                        onClick={() => {
                          if (!selectedTeam.target || !confirmAction(`Delete team ${selectedTeam.name}?`)) return;
                          void mutate(() => client.request("team/delete", { name: selectedTeam.name, target: selectedTeam.target, scope }), { notice: "Team deleted." });
                        }}
                        title={selectedTeam.mutable && selectedTeam.target ? "Delete" : "Only mutable Project/Profile teams can be deleted here"}
                        type="button"
                      >
                        <Trash2 size={14} /> Delete
                      </button>
                    </div>
                  </div>
                  <TeamDefinitionFields row={selectedTeam} />
                  <MarkdownDefinitionPreview
                    copyLabel="Copy team Markdown"
                    editDisabled={busy || !selectedTeam.mutable || !selectedTeam.target || selectedTeamDetail?.loading === true}
                    editDisabledReason={selectedTeam.mutable && selectedTeam.target ? "Team Markdown is still loading" : "Only mutable Project/Profile teams can be edited here"}
                    editLabel={`Edit ${selectedTeam.name} Markdown`}
                    label="Team Markdown preview"
                    loading={selectedTeamDetail?.loading}
                    onCopyText={onCopyText}
                    onEdit={() => void openEditTeam(selectedTeam)}
                    preview={selectedTeamDetail?.rawMarkdown ?? ""}
                    error={selectedTeamDetail?.error}
                  />
                </>
              ) : (
                <div className="capabilityEmpty">Select a team</div>
              )}
            </aside>
          </div>
        </>
      ) : (
        <>
          <div className="capabilitiesToolbar agentDefinitionsToolbar">
            <label>
              <Search size={15} />
              <input aria-label="Search Agents" onChange={(event) => setQuery(event.target.value)} placeholder="Search" value={query} />
            </label>
            <ActionButton disabled={busy} icon={<Plus size={14} />} onClick={openCreate} variant="primary">
              Create agent
            </ActionButton>
          </div>

          {panelError && <div className="capabilityBanner is-error">{panelError}</div>}

          <div className="capabilitiesGrid agentsDefinitionsGrid">
            <div className="capabilityList" role="list">
              {loading && <div className="capabilityEmpty">Loading</div>}
              {!loading && rows.length === 0 && <div className="capabilityEmpty">No agent definitions</div>}
              {rows.map((row) => (
                <div className={row.id === selected?.id ? "capabilityRow capabilityRowWithSwitch agentDefinitionRow is-selected" : "capabilityRow capabilityRowWithSwitch agentDefinitionRow"} key={row.id} role="listitem">
                  <button aria-label={`Agent ${row.name}`} className="capabilityRowSelect" onClick={() => setSelectedId(row.id)} type="button">
                    <span className="capabilityRowMain">
                      <strong>{row.name}</strong>
                      <RowDescription fallback={agentStateLabel(row.state)} value={row.description} />
                      <span className="skillRowMetadata">{agentRowMetadata(row)}</span>
                    </span>
                    <CapabilityBadges row={{
                      ...row,
                      badges: [targetLabel(row.target), agentStateLabel(row.state)].filter(Boolean),
                      status: agentStateLabel(row.state)
                    }} />
                  </button>
                  <Switch
                    ariaLabel={row.enabled ? `Disable ${row.name}` : `Enable ${row.name}`}
                    checked={row.enabled}
                    className="capabilityRowSwitch"
                    disabled={busy || !row.mutable || !row.target}
                    label={row.enabled ? "Enabled" : "Disabled"}
                    onCheckedChange={(enabled) => {
                      if (!row.target) return;
                      void mutate(async () => {
                        const result = await client.request("agent/setEnabled", { name: row.name, target: row.target, enabled, scope });
                        await onAgentSurfaceChanged?.();
                        return result;
                      }, { notice: enabled ? "Agent enabled." : "Agent disabled." });
                    }}
                    showLabel={false}
                    size="compact"
                  />
                </div>
              ))}
            </div>

            <aside className="capabilityDetail agentDefinitionDetail" aria-label="Agent definition detail">
              {draft ? (
                <>
                  <div className="capabilityDetailHeader">
                    <div>
                      <h3>{editing ? editing.name : "Create agent"}</h3>
                      <span>{editing ? [targetLabel(editing.target), editing.sourceLabel].filter(Boolean).join(" · ") : "Project/Profile Markdown definition"}</span>
                    </div>
                  </div>
                  <AgentDefinitionEditorForm
                    busy={busy}
                    draft={draft}
                    editing={Boolean(editing)}
                    onCancel={closeDraft}
                    onChange={setDraft}
                    onModeChange={setDraftMode}
                    onSubmit={saveDraft}
                  />
                </>
              ) : selected ? (
                <>
                  <div className="capabilityDetailHeader">
                    <div>
                      <h3>{selected.name}</h3>
                      <span>{[targetLabel(selected.target), selected.sourceLabel, agentStateLabel(selected.state)].filter(Boolean).join(" · ")}</span>
                    </div>
                    <div className="capabilityDetailHeaderActions">
                      <button
                        disabled={busy || !selected.mutable || !selected.target}
                        onClick={() => {
                          if (!selected.target || !confirmAction(`Delete agent ${selected.name}?`)) return;
                          void mutate(async () => {
                            const result = await client.request("agent/delete", { name: selected.name, target: selected.target, scope });
                            await onAgentSurfaceChanged?.();
                            return result;
                          }, { notice: "Agent deleted." });
                        }}
                        title={selected.mutable && selected.target ? "Delete" : "Only mutable Project/Profile agents can be deleted here"}
                        type="button"
                      >
                        <Trash2 size={14} /> Delete
                      </button>
                    </div>
                  </div>
                  <AgentDefinitionFields row={selected} />
                  <MarkdownDefinitionPreview
                    copyLabel="Copy agent Markdown"
                    editDisabled={busy || !selected.mutable || !selected.target || selectedDetail?.loading === true}
                    editDisabledReason={selected.mutable && selected.target ? "Agent Markdown is still loading" : "Only mutable Project/Profile agents can be edited here"}
                    editLabel={`Edit ${selected.name} Markdown`}
                    label="Agent Markdown preview"
                    loading={selectedDetail?.loading}
                    onCopyText={onCopyText}
                    onEdit={() => void openEdit(selected)}
                    preview={selectedDetail?.rawMarkdown ?? ""}
                    error={selectedDetail?.error}
                  />
                </>
              ) : (
                <div className="capabilityEmpty">Select an agent</div>
              )}
            </aside>
          </div>
        </>
      )}
    </div>
  );
}

function RuntimeProfilesPanel({
  backendDoctor,
  backends,
  busy,
  client,
  loading,
  mutate,
  query,
  rows,
  scope,
  selected,
  onDoctorBackend,
  onQueryChange,
  onSelect
}: {
  backendDoctor: Record<string, WorkbenchBackendDoctor>;
  backends: WorkbenchBackend[];
  busy: boolean;
  client: GatewayClient;
  loading: boolean;
  mutate(action: () => Promise<unknown>, options?: MutationOptions): Promise<boolean>;
  query: string;
  rows: RuntimeProfileRow[];
  scope: GatewayRequestScope;
  selected: RuntimeProfileRow | null;
  onDoctorBackend(backend: WorkbenchBackend): void;
  onQueryChange(value: string): void;
  onSelect(id: string | null): void;
}) {
  const [draft, setDraft] = useState<RuntimeProfileDraft | null>(null);
  const [editing, setEditing] = useState<RuntimeProfileRow | null>(null);
  const [editorError, setEditorError] = useState<string | null>(null);
  const [editorLoadingId, setEditorLoadingId] = useState<string | null>(null);

  function openCreate() {
    setEditing(null);
    setEditorError(null);
    setDraft(emptyRuntimeProfileDraft());
  }

  function closeEditor() {
    setDraft(null);
    setEditing(null);
    setEditorError(null);
  }

  async function openEdit(row: RuntimeProfileRow) {
    if (!runtimeProfileCanCustomize(row)) return;
    setEditorError(null);
    setEditorLoadingId(row.id);
    try {
      const result = await client.request("runtime/profile/read", { id: row.id, scope });
      setEditing(row);
      setDraft(runtimeProfileDraftFromRead(result.profile, result.options));
    } catch (error) {
      setEditorError(errorMessage(error));
    } finally {
      setEditorLoadingId(null);
    }
  }

  async function saveRuntimeProfile(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (!draft) return;
    const id = draft.id.trim();
    if (!id) return;
    if (!editing && rows.some((row) => row.id === id)) {
      setEditorError(`Runtime Profile ${id} already exists. Select it and use Customize or Edit.`);
      return;
    }
    if (draft.runtime === "acp" && !draft.backendRef.trim()) {
      setEditorError("ACP Runtime Profiles require an ACP backend ref.");
      return;
    }
    const options = parseRuntimeOptions(draft.optionsText);
    if (typeof options === "string") {
      setEditorError(options);
      return;
    }
    setEditorError(null);
    const ok = await mutate(() => client.request("runtime/profile/write", {
      id,
      target: draft.target,
      runtime: draft.runtime,
      enabled: draft.enabled,
      label: nullableText(draft.label),
      backendRef: draft.runtime === "acp" ? nullableText(draft.backendRef) : null,
      defaultModel: nullableText(draft.defaultModel),
      defaultMode: nullableText(draft.defaultMode),
      defaultAgent: nullableText(draft.defaultAgent),
      approvalMode: nullableText(draft.approvalMode),
      sandbox: nullableText(draft.sandbox),
      workspaceRoots: splitLines(draft.workspaceRootsText),
      options,
      scope
    }), { notice: editing ? "Runtime profile saved." : "Runtime profile created." });
    if (ok) {
      closeEditor();
      onSelect(id);
    }
  }

  async function deleteRuntimeProfile(row: RuntimeProfileRow) {
    const target = runtimeProfileMutableTarget(row);
    if (!target) return;
    const source = targetLabel(target);
    const fallback = runtimeProfileHasFallback(row)
      ? " The next lower-precedence source or generated profile will remain."
      : "";
    if (!confirmAction(`Delete the ${source} configuration for ${row.label || row.id}?${fallback}`)) return;
    const ok = await mutate(
      () => client.request("runtime/profile/delete", { id: row.id, target, scope }),
      { notice: `${source} Runtime Profile configuration deleted.` }
    );
    if (ok) {
      closeEditor();
      onSelect(null);
    }
  }

  const selectedBackend = selected?.backendRef
    ? backends.find((backend) => backend.id === selected.backendRef) ?? null
    : null;
  const selectedBackendDoctor = selectedBackend ? backendDoctor[selectedBackend.id] ?? null : null;
  return (
    <>
      <div className="capabilitiesToolbar agentDefinitionsToolbar">
        <label>
          <Search size={15} />
          <input aria-label="Search Runtime Profiles" onChange={(event) => onQueryChange(event.target.value)} placeholder="Search" value={query} />
        </label>
        <span className="capabilityToolbarHint">{rows.length} profiles</span>
        <ActionButton disabled={busy} icon={<Plus size={14} />} onClick={openCreate} variant="primary">
          Create profile
        </ActionButton>
      </div>

      {editorError && <div className="capabilityBanner is-error">{editorError}</div>}

      <div className="capabilitiesGrid agentsDefinitionsGrid">
        <div className="capabilityList" role="list">
          {loading && <div className="capabilityEmpty">Loading</div>}
          {!loading && rows.length === 0 && <div className="capabilityEmpty">No runtime profiles</div>}
          {rows.map((row) => (
            <div className={row.id === selected?.id ? "capabilityRow capabilityRowWithSwitch agentDefinitionRow is-selected" : "capabilityRow capabilityRowWithSwitch agentDefinitionRow"} key={row.id} role="listitem">
              <button aria-label={`Runtime Profile ${row.id}`} className="capabilityRowSelect" onClick={() => onSelect(row.id)} type="button">
                <span className="capabilityRowMain">
                  <strong>{row.label || row.id}</strong>
                  <RowDescription fallback={row.healthSummary} value={`${row.runtime} runtime`} />
                  <span className="skillRowMetadata">{runtimeProfileMetadata(row)}</span>
                </span>
                <CapabilityBadges row={{
                  id: row.id,
                  name: row.label || row.id,
                  description: row.runtime,
                  enabled: row.enabled,
                  status: row.healthStatus,
                  badges: [row.provenance, runtimeProfileSource(row), runtimeReadinessLabel(row.healthStatus)],
                  raw: row.raw
                }} />
              </button>
              <Switch
                ariaLabel={row.enabled ? `Disable ${row.id}` : `Enable ${row.id}`}
                checked={row.enabled}
                className="capabilityRowSwitch"
                disabled={busy || !runtimeProfileCanCustomize(row)}
                label={row.enabled ? "Enabled" : "Disabled"}
                onCheckedChange={(enabled) => {
                  void mutate(() => client.request("runtime/profile/setEnabled", {
                    id: row.id,
                    target: runtimeProfileMutableTarget(row) ?? "profile",
                    enabled,
                    scope
                  }), { notice: enabled ? "Runtime profile enabled." : "Runtime profile disabled." });
                }}
                showLabel={false}
                size="compact"
              />
            </div>
          ))}
        </div>

        <aside className="capabilityDetail agentDefinitionDetail" aria-label="Runtime Profile detail">
          {draft ? (
            <>
              <div className="capabilityDetailHeader runtimeProfileDetailHeader">
                <div>
                  <h3>{editing ? (editing.generated ? `Customize ${editing.label || editing.id}` : `Edit ${editing.label || editing.id}`) : "Create Runtime Profile"}</h3>
                  <span>{editing ? `${runtimeProfileSource(editing)} · ${editing.provenance}` : "Project or Profile configuration"}</span>
                </div>
              </div>
              <RuntimeProfileEditorForm
                busy={busy}
                draft={draft}
                editing={editing}
                onCancel={closeEditor}
                onChange={setDraft}
                onSubmit={saveRuntimeProfile}
              />
            </>
          ) : selected ? (
            <>
              <div className="capabilityDetailHeader runtimeProfileDetailHeader">
                <div>
                  <h3>{selected.label || selected.id}</h3>
                  <span>{[selected.provenance, runtimeProfileSource(selected), runtimeReadinessLabel(selected.healthStatus)].filter(Boolean).join(" · ")}</span>
                </div>
                <div className="capabilityDetailHeaderActions">
                  <button
                    disabled={busy || editorLoadingId === selected.id || !runtimeProfileCanCustomize(selected)}
                    onClick={() => void openEdit(selected)}
                    title={runtimeProfileCanCustomize(selected) ? (selected.generated ? "Create a configured override" : "Edit configuration") : "Generated ACP compatibility profiles are managed from ACP Backends"}
                    type="button"
                  >
                    <Edit3 size={14} /> {selected.generated ? "Customize" : "Edit"}
                  </button>
                  <button
                    disabled={busy || !runtimeProfileMutableTarget(selected)}
                    onClick={() => void deleteRuntimeProfile(selected)}
                    title={runtimeProfileMutableTarget(selected) ? `Delete ${targetLabel(runtimeProfileMutableTarget(selected))} configuration` : "Generated profiles cannot be deleted"}
                    type="button"
                  >
                    <Trash2 size={14} /> Delete
                  </button>
                  {selected.runtime === "acp" && (
                    <button
                      disabled={busy || !selectedBackend}
                      onClick={() => selectedBackend && onDoctorBackend(selectedBackend)}
                      title={selectedBackend ? `Doctor ACP backend ${selectedBackend.id}` : `ACP backend ${selected.backendRef || "is not configured"}`}
                      type="button"
                    >
                      <Wrench size={14} /> Doctor backend
                    </button>
                  )}
                </div>
              </div>
              <dl className="capabilityKeyValues">
                <div><dt>Runtime</dt><dd>{selected.runtime} · {selected.provenance}</dd></div>
                <div><dt>Status</dt><dd>{selected.healthSummary}</dd></div>
                <div><dt>Last checked</dt><dd>{formatRuntimeCheckedAt(selected.checkedAtMs)}</dd></div>
                {selected.backendRef && <div><dt>ACP backend</dt><dd>{selected.backendRef}</dd></div>}
                <div><dt>Default model</dt><dd>{selected.defaultModel || "Runtime default"}</dd></div>
                <div><dt>Default mode</dt><dd>{selected.defaultMode || "Runtime default"}</dd></div>
                <div><dt>Default agent</dt><dd>{selected.defaultAgent || "Runtime default"}</dd></div>
                <div><dt>Safety</dt><dd>{[selected.approvalMode || "Runtime approval", selected.sandbox || "Runtime sandbox"].join(" · ")}</dd></div>
                <div><dt>Workspace roots</dt><dd>{selected.workspaceRoots.length > 0 ? selected.workspaceRoots.join(", ") : "Current workspace"}</dd></div>
                <div><dt>Source</dt><dd>{runtimeProfileSource(selected)}</dd></div>
              </dl>
              <section aria-label="Runtime readiness" className="runtimeCapabilitySection">
                <div className="runtimeCapabilitySectionHeader"><h4>Readiness</h4><span>Cached · revision {selected.capabilityRevision}</span></div>
                {selected.readinessStages.length > 0 ? (
                  <div className="runtimeReadinessStages">
                    {selected.readinessStages.map((stage) => (
                      <div className={`runtimeReadinessStage is-${stage.status}`} key={stage.id}>
                        <strong>{readinessStageLabel(stage.id)}</strong>
                        <span>{stage.summary || runtimeReadinessLabel(stage.status)}</span>
                        <small>{stage.observedAtMs == null ? "Not observed" : formatRuntimeCheckedAt(stage.observedAtMs)}</small>
                      </div>
                    ))}
                  </div>
                ) : (
                  <div className="capabilityEmpty">No staged readiness observation yet</div>
                )}
              </section>
              {selected.diagnostics.length > 0 && (
                <div className="capabilityBanner is-error">{selected.diagnostics.join(" · ")}</div>
              )}
              {selected.runtime === "acp" && selected.backendRef && !selectedBackend && (
                <div className="capabilityBanner is-error">ACP backend {selected.backendRef} is not present in the backend catalog.</div>
              )}
              {selectedBackendDoctor && (
                <section aria-label="ACP backend doctor" className="runtimeCapabilitySection">
                  <div className="runtimeCapabilitySectionHeader">
                    <h4>Backend Doctor</h4>
                    <span>{selectedBackendDoctor.ok ? "Ready" : "Needs attention"}</span>
                  </div>
                  <div className="runtimeReadinessStages">
                    {selectedBackendDoctor.checks.map((check) => (
                      <div className={`runtimeReadinessStage is-${check.ok ? "ready" : "error"}`} key={check.name}>
                        <strong>{check.name}</strong>
                        <span>{check.message}</span>
                        <small>{check.path || "No path"}</small>
                      </div>
                    ))}
                  </div>
                </section>
              )}
            </>
          ) : (
            <div className="capabilityEmpty">Select a Runtime Profile</div>
          )}
        </aside>
      </div>
    </>
  );
}

function RuntimeProfileEditorForm({
  busy,
  draft,
  editing,
  onCancel,
  onChange,
  onSubmit
}: {
  busy: boolean;
  draft: RuntimeProfileDraft;
  editing: RuntimeProfileRow | null;
  onCancel(): void;
  onChange(draft: RuntimeProfileDraft): void;
  onSubmit(event: FormEvent<HTMLFormElement>): void;
}) {
  const configuredSource = Boolean(editing && editing.sourceTargets.length > 0);
  const generatedIdentity = Boolean(editing && runtimeProfileHasReservedIdentity(editing));
  return (
    <form aria-label="Runtime Profile" className="capabilityForm agentDefinitionForm runtimeProfileForm" onSubmit={onSubmit}>
      <label>
        <span>Target</span>
        <select aria-label="Runtime Profile target" disabled={busy || configuredSource} onChange={(event) => onChange({ ...draft, target: agentTargetValue(event.target.value) })} value={draft.target}>
          <option value="project">Project</option>
          <option value="profile">Profile</option>
        </select>
        {configuredSource && <small className="runtimeProfileFieldHint">Source is fixed for this configuration.</small>}
      </label>
      <label>
        <span>Profile id</span>
        <input
          aria-label="Runtime Profile id"
          disabled={busy || Boolean(editing)}
          onChange={(event) => onChange({ ...draft, id: event.target.value })}
          pattern="[a-z0-9]+(?:-[a-z0-9]+)*"
          placeholder="review-codex"
          title="Use lowercase letters, numbers, and single hyphens"
          value={draft.id}
        />
      </label>
      <label>
        <span>Runtime</span>
        <select aria-label="Runtime Profile runtime" disabled={busy || generatedIdentity} onChange={(event) => onChange({ ...draft, runtime: runtimeProfileKind(event.target.value) })} value={draft.runtime}>
          {(editing || draft.runtime === "native") && <option value="native">Native</option>}
          <option value="acp">ACP</option>
        </select>
      </label>
      <label>
        <span>Label</span>
        <input aria-label="Runtime Profile label" disabled={busy} onChange={(event) => onChange({ ...draft, label: event.target.value })} placeholder="Review Codex" value={draft.label} />
      </label>
      <label className="agentDefinitionSwitch">
        <span>Enabled</span>
        <Switch ariaLabel="Runtime Profile enabled" checked={draft.enabled} disabled={busy} label={draft.enabled ? "Enabled" : "Disabled"} onCheckedChange={(enabled) => onChange({ ...draft, enabled })} showLabel={false} size="compact" />
      </label>
      {draft.runtime === "acp" && (
        <label>
          <span>ACP backend ref</span>
          <input aria-label="Runtime Profile ACP backend ref" disabled={busy} onChange={(event) => onChange({ ...draft, backendRef: event.target.value })} placeholder="cursor" required value={draft.backendRef} />
        </label>
      )}
      <label>
        <span>Default model</span>
        <input aria-label="Runtime Profile default model" disabled={busy} onChange={(event) => onChange({ ...draft, defaultModel: event.target.value })} placeholder="Runtime default" value={draft.defaultModel} />
      </label>
      <label>
        <span>Default mode</span>
        <input aria-label="Runtime Profile default mode" disabled={busy} onChange={(event) => onChange({ ...draft, defaultMode: event.target.value })} placeholder="Runtime default" value={draft.defaultMode} />
      </label>
      <label>
        <span>Default agent</span>
        <input aria-label="Runtime Profile default agent" disabled={busy} onChange={(event) => onChange({ ...draft, defaultAgent: event.target.value })} placeholder="Runtime default" value={draft.defaultAgent} />
      </label>
      <label>
        <span>Approval mode</span>
        <input aria-label="Runtime Profile approval mode" disabled={busy} onChange={(event) => onChange({ ...draft, approvalMode: event.target.value })} placeholder="Runtime default" value={draft.approvalMode} />
      </label>
      <label>
        <span>Sandbox</span>
        <input aria-label="Runtime Profile sandbox" disabled={busy} onChange={(event) => onChange({ ...draft, sandbox: event.target.value })} placeholder="Runtime default" value={draft.sandbox} />
      </label>
      <label className="agentWideField runtimeProfileCompactTextArea">
        <span>Workspace roots</span>
        <textarea aria-label="Runtime Profile workspace roots" disabled={busy} onChange={(event) => onChange({ ...draft, workspaceRootsText: event.target.value })} placeholder="One absolute path per line" spellCheck={false} value={draft.workspaceRootsText} />
      </label>
      <label className="agentWideField runtimeProfileOptionsField">
        <span>Advanced options (JSON)</span>
        <textarea aria-label="Runtime Profile options" disabled={busy} onChange={(event) => onChange({ ...draft, optionsText: event.target.value })} placeholder="{}" spellCheck={false} value={draft.optionsText} />
      </label>
      <div className="capabilityFormActions">
        <ActionButton disabled={busy} icon={<X size={14} />} onClick={onCancel} type="button" variant="ghost">Cancel</ActionButton>
        <ActionButton disabled={busy || !draft.id.trim() || (draft.runtime === "acp" && !draft.backendRef.trim())} icon={<Save size={14} />} type="submit" variant="primary">
          {editing ? "Save changes" : "Create profile"}
        </ActionButton>
      </div>
    </form>
  );
}

function AgentDefinitionEditorForm({
  busy,
  draft,
  editing,
  onCancel,
  onChange,
  onModeChange,
  onSubmit
}: {
  busy: boolean;
  draft: AgentDraft;
  editing: boolean;
  onCancel(): void;
  onChange(draft: AgentDraft): void;
  onModeChange(mode: AgentDraft["mode"]): void;
  onSubmit(event: FormEvent<HTMLFormElement>): void;
}) {
  return (
    <form aria-label="Agent definition" className="capabilityForm agentDefinitionForm" onSubmit={onSubmit}>
      <div className="agentEditorMode" role="tablist" aria-label="Agent editor mode">
        <button aria-selected={draft.mode === "form"} className={draft.mode === "form" ? "is-selected" : ""} onClick={() => onModeChange("form")} role="tab" type="button">Form</button>
        <button aria-selected={draft.mode === "markdown"} className={draft.mode === "markdown" ? "is-selected" : ""} onClick={() => onModeChange("markdown")} role="tab" type="button">Markdown</button>
      </div>

      <label>
        <span>Target</span>
        <select aria-label="Agent target" disabled={editing || busy} onChange={(event) => onChange({ ...draft, target: agentTargetValue(event.target.value) })} value={draft.target}>
          <option value="project">Project</option>
          <option value="profile">Profile</option>
        </select>
      </label>
      <label>
        <span>Name</span>
        <input aria-label="Agent name" disabled={editing || busy} onChange={(event) => onChange({ ...draft, name: event.target.value })} value={draft.name} />
      </label>
      {draft.mode === "form" ? (
        <>
          <label>
            <span>Description</span>
            <input aria-label="Agent description" disabled={busy} onChange={(event) => onChange({ ...draft, description: event.target.value })} value={draft.description} />
          </label>
          <label className="agentDefinitionSwitch">
            <span>Enabled</span>
            <Switch ariaLabel="Agent enabled" checked={draft.enabled} disabled={busy} label={draft.enabled ? "Enabled" : "Disabled"} onCheckedChange={(enabled) => onChange({ ...draft, enabled })} showLabel={false} size="compact" />
          </label>
          <label className="agentWideField">
            <span>Instructions</span>
            <textarea aria-label="Agent instructions" disabled={busy} onChange={(event) => onChange({ ...draft, instructions: event.target.value })} value={draft.instructions} />
          </label>
          <label>
            <span>Backend ref</span>
            <input aria-label="Agent backend ref" disabled={busy} onChange={(event) => onChange({ ...draft, backendRef: event.target.value })} value={draft.backendRef} />
          </label>
          <label>
            <span>Entrypoints</span>
            <input aria-label="Agent entrypoints" disabled={busy} onChange={(event) => onChange({ ...draft, entrypointsText: event.target.value })} value={draft.entrypointsText} />
          </label>
          <label>
            <span>Tools</span>
            <input aria-label="Agent tools" disabled={busy} onChange={(event) => onChange({ ...draft, toolsText: event.target.value })} value={draft.toolsText} />
          </label>
          <label>
            <span>MCP servers</span>
            <input aria-label="Agent MCP servers" disabled={busy} onChange={(event) => onChange({ ...draft, mcpServersText: event.target.value })} value={draft.mcpServersText} />
          </label>
        </>
      ) : (
        <label className="agentWideField">
          <span>Markdown</span>
          <textarea aria-label="Agent Markdown" disabled={busy} onChange={(event) => onChange({ ...draft, rawMarkdown: event.target.value })} spellCheck={false} value={draft.rawMarkdown} />
        </label>
      )}
      <div className="capabilityFormActions">
        <ActionButton disabled={busy} icon={<X size={14} />} onClick={onCancel} type="button" variant="ghost">Cancel</ActionButton>
        <ActionButton disabled={busy || !draft.name.trim()} icon={<Save size={14} />} type="submit" variant="primary">Save</ActionButton>
      </div>
    </form>
  );
}

function TeamDefinitionEditorForm({
  busy,
  draft,
  editing,
  onCancel,
  onChange,
  onModeChange,
  onSubmit
}: {
  busy: boolean;
  draft: TeamDraft;
  editing: boolean;
  onCancel(): void;
  onChange(draft: TeamDraft): void;
  onModeChange(mode: TeamDraft["mode"]): void;
  onSubmit(event: FormEvent<HTMLFormElement>): void;
}) {
  return (
    <form aria-label="Team definition" className="capabilityForm agentDefinitionForm" onSubmit={onSubmit}>
      <div className="agentEditorMode" role="tablist" aria-label="Team editor mode">
        <button aria-selected={draft.mode === "form"} className={draft.mode === "form" ? "is-selected" : ""} onClick={() => onModeChange("form")} role="tab" type="button">Form</button>
        <button aria-selected={draft.mode === "markdown"} className={draft.mode === "markdown" ? "is-selected" : ""} onClick={() => onModeChange("markdown")} role="tab" type="button">Markdown</button>
      </div>

      <label>
        <span>Target</span>
        <select aria-label="Team target" disabled={editing || busy} onChange={(event) => onChange({ ...draft, target: agentTargetValue(event.target.value) })} value={draft.target}>
          <option value="project">Project</option>
          <option value="profile">Profile</option>
        </select>
      </label>
      <label>
        <span>Name</span>
        <input aria-label="Team name" disabled={editing || busy} onChange={(event) => onChange({ ...draft, name: event.target.value })} value={draft.name} />
      </label>
      {draft.mode === "form" ? (
        <>
          <label>
            <span>Description</span>
            <input aria-label="Team description" disabled={busy} onChange={(event) => onChange({ ...draft, description: event.target.value })} value={draft.description} />
          </label>
          <label>
            <span>Leader</span>
            <input aria-label="Team leader" disabled={busy} onChange={(event) => onChange({ ...draft, leader: event.target.value })} value={draft.leader} />
          </label>
          <label>
            <span>Parallel agents</span>
            <input aria-label="Team max parallel agents" disabled={busy} min={1} max={4} onChange={(event) => onChange({ ...draft, maxParallelAgents: event.target.value })} type="number" value={draft.maxParallelAgents} />
          </label>
          <label className="agentDefinitionSwitch">
            <span>Enabled</span>
            <Switch ariaLabel="Team enabled" checked={draft.enabled} disabled={busy} label={draft.enabled ? "Enabled" : "Disabled"} onCheckedChange={(enabled) => onChange({ ...draft, enabled })} showLabel={false} size="compact" />
          </label>
          <label className="agentWideField">
            <span>Members</span>
            <textarea aria-label="Team members" disabled={busy} onChange={(event) => onChange({ ...draft, membersText: event.target.value })} value={draft.membersText} />
            <small>One member per line. Add named fields such as runtime=codex, option.mode=auto-review, role=review.</small>
          </label>
          <label className="agentWideField">
            <span>Instructions</span>
            <textarea aria-label="Team instructions" disabled={busy} onChange={(event) => onChange({ ...draft, instructions: event.target.value })} value={draft.instructions} />
          </label>
        </>
      ) : (
        <label className="agentWideField">
          <span>Markdown</span>
          <textarea aria-label="Team Markdown" disabled={busy} onChange={(event) => onChange({ ...draft, rawMarkdown: event.target.value })} spellCheck={false} value={draft.rawMarkdown} />
        </label>
      )}
      <div className="capabilityFormActions">
        <ActionButton disabled={busy} icon={<X size={14} />} onClick={onCancel} type="button" variant="ghost">Cancel</ActionButton>
        <ActionButton disabled={busy || !draft.name.trim()} icon={<Save size={14} />} type="submit" variant="primary">Save</ActionButton>
      </div>
    </form>
  );
}

function MarkdownDefinitionPreview({
  copyLabel,
  editDisabled,
  editDisabledReason,
  editLabel,
  error,
  label,
  loading,
  onCopyText,
  onEdit,
  preview
}: {
  copyLabel: string;
  editDisabled?: boolean;
  editDisabledReason?: string;
  editLabel: string;
  error?: string | null | undefined;
  label: string;
  loading?: boolean | undefined;
  onCopyText?: ((text: string) => void | Promise<void>) | undefined;
  onEdit?: (() => void) | undefined;
  preview: string;
}) {
  return (
    <section className="skillPreview markdownDefinitionPreview" aria-label={label}>
      {loading ? "Loading" : error ? error : preview ? (
        <>
          <MarkdownText
            copyLabel={copyLabel}
            copyText={preview}
            onCopyText={onCopyText}
            text={boundedText(preview, 4000)}
          />
          {onEdit && (
            <button
              aria-label={editLabel}
              className="markdownDefinitionEdit"
              disabled={editDisabled}
              onClick={onEdit}
              title={editDisabled ? editDisabledReason : editLabel}
              type="button"
            >
              <Edit3 size={14} aria-hidden />
              <span className="pevo-srOnly">{editLabel}</span>
            </button>
          )}
        </>
      ) : "No preview"}
    </section>
  );
}

function MarkdownDefinitionEditor({
  ariaLabel,
  busy,
  onCancel,
  onChange,
  onSave,
  value
}: {
  ariaLabel: string;
  busy: boolean;
  onCancel(): void;
  onChange(value: string): void;
  onSave(): void;
  value: string;
}) {
  return (
    <section className="markdownDefinitionEditor" aria-label={ariaLabel}>
      <textarea
        aria-label={ariaLabel}
        disabled={busy}
        onChange={(event) => onChange(event.target.value)}
        spellCheck={false}
        value={value}
      />
      <div className="capabilityFormActions">
        <ActionButton disabled={busy} icon={<X size={14} />} onClick={onCancel} type="button" variant="ghost">Cancel</ActionButton>
        <ActionButton disabled={busy || !value.trim()} icon={<Save size={14} />} onClick={onSave} type="button" variant="primary">Save</ActionButton>
      </div>
    </section>
  );
}

function AgentDefinitionFields({ row }: { row: AgentDefinitionRow }) {
  const detailFields = ([
    ["Target", targetLabel(row.target)],
    ["Source", row.sourceLabel],
    ["State", agentStateLabel(row.state)],
    ["Path", row.path ?? ""],
    ["Backend", row.backendRef],
    ["Entrypoints", row.entrypoints.join(", ")],
    ["Tools", row.tools.join(", ")],
    ["MCP Servers", row.mcpServers.join(", ")]
  ] as Array<[string, string]>).filter((entry): entry is [string, string] => entry[1].trim().length > 0);
  return (
    <div className="capabilityStack">
      <dl className="capabilityKeyValues">
        {detailFields.map(([label, value]) => <div key={label}><dt>{label}</dt><dd>{value}</dd></div>)}
      </dl>
      {row.diagnostics.length > 0 && (
        <section className="skillDiagnostics" aria-label="Agent diagnostics">
          {row.diagnostics.map((diagnostic) => <span className="skillIssue" key={diagnostic}>{diagnostic}</span>)}
        </section>
      )}
    </div>
  );
}

function TeamDefinitionFields({ row }: { row: AgentTeamRow }) {
  const members = row.members.map(teamMemberSummary).filter(Boolean).join(", ");
  const detailFields = ([
    ["Target", targetLabel(row.target)],
    ["Source", row.sourceLabel],
    ["State", teamStateLabel(row.state)],
    ["Path", row.path ?? ""],
    ["Leader", row.leader],
    ["Members", members],
    ["Parallel agents", String(row.maxParallelAgents || "")]
  ] as Array<[string, string]>).filter((entry): entry is [string, string] => entry[1].trim().length > 0);
  return (
    <div className="capabilityStack">
      <dl className="capabilityKeyValues">
        {detailFields.map(([label, value]) => <div key={label}><dt>{label}</dt><dd>{value}</dd></div>)}
      </dl>
      {row.diagnostics.length > 0 && (
        <section className="skillDiagnostics" aria-label="Team diagnostics">
          {row.diagnostics.map((diagnostic) => <span className="skillIssue" key={diagnostic}>{diagnostic}</span>)}
        </section>
      )}
    </div>
  );
}

function SkillsPanel({
  busy,
  client,
  createOpen,
  data,
  loading,
  mutate,
  onCloseCreate,
  onCopyText,
  onSelect,
  query,
  refreshToken,
  scope,
  selectedId,
  setSkillInstall,
  skillInstall
}: {
  busy: boolean;
  client: GatewayClient | null;
  createOpen: boolean;
  data: JsonObject | null;
  loading: boolean;
  mutate(action: () => Promise<unknown>, options?: MutationOptions): Promise<boolean>;
  onCloseCreate(): void;
  onCopyText?: ((text: string) => void | Promise<void>) | undefined;
  onSelect(id: string): void;
  query: string;
  refreshToken: number;
  scope: GatewayRequestScope | null;
  selectedId: string | null;
  setSkillInstall(value: SkillInstallDraft): void;
  skillInstall: SkillInstallDraft;
}) {
  const [detail, setDetail] = useState<{ id: string; loading: boolean; value: JsonObject | null; error: string | null } | null>(null);
  const [markdownDraft, setMarkdownDraft] = useState<{ id: string; text: string } | null>(null);

  const allRows = useMemo(() => skillRowsFromData(data), [data]);
  const rows = useMemo(() => {
    const needle = query.trim().toLowerCase();
    return allRows.filter((row) => !needle || `${row.name} ${row.description}`.toLowerCase().includes(needle));
  }, [allRows, query]);

  const activeId = selectedId && rows.some((row) => row.id === selectedId) ? selectedId : rows[0]?.id ?? null;
  const row = rows.find((item) => item.id === activeId) ?? null;

  useEffect(() => {
    setMarkdownDraft(null);
  }, [row?.id]);

  useEffect(() => {
    if (!client || !scope || !row) {
      setDetail(null);
      return;
    }
    let cancelled = false;
    setDetail({ id: row.id, loading: true, value: null, error: null });
    void client.request("skill/read", { name: row.name, path: row.location || null, scope }).then((value) => {
      if (!cancelled) setDetail({ id: row.id, loading: false, value: objectValue(value), error: null });
    }).catch((err) => {
      if (!cancelled) setDetail({ id: row.id, loading: false, value: null, error: errorMessage(err) });
    });
    return () => {
      cancelled = true;
    };
  }, [client, refreshToken, row?.id, row?.location, scope?.cwd]);

  if (!client || !scope) {
    return <div className="capabilityEmpty">Gateway unavailable</div>;
  }

  return (
    <>
      {createOpen && (
        <CreatePanel className="capabilityCreatePanel" description="Install a skill from a local path or Git source." icon={<Plus size={14} />} layout="side" onClose={onCloseCreate} title="Install skill">
          <form className="capabilityForm skillInstallForm" onSubmit={(event) => {
            event.preventDefault();
            if (skillInstall.force && !confirmAction("Install skill with force?")) return;
            void mutate(() => client.request("skill/install", {
              source: skillInstall.source,
              name: skillInstall.name || null,
              target: skillInstall.target,
              force: skillInstall.force,
              scope
            })).then((ok) => {
              if (ok) onCloseCreate();
            });
          }}>
            <input aria-label="Skill source" onChange={(event) => setSkillInstall({ ...skillInstall, source: event.target.value })} placeholder="path or git source" value={skillInstall.source} />
            <input aria-label="Skill name" onChange={(event) => setSkillInstall({ ...skillInstall, name: event.target.value })} placeholder="name" value={skillInstall.name} />
            <select aria-label="Skill target" onChange={(event) => setSkillInstall({ ...skillInstall, target: event.target.value === "project" ? "project" : "profile" })} value={skillInstall.target}>
              <option value="profile">Profile</option>
              <option value="project">Project</option>
            </select>
            <label><input checked={skillInstall.force} onChange={(event) => setSkillInstall({ ...skillInstall, force: event.target.checked })} type="checkbox" /> Force</label>
            <ActionButton disabled={busy || !skillInstall.source.trim()} icon={<Plus size={14} />} type="submit" variant="primary">Install</ActionButton>
          </form>
        </CreatePanel>
      )}

      <div className="capabilitiesGrid skillsGrid">
        <div className="capabilityList" role="list">
          {loading && <div className="capabilityEmpty">Loading</div>}
          {!loading && rows.length === 0 && <div className="capabilityEmpty">No matches</div>}
          {rows.map((item) => {
            const metadata = skillRowMetadata(item);
            return (
              <div
                className={item.id === row?.id ? "capabilityRow skillRow capabilityRowWithSwitch is-selected" : "capabilityRow skillRow capabilityRowWithSwitch"}
                key={item.id}
                role="listitem"
              >
                <button aria-label={`Skill ${item.name}`} className="skillRowSelect" onClick={() => onSelect(item.id)} type="button">
                  <span className="capabilityRowMain">
                    <strong>{item.name}</strong>
                    <RowDescription fallback={readinessLabel(item.readiness)} value={item.description} />
                    {metadata && <span className="skillRowMetadata">{metadata}</span>}
                  </span>
                </button>
                <Switch
                  ariaLabel={item.enabled ? `Disable ${item.name}` : `Enable ${item.name}`}
                  checked={item.enabled}
                  className="capabilityRowSwitch"
                  disabled={busy}
                  label={item.enabled ? "Enabled" : "Disabled"}
                  onCheckedChange={(enabled) => void mutate(() => client.request("skill/setEnabled", { name: item.name, enabled, target: mutableTargetForSkill(item) ?? skillInstall.target, scope }))}
                  showLabel={false}
                  size="compact"
                />
              </div>
            );
          })}
        </div>

        <aside className="capabilityDetail skillDetail" aria-label="Skills detail">
          {row ? (
            <>
              <div className="capabilityDetailHeader">
                <div>
                  <h3>{row.name}</h3>
                  <span>{skillDetailSummary(row)}</span>
                </div>
                <div className="capabilityDetailHeaderActions">
                  <button
                    disabled={busy || !mutableTargetForSkill(row)}
                    onClick={() => {
                      const target = mutableTargetForSkill(row);
                      if (!target || !confirmAction(`Uninstall skill ${row.name}?`)) return;
                      void mutate(() => client.request("skill/uninstall", { name: row.name, path: row.location || null, target, scope }));
                    }}
                    title={mutableTargetForSkill(row) ? "Uninstall" : "Only profile/project-installed skills can be uninstalled here"}
                    type="button"
                  >
                    <Trash2 size={14} /> Uninstall
                  </button>
                </div>
              </div>

              <SkillDetailFields
                busy={busy}
                detail={detail?.id === row.id ? detail : null}
                markdownDraft={markdownDraft?.id === row.id ? markdownDraft.text : null}
                onCancelMarkdownEdit={() => setMarkdownDraft(null)}
                onChangeMarkdownDraft={(text) => setMarkdownDraft({ id: row.id, text })}
                onCopyText={onCopyText}
                onEditMarkdown={(preview) => setMarkdownDraft({ id: row.id, text: preview })}
                onSaveMarkdown={(text) => {
                  const target = mutableMarkdownTargetForSkill(row);
                  if (!target) return;
                  void mutate(() => client.request("skill/write", {
                    name: row.name,
                    path: row.location || null,
                    target,
                    rawMarkdown: text,
                    scope
                  }), { notice: "Skill saved." }).then((ok) => {
                    if (ok) setMarkdownDraft(null);
                  });
                }}
                row={row}
              />
            </>
          ) : (
            <div className="capabilityEmpty">Select an item</div>
          )}
        </aside>
      </div>

    </>
  );
}

function SkillDetailFields({
  busy,
  detail,
  markdownDraft,
  onCancelMarkdownEdit,
  onChangeMarkdownDraft,
  onCopyText,
  onEditMarkdown,
  onSaveMarkdown,
  row
}: {
  busy: boolean;
  detail: { loading: boolean; value: JsonObject | null; error: string | null } | null;
  markdownDraft: string | null;
  onCancelMarkdownEdit(): void;
  onChangeMarkdownDraft(text: string): void;
  onCopyText?: ((text: string) => void | Promise<void>) | undefined;
  onEditMarkdown(preview: string): void;
  onSaveMarkdown(text: string): void;
  row: SkillRow;
}) {
  const value = detail?.value ?? {};
  const linkedFiles = objectField(value, "linked_files");
  const preview = stringField(value, "preview_content") || stringField(value, "content");
  const entrypoint = shouldShowSkillEntrypoint(row) ? row.location : "";
  const detailFields: Array<[string, string]> = [
    ["Entrypoint", entrypoint],
    ["Skill Dir", row.skillDir],
    ["Platform", row.supported ? "supported" : "unsupported"],
    ["Tags", row.tags.join(", ")],
    ["Missing Env", row.missingEnvVars.join(", ")],
    ["Missing Credentials", row.missingCredentialFiles.join(", ")],
    ["Tools", row.requiredTools.join(", ")],
    ["Toolsets", row.requiredToolsets.join(", ")],
    ["Linked Files", linkedFilesText(linkedFiles)]
  ];
  const detailRows = detailFields.filter(([, fieldValue]) => fieldValue.trim().length > 0);

  return (
    <div className="skillDetailBody">
      <div className="skillDetailSummary">
        <dl className="capabilityKeyValues skillKeyValues">
          {detailRows.map(([label, fieldValue]) => <div key={label}><dt>{label}</dt><dd>{fieldValue}</dd></div>)}
        </dl>
        {row.issues.length > 0 && (
          <section className="skillDiagnostics" aria-label="Skill diagnostics">
            {row.issues.map((issue) => <span className="skillIssue" key={issue}>{issue}</span>)}
          </section>
        )}
      </div>
      {markdownDraft !== null ? (
        <MarkdownDefinitionEditor
          ariaLabel="SKILL.md editor"
          busy={busy}
          onCancel={onCancelMarkdownEdit}
          onChange={onChangeMarkdownDraft}
          onSave={() => onSaveMarkdown(markdownDraft)}
          value={markdownDraft}
        />
      ) : (
        <MarkdownDefinitionPreview
          copyLabel="Copy SKILL.md"
          editDisabled={busy || !mutableMarkdownTargetForSkill(row) || detail?.loading === true}
          editDisabledReason={skillMarkdownEditReason(row, detail?.loading === true)}
          editLabel={`Edit ${row.name} SKILL.md`}
          error={detail?.error}
          label="SKILL.md preview"
          loading={detail?.loading}
          onCopyText={onCopyText}
          onEdit={preview ? () => onEditMarkdown(preview) : undefined}
          preview={preview}
        />
      )}
    </div>
  );
}

function shouldShowSkillEntrypoint(row: SkillRow): boolean {
  if (!row.location) return false;
  if (!row.skillDir) return true;
  return normalizePathText(row.location) !== `${normalizePathText(row.skillDir)}/SKILL.md`;
}

function normalizePathText(value: string): string {
  return value.replace(/\\/g, "/").replace(/\/+$/, "");
}

function CapabilityActions({
  busy,
  client,
  mutate,
  onOAuthSession,
  onPluginConnectSession,
  pluginDetail,
  row,
  scope,
  setToolPolicyDraft,
  tab,
  toolPolicyDraft
}: {
  busy: boolean;
  client: GatewayClient | null;
  mutate(action: () => Promise<unknown>, options?: MutationOptions): Promise<boolean>;
  onOAuthSession(sessionId: string | null): void;
  onPluginConnectSession(sessionId: string | null): void;
  pluginDetail: JsonObject | null;
  row: CapabilityRow;
  scope: GatewayRequestScope | null;
  setToolPolicyDraft(value: { enabledTools: string; disabledTools: string }): void;
  tab: CapabilityTab;
  toolPolicyDraft: { enabledTools: string; disabledTools: string };
}) {
  if (!client || !scope) return null;
  if (tab === "tools") {
    const canToggleModes = toolsetModeMutable(row);
    const canRemove = toolsetRemovable(row);
    if (!canToggleModes && !canRemove) return null;
    return (
      <div className="capabilityActionGrid">
        {canToggleModes && (
          <>
            <button disabled={busy} onClick={() => void mutate(() => client.request("tool/setEnabled", { name: row.name, mode: "default", enabled: !modeEnabled(row.raw, "default"), scope }))} type="button">
              Default {modeEnabled(row.raw, "default") ? "Off" : "On"}
            </button>
            <button disabled={busy} onClick={() => void mutate(() => client.request("tool/setEnabled", { name: row.name, mode: "plan", enabled: !modeEnabled(row.raw, "plan"), scope }))} type="button">
              Plan {modeEnabled(row.raw, "plan") ? "Off" : "On"}
            </button>
          </>
        )}
        {canRemove && (
          <button disabled={busy} onClick={() => confirmAction(`Remove toolset ${row.name}?`) && void mutate(() => client.request("tool/remove", { name: row.name, scope }))} type="button">
            <Trash2 size={14} /> Remove
          </button>
        )}
      </div>
    );
  }
  if (tab === "mcp") {
    const policy = objectField(row.raw, "policy");
    const enabledTools = arrayStrings(policy.enabledTools).join(", ");
    const disabledTools = arrayStrings(policy.disabledTools).join(", ");
    return (
      <div className="capabilityStack">
        <div className="capabilityActionGrid">
          <button disabled={busy} onClick={() => void mutate(() => client.request("mcp/test", { name: row.name, scope }))} type="button">
            <Play size={14} /> Test
          </button>
          <button disabled={busy} onClick={() => void startOAuth(client, scope, row.name, onOAuthSession, mutate)} type="button">
            <LogIn size={14} /> Login
          </button>
          <button disabled={busy} onClick={() => void mutate(() => client.request("mcp/oauth/logout", { name: row.name, scope }))} type="button">
            <LogOut size={14} /> Logout
          </button>
          <button disabled={busy} onClick={() => confirmAction(`Remove MCP server ${row.name}?`) && void mutate(() => client.request("mcp/remove", { name: row.name, scope }))} type="button">
            <Trash2 size={14} /> Remove
          </button>
        </div>
        <div className="capabilityInlineFields">
          <input aria-label="Enabled MCP tools" onChange={(event) => setToolPolicyDraft({ ...toolPolicyDraft, enabledTools: event.target.value })} placeholder={enabledTools || "enabled tools"} value={toolPolicyDraft.enabledTools} />
          <input aria-label="Disabled MCP tools" onChange={(event) => setToolPolicyDraft({ ...toolPolicyDraft, disabledTools: event.target.value })} placeholder={disabledTools || "disabled tools"} value={toolPolicyDraft.disabledTools} />
          <button disabled={busy} onClick={() => void mutate(() => client.request("mcp/setToolPolicy", { name: row.name, enabledTools: splitList(toolPolicyDraft.enabledTools || enabledTools) || null, disabledTools: splitList(toolPolicyDraft.disabledTools || disabledTools) ?? [], scope }))} type="button">
            Save Policy
          </button>
        </div>
      </div>
    );
  }
  if (tab === "plugins") {
    const detailPlugin = objectField(pluginDetail, "plugin");
    const trust = Object.keys(detailPlugin).length > 0
      ? objectField(detailPlugin, "trust")
      : objectField(row.raw, "trust");
    const needsTrust = boolField(trust, "required") && stringField(trust, "status") !== "trusted";
    const selector = pluginSelector(row);
    const scopeName = pluginMutationScope(row);
    const packageMutable = pluginPackageMutable(row);
    const removable = pluginRemovable(row);
    const codexAuthority = pluginAuthorityKind(row) === "codex";
    const installed = boolField(row.raw, "installed");
    const policy = objectField(row.raw, "policy");
    const profileEnabled = boolField(policy, "profileEnabled");
    const projectOverride = objectValue(policy).projectOverride;
    const connectTarget = codexConnectTarget(pluginDetail);
    const marketplace = stringField(objectField(row.raw, "authority"), "marketplace");
    return (
      <div className="capabilityActionGrid">
        <button disabled={busy} onClick={() => void mutate(() => client.request("plugin/doctor", { selector, scope }))} type="button">
          <Play size={14} /> Doctor
        </button>
        {needsTrust && packageMutable && (
          <button disabled={busy} onClick={() => confirmAction(`Trust plugin ${row.name} for its current package fingerprint?`) && void mutate(() => client.request("plugin/setTrust", { selector, scopeName, trusted: true, scope }))} type="button">
            <LogIn size={14} /> Trust
          </button>
        )}
        {codexAuthority && installed && (
          <>
            <button disabled={busy} onClick={() => void mutate(() => client.request("plugin/setEnabled", { selector, scopeName: "profile", enabled: !profileEnabled, scope }))} type="button">
              {profileEnabled ? "Disable in profile" : "Enable in profile"}
            </button>
            <button disabled={busy || projectOverride === false} onClick={() => void mutate(() => client.request("plugin/setEnabled", { selector, scopeName: "project", enabled: false, scope }))} type="button">
              Disable in this project
            </button>
            <button disabled={busy || projectOverride == null} onClick={() => void mutate(() => client.request("plugin/setEnabled", { selector, scopeName: "project", enabled: null, scope }))} type="button">
              Reset project override
            </button>
          </>
        )}
        {codexAuthority && installed && connectTarget && (
          <button disabled={busy} onClick={() => void startPluginConnect(client, scope, selector, connectTarget, onPluginConnectSession, mutate)} type="button">
            <LogIn size={14} /> Connect
          </button>
        )}
        {codexAuthority && installed && marketplace && (
          <button disabled={busy} onClick={() => void mutate(() => client.request("plugin/catalog/upgrade", { authority: "codex", name: marketplace, scope }))} type="button">
            <RefreshCw size={14} /> Upgrade
          </button>
        )}
        {packageMutable && removable && (
          <button disabled={busy} onClick={() => confirmAction(`Uninstall plugin ${row.name}?`) && void mutate(() => client.request("plugin/uninstall", { selector, scopeName, scope }))} type="button">
            <Trash2 size={14} /> Uninstall
          </button>
        )}
        {codexAuthority && !installed && (
          <button disabled={busy} onClick={() => void mutate(() => client.request("plugin/install", { source: selector, scope }))} type="button">
            <Plus size={14} /> Install
          </button>
        )}
      </div>
    );
  }
  return (
    <div className="capabilityActionGrid">
      <button disabled={busy} onClick={() => confirmAction(`Uninstall skill ${row.name}?`) && void mutate(() => client.request("skill/uninstall", { name: row.name, scope }))} type="button">
        <Trash2 size={14} /> Uninstall
      </button>
    </div>
  );
}

function CodexAuthorityCard({
  authority,
  busy,
  client,
  mutate,
  scope
}: {
  authority: JsonObject;
  busy: boolean;
  client: GatewayClient | null;
  mutate(action: () => Promise<unknown>, options?: MutationOptions): Promise<boolean>;
  scope: GatewayRequestScope | null;
}) {
  const resolvedBinary = stringField(authority, "resolvedBinary");
  const [binary, setBinary] = useState(resolvedBinary === "codex" ? "" : resolvedBinary);
  const binaryRef = useRef(binary);
  const binaryInputRef = useRef<HTMLInputElement | null>(null);
  const enabled = boolField(authority, "enabled");
  if (!client || !scope) return null;
  const write = (nextEnabled: boolean) => client.request("plugin/authority/write", {
    enabled: nextEnabled,
    binary: (binaryInputRef.current?.value ?? binaryRef.current).trim() || null,
    scope
  });
  return (
    <section aria-label="Codex plugin compatibility" className="pluginInspection">
      <div className="capabilityDetailHeader">
        <div>
          <h3>Codex compatibility</h3>
          <span>{[stringField(authority, "runtime") || "disabled", stringField(authority, "auth") || "unavailable"].join(" · ")}</span>
        </div>
        <Switch
          ariaLabel={enabled ? "Disable Codex plugins" : "Enable Codex plugins"}
          checked={enabled}
          disabled={busy}
          label={enabled ? "Enabled" : "Disabled"}
          onCheckedChange={(nextEnabled) => void mutate(() => write(nextEnabled), { notice: nextEnabled ? "Codex plugin compatibility enabled." : "Codex plugin compatibility disabled." })}
          size="compact"
        />
      </div>
      <div className="capabilityInlineFields">
        <input aria-label="Codex binary path" ref={binaryInputRef} onChange={(event) => {
          binaryRef.current = event.target.value;
          setBinary(event.target.value);
        }} placeholder="codex (from PATH)" value={binary} />
        <button disabled={busy} onClick={() => void mutate(() => write(enabled), { notice: "Codex binary setting saved." })} type="button">
          <Save size={14} /> Save
        </button>
        <button disabled={busy || !enabled} onClick={() => void mutate(() => client.request("plugin/authority/refresh", { scope }), { notice: "Codex compatibility refreshed." })} type="button">
          <RefreshCw size={14} /> Refresh
        </button>
      </div>
      <dl className="capabilityKeyValues pluginInspectionGrid">
        <div><dt>Version</dt><dd>{stringField(authority, "version") || "Not resolved"}</dd></div>
        <div><dt>Profile</dt><dd>{stringField(authority, "compatibilityProfile") || "codex-plugin/8604689e"}</dd></div>
        <div><dt>Private home</dt><dd>{stringField(authority, "privateHome") || "Unavailable"}</dd></div>
        <div><dt>Inventory</dt><dd>{boolField(authority, "inventoryReady") ? `Ready · generation ${String(objectValue(authority).generation ?? "-")}` : "Not ready"}</dd></div>
      </dl>
      {stringField(authority, "reason") && <div className="capabilityBanner is-error">{stringField(authority, "reason")}</div>}
    </section>
  );
}

function PluginOperationResult({ value }: { value: JsonObject }) {
  const safeState = stringField(value, "safeState") || "Repair required";
  const failedStep = stringField(value, "failedStep") || "unknown step";
  const completedSteps = arrayStrings(value.completedSteps).map((step) => step.replaceAll("_", " "));
  return (
    <section aria-label="Plugin operation result" className="capabilityBanner is-error pluginOperationResult" role="status">
      <strong>Partial install · {safeState}</strong>
      <span>Failed at {failedStep.replaceAll("_", " ")}. The materialized plugin was not automatically removed.</span>
      {completedSteps.length > 0 && <small>Completed: {completedSteps.join(" · ")}</small>}
    </section>
  );
}

function CapabilityForms(props: {
  busy: boolean;
  client: GatewayClient | null;
  mcpDraft: { name: string; transport: string; command: string; url: string; bearerTokenEnvVar: string; oauthClientId: string };
  mutate(action: () => Promise<unknown>, options?: MutationOptions): Promise<boolean>;
  onClose(): void;
  open: boolean;
  pluginInstall: PluginInstallDraft;
  scope: GatewayRequestScope | null;
  setMcpDraft(value: { name: string; transport: string; command: string; url: string; bearerTokenEnvVar: string; oauthClientId: string }): void;
  setPluginInstall(value: PluginInstallDraft): void;
  setToolDraft(value: { name: string; description: string; tools: string; includes: string; force: boolean }): void;
  tab: CapabilityTab;
  toolDraft: { name: string; description: string; tools: string; includes: string; force: boolean };
}) {
  const { busy, client, mutate, onClose, open, scope, tab } = props;
  if (!client || !scope || !open) return null;
  if (tab === "plugins") {
    const draft = props.pluginInstall;
    const inspectParams = {
      source: draft.source,
      sourceKind: draft.kind,
      gitRef: null,
      npmVersion: draft.kind === "npm" ? draft.npmVersion || null : null,
      npmRegistry: draft.kind === "npm" ? draft.npmRegistry || null : null,
      adapterMode: draft.adapterMode,
      scope
    };
    return (
      <CreatePanel className="capabilityCreatePanel" description="Inspect and install a plugin package." icon={<Plus size={14} />} layout="side" onClose={onClose} title="Install plugin">
        <form className="capabilityForm" onSubmit={(event) => {
          event.preventDefault();
          if (draft.force && !confirmAction("Install plugin with force?")) return;
          void mutate(() => client.request("plugin/install", { ...inspectParams, force: draft.force })).then((ok) => {
            if (ok) onClose();
          });
        }}>
          <input aria-label="Plugin source" onChange={(event) => props.setPluginInstall({ ...draft, source: event.target.value, inspection: null })} placeholder="path, git source, or npm package" value={draft.source} />
          <select aria-label="Plugin source kind" onChange={(event) => props.setPluginInstall({ ...draft, kind: pluginKindValue(event.target.value), inspection: null })} value={draft.kind}>
            <option value="local">Local</option>
            <option value="git">Git</option>
            <option value="npm">npm</option>
          </select>
          {draft.kind === "npm" && (
            <>
              <input aria-label="Npm package version" onChange={(event) => props.setPluginInstall({ ...draft, npmVersion: event.target.value, inspection: null })} placeholder="version" value={draft.npmVersion} />
              <input aria-label="Npm registry" onChange={(event) => props.setPluginInstall({ ...draft, npmRegistry: event.target.value, inspection: null })} placeholder="registry" value={draft.npmRegistry} />
            </>
          )}
          <select aria-label="Plugin adapter mode" onChange={(event) => props.setPluginInstall({ ...draft, adapterMode: pluginAdapterModeValue(event.target.value), inspection: null })} value={draft.adapterMode}>
            <option value="manifest_only">Manifest only</option>
            <option value="adapter_host">Adapter host</option>
            <option value="disabled">Disabled</option>
          </select>
          <label><input checked={draft.force} onChange={(event) => props.setPluginInstall({ ...draft, force: event.target.checked })} type="checkbox" /> Force</label>
          <div className="capabilityFormActions">
            <ActionButton disabled={busy || !draft.source.trim()} icon={<Search size={14} />} onClick={() => {
              void mutate(async () => {
                const result = await client.request("plugin/import/inspect", inspectParams);
                props.setPluginInstall({ ...draft, inspection: objectField(result, "inspection") });
                return result;
              }, { notice: "Inspection complete.", refresh: false });
            }} type="button" variant="neutral">Inspect</ActionButton>
            <ActionButton disabled={busy || !draft.source.trim()} icon={<Plus size={14} />} type="submit" variant="primary">Install</ActionButton>
          </div>
          {draft.inspection && <PluginInspectionSummary inspection={draft.inspection} />}
        </form>
      </CreatePanel>
    );
  }
  if (tab === "mcp") {
    const draft = props.mcpDraft;
    return (
      <CreatePanel className="capabilityCreatePanel" description="Add a profile-scoped stdio or HTTP MCP server." icon={<Plus size={14} />} layout="side" onClose={onClose} title="Add MCP server">
        <form className="capabilityForm" onSubmit={(event) => {
          event.preventDefault();
          void mutate(() => client.request("mcp/upsert", {
            name: draft.name,
            transport: draft.transport,
            command: draft.transport === "stdio" ? draft.command : null,
            url: draft.transport === "streamable_http" ? draft.url : null,
            bearerTokenEnvVar: draft.bearerTokenEnvVar || null,
            oauthClientId: draft.oauthClientId || null,
            scope
          })).then((ok) => {
            if (ok) onClose();
          });
        }}>
          <input aria-label="MCP name" onChange={(event) => props.setMcpDraft({ ...draft, name: event.target.value })} placeholder="name" value={draft.name} />
          <select aria-label="MCP transport" onChange={(event) => props.setMcpDraft({ ...draft, transport: event.target.value })} value={draft.transport}>
            <option value="stdio">stdio</option>
            <option value="streamable_http">HTTP</option>
          </select>
          <input aria-label="MCP command or URL" onChange={(event) => draft.transport === "stdio" ? props.setMcpDraft({ ...draft, command: event.target.value }) : props.setMcpDraft({ ...draft, url: event.target.value })} placeholder={draft.transport === "stdio" ? "command" : "url"} value={draft.transport === "stdio" ? draft.command : draft.url} />
          <input aria-label="Bearer token env var" onChange={(event) => props.setMcpDraft({ ...draft, bearerTokenEnvVar: event.target.value })} placeholder="bearer env" value={draft.bearerTokenEnvVar} />
          <input aria-label="OAuth client id" onChange={(event) => props.setMcpDraft({ ...draft, oauthClientId: event.target.value })} placeholder="client id" value={draft.oauthClientId} />
          <ActionButton disabled={busy || !draft.name.trim()} icon={<Plus size={14} />} type="submit" variant="primary">Save</ActionButton>
        </form>
      </CreatePanel>
    );
  }
  const draft = props.toolDraft;
  const builtInToolsetName = isBuiltInToolsetName(draft.name);
  return (
    <CreatePanel className="capabilityCreatePanel" description="Create or overwrite a custom toolset." icon={<Plus size={14} />} layout="side" onClose={onClose} title="Create toolset">
      <form className="capabilityForm" onSubmit={(event) => {
        event.preventDefault();
        if (isBuiltInToolsetName(draft.name)) return;
        if (draft.force && !confirmAction("Overwrite toolset?")) return;
        void mutate(() => client.request("tool/create", {
          name: draft.name,
          description: draft.description || null,
          tools: splitList(draft.tools) ?? [],
          includes: splitList(draft.includes) ?? [],
          force: draft.force,
          scope
        })).then((ok) => {
          if (ok) onClose();
        });
      }}>
        <input aria-label="Toolset name" onChange={(event) => props.setToolDraft({ ...draft, name: event.target.value })} placeholder="name" value={draft.name} />
        <input aria-label="Toolset description" onChange={(event) => props.setToolDraft({ ...draft, description: event.target.value })} placeholder="description" value={draft.description} />
        <input aria-label="Tool names" onChange={(event) => props.setToolDraft({ ...draft, tools: event.target.value })} placeholder="tools" value={draft.tools} />
        <input aria-label="Included toolsets" onChange={(event) => props.setToolDraft({ ...draft, includes: event.target.value })} placeholder="includes" value={draft.includes} />
        <label><input checked={draft.force} onChange={(event) => props.setToolDraft({ ...draft, force: event.target.checked })} type="checkbox" /> Force</label>
        <ActionButton disabled={busy || !draft.name.trim() || builtInToolsetName} icon={<Plus size={14} />} title={builtInToolsetName ? "Built-in toolsets cannot be overwritten" : "Save"} type="submit" variant="primary">Save</ActionButton>
      </form>
    </CreatePanel>
  );
}

function agentRowsFromData(data: JsonObject | null): AgentDefinitionRow[] {
  if (!data) return [];
  return [
    ...agentRowsFromArray(arrayObjects(data.agents), "active"),
    ...agentRowsFromArray(arrayObjects(data.shadowedAgents), "shadowed"),
    ...agentRowsFromArray(arrayObjects(data.disabledAgents), "disabled")
  ].filter((row) => row.target === "project" || row.target === "profile");
}

function agentRowsFromArray(values: JsonObject[], state: AgentDefinitionState): AgentDefinitionRow[] {
  return values.map((agent, index) => {
    const target = parseAgentTarget(objectValue(agent).target);
    const name = stringField(agent, "name");
    const source = stringField(agent, "source");
    const sourceLabel = stringField(agent, "sourceLabel") || source;
    const path = optionalString(agent.path);
    const backend = objectField(agent, "backend");
    return {
      id: `${state}:${target ?? source}:${name}:${path ?? index}`,
      name,
      description: stringField(agent, "description"),
      enabled: objectValue(agent).enabled !== false,
      source,
      sourceLabel: sourceLabel || targetLabel(target),
      target,
      mutable: boolField(agent, "mutable"),
      path,
      entrypoints: arrayStrings(agent.entrypoints),
      tools: arrayStrings(agent.tools),
      mcpServers: arrayStrings(agent.mcpServers),
      diagnostics: arrayObjects(agent.diagnostics).map((diagnostic) => stringField(diagnostic, "message")).filter(Boolean),
      backendRef: stringField(backend, "ref"),
      state,
      raw: agent
    };
  }).filter((row) => row.name);
}

function agentTeamRowsFromData(data: JsonObject | null): AgentTeamRow[] {
  const teams = objectField(data, "teams");
  return [
    ...agentTeamRowsFromArray(arrayObjects(teams.teams), "active"),
    ...agentTeamRowsFromArray(arrayObjects(teams.shadowedTeams), "shadowed"),
    ...agentTeamRowsFromArray(arrayObjects(teams.disabledTeams), "disabled")
  ].filter((row) => row.target === "project" || row.target === "profile");
}

function agentTeamRowsFromArray(values: JsonObject[], state: AgentDefinitionState): AgentTeamRow[] {
  return values.map((team, index) => {
    const target = parseAgentTarget(objectValue(team).target);
    const name = stringField(team, "name");
    const source = stringField(team, "source");
    const sourceLabel = stringField(team, "sourceLabel") || source;
    const path = optionalString(team.path);
    const maxParallelAgents = Number(objectValue(team).maxParallelAgents);
    return {
      id: `${state}:${target ?? source}:${name}:${path ?? index}`,
      name,
      description: stringField(team, "description"),
      enabled: objectValue(team).enabled !== false,
      source,
      sourceLabel: sourceLabel || targetLabel(target),
      target,
      mutable: boolField(team, "mutable"),
      path,
      leader: stringField(team, "leader"),
      members: arrayObjects(team.members),
      maxParallelAgents: Number.isFinite(maxParallelAgents) ? maxParallelAgents : 4,
      diagnostics: arrayObjects(team.diagnostics).map((diagnostic) => stringField(diagnostic, "message")).filter(Boolean),
      state,
      raw: team
    };
  }).filter((row) => row.name);
}

function runtimeProfileRowsFromData(data: JsonObject | null): RuntimeProfileRow[] {
  const runtimeProfiles = objectField(data, "runtimeProfiles");
  return arrayObjects(runtimeProfiles.profiles).map((profile) => {
    const health = objectField(profile, "health");
    return {
      id: stringField(profile, "id"),
      label: stringField(profile, "label"),
      runtime: stringField(profile, "runtime"),
      enabled: objectValue(profile).enabled !== false,
      generated: boolField(profile, "generated"),
      configured: boolField(profile, "configured"),
      backendRef: stringField(profile, "backendRef"),
      provenance: stringField(profile, "provenance") || runtimeProvenanceForKind(stringField(profile, "runtime")),
      profileRevision: decimalRevisionField(profile, "profileRevision"),
      capabilityRevision: decimalRevisionField(profile, "capabilityRevision"),
      defaultModel: stringField(profile, "defaultModel"),
      defaultMode: stringField(profile, "defaultMode"),
      defaultAgent: stringField(profile, "defaultAgent"),
      approvalMode: stringField(profile, "approvalMode"),
      sandbox: stringField(profile, "sandbox"),
      workspaceRoots: arrayStrings(profile.workspaceRoots),
      healthStatus: stringField(health, "status") || "unchecked",
      healthSummary: stringField(health, "summary") || "Not checked",
      checkedAtMs: nullableNumberField(health, "checkedAtMs"),
      readinessStages: arrayObjects(profile.readinessStages).map((stage) => ({
        id: stringField(stage, "id"),
        status: stringField(stage, "status") || "unchecked",
        summary: stringField(stage, "summary"),
        observedAtMs: nullableNumberField(stage, "observedAtMs")
      })).filter((stage) => stage.id),
      sourceTargets: arrayStrings(profile.sourceTargets).map(agentTargetValue),
      diagnostics: arrayObjects(profile.diagnostics).map((diagnostic) => stringField(diagnostic, "message")).filter(Boolean),
      raw: profile
    };
  }).filter((row) => row.id);
}

function emptyRuntimeProfileDraft(): RuntimeProfileDraft {
  return {
    target: "project",
    id: "",
    runtime: "acp",
    enabled: true,
    label: "",
    backendRef: "",
    defaultModel: "",
    defaultMode: "",
    defaultAgent: "",
    approvalMode: "",
    sandbox: "",
    workspaceRootsText: "",
    optionsText: ""
  };
}

function runtimeProfileDraftFromRead(profile: RuntimeProfileView, options: unknown | null): RuntimeProfileDraft {
  return {
    target: runtimeProfileMutableTargetFromSources(profile.sourceTargets) ?? "profile",
    id: profile.id,
    runtime: runtimeProfileKind(profile.runtime),
    enabled: profile.enabled,
    label: profile.label,
    backendRef: profile.backendRef ?? "",
    defaultModel: profile.defaultModel ?? "",
    defaultMode: profile.defaultMode ?? "",
    defaultAgent: profile.defaultAgent ?? "",
    approvalMode: profile.approvalMode ?? "",
    sandbox: profile.sandbox ?? "",
    workspaceRootsText: profile.workspaceRoots.join("\n"),
    optionsText: options == null ? "" : JSON.stringify(options, null, 2)
  };
}

function runtimeProfileKind(value: string): RuntimeProfileKind {
  if (value === "native" || value === "acp") return value;
  throw new Error(`Unsupported Runtime Profile kind: ${value}`);
}

function runtimeProfileMutableTarget(row: RuntimeProfileRow): BackendConfigTarget | null {
  return runtimeProfileMutableTargetFromSources(row.sourceTargets);
}

function runtimeProfileMutableTargetFromSources(sourceTargets: BackendConfigTarget[]): BackendConfigTarget | null {
  if (sourceTargets.includes("project")) return "project";
  if (sourceTargets.includes("profile")) return "profile";
  return null;
}

function runtimeProfileCanCustomize(row: RuntimeProfileRow): boolean {
  if (row.runtime !== "native" && row.runtime !== "acp") return false;
  if (runtimeProfileMutableTarget(row)) return true;
  return row.generated && runtimeProfileHasReservedIdentity(row);
}

function runtimeProfileHasReservedIdentity(row: RuntimeProfileRow): boolean {
  return row.id === "native" || row.id === "codex" || row.id === "opencode";
}

function runtimeProfileHasFallback(row: RuntimeProfileRow): boolean {
  return row.sourceTargets.length > 1 || runtimeProfileHasReservedIdentity(row);
}

function parseRuntimeOptions(value: string): JsonObject | null | string {
  const source = value.trim();
  if (!source) return null;
  try {
    const parsed: unknown = JSON.parse(source);
    if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) {
      return "Advanced options must be a JSON object.";
    }
    return parsed as JsonObject;
  } catch {
    return "Advanced options must be valid JSON.";
  }
}

function splitLines(value: string): string[] {
  return value.split(/\r?\n/).map((entry) => entry.trim()).filter(Boolean);
}

function nullableText(value: string): string | null {
  const trimmed = value.trim();
  return trimmed || null;
}

function emptyAgentDraft(): AgentDraft {
  return {
    mode: "form",
    target: "project",
    name: "",
    description: "",
    enabled: true,
    instructions: "",
    backendRef: "",
    entrypointsText: "subagent",
    toolsText: "",
    mcpServersText: "",
    rawMarkdown: "---\nname: \ndescription: \nenabled: true\nentrypoints: [subagent]\n---\n"
  };
}

function emptyTeamDraft(): TeamDraft {
  return {
    mode: "form",
    target: "project",
    name: "",
    description: "",
    enabled: true,
    leader: "general",
    membersText: "researcher: general",
    maxParallelAgents: "4",
    instructions: "",
    rawMarkdown: ""
  };
}

function agentDraftFromRead(row: AgentDefinitionRow, agent: JsonObject, instructions: string, rawMarkdown: string): AgentDraft {
  const backend = objectField(agent, "backend");
  return {
    mode: "form",
    target: row.target ?? parseAgentTarget(agent.target) ?? "project",
    name: stringField(agent, "name") || row.name,
    description: stringField(agent, "description") || row.description,
    enabled: objectValue(agent).enabled !== false,
    instructions,
    backendRef: stringField(backend, "ref"),
    entrypointsText: arrayStrings(agent.entrypoints).join(", "),
    toolsText: arrayStrings(agent.tools).join(", "),
    mcpServersText: arrayStrings(agent.mcpServers).join(", "),
    rawMarkdown
  };
}

function renderAgentDraftMarkdown(draft: AgentDraft): string {
  const lines = [
    "---",
    `name: ${draft.name.trim()}`,
    `description: ${draft.description.trim()}`,
    `enabled: ${draft.enabled ? "true" : "false"}`
  ];
  const entrypoints = splitList(draft.entrypointsText) ?? [];
  if (draft.backendRef.trim()) {
    lines.push("backend:", `  ref: ${draft.backendRef.trim()}`);
  }
  if (entrypoints.length > 0) {
    lines.push(`entrypoints: [${entrypoints.join(", ")}]`);
  }
  const tools = splitList(draft.toolsText) ?? [];
  if (tools.length > 0) {
    lines.push("tools:", ...tools.map((tool) => `  - ${tool}`));
  }
  const mcpServers = splitList(draft.mcpServersText) ?? [];
  if (mcpServers.length > 0) {
    lines.push("mcpServers:", ...mcpServers.map((server) => `  - ${server}`));
  }
  lines.push("---", draft.instructions);
  return lines.join("\n");
}

function teamDraftFromRead(row: AgentTeamRow, team: JsonObject, instructions: string, rawMarkdown: string): TeamDraft {
  const maxParallelAgents = Number(objectValue(team).maxParallelAgents || row.maxParallelAgents || 4);
  return {
    mode: "form",
    target: row.target ?? parseAgentTarget(team.target) ?? "project",
    name: stringField(team, "name") || row.name,
    description: stringField(team, "description") || row.description,
    enabled: objectValue(team).enabled !== false,
    leader: stringField(team, "leader") || row.leader,
    membersText: teamMembersText(arrayObjects(team.members).length ? arrayObjects(team.members) : row.members),
    maxParallelAgents: String(Number.isFinite(maxParallelAgents) ? maxParallelAgents : 4),
    instructions,
    rawMarkdown
  };
}

function renderTeamDraftMarkdown(draft: TeamDraft): string {
  const members = parseTeamMembersText(draft.membersText);
  const lines = [
    "---",
    `name: ${draft.name.trim()}`,
    `description: ${draft.description.trim()}`,
    `enabled: ${draft.enabled ? "true" : "false"}`,
    `leader: ${draft.leader.trim()}`,
    `maxParallelAgents: ${Number(draft.maxParallelAgents) || 4}`,
    "members:",
    ...members.flatMap(renderTeamMemberMarkdown),
    "---",
    draft.instructions
  ];
  return lines.join("\n");
}

function parseTeamMembersText(value: string): TeamMemberInput[] {
  return value.split(/\r?\n/)
    .map((line) => line.trim())
    .filter(Boolean)
    .map((line) => {
      const [head, ...fields] = line.split("|").map((part) => part.trim());
      const safeHead = head ?? "";
      const [idPart = "", agentPart = ""] = safeHead.includes(":")
        ? safeHead.split(/:(.*)/s).slice(0, 2)
        : [safeHead, safeHead];
      let role = "";
      let description = "";
      let maxTurns: number | null = null;
      let runtimeRef: string | null = null;
      let runtimeProfileRevision: string | null = null;
      const runtimeOptions: Record<string, string> = {};
      const legacyFields: string[] = [];
      for (const field of fields) {
        const separator = field.indexOf("=");
        if (separator < 0) {
          legacyFields.push(field);
          continue;
        }
        const key = field.slice(0, separator).trim();
        const fieldValue = field.slice(separator + 1).trim();
        if (key === "role") role = fieldValue;
        else if (key === "description") description = fieldValue;
        else if (key === "maxTurns") {
          const parsed = Number(fieldValue);
          maxTurns = Number.isFinite(parsed) && parsed > 0 ? parsed : null;
        } else if (key === "runtime") runtimeRef = fieldValue || null;
        else if (key === "revision") {
          runtimeProfileRevision = /^\d+$/.test(fieldValue) && fieldValue !== "0" ? fieldValue : null;
        } else if (key.startsWith("option.") && key.length > "option.".length && fieldValue) {
          runtimeOptions[key.slice("option.".length)] = fieldValue;
        }
      }
      // Keep the pre-runtime positional syntax readable for existing drafts.
      if (legacyFields.length > 0) role ||= legacyFields[0] ?? "";
      if (legacyFields.length > 1) description ||= legacyFields[1] ?? "";
      if (legacyFields.length > 2 && maxTurns == null) {
        const parsed = Number(legacyFields[2]);
        maxTurns = Number.isFinite(parsed) && parsed > 0 ? parsed : null;
      }
      return {
        id: idPart.trim(),
        agent: (agentPart || idPart).trim(),
        runtimeRef,
        runtimeOptions,
        runtimeProfileRevision,
        role: role || null,
        description: description || null,
        maxTurns
      };
    })
    .filter((member) => stringField(member, "id") && stringField(member, "agent"));
}

function teamMembersText(members: JsonObject[]): string {
  return members.map((member) => {
    const base = `${stringField(member, "id")}: ${stringField(member, "agent")}`;
    const runtimeOptions = objectField(member, "runtimeOptions");
    const tail = [
      namedTeamMemberField("role", stringField(member, "role")),
      namedTeamMemberField("description", stringField(member, "description")),
      namedTeamMemberField("maxTurns", displayValue(objectValue(member).maxTurns)),
      namedTeamMemberField("runtime", stringField(member, "runtimeRef")),
      namedTeamMemberField("revision", displayValue(objectValue(member).runtimeProfileRevision)),
      ...Object.keys(runtimeOptions).sort().map((key) => namedTeamMemberField(`option.${key}`, stringField(runtimeOptions, key)))
    ].filter(Boolean);
    return tail.length ? `${base} | ${tail.join(" | ")}` : base;
  }).join("\n");
}

function namedTeamMemberField(name: string, value: string): string {
  return value ? `${name}=${value}` : "";
}

function renderTeamMemberMarkdown(member: TeamMemberInput): string[] {
  const lines = [
    `  - id: ${JSON.stringify(member.id)}`,
    `    agent: ${JSON.stringify(member.agent)}`
  ];
  if (member.runtimeRef) lines.push(`    runtimeRef: ${JSON.stringify(member.runtimeRef)}`);
  if (member.runtimeProfileRevision) lines.push(`    runtimeProfileRevision: ${member.runtimeProfileRevision}`);
  const options = member.runtimeOptions ?? {};
  if (Object.keys(options).length > 0) {
    lines.push("    runtimeOptions:");
    for (const key of Object.keys(options).sort()) {
      lines.push(`      ${JSON.stringify(key)}: ${JSON.stringify(options[key])}`);
    }
  }
  if (member.role) lines.push(`    role: ${JSON.stringify(member.role)}`);
  if (member.description) lines.push(`    description: ${JSON.stringify(member.description)}`);
  if (member.maxTurns) lines.push(`    maxTurns: ${member.maxTurns}`);
  return lines;
}

function teamMemberSummary(member: JsonObject): string {
  const id = stringField(member, "id");
  const agent = stringField(member, "agent");
  const role = stringField(member, "role");
  return [id && agent && id !== agent ? `${id}:${agent}` : id || agent, role].filter(Boolean).join(" ");
}

function parseAgentTarget(value: unknown): BackendConfigTarget | null {
  return value === "project" || value === "profile" ? value : null;
}

function agentTargetValue(value: string): BackendConfigTarget {
  return value === "profile" ? "profile" : "project";
}

function targetLabel(value: BackendConfigTarget | null): string {
  if (value === "project") return "Project";
  if (value === "profile") return "Profile";
  return "";
}

function agentStateLabel(value: AgentDefinitionState): string {
  if (value === "shadowed") return "Shadowed";
  if (value === "disabled") return "Disabled";
  return "Active";
}

function teamStateLabel(value: AgentDefinitionState): string {
  return agentStateLabel(value);
}

function agentRowMetadata(row: AgentDefinitionRow): string {
  const values = [targetLabel(row.target), row.sourceLabel];
  if (row.state !== "active") values.push(agentStateLabel(row.state));
  if (!row.mutable) values.push("Read-only");
  if (row.diagnostics.length > 0) values.push("Diagnostics");
  return values.filter(Boolean).join(" · ");
}

function teamRowMetadata(row: AgentTeamRow): string {
  const values = [targetLabel(row.target), row.sourceLabel, `leader ${row.leader || "-"}`, `${row.members.length} members`];
  if (row.state !== "active") values.push(teamStateLabel(row.state));
  if (!row.mutable) values.push("Read-only");
  if (row.diagnostics.length > 0) values.push("Diagnostics");
  return values.filter(Boolean).join(" · ");
}

function runtimeProfileMetadata(row: RuntimeProfileRow): string {
  const backend = row.backendRef ? `backend ${row.backendRef}` : "";
  return [row.provenance, runtimeProfileSource(row), runtimeReadinessLabel(row.healthStatus), formatRuntimeCheckedAt(row.checkedAtMs), backend].filter(Boolean).join(" · ");
}

function runtimeProfileSource(row: RuntimeProfileRow): string {
  return row.sourceTargets.length > 0
    ? row.sourceTargets.map(targetLabel).join(" + ")
    : row.generated ? "Generated" : "Configured";
}

function runtimeProvenanceForKind(runtime: string): string {
  if (runtime === "acp") return "ACP";
  if (runtime === "native") return "Native";
  return "Unsupported";
}

function runtimeReadinessLabel(status: string): string {
  if (status === "needsAuth") return "Needs auth";
  if (status === "ready") return "Ready";
  if (status === "missing") return "Missing";
  if (status === "unsupported") return "Unsupported";
  if (status === "error") return "Error";
  return "Unchecked";
}

function readinessStageLabel(id: string): string {
  return id.split(/[-_]/g).filter(Boolean).map((part) => `${part.charAt(0).toUpperCase()}${part.slice(1)}`).join(" ");
}

async function requestTab(client: GatewayClient, tab: CapabilityTab, scope: GatewayRequestScope) {
  if (tab === "agents") {
    const backendList = await client.request("backend/list", { scope });
    const [agents, teams, runtimeProfiles] = await Promise.all([
      client.request("agent/list", { scope }),
      client.request("team/list", { scope }),
      client.request("runtime/profile/list", { scope })
    ]);
    return {
      ...objectValue(agents),
      ...objectValue(backendList),
      teams: objectValue(teams),
      runtimeProfiles: objectValue(runtimeProfiles)
    };
  }
  if (tab === "skills") return client.request("skill/list", { scope });
  if (tab === "plugins") return client.request("plugin/list", { scope });
  if (tab === "mcp") return client.request("mcp/list", { scope });
  return client.request("tool/list", { scope });
}

function manageAcpBackend(
  client: GatewayClient,
  scope: GatewayRequestScope,
  id: string,
  action: ManagedBackendAction
) {
  const params = { id, scope };
  if (action === "install") return client.request("backend/install", params);
  if (action === "repair") return client.request("backend/repair", params);
  return client.request("backend/upgrade", params);
}

async function setCapabilityEnabled(client: GatewayClient | null, scope: GatewayRequestScope | null, tab: CapabilityTab, row: CapabilityRow, enabled: boolean) {
  if (!client || !scope) return;
  if (tab === "skills") return client.request("skill/setEnabled", { name: row.name, enabled, scope });
  if (tab === "plugins") return client.request("plugin/setEnabled", {
    selector: pluginSelector(row),
    scopeName: pluginEnablementScope(row),
    enabled,
    scope
  });
  if (tab === "mcp") return client.request("mcp/setEnabled", { name: row.name, enabled, scope });
}

async function startOAuth(
  client: GatewayClient,
  scope: GatewayRequestScope,
  name: string,
  onOAuthSession: (sessionId: string | null) => void,
  mutate: (action: () => Promise<unknown>) => Promise<boolean>
) {
  await mutate(async () => {
    const result = await client.request("mcp/oauth/start", { name, scope });
    const url = stringField(result, "authorizationUrl");
    const sessionId = stringField(result, "sessionId");
    if (url) window.open(url, "_blank", "noopener,noreferrer");
    if (sessionId) onOAuthSession(sessionId);
    return result;
  });
}

type CodexConnectTarget = { componentId: string; kind: "app" | "mcp" };

function codexConnectTarget(detail: JsonObject | null): CodexConnectTarget | null {
  if (!detail) return null;
  const plugin = Object.keys(objectField(detail, "manifest")).length > 0
    ? objectField(detail, "manifest")
    : objectField(detail, "plugin");
  const app = arrayObjects(plugin.apps)[0];
  const appId = app ? stringField(app, "id") || stringField(app, "name") : "";
  if (appId) return { componentId: appId, kind: "app" };
  const mcp = arrayObjects(plugin.mcpServers)[0];
  const mcpId = mcp ? stringField(mcp, "name") || stringField(mcp, "id") : "";
  return mcpId ? { componentId: mcpId, kind: "mcp" } : null;
}

async function startPluginConnect(
  client: GatewayClient,
  scope: GatewayRequestScope,
  selector: string,
  target: CodexConnectTarget,
  onSession: (sessionId: string | null) => void,
  mutate: (action: () => Promise<unknown>, options?: MutationOptions) => Promise<boolean>
) {
  await mutate(async () => {
    const result = await client.request("plugin/connect/start", {
      selector,
      componentId: target.componentId,
      kind: target.kind,
      scope
    });
    const url = stringField(result, "installUrl") || stringField(result, "authorizationUrl");
    const sessionId = stringField(result, "sessionId");
    if (url) window.open(url, "_blank", "noopener,noreferrer");
    if (sessionId) onSession(sessionId);
    return result;
  }, { notice: "Plugin connection started.", refresh: false });
}

function skillRowsFromData(data: JsonObject | null): SkillRow[] {
  if (!data) return [];
  return arrayObjects(data.skills).map((skill) => {
    const location = stringField(skill, "location");
    const id = stringField(skill, "id") || location || stringField(skill, "name");
    const readiness = stringField(skill, "readiness_status") || "available";
    const source = stringField(skill, "source") || "unknown";
    const sourceLabel = skillSourceDisplayLabel(stringField(skill, "source_label") || source);
    return {
      id,
      name: stringField(skill, "name"),
      description: stringField(skill, "description"),
      enabled: boolField(skill, "enabled"),
      status: readiness,
      badges: [sourceLabel, readiness].filter(Boolean),
      raw: skill,
      collisionGroup: arrayStrings(skill.collision_group),
      issues: arrayStrings(skill.issues),
      location,
      missingCredentialFiles: arrayStrings(skill.missing_credential_files),
      missingEnvVars: arrayStrings(skill.missing_required_environment_variables),
      promptVisible: boolField(skill, "prompt_visible"),
      readiness,
      requiredTools: arrayStrings(skill.required_tools),
      requiredToolsets: arrayStrings(skill.required_toolsets),
      skillDir: stringField(skill, "skill_dir"),
      source,
      sourceLabel,
      supported: skill.supported_on_current_platform !== false,
      tags: arrayStrings(skill.tags)
    };
  });
}

function readinessLabel(value: string): string {
  if (value === "setup_needed") return "Setup Needed";
  if (value === "unsupported") return "Unsupported";
  if (value === "available" || value === "ready") return "Available";
  return labelForKey(value || "unknown");
}

function skillSourceDisplayLabel(value: string): string {
  switch (value.trim()) {
    case "project":
    case "agents":
    case "Project":
      return "Project";
    case "explicit":
    case "global":
    case "agents_global":
    case "config":
    case "install_source":
    case "User":
      return "User";
    case "plugin":
    case "system":
    case "builtin":
    case "built_in":
    case "core":
    case "System":
      return "System";
    default:
      return "";
  }
}

function skillRowMetadata(row: SkillRow): string {
  const values = [row.sourceLabel];
  if (!row.supported) {
    values.push("Unsupported");
  } else if (row.readiness !== "available" && row.readiness !== "ready") {
    values.push(readinessLabel(row.readiness));
  }
  if (row.collisionGroup.length > 0) values.push("Collision");
  return values.filter(Boolean).join(" · ");
}

function skillDetailSummary(row: SkillRow): string {
  return [row.sourceLabel, readinessLabel(row.readiness)].filter(Boolean).join(" · ");
}

function mutableTargetForSkill(row: SkillRow): "global" | "project" | null {
  if (row.source === "project") return "project";
  if (row.source === "global") return "global";
  return null;
}

function mutableMarkdownTargetForSkill(row: SkillRow): "global" | "project" | null {
  if (!row.location || !row.skillDir) return null;
  const location = normalizePathText(row.location);
  const skillDir = normalizePathText(row.skillDir);
  if (location !== `${skillDir}/SKILL.md`) return null;
  return mutableTargetForSkill(row);
}

function skillMarkdownEditReason(row: SkillRow, loading: boolean): string {
  if (loading) return "SKILL.md is still loading";
  if (!mutableTargetForSkill(row)) return "Only Project/Profile skills can be edited here";
  return "Only SKILL.md package files can be edited here";
}

function linkedFilesText(value: JsonObject): string {
  return Object.entries(value)
    .flatMap(([group, files]) => arrayStrings(files).map((file) => file.startsWith(`${group}/`) ? file : `${group}/${file}`))
    .join(", ");
}

function boundedText(value: string, limit: number): string {
  return value.length > limit ? `${value.slice(0, limit)}...[truncated]` : value;
}

function rowsForTab(tab: CapabilityTab, data: JsonObject | null): CapabilityRow[] {
  if (!data) return [];
  if (tab === "skills") {
    return arrayObjects(data.skills).map((skill) => ({
      id: stringField(skill, "id") || stringField(skill, "location") || stringField(skill, "name"),
      name: stringField(skill, "name"),
      description: stringField(skill, "description"),
      enabled: boolField(skill, "enabled"),
      status: stringField(skill, "readiness_status") || "ready",
      badges: [
        skillSourceDisplayLabel(stringField(skill, "source_label") || stringField(skill, "source")),
        stringField(skill, "category")
      ].filter(Boolean),
      raw: skill
    }));
  }
  if (tab === "plugins") {
    return arrayObjects(data.plugins).map((plugin) => ({
      id: stringField(plugin, "selector") || stringField(plugin, "source_id") || stringField(plugin, "name"),
      name: stringField(plugin, "name"),
      description: stringField(plugin, "description"),
      enabled: boolField(plugin, "enabled"),
      status: stringField(plugin, "status") || stringField(plugin, "readiness") || "Installed",
      badges: [
        pluginAuthorityLabel(plugin),
        targetLabel(parseAgentTarget(stringField(plugin, "scope_name"))),
        stringField(plugin, "manifest_kind"),
        stringField(plugin, "source_kind")
      ].filter(Boolean),
      raw: plugin
    }));
  }
  if (tab === "mcp") {
    return arrayObjects(data.servers).map((server) => {
      const transport = objectField(server, "transport");
      return {
        id: stringField(server, "name"),
        name: stringField(server, "name"),
        description: stringField(transport, "url") || stringField(transport, "command"),
        enabled: boolField(server, "enabled"),
        status: stringField(transport, "kind") || "mcp",
        badges: [stringField(server, "sourceKind"), boolField(server, "required") ? "required" : ""].filter(Boolean),
        raw: server
      };
    });
  }
  const modes = objectField(data, "modes");
  return arrayObjects(data.toolsets).map((toolset) => ({
    id: stringField(toolset, "name"),
    name: stringField(toolset, "name"),
    description: stringField(toolset, "description") || arrayStrings(toolset.tools).join(", "),
    enabled: modeEnabled({ ...toolset, modes }, "default"),
    status: stringField(toolset, "source") || "toolset",
    badges: [`default:${modeEnabled({ ...toolset, modes }, "default") ? "on" : "off"}`, `plan:${modeEnabled({ ...toolset, modes }, "plan") ? "on" : "off"}`],
    raw: { ...toolset, modes }
  }));
}

type CapabilityListEntry =
  | { kind: "row"; row: CapabilityRow }
  | { kind: "section"; label: string; description?: string };

function capabilityListEntries(tab: CapabilityTab, rows: CapabilityRow[]): CapabilityListEntry[] {
  if (tab !== "plugins") return rows.map((row) => ({ kind: "row", row }));
  const installed = rows.filter((row) => boolField(row.raw, "installed") || pluginAuthorityKind(row) !== "codex");
  const codexCatalog = rows.filter((row) => pluginAuthorityKind(row) === "codex" && !boolField(row.raw, "installed"));
  return [
    { kind: "section", label: "Installed" },
    ...installed.map((row): CapabilityListEntry => ({ kind: "row", row })),
    { kind: "section", label: "Codex Catalog" },
    ...codexCatalog.map((row): CapabilityListEntry => ({ kind: "row", row })),
    { kind: "section", label: "Psychevo Catalog", description: "Use Install plugin to inspect and add native packages." },
    { kind: "section", label: "Marketplace Management", description: "Codex marketplace sources remain authority-qualified." }
  ];
}

function modeEnabled(row: JsonObject, mode: "default" | "plan"): boolean {
  const modes = objectField(row, "modes");
  const modeConfig = objectField(modes, mode);
  const name = stringField(row, "name");
  if (arrayStrings(modeConfig.disabled_toolsets).includes(name)) return false;
  const enabled = arrayStrings(modeConfig.enabled_toolsets);
  return enabled.length === 0 || enabled.includes(name);
}

function toolsetModeMutable(row: CapabilityRow): boolean {
  const value = objectValue(row.raw).mode_mutable;
  return typeof value === "boolean" ? value : true;
}

function pluginSelector(row: CapabilityRow): string {
  return stringField(row.raw, "selector") || row.id;
}

function pluginAuthorityKind(row: CapabilityRow): string {
  return stringField(objectField(row.raw, "authority"), "kind") || "psychevo";
}

function pluginAuthorityLabel(plugin: JsonObject): string {
  const kind = stringField(objectField(plugin, "authority"), "kind");
  if (kind === "codex") return "Codex";
  if (kind === "psychevo") return "Psychevo";
  return "";
}

function PluginComponentStatuses({
  detail,
  row
}: {
  detail: { loading: boolean; value: JsonObject | null; error: string | null } | null;
  row: CapabilityRow;
}) {
  if (detail?.loading) return <div className="capabilityEmpty">Loading component evidence</div>;
  if (detail?.error) return <div className="capabilityBanner is-error">{detail.error}</div>;
  const record = detail?.value ? objectField(detail.value, "plugin") : row.raw;
  const statuses = arrayObjects(record.component_statuses ?? record.componentStatuses);
  if (statuses.length === 0) {
    return <div className="capabilityEmpty">No declared plugin components</div>;
  }
  const hasReadyComponent = statuses.some((status) => stringField(status, "readiness") === "ready");
  const hasLimitedComponent = statuses.some((status) => [
    "unavailable",
    "unsupported",
    "needs_trust",
    "needs_setup"
  ].includes(stringField(status, "readiness")));
  return (
    <section className="skillDiagnostics" aria-label="Plugin component compatibility">
      {hasReadyComponent && hasLimitedComponent && (
        <div className="capabilityBanner">Partially compatible · usable components remain available</div>
      )}
      {statuses.map((status) => {
        const component = stringField(status, "component");
        const level = stringField(status, "highestLevel") || stringField(status, "highest_level");
        const owner = stringField(status, "executionOwner") || stringField(status, "execution_owner");
        const readiness = stringField(status, "readiness");
        const reason = stringField(status, "reason");
        return (
          <div className="skillIssue" key={`${component}:${owner}`}>
            <strong>{readinessStageLabel(component)}</strong>
            <span>{[readinessStageLabel(level), readinessStageLabel(owner), readinessStageLabel(readiness)].filter(Boolean).join(" · ")}</span>
            {reason && <small>{reason}</small>}
          </div>
        );
      })}
    </section>
  );
}

function pluginMutationScope(row: CapabilityRow): "profile" | "project" {
  const projected = stringField(row.raw, "scope_name");
  if (projected === "project") return "project";
  if (projected === "profile") return "profile";
  return stringField(row.raw, "scope") === "local" ? "project" : "profile";
}

function pluginEnablementScope(row: CapabilityRow): "profile" | "project" {
  const projected = stringField(row.raw, "enablement_scope_name");
  if (projected === "project") return "project";
  if (projected === "profile") return "profile";
  return pluginMutationScope(row);
}

function pluginEnablementMutable(row: CapabilityRow): boolean {
  const value = objectValue(row.raw).enablement_mutable;
  return typeof value === "boolean" ? value : true;
}

function pluginPackageMutable(row: CapabilityRow): boolean {
  const value = objectValue(row.raw).package_mutable;
  return typeof value === "boolean" ? value : true;
}

function pluginRemovable(row: CapabilityRow): boolean {
  const value = objectValue(row.raw).removable;
  return typeof value === "boolean" ? value : true;
}

function toolsetRemovable(row: CapabilityRow): boolean {
  const value = objectValue(row.raw).removable;
  return typeof value === "boolean" ? value : stringField(row.raw, "source") === "custom";
}

function isBuiltInToolsetName(value: string): boolean {
  return ["coding-core", "web"].includes(value.trim());
}

function PluginInspectionSummary({ inspection }: { inspection: JsonObject }) {
  const lanes = arrayStrings(inspection.target_lanes);
  const unsupported = arrayStrings(inspection.unsupported_lanes);
  const stages = arrayObjects(inspection.stages);
  return (
    <section className="pluginInspection" aria-label="Plugin inspection">
      <div>
        <strong>{stringField(inspection, "name") || "Plugin"}</strong>
        <span>{[stringField(inspection, "framework"), stringField(inspection, "status")].filter(Boolean).join(" / ")}</span>
      </div>
      <dl className="capabilityKeyValues pluginInspectionGrid">
        <div><dt>Source</dt><dd>{stringField(inspection, "source_kind") || "local"}</dd></div>
        <div><dt>Mode</dt><dd>{stringField(inspection, "adapter_mode") || "manifest_only"}</dd></div>
        {lanes.length > 0 && <div><dt>Lanes</dt><dd>{lanes.join(", ")}</dd></div>}
        {unsupported.length > 0 && <div><dt>Unsupported</dt><dd>{unsupported.join(", ")}</dd></div>}
      </dl>
      {stages.length > 0 && (
        <div className="pluginInspectionStages">
          {stages.slice(0, 5).map((stage, index) => (
            <span className="capabilityChip" key={`${stringField(stage, "stage")}-${index}`}>
              {stringField(stage, "stage")}: {stringField(stage, "status")}
            </span>
          ))}
        </div>
      )}
    </section>
  );
}

function pluginKindValue(value: string): PluginInstallDraft["kind"] {
  return value === "git" || value === "npm" ? value : "local";
}

function pluginAdapterModeValue(value: string): PluginInstallDraft["adapterMode"] {
  if (value === "adapter_host" || value === "disabled") return value;
  return "manifest_only";
}

function KeyValueView({ value }: { value: JsonObject }) {
  const entries = Object.entries(value).filter(([, entry]) => entry !== null && entry !== undefined);
  return (
    <dl className="capabilityKeyValues">
      {entries.slice(0, 14).map(([key, entry]) => (
        <div key={key}>
          <dt>{labelForKey(key)}</dt>
          <dd>{displayValue(entry)}</dd>
        </div>
      ))}
    </dl>
  );
}

function objectValue(value: unknown): JsonObject {
  return value && typeof value === "object" && !Array.isArray(value) ? value as JsonObject : {};
}

function objectField(value: unknown, key: string): JsonObject {
  const object = objectValue(value);
  return objectValue(object[key]);
}

function arrayObjects(value: unknown): JsonObject[] {
  return Array.isArray(value) ? value.map(objectValue) : [];
}

function arrayStrings(value: unknown): string[] {
  return Array.isArray(value) ? value.filter((entry): entry is string => typeof entry === "string") : [];
}

function stringField(value: unknown, key: string): string {
  const entry = objectValue(value)[key];
  return typeof entry === "string" ? entry : "";
}

function optionalString(value: unknown): string | null {
  return typeof value === "string" ? value : null;
}

function boolField(value: unknown, key: string): boolean {
  return objectValue(value)[key] === true;
}

function numberField(value: unknown, key: string): number {
  const entry = objectValue(value)[key];
  return typeof entry === "number" && Number.isFinite(entry) ? entry : 0;
}

function decimalRevisionField(value: unknown, key: string): string {
  const entry = objectValue(value)[key];
  if (typeof entry !== "string" || !/^(?:0|[1-9][0-9]*)$/.test(entry)) return "0";
  try {
    return BigInt(entry) <= 18_446_744_073_709_551_615n ? entry : "0";
  } catch {
    return "0";
  }
}

function nullableNumberField(value: unknown, key: string): number | null {
  const entry = objectValue(value)[key];
  return typeof entry === "number" && Number.isFinite(entry) ? entry : null;
}

function displayValue(value: unknown): string {
  if (typeof value === "string") return value;
  if (typeof value === "number" || typeof value === "boolean") return String(value);
  if (Array.isArray(value)) return value.map(displayValue).join(", ");
  if (value && typeof value === "object") return JSON.stringify(value);
  return "";
}

function splitList(value: string): string[] | null {
  const values = value.split(",").map((entry) => entry.trim()).filter(Boolean);
  return values.length ? values : null;
}

function tabLabel(tab: CapabilityTab): string {
  return TABS.find((item) => item.id === tab)?.label ?? tab;
}

function createActionLabel(tab: CapabilityTab): string {
  if (tab === "agents") return "Create agent";
  if (tab === "skills") return "Install skill";
  if (tab === "plugins") return "Install plugin";
  if (tab === "mcp") return "Add MCP server";
  return "Create toolset";
}

function hasCapabilityRowSwitch(tab: CapabilityTab): boolean {
  return tab === "plugins" || tab === "mcp";
}

function rowKindLabel(tab: CapabilityTab): string {
  if (tab === "agents") return "Agent";
  if (tab === "plugins") return "Plugin";
  if (tab === "mcp") return "MCP";
  if (tab === "tools") return "Toolset";
  return "Skill";
}

function CapabilityBadges({ row }: { row: CapabilityRow }) {
  if (row.badges.length === 0) return null;
  return (
    <span className="capabilityRowMeta">
      {row.badges.slice(0, 2).map((badge, index) => <span className="capabilityChip" key={`${index}:${badge}`}>{badge}</span>)}
    </span>
  );
}

function RowDescription({ fallback, value }: { fallback: string; value: string }) {
  const text = value || fallback;
  return <small title={text}>{text}</small>;
}

function labelForKey(value: string): string {
  return value.replace(/_/g, " ").replace(/([a-z])([A-Z])/g, "$1 $2");
}

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

function confirmAction(message: string): boolean {
  return window.confirm(message);
}
