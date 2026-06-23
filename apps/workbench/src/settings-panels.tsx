import { useEffect, useMemo, useRef, useState, type ReactNode, type RefObject } from "react";
import {
  Activity,
  Archive,
  ArrowLeft,
  BarChart3,
  Bot,
  BrainCircuit,
  Bug,
  Edit3,
  MessageCircle,
  Moon,
  Palette,
  Play,
  PlugZap,
  Plus,
  Settings2,
  RotateCcw,
  Save,
  Search,
  Sun,
  Trash2,
  Wrench,
  X
} from "lucide-react";
import type { GatewayClient } from "@psychevo/client";
import type {
  AuxiliaryModelAssignmentView,
  ChannelUpdateParams,
  ChannelWechatQrPollResult,
  ChannelWechatQrStartResult,
  ModelOptionView,
  ModelProviderView,
  ModelSettingsResult,
  SettingsReadResult,
  SessionSummary
} from "@psychevo/protocol";
import { prettyJson } from "./data";
import type {
  Appearance,
  BackendCommandJson,
  BackendDraft,
  SessionBrowserWorkspaceState,
  SettingsSection,
  WorkbenchUsageStats,
  WorkbenchBackend,
  WorkbenchBackendDoctor,
  WorkbenchChannel,
  WorkbenchChannelDoctor,
  WorkbenchChannelSource
} from "./types";
import { ModelReasoningSelector, reasoningEffortsForModelOption } from "./model-picker";

const BACKEND_ENTRYPOINTS = ["peer", "subagent"] as const;
const BACKEND_CLIENT_CAPABILITIES = ["fs.read", "fs.write", "terminal"] as const;
type ChannelUpdateDraft = Partial<Omit<ChannelUpdateParams, "id" | "scope">>;
type ChannelSettingsControls = SettingsReadResult["controls"];
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
const SETTINGS_SECTIONS: Array<{ id: SettingsSection; label: string; description: string }> = [
  { id: "appearance", label: "Appearance", description: "Theme" },
  { id: "models", label: "Models", description: "Providers and auxiliary models" },
  { id: "usage", label: "Usage", description: "Tokens and cost" },
  { id: "debug", label: "Debug", description: "Developer diagnostics" },
  { id: "agents", label: "Agents", description: "Profile ACP backends" },
  { id: "channels", label: "Channels", description: "Messaging connections" },
  { id: "archived", label: "Archived sessions", description: "Restore or delete" }
];
export function SettingsPage({
  appearance,
  archivedSessions,
  backendDraft,
  backendDoctor,
  backends,
  channelDoctor,
  channels,
  client,
  controls,
  debugEnabled,
  disabled,
  section,
  usageStats,
  usageStatsError,
  usageStatsLoading,
  onAppearanceChange,
  onCancelBackendEdit,
  onChangeBackendDraft,
  onDebugChange,
  onDeleteArchivedSession,
  onDeleteBackend,
  onDeleteChannel,
  onDoctorChannel,
  onDoctorChannels,
  onDoctorBackend,
  onEditBackend,
  onNewBackend,
  onOpenTranscript,
  onLoadChannelSources,
  onModelAssignmentSaved,
  onModelCatalogLoaded,
  onPollWechatQrSetup,
  onRestoreArchivedSession,
  onSaveBackendDraft,
  onSectionChange,
  onRefreshUsageStats,
  onSetBackendEnabled,
  onSetBackendEntrypoints,
  onSetChannelEnabled,
  onStartWechatQrSetup,
  onUpdateChannel,
  sessionBrowserWorkspaces,
  workdir
}: {
  appearance: Appearance;
  archivedSessions: SessionSummary[];
  backendDraft: BackendDraft | null;
  backendDoctor: Record<string, WorkbenchBackendDoctor>;
  backends: WorkbenchBackend[];
  channelDoctor: Record<string, WorkbenchChannelDoctor>;
  channels: WorkbenchChannel[];
  client: GatewayClient | null;
  controls: ChannelSettingsControls;
  debugEnabled: boolean;
  disabled: boolean;
  section: SettingsSection;
  usageStats: WorkbenchUsageStats | null;
  usageStatsError: string | null;
  usageStatsLoading: boolean;
  onAppearanceChange(value: Appearance): void;
  onCancelBackendEdit(): void;
  onChangeBackendDraft(draft: BackendDraft): void;
  onDebugChange(value: boolean): void;
  onDeleteArchivedSession(threadId: string): void;
  onDeleteBackend(backend: WorkbenchBackend): void;
  onDeleteChannel(channel: WorkbenchChannel): Promise<void>;
  onDoctorChannel(channel: WorkbenchChannel): void;
  onDoctorChannels(): void;
  onDoctorBackend(backend: WorkbenchBackend): void;
  onEditBackend(backend: WorkbenchBackend): void;
  onNewBackend(): void;
  onOpenTranscript(): void;
  onLoadChannelSources(channel: WorkbenchChannel): Promise<WorkbenchChannelSource[]>;
  onModelAssignmentSaved(): Promise<void>;
  onModelCatalogLoaded(options: ModelOptionView[]): void;
  onPollWechatQrSetup(sessionId: string): Promise<ChannelWechatQrPollResult>;
  onRestoreArchivedSession(threadId: string): void;
  onSaveBackendDraft(draft: BackendDraft): void;
  onSectionChange(value: SettingsSection): void;
  onRefreshUsageStats(): void;
  onSetBackendEnabled(backend: WorkbenchBackend, enabled: boolean): void;
  onSetBackendEntrypoints(backend: WorkbenchBackend, entrypoints: string[]): void;
  onSetChannelEnabled(channel: WorkbenchChannel, enabled: boolean): void;
  onStartWechatQrSetup(): Promise<ChannelWechatQrStartResult>;
  onUpdateChannel(channel: WorkbenchChannel, draft: ChannelUpdateDraft): Promise<WorkbenchChannel>;
  sessionBrowserWorkspaces: SessionBrowserWorkspaceState[];
  workdir: string;
}) {
  const [query, setQuery] = useState("");
  const active = SETTINGS_SECTIONS.find((item) => item.id === section) ?? SETTINGS_SECTIONS[0]!;
  const normalizedQuery = query.trim().toLowerCase();
  const primarySections = SETTINGS_SECTIONS.filter((item) => item.id !== "archived");
  const archivedSection = SETTINGS_SECTIONS.find((item) => item.id === "archived")!;
  const sectionMatches = (item: (typeof SETTINGS_SECTIONS)[number]) => (
    !normalizedQuery
    || item.label.toLowerCase().includes(normalizedQuery)
    || item.description.toLowerCase().includes(normalizedQuery)
  );
  const visiblePrimarySections = primarySections.filter(sectionMatches);
  const showArchivedSection = sectionMatches(archivedSection);
  return (
    <section className="centerPage settingsPage" aria-label="Settings">
      <div className="settingsShell">
        <aside className="settingsNav" aria-label="Settings sections">
          <div className="settingsNavTop">
            <button className="settingsBackButton" onClick={onOpenTranscript} type="button">
              <ArrowLeft size={15} />
              <span>Back to app</span>
            </button>
            <label className="settingsSearch">
              <Search size={14} aria-hidden />
              <input
                aria-label="Search settings"
                onChange={(event) => setQuery(event.currentTarget.value)}
                placeholder="Search settings"
                type="search"
                value={query}
              />
            </label>
          </div>
          <div className="settingsNavGroups">
            {visiblePrimarySections.map((item) => (
              <button
                aria-current={item.id === section ? "page" : undefined}
                className={item.id === section ? "is-selected" : ""}
                key={item.id}
                onClick={() => onSectionChange(item.id)}
                type="button"
              >
                {settingsSectionIcon(item.id, 15)}
                <span>{item.label}</span>
              </button>
            ))}
            {visiblePrimarySections.length === 0 && !showArchivedSection && (
              <p className="settingsNavEmpty">No settings found</p>
            )}
          </div>
          <div className="settingsNavFooter">
            {showArchivedSection && (
              <button
                aria-current={section === archivedSection.id ? "page" : undefined}
                className={section === archivedSection.id ? "is-selected" : ""}
                onClick={() => onSectionChange(archivedSection.id)}
                type="button"
              >
                {settingsSectionIcon(archivedSection.id, 15)}
                <span>{archivedSection.label}</span>
              </button>
            )}
          </div>
        </aside>
        <div className="settingsContent">
          <div className="settingsContentInner">
            <header className="settingsSectionHeader">
              <span>{settingsSectionIcon(active.id, 17)}</span>
              <div>
                <h3>{active.label}</h3>
              </div>
            </header>
            <SettingsSectionPanel
              appearance={appearance}
              archivedSessions={archivedSessions}
              backendDraft={backendDraft}
              backendDoctor={backendDoctor}
              backends={backends}
              channelDoctor={channelDoctor}
              channels={channels}
              client={client}
              controls={controls}
              debugEnabled={debugEnabled}
              disabled={disabled}
              section={section}
              usageStats={usageStats}
              usageStatsError={usageStatsError}
              usageStatsLoading={usageStatsLoading}
              onAppearanceChange={onAppearanceChange}
              onCancelBackendEdit={onCancelBackendEdit}
              onChangeBackendDraft={onChangeBackendDraft}
              onDebugChange={onDebugChange}
              onDeleteArchivedSession={onDeleteArchivedSession}
              onDeleteBackend={onDeleteBackend}
              onDeleteChannel={onDeleteChannel}
              onDoctorChannel={onDoctorChannel}
              onDoctorChannels={onDoctorChannels}
              onDoctorBackend={onDoctorBackend}
              onEditBackend={onEditBackend}
              onNewBackend={onNewBackend}
              onLoadChannelSources={onLoadChannelSources}
              onModelAssignmentSaved={onModelAssignmentSaved}
              onModelCatalogLoaded={onModelCatalogLoaded}
              onPollWechatQrSetup={onPollWechatQrSetup}
              onRestoreArchivedSession={onRestoreArchivedSession}
              onSaveBackendDraft={onSaveBackendDraft}
              onRefreshUsageStats={onRefreshUsageStats}
              onSetBackendEnabled={onSetBackendEnabled}
              onSetBackendEntrypoints={onSetBackendEntrypoints}
              onSetChannelEnabled={onSetChannelEnabled}
              onStartWechatQrSetup={onStartWechatQrSetup}
              onUpdateChannel={onUpdateChannel}
              sessionBrowserWorkspaces={sessionBrowserWorkspaces}
              workdir={workdir}
            />
          </div>
        </div>
      </div>
    </section>
  );
}

function SettingsSectionPanel({
  appearance,
  archivedSessions,
  backendDraft,
  backendDoctor,
  backends,
  channelDoctor,
  channels,
  client,
  controls,
  debugEnabled,
  disabled,
  section,
  usageStats,
  usageStatsError,
  usageStatsLoading,
  onAppearanceChange,
  onCancelBackendEdit,
  onChangeBackendDraft,
  onDebugChange,
  onDeleteArchivedSession,
  onDeleteBackend,
  onDeleteChannel,
  onDoctorChannel,
  onDoctorChannels,
  onDoctorBackend,
  onEditBackend,
  onNewBackend,
  onLoadChannelSources,
  onModelAssignmentSaved,
  onModelCatalogLoaded,
  onPollWechatQrSetup,
  onRefreshUsageStats,
  onRestoreArchivedSession,
  onSaveBackendDraft,
  onSetBackendEnabled,
  onSetBackendEntrypoints,
  onSetChannelEnabled,
  onStartWechatQrSetup,
  onUpdateChannel,
  sessionBrowserWorkspaces,
  workdir
}: {
  appearance: Appearance;
  archivedSessions: SessionSummary[];
  backendDraft: BackendDraft | null;
  backendDoctor: Record<string, WorkbenchBackendDoctor>;
  backends: WorkbenchBackend[];
  channelDoctor: Record<string, WorkbenchChannelDoctor>;
  channels: WorkbenchChannel[];
  client: GatewayClient | null;
  controls: ChannelSettingsControls;
  debugEnabled: boolean;
  disabled: boolean;
  section: SettingsSection;
  usageStats: WorkbenchUsageStats | null;
  usageStatsError: string | null;
  usageStatsLoading: boolean;
  onAppearanceChange(value: Appearance): void;
  onCancelBackendEdit(): void;
  onChangeBackendDraft(draft: BackendDraft): void;
  onDebugChange(value: boolean): void;
  onDeleteArchivedSession(threadId: string): void;
  onDeleteBackend(backend: WorkbenchBackend): void;
  onDeleteChannel(channel: WorkbenchChannel): Promise<void>;
  onDoctorChannel(channel: WorkbenchChannel): void;
  onDoctorChannels(): void;
  onDoctorBackend(backend: WorkbenchBackend): void;
  onEditBackend(backend: WorkbenchBackend): void;
  onNewBackend(): void;
  onLoadChannelSources(channel: WorkbenchChannel): Promise<WorkbenchChannelSource[]>;
  onModelAssignmentSaved(): Promise<void>;
  onModelCatalogLoaded(options: ModelOptionView[]): void;
  onPollWechatQrSetup(sessionId: string): Promise<ChannelWechatQrPollResult>;
  onRefreshUsageStats(): void;
  onRestoreArchivedSession(threadId: string): void;
  onSaveBackendDraft(draft: BackendDraft): void;
  onSetBackendEnabled(backend: WorkbenchBackend, enabled: boolean): void;
  onSetBackendEntrypoints(backend: WorkbenchBackend, entrypoints: string[]): void;
  onSetChannelEnabled(channel: WorkbenchChannel, enabled: boolean): void;
  onStartWechatQrSetup(): Promise<ChannelWechatQrStartResult>;
  onUpdateChannel(channel: WorkbenchChannel, draft: ChannelUpdateDraft): Promise<WorkbenchChannel>;
  sessionBrowserWorkspaces: SessionBrowserWorkspaceState[];
  workdir: string;
}) {
  switch (section) {
    case "appearance":
      return (
        <div className="settingsRows">
          <SettingsOptionRow title="Theme" description="Dark, neutral light, or warm paper Workbench appearance.">
            <div className="segmentedControl">
              <button className={appearance === "dark" ? "is-selected" : ""} onClick={() => onAppearanceChange("dark")} type="button">
                <Moon size={15} /> Dark
              </button>
              <button className={appearance === "light" ? "is-selected" : ""} onClick={() => onAppearanceChange("light")} type="button">
                <Sun size={15} /> Light
              </button>
              <button className={appearance === "warm" ? "is-selected" : ""} onClick={() => onAppearanceChange("warm")} type="button">
                <Palette size={15} /> Warm
              </button>
            </div>
          </SettingsOptionRow>
        </div>
      );
    case "usage":
      return (
        <UsageSettingsPanel
          loading={usageStatsLoading}
          stats={usageStats}
          error={usageStatsError}
          onRefresh={onRefreshUsageStats}
        />
      );
    case "models":
      return (
        <ModelsSettingsPanel
          client={client}
          disabled={disabled}
          onModelAssignmentSaved={onModelAssignmentSaved}
          onModelCatalogLoaded={onModelCatalogLoaded}
          workdir={workdir}
        />
      );
    case "archived":
      return (
        <ArchivedSessionsPanel
          disabled={disabled}
          sessions={archivedSessions}
          onDelete={onDeleteArchivedSession}
          onRestore={onRestoreArchivedSession}
        />
      );
    case "debug":
      return (
        <div className="settingsRows">
          <SettingsOptionRow title="Show debug tab" description="Recent Gateway notifications in the right inspector.">
            <label className="switchControl">
              <input checked={debugEnabled} onChange={(event) => onDebugChange(event.target.checked)} type="checkbox" />
              <span>{debugEnabled ? "On" : "Off"}</span>
            </label>
          </SettingsOptionRow>
        </div>
      );
    case "agents":
      return (
        <AgentsConfigPanel
          backendDraft={backendDraft}
          backendDoctor={backendDoctor}
          backends={backends}
          disabled={disabled}
          onCancelBackendEdit={onCancelBackendEdit}
          onChangeBackendDraft={onChangeBackendDraft}
          onDeleteBackend={onDeleteBackend}
          onDoctorBackend={onDoctorBackend}
          onEditBackend={onEditBackend}
          onNewBackend={onNewBackend}
          onSaveBackendDraft={onSaveBackendDraft}
          onSetBackendEnabled={onSetBackendEnabled}
          onSetBackendEntrypoints={onSetBackendEntrypoints}
        />
      );
    case "channels":
      return (
        <ChannelsSettingsPanel
          channelDoctor={channelDoctor}
          channels={channels}
          controls={controls}
          disabled={disabled}
          onDeleteChannel={onDeleteChannel}
          onDoctorChannel={onDoctorChannel}
          onDoctorChannels={onDoctorChannels}
          onLoadChannelSources={onLoadChannelSources}
          onPollWechatQrSetup={onPollWechatQrSetup}
          onSetChannelEnabled={onSetChannelEnabled}
          onStartWechatQrSetup={onStartWechatQrSetup}
          onUpdateChannel={onUpdateChannel}
          sessionBrowserWorkspaces={sessionBrowserWorkspaces}
          workdir={workdir}
        />
      );
  }
}

function SettingsOptionRow({
  children,
  description,
  title
}: {
  children: ReactNode;
  description: string;
  title: string;
}) {
  return (
    <div className="settingsRow">
      <div>
        <strong>{title}</strong>
        <span>{description}</span>
      </div>
      {children}
    </div>
  );
}

type ProviderDraft = {
  providerId: string;
  label: string;
  baseUrl: string;
  apiKeyEnv: string;
  apiKey: string;
  noAuth: boolean;
};

type AssignmentDraft = {
  model: string;
  reasoningEffort: string;
};

function ModelsSettingsPanel({
  client,
  disabled,
  onModelAssignmentSaved,
  onModelCatalogLoaded,
  workdir
}: {
  client: GatewayClient | null;
  disabled: boolean;
  onModelAssignmentSaved(): Promise<void>;
  onModelCatalogLoaded(options: ModelOptionView[]): void;
  workdir: string;
}) {
  const [settings, setSettings] = useState<ModelSettingsResult | null>(null);
  const [providerDrafts, setProviderDrafts] = useState<Record<string, ProviderDraft>>({});
  const [catalog, setCatalog] = useState<Record<string, ModelOptionView[]>>({});
  const [defaultDraft, setDefaultDraft] = useState<AssignmentDraft>({ model: "", reasoningEffort: "none" });
  const [auxDrafts, setAuxDrafts] = useState<Record<string, AssignmentDraft>>({});
  const [loading, setLoading] = useState(false);
  const [busyKey, setBusyKey] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);

  async function loadModelSettings() {
    if (!client) {
      setSettings(null);
      return;
    }
    setLoading(true);
    setError(null);
    try {
      const result = await client.request("model/settings/read", {
        scope: "global",
        workdir
      });
      setSettings(result);
      setDefaultDraft({
        model: result.defaultModel ?? "",
        reasoningEffort: result.defaultReasoningEffort ?? "none"
      });
      setAuxDrafts(Object.fromEntries(result.auxiliary.map((item) => {
        const model = item.effectiveModel ?? (item.provider !== "auto" && item.model ? `${item.provider}/${item.model}` : "");
        return [
          item.task,
          {
            model,
            reasoningEffort: item.reasoningEffort ?? "none"
          }
        ];
      })));
      setProviderDrafts((current) => mergeProviderDrafts(current, result.providers));
    } catch (nextError) {
      setError(errorMessage(nextError));
    } finally {
      setLoading(false);
    }
  }

  useEffect(() => {
    void loadModelSettings();
  }, [client, workdir]);

  const modelOptions = useMemo(() => {
    const merged = new Map<string, ModelOptionView>();
    for (const option of settings?.modelOptions ?? []) {
      merged.set(option.value, option);
    }
    for (const options of Object.values(catalog)) {
      for (const option of options) {
        merged.set(option.value, option);
      }
    }
    return [...merged.values()].sort((left, right) => left.value.localeCompare(right.value));
  }, [catalog, settings]);
  const freeSelection = modelOptions.find((option) => option.value === defaultDraft.model && option.free && option.provider === "opencode-zen")
    ?? Object.values(auxDrafts)
      .map((value) => modelOptions.find((option) => option.value === value.model && option.free && option.provider === "opencode-zen"))
      .find(Boolean);

  async function fetchProviderCatalog(provider: ModelProviderView) {
    if (!client) return;
    setBusyKey(`catalog:${provider.id}`);
    setError(null);
    setNotice(null);
    try {
      const result = await client.request("model/provider/catalog", {
        scope: "global",
        providerId: provider.id,
        refresh: true,
        workdir
      });
      setCatalog((current) => ({ ...current, [result.providerId]: result.models }));
      onModelCatalogLoaded(result.models);
      setNotice(`${provider.label}: catalog updated`);
    } catch (nextError) {
      setError(errorMessage(nextError));
    } finally {
      setBusyKey(null);
    }
  }

  async function saveProvider(provider: ModelProviderView) {
    if (!client) return;
    const draft = providerDrafts[provider.id];
    if (!draft) return;
    setBusyKey(`provider:${provider.id}`);
    setError(null);
    setNotice(null);
    try {
      const result = await client.request("model/provider/save", {
        scope: "global",
        providerId: draft.providerId,
        label: draft.label,
        baseUrl: draft.baseUrl,
        apiKeyEnv: draft.noAuth ? null : draft.apiKeyEnv || null,
        apiKey: draft.noAuth ? null : draft.apiKey || null,
        noAuth: draft.noAuth
      });
      setSettings(result);
      setProviderDrafts((current) => mergeProviderDrafts(current, result.providers));
      const savedProvider = result.providers.find((item) => item.id === draft.providerId || item.id === provider.id);
      if (savedProvider?.id === "opencode-zen" && savedProvider.canFetchModels) {
        const catalogResult = await client.request("model/provider/catalog", {
          scope: "global",
          providerId: savedProvider.id,
          refresh: true,
          workdir
        });
        setCatalog((current) => ({ ...current, [catalogResult.providerId]: catalogResult.models }));
        onModelCatalogLoaded(catalogResult.models);
        setNotice(`${draft.label}: saved; catalog updated`);
      } else {
        setNotice(`${draft.label}: saved`);
      }
    } catch (nextError) {
      setError(errorMessage(nextError));
    } finally {
      setBusyKey(null);
    }
  }

  async function saveAssignment(target: "default" | "auxiliary", draft: AssignmentDraft, task?: string) {
    if (!client) return;
    const split = splitModelValue(draft.model);
    if (!split && target === "default") {
      setError("Default model must use provider/model");
      return;
    }
    setBusyKey(target === "default" ? "assignment:default" : `assignment:${task}`);
    setError(null);
    setNotice(null);
    try {
      await client.request("model/assignment/set", {
        scope: "global",
        target,
        task: task ?? null,
        provider: split?.provider ?? "auto",
        model: split?.model ?? "",
        reasoningEffort: draft.reasoningEffort || "none"
      });
      setNotice(target === "default" ? "Default model saved" : `${formatAuxTaskLabel(task ?? "")}: saved`);
      await loadModelSettings();
      if (target === "default") {
        await onModelAssignmentSaved();
      }
    } catch (nextError) {
      setError(errorMessage(nextError));
    } finally {
      setBusyKey(null);
    }
  }

  return (
    <section className="modelsSettingsPanel" aria-label="Models">
      <div className="modelSettingsToolbar">
        <button
          aria-label="Refresh model settings"
          disabled={disabled || loading || !client}
          onClick={() => void loadModelSettings()}
          title="Refresh model settings"
          type="button"
        >
          <RotateCcw size={13} />
          <span>Refresh</span>
        </button>
      </div>
      {error && <div className="modelSettingsMessage is-error" role="alert">{error}</div>}
      {notice && <div className="modelSettingsMessage">{notice}</div>}
      {freeSelection && (
        <div className="modelSettingsMessage is-warning">
          OpenCode Zen free models may route data through free endpoints with different retention policies.
        </div>
      )}
      <section className="modelAssignmentPanel" aria-label="Model assignments">
        <ModelAssignmentRow
          busy={busyKey === "assignment:default"}
          disabled={disabled || !client}
          label="Default model"
          options={modelOptions}
          value={defaultDraft}
          onChange={setDefaultDraft}
          onSave={() => void saveAssignment("default", defaultDraft)}
        />
        {(settings?.auxiliary ?? defaultAuxiliaryAssignments()).map((assignment) => (
          <ModelAssignmentRow
            busy={busyKey === `assignment:${assignment.task}`}
            disabled={disabled || !client}
            key={assignment.task}
            label={assignment.label}
            options={modelOptions}
            resetLabel="Inherit default"
            value={auxDrafts[assignment.task] ?? { model: "", reasoningEffort: "none" }}
            onChange={(value) => setAuxDrafts((current) => ({ ...current, [assignment.task]: value }))}
            onSave={() => void saveAssignment("auxiliary", auxDrafts[assignment.task] ?? { model: "", reasoningEffort: "none" }, assignment.task)}
          />
        ))}
      </section>
      <section className="modelProvidersPanel" aria-label="Providers">
        {(settings?.providers ?? []).map((provider) => (
          <ProviderSettingsRow
            busy={busyKey === `provider:${provider.id}` || busyKey === `catalog:${provider.id}`}
            catalogCount={catalog[provider.id]?.length ?? 0}
            disabled={disabled || !client}
            draft={providerDrafts[provider.id] ?? providerDraftFromView(provider)}
            key={provider.id}
            provider={provider}
            onDraftChange={(draft) => setProviderDrafts((current) => ({ ...current, [provider.id]: draft }))}
            onFetch={() => void fetchProviderCatalog(provider)}
            onSave={() => void saveProvider(provider)}
          />
        ))}
        {!settings && !loading && <div className="modelSettingsMessage">Model settings unavailable</div>}
      </section>
    </section>
  );
}

function ProviderSettingsRow({
  busy,
  catalogCount,
  disabled,
  draft,
  provider,
  onDraftChange,
  onFetch,
  onSave
}: {
  busy: boolean;
  catalogCount: number;
  disabled: boolean;
  draft: ProviderDraft;
  provider: ModelProviderView;
  onDraftChange(draft: ProviderDraft): void;
  onFetch(): void;
  onSave(): void;
}) {
  const saveDisabled = disabled || busy || !draft.providerId.trim() || !draft.label.trim() || !draft.baseUrl.trim();
  return (
    <div className="modelProviderRow">
      <div className="modelProviderIdentity">
        <strong>{provider.label}</strong>
        {providerSecondaryStatus(provider) && <span>{providerSecondaryStatus(provider)}</span>}
      </div>
      <div className="modelProviderFields">
        {provider.id === "custom" && (
          <label>
            <span>ID</span>
            <input
              disabled={disabled || busy}
              onChange={(event) => onDraftChange({ ...draft, providerId: event.currentTarget.value })}
              value={draft.providerId}
            />
          </label>
        )}
        {provider.id === "custom" && (
          <label>
            <span>Label</span>
            <input
              disabled={disabled || busy}
              onChange={(event) => onDraftChange({ ...draft, label: event.currentTarget.value })}
              value={draft.label}
            />
          </label>
        )}
        <label>
          <span>Base URL</span>
          <input
            disabled={disabled || busy}
            onChange={(event) => onDraftChange({ ...draft, baseUrl: event.currentTarget.value })}
            value={draft.baseUrl}
          />
        </label>
        {!draft.noAuth && (
          <>
            <label>
              <span>API key env</span>
              <input
                disabled={disabled || busy}
                onChange={(event) => onDraftChange({ ...draft, apiKeyEnv: event.currentTarget.value })}
                value={draft.apiKeyEnv}
              />
            </label>
            <label>
              <span>API key</span>
              <input
                disabled={disabled || busy}
                onChange={(event) => onDraftChange({ ...draft, apiKey: event.currentTarget.value })}
                type="password"
                value={draft.apiKey}
              />
            </label>
          </>
        )}
        <label className="modelNoAuthToggle">
          <input
            checked={draft.noAuth}
            disabled={disabled || busy}
            onChange={(event) => onDraftChange({ ...draft, noAuth: event.currentTarget.checked, apiKey: "" })}
            type="checkbox"
          />
          <span>No auth</span>
        </label>
      </div>
      <div className="modelProviderActions">
        <button disabled={disabled || busy || !provider.canFetchModels} onClick={onFetch} type="button">
          <Search size={13} />
          <span>{catalogCount ? `${catalogCount} models` : "Fetch"}</span>
        </button>
        <button disabled={saveDisabled} onClick={onSave} type="button">
          <Save size={13} />
          <span>{busy ? "Saving" : "Save"}</span>
        </button>
      </div>
    </div>
  );
}

function ModelAssignmentRow({
  busy,
  disabled,
  label,
  options,
  resetLabel,
  value,
  onChange,
  onSave
}: {
  busy: boolean;
  disabled: boolean;
  label: string;
  options: ModelOptionView[];
  resetLabel?: string;
  value: AssignmentDraft;
  onChange(value: AssignmentDraft): void;
  onSave(): void;
}) {
  const selectedOption = options.find((option) => option.value === value.model) ?? null;
  const reasoningOptions = reasoningEffortsForModelOption(selectedOption);
  const reasoningEffort = reasoningOptions.includes(value.reasoningEffort) ? value.reasoningEffort : "none";
  const modelDisabled = disabled || busy;
  function updateModel(model: string | null) {
    const nextModel = model ?? "";
    const nextOption = options.find((option) => option.value === nextModel) ?? null;
    const nextReasoningOptions = reasoningEffortsForModelOption(nextOption);
    onChange({
      model: nextModel,
      reasoningEffort: nextReasoningOptions.includes(value.reasoningEffort) ? value.reasoningEffort : "none"
    });
  }
  return (
    <div className="modelAssignmentRow">
      <div>
        <strong>{label}</strong>
      </div>
      <div className="modelAssignmentControls">
        <ModelReasoningSelector
          ariaLabel={label}
          className="modelAssignmentPicker"
          disabled={modelDisabled}
          emptyLabel="Select model"
          model={value.model || null}
          options={options}
          placement="bottom-start"
          resetLabel={resetLabel}
          variant={reasoningEffort}
          onModelChange={updateModel}
          onVariantChange={(nextReasoning) => onChange({ ...value, reasoningEffort: nextReasoning })}
        />
        <button disabled={disabled || busy || (!resetLabel && !value.model.trim())} onClick={onSave} type="button">
          <Save size={13} />
          <span>{busy ? "Saving" : "Save"}</span>
        </button>
      </div>
    </div>
  );
}

function mergeProviderDrafts(current: Record<string, ProviderDraft>, providers: ModelProviderView[]): Record<string, ProviderDraft> {
  const next = { ...current };
  for (const provider of providers) {
    next[provider.id] = next[provider.id] ?? providerDraftFromView(provider);
  }
  return next;
}

function providerDraftFromView(provider: ModelProviderView): ProviderDraft {
  return {
    providerId: provider.id === "custom" ? "" : provider.id,
    label: provider.id === "custom" ? "" : provider.label,
    baseUrl: provider.baseUrl ?? (provider.id === "custom" ? "http://127.0.0.1:1234/v1" : ""),
    apiKeyEnv: provider.apiKeyEnv ?? defaultApiKeyEnv(provider.id),
    apiKey: "",
    noAuth: provider.noAuth
  };
}

function defaultApiKeyEnv(providerId: string): string {
  return `${providerId.toUpperCase().replace(/[^A-Z0-9]+/g, "_").replace(/^_+|_+$/g, "")}_API_KEY`;
}

function splitModelValue(value: string): { provider: string; model: string } | null {
  const trimmed = value.trim();
  const index = trimmed.indexOf("/");
  if (index <= 0 || index === trimmed.length - 1) return null;
  return {
    provider: trimmed.slice(0, index),
    model: trimmed.slice(index + 1)
  };
}

function defaultAuxiliaryAssignments(): AuxiliaryModelAssignmentView[] {
  return [
    { task: "title_generation", label: "Title generation", provider: "auto", model: "", reasoningEffort: null, effectiveModel: null },
    { task: "compression", label: "Context compression", provider: "auto", model: "", reasoningEffort: null, effectiveModel: null }
  ];
}

function formatAuxTaskLabel(task: string): string {
  switch (task) {
    case "title_generation":
      return "Title generation";
    case "compression":
      return "Context compression";
    default:
      return task;
  }
}

function providerSecondaryStatus(provider: ModelProviderView): string | null {
  return provider.configured ? "Configured" : null;
}

function errorMessage(value: unknown): string {
  return value instanceof Error ? value.message : String(value);
}

function UsageSettingsPanel({
  error,
  loading,
  stats,
  onRefresh
}: {
  error: string | null;
  loading: boolean;
  stats: WorkbenchUsageStats | null;
  onRefresh(): void;
}) {
  const windows = stats?.windows ?? [];
  const primaryWindows = ["all", "30d", "7d"]
    .map((id) => windows.find((window) => window.id === id))
    .filter((window): window is WorkbenchUsageStats["windows"][number] => Boolean(window));
  return (
    <section className="usageSettingsPanel" aria-label="Usage">
      <div className="usageSettingsToolbar">
        <span>{stats ? `Updated ${formatShortDateTime(stats.generatedAtMs)}` : loading ? "Loading" : "No data"}</span>
        <button aria-label="Refresh usage" disabled={loading} onClick={onRefresh} title="Refresh usage" type="button">
          <RotateCcw size={13} />
          <span>Refresh</span>
        </button>
      </div>
      {error && <div className="usageSettingsError" role="alert">{error}</div>}
      {primaryWindows.length > 0 ? (
        <div className="usageWindowGrid">
          {primaryWindows.map((window) => <UsageWindowCard key={window.id} window={window} />)}
        </div>
      ) : (
        <div className="usageSettingsEmpty">{loading ? "Loading usage" : "No usage recorded"}</div>
      )}
      {stats && <UsageActivityHeatmap activity={stats.activity} />}
    </section>
  );
}

function UsageWindowCard({ window }: { window: WorkbenchUsageStats["windows"][number] }) {
  const inputTokens = window.billableInputTokens + window.cacheReadTokens + window.cacheWriteTokens;
  return (
    <section className="usageWindowCard" aria-label={window.label}>
      <header>
        <span>{window.label}</span>
        <strong>{formatCompactNumber(window.reportedTotalTokens)}</strong>
      </header>
      <div className="usageWindowMetrics">
        <div>
          <span>Cost</span>
          <strong>{formatUsageCost(window)}</strong>
        </div>
        <div>
          <span>Cache read</span>
          <strong>{formatPercent(window.cacheReadPercent)}</strong>
        </div>
        <div>
          <span>Sessions</span>
          <strong>{formatCompactNumber(window.sessionCount)}</strong>
        </div>
      </div>
      <dl className="usageBreakdown">
        <div><dt>Input</dt><dd>{formatCompactNumber(inputTokens)}</dd></div>
        <div><dt>Output</dt><dd>{formatCompactNumber(window.billableOutputTokens)}</dd></div>
        <div><dt>Reasoning</dt><dd>{formatCompactNumber(window.reasoningTokens)}</dd></div>
        <div><dt>Cache write</dt><dd>{formatCompactNumber(window.cacheWriteTokens)}</dd></div>
        {window.unknownPricingCount > 0 && (
          <div><dt>Unknown pricing</dt><dd>{formatCompactNumber(window.unknownPricingCount)}</dd></div>
        )}
      </dl>
    </section>
  );
}

function UsageActivityHeatmap({ activity }: { activity: WorkbenchUsageStats["activity"] }) {
  const days = activity.days;
  const positiveTokenScale = heatmapPositiveTokenScale(days);
  const startPadding = days[0] ? new Date(`${days[0].date}T00:00:00`).getDay() : 0;
  const cells: Array<null | WorkbenchUsageStats["activity"]["days"][number]> = [
    ...Array.from({ length: startPadding }, () => null),
    ...days
  ];
  const weekCount = Math.max(1, Math.ceil(cells.length / 7));
  const monthLabels = heatmapMonthLabels(cells, weekCount);
  return (
    <section className="usageHeatmapPanel" aria-label="Token activity">
      <header>
        <span><BarChart3 size={14} /> Token activity</span>
        <small>{activity.startDate} to {activity.endDate}</small>
      </header>
      <div className="usageHeatmapScroller">
        <div
          className="usageHeatmapMonths"
          style={{ gridTemplateColumns: `repeat(${weekCount}, 11px)` }}
        >
          {monthLabels.map((label) => (
            <span key={`${label.month}-${label.week}`} style={{ gridColumn: `${label.week + 1} / span ${label.span}` }}>
              {label.month}
            </span>
          ))}
        </div>
        <div className="usageHeatmapBody">
          <div className="usageHeatmapWeekdays" aria-hidden>
            <span>Sun</span>
            <span>Mon</span>
            <span>Tue</span>
            <span>Wed</span>
            <span>Thu</span>
            <span>Fri</span>
            <span>Sat</span>
          </div>
          <div
            className="usageHeatmapGrid"
            style={{ gridTemplateColumns: `repeat(${weekCount}, 11px)` }}
          >
            {cells.map((day, index) => {
              const level = day ? heatmapLevel(day.reportedTotalTokens, positiveTokenScale) : 0;
              return (
                <span
                  aria-label={day ? `${day.date}: ${day.reportedTotalTokens} tokens` : undefined}
                  className={day ? "usageHeatmapCell" : "usageHeatmapCell is-empty"}
                  data-level={level}
                  key={day?.date ?? `pad-${index}`}
                  title={day ? `${day.date}: ${formatCompactNumber(day.reportedTotalTokens)} tokens` : undefined}
                />
              );
            })}
          </div>
        </div>
      </div>
    </section>
  );
}

function heatmapPositiveTokenScale(days: WorkbenchUsageStats["activity"]["days"]): number[] {
  return [...new Set(days
    .map((day) => day.reportedTotalTokens)
    .filter((tokens) => tokens > 0))]
    .sort((left, right) => left - right);
}

function heatmapLevel(tokens: number, positiveTokenScale: number[]): number {
  if (tokens <= 0) {
    return 0;
  }
  if (positiveTokenScale.length <= 1) {
    return 4;
  }
  const index = positiveTokenScale.findIndex((value) => tokens <= value);
  const boundedIndex = index >= 0 ? index : positiveTokenScale.length - 1;
  const ratio = boundedIndex / (positiveTokenScale.length - 1);
  return Math.max(1, Math.min(4, Math.round(ratio * 3) + 1));
}

function heatmapMonthLabels(
  cells: Array<null | WorkbenchUsageStats["activity"]["days"][number]>,
  weekCount: number
): Array<{ month: string; span: number; week: number }> {
  const labels: Array<{ month: string; span: number; week: number }> = [];
  let lastMonth = "";
  for (let index = 0; index < cells.length; index += 1) {
    const day = cells[index];
    if (!day) {
      continue;
    }
    const date = new Date(`${day.date}T00:00:00`);
    const month = date.toLocaleString(undefined, { month: "short" });
    const week = Math.floor(index / 7);
    if (month !== lastMonth) {
      labels.push({ month, span: 1, week });
      lastMonth = month;
    }
  }
  return labels;
}

function formatUsageCost(window: WorkbenchUsageStats["windows"][number]): string {
  if (window.costStatus === "unknown" && window.estimatedCostNanodollars === 0) {
    return "Unknown";
  }
  const value = formatNanodollars(window.estimatedCostNanodollars);
  return window.unknownPricingCount > 0 ? `${value} + unknown` : value;
}

function formatCompactNumber(value: number): string {
  return new Intl.NumberFormat(undefined, { maximumFractionDigits: 1, notation: "compact" }).format(value);
}

function formatPercent(value: number | null): string {
  return value === null ? "-" : `${Math.round(value)}%`;
}

function formatNanodollars(value: number): string {
  return `$${(value / 1_000_000_000).toFixed(6)}`;
}

function formatShortDateTime(value: number): string {
  return new Date(value).toLocaleString(undefined, {
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
    month: "short"
  });
}

function ArchivedSessionsPanel({
  disabled,
  sessions,
  onDelete,
  onRestore
}: {
  disabled: boolean;
  sessions: SessionSummary[];
  onDelete(threadId: string): void;
  onRestore(threadId: string): void;
}) {
  return (
    <section className="archivedSessionsPanel" aria-label="Archived sessions">
      {sessions.length === 0 ? (
        <p>No archived sessions.</p>
      ) : (
        <div className="archivedSessionList">
          {sessions.map((session) => {
            const title = session.displayTitle?.trim() || session.title?.trim() || shortSessionId(session.id);
            const workspace = session.project?.label || session.project?.displayPath || session.workdir || "workspace";
            return (
              <div className="archivedSessionRow" key={session.id}>
                <div>
                  <strong>{title}</strong>
                  <span>{workspace}</span>
                </div>
                <div className="archivedSessionActions">
                  <button aria-label={`Restore ${title}`} disabled={disabled} onClick={() => onRestore(session.id)} title="Restore" type="button">
                    <RotateCcw size={13} />
                  </button>
                  <button aria-label={`Delete ${title}`} disabled={disabled} onClick={() => onDelete(session.id)} title="Delete" type="button">
                    <Trash2 size={13} />
                  </button>
                </div>
              </div>
            );
          })}
        </div>
      )}
    </section>
  );
}

type ChannelChoice = "wechat" | "telegram" | "feishu" | "lark";

const CHANNEL_CHOICES: ChannelChoice[] = ["wechat", "telegram", "feishu", "lark"];

function ChannelsSettingsPanel({
  channelDoctor,
  channels,
  controls,
  disabled,
  onDeleteChannel,
  onDoctorChannel,
  onDoctorChannels,
  onLoadChannelSources,
  onPollWechatQrSetup,
  onSetChannelEnabled,
  onStartWechatQrSetup,
  onUpdateChannel,
  sessionBrowserWorkspaces,
  workdir
}: {
  channelDoctor: Record<string, WorkbenchChannelDoctor>;
  channels: WorkbenchChannel[];
  controls: ChannelSettingsControls;
  disabled: boolean;
  onDeleteChannel(channel: WorkbenchChannel): Promise<void>;
  onDoctorChannel(channel: WorkbenchChannel): void;
  onDoctorChannels(): void;
  onLoadChannelSources(channel: WorkbenchChannel): Promise<WorkbenchChannelSource[]>;
  onPollWechatQrSetup(sessionId: string): Promise<ChannelWechatQrPollResult>;
  onSetChannelEnabled(channel: WorkbenchChannel, enabled: boolean): void;
  onStartWechatQrSetup(): Promise<ChannelWechatQrStartResult>;
  onUpdateChannel(channel: WorkbenchChannel, draft: ChannelUpdateDraft): Promise<WorkbenchChannel>;
  sessionBrowserWorkspaces: SessionBrowserWorkspaceState[];
  workdir: string;
}) {
  const [selectedChannelId, setSelectedChannelId] = useState<string | null>(null);
  const [selectedChannelChoice, setSelectedChannelChoice] = useState<ChannelChoice>("wechat");
  const panelRef = useRef<HTMLElement | null>(null);
  const selectedChannel = channels.find((channel) => channel.id === selectedChannelId) ?? null;
  const configuredChoiceChannel = channels.find((channel) => channel.channel === selectedChannelChoice) ?? null;
  useEffect(() => {
    const settingsContent = panelRef.current?.closest<HTMLElement>(".settingsContent");
    settingsContent?.scrollTo?.({ top: 0, left: 0 });
  }, [selectedChannelId]);
  if (selectedChannel) {
    return (
      <ChannelSettingsDetail
        channel={selectedChannel}
        controls={controls}
        doctor={channelDoctor[selectedChannel.id] ?? null}
        disabled={disabled}
        onBack={() => setSelectedChannelId(null)}
        onDelete={async () => {
          await onDeleteChannel(selectedChannel);
          setSelectedChannelId(null);
        }}
        onLoadSources={() => onLoadChannelSources(selectedChannel)}
        onUpdate={(draft) => onUpdateChannel(selectedChannel, draft)}
        rootRef={panelRef}
        sessionBrowserWorkspaces={sessionBrowserWorkspaces}
        workdir={workdir}
      />
    );
  }
  return (
    <section className="agentSurfacePanel channelsSettingsPanel" aria-label="Channels" ref={panelRef}>
      <header className="agentSurfaceHeaderWithAction channelSettingsToolbar">
        <span><MessageCircle size={15} /> Connected Channels <b>{channels.length}</b></span>
        <div className="channelToolbarActions">
          <button aria-label="Doctor Channels" disabled={disabled} onClick={onDoctorChannels} title="Doctor Channels" type="button">
            <Wrench size={13} />
            <span>Doctor</span>
          </button>
          <button aria-label="Add custom channel" disabled={disabled} onClick={() => setSelectedChannelChoice("telegram")} title="Add custom channel" type="button">
            <Plus size={13} />
            <span>Add custom</span>
          </button>
          <button aria-label="Start Channels" disabled={disabled} onClick={onDoctorChannels} title="Start Channels" type="button">
            <Play size={13} />
            <span>Start</span>
          </button>
        </div>
      </header>
      <div className="agentSurfaceList channelSurfaceList">
        {channels.map((channel) => {
          const doctor = channelDoctor[channel.id] ?? null;
          return (
            <div className="agentSurfaceRow channelSurfaceRow" key={channel.id}>
              <div className="channelRowMain">
                <div className="channelIdentityLine">
                  <strong>{channel.label || channel.id}</strong>
                  <ChannelStatusPill status={channel.runtimeStatus} />
                </div>
                <span>{formatChannelName(channel.channel)} · {channel.id} · {channel.transport}</span>
                <div className="channelInlineStates" aria-label={`${channel.id} status`}>
                  <small>Credential {channel.credential.status}</small>
                  <small>Allowlist {channel.allowlist.status}</small>
                  <small>Runner {channel.runner.state}</small>
                  <small>{channelRuntimeSummary(channel, workdir)}</small>
                </div>
                {doctor && (
                  <small className={channelDoctorOk(doctor) ? "agentSurfaceOk" : "agentSurfaceWarning"}>
                    {doctor.checks.map((check) => `${check.name}: ${check.status}`).join(" · ")}
                  </small>
                )}
              </div>
              <div className="agentBackendSide">
                <label className="backendSwitch">
                  <input
                    aria-label={`${channel.enabled ? "Disable" : "Enable"} ${channel.id}`}
                    checked={channel.enabled}
                    disabled={disabled}
                    onChange={(event) => onSetChannelEnabled(channel, event.currentTarget.checked)}
                    role="switch"
                    type="checkbox"
                  />
                  <span className="backendSwitchTrack" aria-hidden />
                  <span>{channel.enabled ? "Enabled" : "Disabled"}</span>
                </label>
                <div className="agentBackendActions">
                  <button aria-label={`Test ${channel.id}`} disabled={disabled} onClick={() => onDoctorChannel(channel)} title="Test" type="button">
                    <Wrench size={13} />
                  </button>
                  <button aria-label={`Settings ${channel.id}`} disabled={disabled} onClick={() => setSelectedChannelId(channel.id)} title="Settings" type="button">
                    <Settings2 size={13} />
                  </button>
                </div>
              </div>
            </div>
          );
        })}
        {channels.length === 0 && <p>No channels configured.</p>}
      </div>
      <section className="channelAddSection" aria-label="Add channel">
        <header>
          <span><Plus size={14} /> Add channel</span>
        </header>
        <div className="channelPlatformPicker" role="tablist" aria-label="Channel type">
          {CHANNEL_CHOICES.map((channel) => (
            <button
              aria-selected={selectedChannelChoice === channel}
              className={selectedChannelChoice === channel ? "is-selected" : ""}
              key={channel}
              onClick={() => setSelectedChannelChoice(channel)}
              role="tab"
              type="button"
            >
              {formatChannelName(channel)}
            </button>
          ))}
        </div>
        <ChannelSetupCard
          channel={selectedChannelChoice}
          disabled={disabled}
          existingChannel={configuredChoiceChannel}
          onPollWechatQrSetup={onPollWechatQrSetup}
          onStartWechatQrSetup={onStartWechatQrSetup}
        />
      </section>
    </section>
  );
}

function ChannelSettingsDetail({
  channel,
  controls,
  disabled,
  doctor,
  onBack,
  onDelete,
  onLoadSources,
  onUpdate,
  rootRef,
  sessionBrowserWorkspaces,
  workdir
}: {
  channel: WorkbenchChannel;
  controls: ChannelSettingsControls;
  disabled: boolean;
  doctor: WorkbenchChannelDoctor | null;
  onBack(): void;
  onDelete(): Promise<void>;
  onLoadSources(): Promise<WorkbenchChannelSource[]>;
  onUpdate(draft: ChannelUpdateDraft): Promise<WorkbenchChannel>;
  rootRef: RefObject<HTMLElement | null>;
  sessionBrowserWorkspaces: SessionBrowserWorkspaceState[];
  workdir: string;
}) {
  const [draft, setDraft] = useState<ChannelSettingsDraft>(() => channelDraftFromChannel(channel));
  const [discardPrompt, setDiscardPrompt] = useState(false);
  const [deletePrompt, setDeletePrompt] = useState(false);
  const [saving, setSaving] = useState(false);
  const [deleting, setDeleting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [savedNotice, setSavedNotice] = useState<string | null>(null);
  const [sourceError, setSourceError] = useState<string | null>(null);
  const [sourceLoading, setSourceLoading] = useState(false);
  const [sources, setSources] = useState<WorkbenchChannelSource[] | null>(null);

  useEffect(() => {
    setDraft(channelDraftFromChannel(channel));
    setDiscardPrompt(false);
    setDeletePrompt(false);
    setError(null);
    setSavedNotice(null);
    setSourceError(null);
    setSourceLoading(false);
    setSources(null);
  }, [channel.id]);

  const savedSignature = channelDraftSignature(channelDraftFromChannel(channel));
  const draftSignature = channelDraftSignature(draft);
  const dirty = draftSignature !== savedSignature;
  const permissionOptions = channelPermissionOptions(controls, channel, draft);
  const modelOptions = channelModelOptions(controls, channel, draft);
  const workspaceOptions = channelWorkspaceOptions(sessionBrowserWorkspaces);
  const busy = disabled || saving || deleting;

  function updateDraft(patch: Partial<ChannelSettingsDraft>) {
    setError(null);
    setSavedNotice(null);
    setDiscardPrompt(false);
    setDraft((current) => ({ ...current, ...patch }));
  }

  function requestBack() {
    if (dirty) {
      setDiscardPrompt(true);
      return;
    }
    onBack();
  }

  function cancelEdits() {
    setDraft(channelDraftFromChannel(channel));
    setDiscardPrompt(false);
    setError(null);
    setSavedNotice(null);
  }

  async function saveDraft() {
    if (!dirty || busy) {
      return;
    }
    const workspaceChanged = draft.workdir.trim() !== channelDraftFromChannel(channel).workdir.trim();
    setSaving(true);
    setError(null);
    setSavedNotice(null);
    try {
      const nextChannel = await onUpdate(channelUpdateDraftFromDraft(draft));
      setDraft(channelDraftFromChannel(nextChannel));
      setDiscardPrompt(false);
      setSavedNotice(workspaceChanged ? "Next message will start in the new workspace." : null);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setSaving(false);
    }
  }

  async function deleteChannel() {
    if (busy) {
      return;
    }
    setDeleting(true);
    setError(null);
    try {
      await onDelete();
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
      setDeleting(false);
    }
  }

  async function loadSources() {
    if (sourceLoading || sources !== null) {
      return;
    }
    setSourceLoading(true);
    setSourceError(null);
    try {
      setSources(await onLoadSources());
    } catch (err) {
      setSourceError(err instanceof Error ? err.message : String(err));
    } finally {
      setSourceLoading(false);
    }
  }

  return (
    <section className="channelsSettingsPanel channelDetailPage" aria-label="Channel settings" ref={rootRef}>
      <header className="channelDetailHeader channelDetailHeaderStaged">
        <button aria-label="Back to Channels" onClick={requestBack} title="Back to Channels" type="button">
          <ArrowLeft size={14} />
          <span>Back</span>
        </button>
        <div className="channelDetailTitle">
          <strong>{draft.label.trim() || channel.id}</strong>
          <span>{formatChannelName(channel.channel)} · {channel.transport}</span>
        </div>
        <div className="channelDetailActions">
          {dirty && (
            <button disabled={busy} onClick={cancelEdits} type="button">
              <X size={13} />
              <span>Cancel</span>
            </button>
          )}
          <button
            className="channelDetailSave"
            disabled={busy || !dirty}
            onClick={() => void saveDraft()}
            type="button"
          >
            <Save size={13} />
            <span>{saving ? "Saving" : "Save"}</span>
          </button>
        </div>
      </header>
      {discardPrompt && (
        <div className="channelDiscardNotice" role="alert">
          <strong>Discard unsaved changes?</strong>
          <button disabled={busy} onClick={() => { cancelEdits(); onBack(); }} type="button">
            Discard changes
          </button>
          <button disabled={busy} onClick={() => setDiscardPrompt(false)} type="button">
            Keep editing
          </button>
        </div>
      )}
      {error && <p className="channelDetailError" role="alert">{error}</p>}
      {savedNotice && <p className="channelSavedNotice" role="status">{savedNotice}</p>}
      <div className="channelHealthSummary" aria-label={`${channel.id} channel health summary`}>
        <ChannelHealthItem label="Config" tone={channelStatusTone(channel.runtimeStatus)} value={channel.runtimeStatus} />
        <ChannelHealthItem label="Runner" tone={channelRunnerTone(channel.runner.state)} value={channel.runner.state} />
        <ChannelHealthItem label="Credential" tone={channelStatusTone(channel.credential.status)} value={channel.credential.status} />
        <ChannelHealthItem label="Allowlist" tone={channelStatusTone(channel.allowlist.status)} value={channelAllowlistSummary(channel)} />
        <ChannelHealthItem label="Runtime" tone="muted" value={channelRuntimeDefaultsSummary(draft)} />
      </div>
      {doctor && (
        <div className="channelDoctorResult" role="status" aria-label={`${channel.id} doctor checks`}>
          {doctor.checks.map((check) => (
            <div className={`channelDoctorCheck is-${channelStatusTone(check.status)}`} key={check.name}>
              <span>{check.name}</span>
              <strong>{check.status}</strong>
              <small>{check.message}</small>
            </div>
          ))}
        </div>
      )}
      <div className="channelDetailsForm">
        <ChannelDetailSection title="Connection">
          <ChannelFormRow label="Label" hint="Shown in the connected channel list.">
            <div className="channelControl">
              <input
                aria-label="Channel label"
                disabled={busy}
                onChange={(event) => updateDraft({ label: event.currentTarget.value })}
                type="text"
                value={draft.label}
              />
            </div>
          </ChannelFormRow>
        </ChannelDetailSection>

        <ChannelDetailSection title="Access control">
          <ChannelFormRow label="Group mention" hint="Groups only start work after an explicit mention.">
            <label className="channelInlineToggle">
              <input
                aria-label="Require mention in groups"
                checked={draft.requireMention}
                disabled={busy}
                onChange={(event) => updateDraft({ requireMention: event.currentTarget.checked })}
                type="checkbox"
              />
              <span>{draft.requireMention ? "Required" : "Not required"}</span>
            </label>
          </ChannelFormRow>
          <ChannelFormRow label="Allowed callers" hint="Comma or newline separated ids, saved as structured allowlists.">
            <div className="channelTextareaPair">
              <textarea
                aria-label="Allowed direct users"
                disabled={busy}
                onChange={(event) => updateDraft({ allowUsersText: event.currentTarget.value })}
                rows={4}
                value={draft.allowUsersText}
              />
              <textarea
                aria-label="Allowed groups"
                disabled={busy}
                onChange={(event) => updateDraft({ allowGroupsText: event.currentTarget.value })}
                rows={4}
                value={draft.allowGroupsText}
              />
            </div>
          </ChannelFormRow>
        </ChannelDetailSection>

        <ChannelDetailSection title="Runtime settings">
          <ChannelFormRow label="Permission mode" hint="Controls write and command approval defaults for this channel.">
            <div className="channelSegmentedControl" role="group" aria-label="Permission mode">
              {permissionOptions.map((option) => (
                <button
                  className={draft.permissionMode === option ? "is-selected" : ""}
                  disabled={busy}
                  key={option}
                  onClick={() => updateDraft({ permissionMode: option })}
                  type="button"
                >
                  {permissionModeLabel(option)}
                </button>
              ))}
            </div>
          </ChannelFormRow>
          <ChannelFormRow label="Model" hint="Blank uses the profile default model.">
            <div className="channelControl">
              <select
                aria-label="Channel model"
                disabled={busy}
                onChange={(event) => updateDraft({ model: event.currentTarget.value })}
                value={draft.model}
              >
                <option value="">Profile default</option>
                {modelOptions.map((option) => (
                  <option key={option} value={option}>{modelOptionLabel(option, channel, controls)}</option>
                ))}
              </select>
            </div>
          </ChannelFormRow>
          <ChannelFormRow label="Workspace" hint="Changing workspace starts a fresh channel thread on the next message. Current running work is not interrupted.">
            <div className="channelControl channelWorkspaceControl">
              <select
                aria-label="Channel workspace preset"
                disabled={busy}
                onChange={(event) => {
                  const value = event.currentTarget.value;
                  if (value !== CHANNEL_WORKSPACE_MANUAL_VALUE) {
                    updateDraft({ workdir: value });
                  }
                }}
                value={channelWorkspaceSelectValue(draft.workdir, workspaceOptions)}
              >
                <option value="">Profile default</option>
                {workspaceOptions.map((path) => (
                  <option key={path} value={path}>{channelWorkspaceOptionLabel(path)}</option>
                ))}
                {channelWorkspaceSelectValue(draft.workdir, workspaceOptions) === CHANNEL_WORKSPACE_MANUAL_VALUE && (
                  <option value={CHANNEL_WORKSPACE_MANUAL_VALUE}>Manual path</option>
                )}
              </select>
              <input
                aria-label="Channel workspace"
                disabled={busy}
                onChange={(event) => updateDraft({ workdir: event.currentTarget.value })}
                placeholder={workdir}
                type="text"
                value={draft.workdir}
              />
            </div>
          </ChannelFormRow>
        </ChannelDetailSection>

        <details
          className="channelAdvancedDiagnostics"
          onToggle={(event) => {
            if (event.currentTarget.open) {
              void loadSources();
            }
          }}
        >
          <summary>Advanced diagnostics</summary>
          <ChannelCredentialRow
            disabled={busy}
            hint="Advanced credential-name override for custom or manual setups. Raw secrets stay in the profile environment."
            label="Credential env"
            onChange={(value) => updateDraft({ credentialEnv: value })}
            status={channel.credential.status}
            value={draft.credentialEnv}
          />
          <ChannelFormRow label="Runner activity" hint="Diagnostics are read-only and secret-free.">
            <dl className="channelRunnerGrid">
              <div><dt>State</dt><dd>{channel.runner.state}</dd></div>
              <div><dt>Reason</dt><dd>{channel.runner.reason ?? "none"}</dd></div>
              <div><dt>Last poll</dt><dd>{formatRunnerTimestamp(channel.runner.lastPollAtMs)}</dd></div>
              <div><dt>Last healthy poll</dt><dd>{formatRunnerTimestamp(channel.runner.lastHealthyPollAtMs)}</dd></div>
              <div><dt>Last inbound</dt><dd>{formatRunnerTimestamp(channel.runner.lastInboundAtMs)}</dd></div>
              <div><dt>Last outbound</dt><dd>{formatRunnerTimestamp(channel.runner.lastOutboundAtMs)}</dd></div>
              <div><dt>Last iLink code</dt><dd>{channel.runner.lastIlinkErrcode == null ? "none" : String(channel.runner.lastIlinkErrcode)}</dd></div>
              <div><dt>Last error</dt><dd>{channel.runner.lastError ?? "none"}</dd></div>
            </dl>
          </ChannelFormRow>
          <ChannelFormRow label="Remote lanes" hint="Shows which remote chats are bound to local threads.">
            <ChannelSourceList
              error={sourceError}
              loading={sourceLoading}
              sources={sources}
            />
          </ChannelFormRow>
        </details>

        <section className="channelDangerSection" aria-labelledby="channel-danger-heading">
          <div>
            <h4 id="channel-danger-heading">Danger zone</h4>
            <span>Remove this channel config. .env secret values are left intact.</span>
          </div>
          <div className="channelDangerActions">
            {deletePrompt ? (
              <>
                <button disabled={busy} onClick={() => void deleteChannel()} type="button">
                  {deleting ? "Removing" : "Confirm remove"}
                </button>
                <button disabled={busy} onClick={() => setDeletePrompt(false)} type="button">
                  Cancel
                </button>
              </>
            ) : (
              <button disabled={busy} onClick={() => setDeletePrompt(true)} type="button">
                <Trash2 size={13} />
                <span>Remove channel</span>
              </button>
            )}
          </div>
        </section>
      </div>
    </section>
  );
}

type ChannelSettingsDraft = {
  label: string;
  enabled: boolean;
  workdir: string;
  model: string;
  permissionMode: string;
  requireMention: boolean;
  allowUsersText: string;
  allowGroupsText: string;
  credentialEnv: string;
};

function ChannelDetailSection({
  children,
  title
}: {
  children: ReactNode;
  title: string;
}) {
  return (
    <section className="channelDetailSection" aria-labelledby={`channel-section-${sectionDomId(title)}`}>
      <h4 id={`channel-section-${sectionDomId(title)}`}>{title}</h4>
      {children}
    </section>
  );
}

function ChannelFormRow({
  children,
  hint,
  label
}: {
  children: ReactNode;
  hint: string;
  label: string;
}) {
  return (
    <div className="channelFormRow">
      <div className="channelFormCopy">
        <strong>{label}</strong>
        <span>{hint}</span>
      </div>
      {children}
    </div>
  );
}

function ChannelCredentialRow({
  disabled,
  hint,
  label,
  onChange,
  status,
  value
}: {
  disabled: boolean;
  hint: string;
  label: string;
  onChange(value: string): void;
  status: string;
  value: string;
}) {
  return (
    <ChannelFormRow label={label} hint={hint}>
      <div className="channelCredentialControl">
        <div className="channelCredentialLine">
          <code>{value.trim() || "platform default"}</code>
          <small className={`channelCredentialStatus is-${channelStatusTone(status)}`}>{status}</small>
        </div>
        <input
          aria-label={label}
          disabled={disabled}
          onChange={(event) => onChange(event.currentTarget.value)}
          type="text"
          value={value}
        />
      </div>
    </ChannelFormRow>
  );
}

function ChannelSourceList({
  error,
  loading,
  sources
}: {
  error: string | null;
  loading: boolean;
  sources: WorkbenchChannelSource[] | null;
}) {
  if (loading && sources === null) {
    return <p className="channelSourceEmpty">Loading remote lanes...</p>;
  }
  if (error) {
    return <p className="channelDetailError" role="alert">{error}</p>;
  }
  if (!sources || sources.length === 0) {
    return <p className="channelSourceEmpty">No remote lanes have started a local thread yet.</p>;
  }
  return (
    <div className="channelSourceList">
      {sources.map((source) => (
        <div className="channelSourceItem" key={source.sourceKey}>
          <div>
            <strong>{source.visibleName ?? `${source.platform} channel lane`}</strong>
            <span>{source.threadTitle ?? source.threadId}</span>
          </div>
          <dl>
            <div><dt>Workdir</dt><dd>{source.workdir || "unknown"}</dd></div>
            <div><dt>Activity</dt><dd>{source.activityStatus}{source.queuedTurns > 0 ? ` (${source.queuedTurns} queued)` : ""}</dd></div>
            <div><dt>Chat</dt><dd>{source.chatLabel ?? "unknown"}</dd></div>
            <div><dt>User</dt><dd>{source.userLabel ?? "unknown"}</dd></div>
          </dl>
        </div>
      ))}
    </div>
  );
}

function sectionDomId(title: string): string {
  return title.toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-|-$/g, "");
}

const DEFAULT_PERMISSION_MODE_OPTIONS = ["default", "acceptEdits", "dontAsk", "bypassPermissions"];
const CHANNEL_WORKSPACE_MANUAL_VALUE = "__manual__";

function channelWorkspaceOptions(workspaces: SessionBrowserWorkspaceState[]): string[] {
  const seen = new Set<string>();
  const options: string[] = [];
  for (const workspace of workspaces) {
    const workdir = workspace.workdir.trim();
    if (!workdir || seen.has(workdir)) {
      continue;
    }
    seen.add(workdir);
    options.push(workdir);
  }
  return options;
}

function channelWorkspaceSelectValue(value: string, options: string[]): string {
  const workdir = value.trim();
  if (!workdir) {
    return "";
  }
  return options.includes(workdir) ? workdir : CHANNEL_WORKSPACE_MANUAL_VALUE;
}

function channelWorkspaceOptionLabel(workdir: string): string {
  const normalized = workdir.trim();
  const trimmed = normalized.replace(/[\\/]+$/, "");
  const segments = trimmed.split(/[\\/]/).filter(Boolean);
  const basename = segments[segments.length - 1] ?? "Workspace";
  return basename && basename !== normalized ? `${basename} - ${normalized}` : normalized;
}

function channelDraftFromChannel(channel: WorkbenchChannel): ChannelSettingsDraft {
  return {
    label: channel.label ?? "",
    enabled: channel.enabled,
    workdir: channel.workdir ?? "",
    model: channel.model ?? "",
    permissionMode: channel.permissionMode ?? "default",
    requireMention: channel.requireMention,
    allowUsersText: channel.allowlist.users.join("\n"),
    allowGroupsText: channel.allowlist.groups.join("\n"),
    credentialEnv: channel.credential.env ?? ""
  };
}

function channelUpdateDraftFromDraft(draft: ChannelSettingsDraft): ChannelUpdateDraft {
  return {
    label: draft.label.trim(),
    enabled: draft.enabled,
    workdir: draft.workdir.trim(),
    model: draft.model.trim(),
    permissionMode: draft.permissionMode,
    requireMention: draft.requireMention,
    allowUsers: splitChannelListText(draft.allowUsersText),
    allowGroups: splitChannelListText(draft.allowGroupsText),
    credentialEnv: draft.credentialEnv.trim()
  };
}

function channelDraftSignature(draft: ChannelSettingsDraft): string {
  return JSON.stringify(channelUpdateDraftFromDraft(draft));
}

function splitChannelListText(value: string): string[] {
  const seen = new Set<string>();
  const items: string[] = [];
  for (const part of value.split(/[,\n]/)) {
    const item = part.trim();
    if (!item || seen.has(item)) {
      continue;
    }
    seen.add(item);
    items.push(item);
  }
  return items;
}

function channelPermissionOptions(
  controls: ChannelSettingsControls,
  channel: WorkbenchChannel,
  draft: ChannelSettingsDraft
): string[] {
  return uniqueStrings([
    ...(controls?.permissionModeOptions ?? DEFAULT_PERMISSION_MODE_OPTIONS),
    "default",
    channel.permissionMode ?? "",
    draft.permissionMode
  ]).filter((value) => DEFAULT_PERMISSION_MODE_OPTIONS.includes(value));
}

function channelModelOptions(
  controls: ChannelSettingsControls,
  channel: WorkbenchChannel,
  draft: ChannelSettingsDraft
): string[] {
  return uniqueStrings([
    ...(controls?.modelOptions ?? []),
    channel.model ?? "",
    draft.model
  ]).filter(Boolean);
}

function uniqueStrings(values: string[]): string[] {
  const seen = new Set<string>();
  const out: string[] = [];
  for (const value of values) {
    const item = value.trim();
    if (!item || seen.has(item)) {
      continue;
    }
    seen.add(item);
    out.push(item);
  }
  return out;
}

function permissionModeLabel(value: string): string {
  switch (value) {
    case "acceptEdits":
      return "Accept edits";
    case "dontAsk":
      return "Inline approvals";
    case "bypassPermissions":
      return "Bypass permissions";
    default:
      return "Profile default";
  }
}

function modelOptionLabel(value: string, channel: WorkbenchChannel, controls: ChannelSettingsControls): string {
  if (channel.model === value && !(controls?.modelOptions ?? []).includes(value)) {
    return `${value} (current)`;
  }
  return value;
}

type WechatQrSetupState = {
  done: boolean;
  error: string | null;
  expiresAtMs: number | null;
  intervalMs: number;
  loading: boolean;
  message: string;
  qrImage: string | null;
  qrSvg: string | null;
  qrUrl: string | null;
  sessionId: string | null;
  status: string;
};

const EMPTY_WECHAT_QR_SETUP: WechatQrSetupState = {
  done: false,
  error: null,
  expiresAtMs: null,
  intervalMs: 3000,
  loading: false,
  message: "Generate a QR code, scan it with WeChat, then Psychevo saves the iLink token locally.",
  qrImage: null,
  qrSvg: null,
  qrUrl: null,
  sessionId: null,
  status: "idle"
};

function ChannelSetupCard({
  channel,
  disabled,
  existingChannel,
  onPollWechatQrSetup,
  onStartWechatQrSetup
}: {
  channel: ChannelChoice;
  disabled: boolean;
  existingChannel: WorkbenchChannel | null;
  onPollWechatQrSetup(sessionId: string): Promise<ChannelWechatQrPollResult>;
  onStartWechatQrSetup(): Promise<ChannelWechatQrStartResult>;
}) {
  const [wechatQr, setWechatQr] = useState<WechatQrSetupState>(EMPTY_WECHAT_QR_SETUP);
  const [qrNowMs, setQrNowMs] = useState(() => Date.now());
  useEffect(() => {
    if (channel !== "wechat" || !wechatQr.sessionId || wechatQr.done || disabled) {
      return undefined;
    }
    const sessionId = wechatQr.sessionId;
    const timer = window.setInterval(() => {
      void onPollWechatQrSetup(sessionId)
        .then((result) => {
          const terminal = isWechatQrTerminalStatus(result.status, result.done);
          setWechatQr((current) => ({
            ...current,
            done: result.done,
            error: terminal && !result.done ? result.message : null,
            expiresAtMs: terminal ? null : (result.expiresAtMs ?? current.expiresAtMs),
            loading: false,
            message: result.message,
            qrImage: terminal ? null : current.qrImage,
            qrSvg: terminal ? null : current.qrSvg,
            qrUrl: terminal ? null : current.qrUrl,
            sessionId: terminal ? null : current.sessionId,
            status: result.status
          }));
        })
        .catch((error: unknown) => {
          const terminal = isWechatQrSessionLostError(error);
          setWechatQr((current) => ({
            ...current,
            error: qrSetupErrorMessage(error),
            expiresAtMs: terminal ? null : current.expiresAtMs,
            loading: false,
            qrImage: terminal ? null : current.qrImage,
            qrSvg: terminal ? null : current.qrSvg,
            qrUrl: terminal ? null : current.qrUrl,
            sessionId: terminal ? null : current.sessionId,
            status: "error"
          }));
        });
    }, wechatQr.intervalMs);
    return () => window.clearInterval(timer);
  }, [channel, disabled, onPollWechatQrSetup, wechatQr.done, wechatQr.intervalMs, wechatQr.sessionId]);

  useEffect(() => {
    if (channel !== "wechat" || !wechatQr.sessionId || !wechatQr.expiresAtMs || wechatQr.done || wechatQr.status === "expired") {
      return undefined;
    }
    setQrNowMs(Date.now());
    const timer = window.setInterval(() => setQrNowMs(Date.now()), 1000);
    return () => window.clearInterval(timer);
  }, [channel, wechatQr.done, wechatQr.expiresAtMs, wechatQr.sessionId, wechatQr.status]);

  useEffect(() => {
    if (channel !== "wechat" || !wechatQr.sessionId || !wechatQr.expiresAtMs || wechatQr.done || wechatQr.status === "expired") {
      return;
    }
    if (qrNowMs < wechatQr.expiresAtMs) {
      return;
    }
    setWechatQr((current) => {
      if (!current.sessionId || !current.expiresAtMs || current.done || current.status === "expired" || qrNowMs < current.expiresAtMs) {
        return current;
      }
      return {
        ...current,
        error: "WeChat QR session expired. Generate a new code.",
        expiresAtMs: null,
        loading: false,
        message: "WeChat QR session expired. Generate a new code.",
        qrImage: null,
        qrSvg: null,
        qrUrl: null,
        sessionId: null,
        status: "expired"
      };
    });
  }, [channel, qrNowMs, wechatQr.done, wechatQr.expiresAtMs, wechatQr.sessionId, wechatQr.status]);

  async function startWechatQr() {
    setQrNowMs(Date.now());
    setWechatQr((current) => ({ ...current, error: null, loading: true, status: "starting" }));
    try {
      const result = await onStartWechatQrSetup();
      setWechatQr({
        done: false,
        error: null,
        expiresAtMs: result.expiresAtMs,
        intervalMs: result.intervalMs,
        loading: false,
        message: result.message,
        qrImage: result.qrImage ?? null,
        qrSvg: result.qrSvg,
        qrUrl: result.qrUrl,
        sessionId: result.sessionId,
        status: result.status
      });
    } catch (error) {
      setWechatQr((current) => ({
        ...current,
        error: qrSetupErrorMessage(error),
        loading: false,
        status: "error"
      }));
    }
  }

  async function checkWechatQr() {
    if (!wechatQr.sessionId) {
      return;
    }
    setWechatQr((current) => ({ ...current, error: null, loading: true }));
    try {
      const result = await onPollWechatQrSetup(wechatQr.sessionId);
      const terminal = isWechatQrTerminalStatus(result.status, result.done);
      setWechatQr((current) => ({
        ...current,
        done: result.done,
        error: terminal && !result.done ? result.message : null,
        expiresAtMs: terminal ? null : (result.expiresAtMs ?? current.expiresAtMs),
        loading: false,
        message: result.message,
        qrImage: terminal ? null : current.qrImage,
        qrSvg: terminal ? null : current.qrSvg,
        qrUrl: terminal ? null : current.qrUrl,
        sessionId: terminal ? null : current.sessionId,
        status: result.status
      }));
    } catch (error) {
      const terminal = isWechatQrSessionLostError(error);
      setWechatQr((current) => ({
        ...current,
        error: qrSetupErrorMessage(error),
        expiresAtMs: terminal ? null : current.expiresAtMs,
        loading: false,
        qrImage: terminal ? null : current.qrImage,
        qrSvg: terminal ? null : current.qrSvg,
        qrUrl: terminal ? null : current.qrUrl,
        sessionId: terminal ? null : current.sessionId,
        status: "error"
      }));
    }
  }

  if (channel === "wechat") {
    const reconnectRequired = existingChannel?.runner.reason === "needs_qr_login";
    const loginPending = existingChannel?.runner.reason === "qr_login_pending";
    const connectedWechat = existingChannel && existingChannel.credential.status === "present" && existingChannel.allowlist.status === "present" && !reconnectRequired && !loginPending;
    if (loginPending && !wechatQr.sessionId && !wechatQr.loading && !wechatQr.qrImage && !wechatQr.qrSvg) {
      return (
        <div className="channelSetupCard channelSetupCardPending">
          <div className="channelConnectedMark" aria-hidden>
            <Activity size={22} />
          </div>
          <div className="channelWechatSetupBody">
            <strong>WeChat polling is starting</strong>
            <span>{wechatQr.done && wechatQr.message ? wechatQr.message : "Credentials are saved. Gateway is starting polling."}</span>
            <small>Send a DM to the iLink bot while the Gateway starts polling.</small>
            <div className="channelWechatActions">
              <button disabled={disabled || wechatQr.loading} onClick={() => void startWechatQr()} type="button">
                <RotateCcw size={13} />
                <span>Reconnect QR</span>
              </button>
            </div>
            <div className="channelSetupFields">
              <span>WECHAT_BOT_TOKEN</span>
              <span>WECHAT_ACCOUNT_ID</span>
              <span>qr_login_pending</span>
            </div>
          </div>
        </div>
      );
    }
    if (reconnectRequired && !wechatQr.sessionId && !wechatQr.loading && !wechatQr.qrImage && !wechatQr.qrSvg) {
      return (
        <div className="channelSetupCard channelSetupCardReconnect">
          <div className="channelConnectedMark" aria-hidden>
            <RotateCcw size={22} />
          </div>
          <div className="channelWechatSetupBody">
            <strong>WeChat reconnect required</strong>
            <span>The iLink login expired. Generate a new QR code and scan it again to resume polling.</span>
            {existingChannel?.runner.lastError && <small className="agentSurfaceWarning">{existingChannel.runner.lastError}</small>}
            <div className="channelWechatActions">
              <button disabled={disabled || wechatQr.loading} onClick={() => void startWechatQr()} type="button">
                <RotateCcw size={13} />
                <span>Reconnect QR</span>
              </button>
            </div>
            <div className="channelSetupFields">
              <span>WECHAT_BOT_TOKEN</span>
              <span>WECHAT_ACCOUNT_ID</span>
              <span>needs_qr_login</span>
            </div>
          </div>
        </div>
      );
    }
    if (connectedWechat && !wechatQr.sessionId && !wechatQr.loading && !wechatQr.qrImage && !wechatQr.qrSvg) {
      return (
        <div className="channelSetupCard channelSetupCardConnected">
          <div className="channelConnectedMark" aria-hidden>
            <PlugZap size={22} />
          </div>
          <div className="channelWechatSetupBody">
            <strong>WeChat connected</strong>
            <span>Credential and DM allowlist are present. The Gateway runner state is {existingChannel.runner.state}.</span>
            {existingChannel.runner.lastError && <small className="agentSurfaceWarning">{existingChannel.runner.lastError}</small>}
            <div className="channelWechatActions">
              <button disabled={disabled || wechatQr.loading} onClick={() => void startWechatQr()} type="button">
                <RotateCcw size={13} />
                <span>Reconnect QR</span>
              </button>
            </div>
            <div className="channelSetupFields">
              <span>WECHAT_BOT_TOKEN</span>
              <span>WECHAT_ACCOUNT_ID</span>
              <span>DM allowlist</span>
            </div>
          </div>
        </div>
      );
    }
    return (
      <div className="channelSetupCard channelSetupCardWechat">
        <div className="channelWechatQrBox" aria-label="WeChat QR code">
          {wechatQr.qrImage ? (
            <img alt="WeChat QR code" className="channelWechatQrImage" src={wechatQr.qrImage} />
          ) : wechatQr.qrSvg ? (
            <div className="channelWechatQrSvg" dangerouslySetInnerHTML={{ __html: wechatQr.qrSvg }} />
          ) : (
            <QrPlaceholder />
          )}
        </div>
        <div className="channelWechatSetupBody" aria-live="polite">
          <strong>WeChat setup</strong>
          <span>{wechatQr.message}</span>
          {wechatQr.done && <small>The token is saved in the active profile .env.</small>}
          {wechatQr.expiresAtMs && !wechatQr.done && (
            <small className="channelWechatTimer">{formatQrTimeLeft(wechatQr.expiresAtMs, qrNowMs)}</small>
          )}
          {wechatQr.error && <small className="agentSurfaceWarning">{wechatQr.error}</small>}
          <div className="channelWechatActions">
            <button disabled={disabled || wechatQr.loading} onClick={() => void startWechatQr()} type="button">
              <MessageCircle size={13} />
              <span>{wechatQr.status === "error" || wechatQr.status === "expired" || wechatQr.status === "needs_qr_login" ? "Generate again" : "Generate QR"}</span>
            </button>
            <button disabled={disabled || wechatQr.loading || !wechatQr.sessionId || wechatQr.done} onClick={() => void checkWechatQr()} type="button">
              <Wrench size={13} />
              <span>Check status</span>
            </button>
          </div>
          <div className="channelSetupFields">
            <span>WECHAT_BOT_TOKEN</span>
            <span>WECHAT_ACCOUNT_ID</span>
            <span>DM allowlist</span>
          </div>
        </div>
      </div>
    );
  }
  const setup = channelSetupCopy(channel);
  return (
    <div className="channelSetupCard">
      <strong>{setup.title}</strong>
      <span>{setup.primary}</span>
      <code>{setup.command}</code>
      <div className="channelSetupFields">
        {setup.fields.map((field) => (
          <span key={field}>{field}</span>
        ))}
      </div>
    </div>
  );
}

function QrPlaceholder() {
  return (
    <div className="channelQrPlaceholder" aria-hidden>
      <span />
      <span />
      <span />
      <span />
    </div>
  );
}

function channelRuntimeDefaultsSummary(draft: ChannelSettingsDraft): string {
  const model = draft.model.trim() || "profile model";
  const workspace = draft.workdir.trim() ? "custom workspace" : "default workspace";
  return `${permissionModeLabel(draft.permissionMode)} · ${model} · ${workspace}`;
}

function ChannelHealthItem({
  label,
  tone,
  value
}: {
  label: string;
  tone: "danger" | "muted" | "ok" | "warning";
  value: string;
}) {
  return (
    <span className={`channelHealthItem is-${tone}`}>
      <span>{label}</span>
      <strong>{value}</strong>
    </span>
  );
}

function formatQrTimeLeft(expiresAtMs: number, nowMs: number): string {
  const seconds = Math.max(0, Math.ceil((expiresAtMs - nowMs) / 1000));
  if (seconds === 0) {
    return "QR expired";
  }
  return `${seconds}s left`;
}

function isWechatQrTerminalStatus(status: string, done: boolean): boolean {
  return done || status === "expired" || status === "needs_qr_login";
}

function isWechatQrSessionLostError(error: unknown): boolean {
  const message = error instanceof Error ? error.message : String(error);
  return message.includes("QR session not found");
}

function qrSetupErrorMessage(error: unknown): string {
  const message = error instanceof Error ? error.message : String(error);
  if (message.includes("QR session not found")) {
    return "This QR session has expired, completed, or was created before the Gateway restarted. Generate a new code to reconnect.";
  }
  return message;
}

function channelSetupCopy(channel: ChannelChoice): { command: string; fields: string[]; primary: string; title: string } {
  switch (channel) {
    case "wechat":
      return {
        title: "WeChat setup",
        primary: "Generate a QR code, scan it with WeChat, and store the iLink token in the active profile.",
        command: "pevo gateway setup --channel wechat --qr",
        fields: ["QR login", "WECHAT_BOT_TOKEN", "WECHAT_ACCOUNT_ID", "allow_users"]
      };
    case "telegram":
      return {
        title: "Telegram setup",
        primary: "Create a bot with BotFather and paste the token through stdin.",
        command: "pevo gateway setup --channel telegram --id telegram --allow-user CHAT_ID --credential-stdin",
        fields: ["BotFather token", "TELEGRAM_BOT_TOKEN", "allow_users or allow_groups"]
      };
    case "feishu":
      return {
        title: "Feishu setup",
        primary: "Configure app id and secret env vars for the Feishu long-connection adapter.",
        command: "pevo gateway setup --channel feishu --id feishu --allow-group OPEN_CHAT_ID --credential-stdin",
        fields: ["FEISHU_APP_ID", "FEISHU_APP_SECRET", "allow_groups"]
      };
    case "lark":
      return {
        title: "Lark setup",
        primary: "Configure app id and secret env vars for the Lark long-connection adapter.",
        command: "pevo gateway setup --channel lark --id lark --allow-group OPEN_CHAT_ID --credential-stdin",
        fields: ["LARK_APP_ID", "LARK_APP_SECRET", "allow_groups"]
      };
  }
}

function ChannelStatusPill({ status }: { status: string }) {
  return <small className={`channelStatusPill is-${status} is-${channelStatusTone(status)}`}>{status}</small>;
}

function channelDoctorOk(doctor: WorkbenchChannelDoctor): boolean {
  return doctor.checks.every((check) => check.status === "ok" || check.status === "skipped");
}

function channelRuntimeSummary(channel: WorkbenchChannel, fallbackWorkdir: string): string {
  const model = channel.model ?? "default model";
  const workdir = channel.workdir ?? fallbackWorkdir;
  return `${model} · ${workdir}`;
}

function channelRunnerTone(status: string): "danger" | "muted" | "ok" | "warning" {
  switch (status) {
    case "running":
      return "ok";
    case "blocked":
      return "warning";
    case "error":
      return "danger";
    default:
      return "muted";
  }
}

function formatRunnerActivity(channel: WorkbenchChannel): string {
  if (channel.runner.reason === "qr_login_pending") {
    return "polling start pending";
  }
  if (channel.runner.reason === "needs_qr_login") {
    return "QR reconnect required";
  }
  if (channel.runner.lastOutboundAtMs) {
    return `outbound ${formatRunnerTimestamp(channel.runner.lastOutboundAtMs)}`;
  }
  if (channel.runner.lastInboundAtMs) {
    return `inbound ${formatRunnerTimestamp(channel.runner.lastInboundAtMs)}`;
  }
  if (channel.runner.lastPollAtMs) {
    return `poll ${formatRunnerTimestamp(channel.runner.lastPollAtMs)}`;
  }
  if (channel.runner.reason) {
    return channel.runner.reason;
  }
  return channel.runner.lastError ?? "no activity yet";
}

function formatRunnerTimestamp(value: number | null | undefined): string {
  if (!value) {
    return "never";
  }
  return new Date(value).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit", second: "2-digit" });
}

function channelAllowlistSummary(channel: WorkbenchChannel): string {
  const parts = [];
  if (channel.allowlist.users.length) {
    parts.push(`${channel.allowlist.users.length} user${channel.allowlist.users.length === 1 ? "" : "s"}`);
  }
  if (channel.allowlist.groups.length) {
    parts.push(`${channel.allowlist.groups.length} group${channel.allowlist.groups.length === 1 ? "" : "s"}`);
  }
  return parts.length ? parts.join(", ") : "none";
}

function channelStatusTone(status: string): "danger" | "muted" | "ok" | "warning" {
  switch (status) {
    case "ok":
    case "present":
    case "ready":
    case "running":
      return "ok";
    case "blocked":
    case "error":
    case "fail":
    case "missing":
      return "danger";
    case "needs_qr_login":
    case "qr_login_pending":
    case "needs_account":
    case "needs_allow_user":
    case "group_limited":
    case "warn":
      return "warning";
    default:
      return "muted";
  }
}

function formatChannelName(value: string): string {
  switch (value) {
    case "wechat":
      return "WeChat";
    case "telegram":
      return "Telegram";
    case "feishu":
      return "Feishu";
    case "lark":
      return "Lark";
    default:
      return value;
  }
}

function settingsSectionIcon(section: SettingsSection, size: number): ReactNode {
  switch (section) {
    case "appearance":
      return <Sun size={size} />;
    case "models":
      return <BrainCircuit size={size} />;
    case "usage":
      return <Activity size={size} />;
    case "archived":
      return <Archive size={size} />;
    case "debug":
      return <Bug size={size} />;
    case "agents":
      return <Bot size={size} />;
    case "channels":
      return <MessageCircle size={size} />;
  }
}

function AgentsConfigPanel({
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
          <span>CWD</span>
          <input
            aria-label="CWD"
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



function shortSessionId(id: string): string {
  return id.length <= 12 ? id : `${id.slice(0, 8)}...`;
}
