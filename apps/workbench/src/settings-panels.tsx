import { useState, type ReactNode } from "react";
import {
  Activity,
  Archive,
  ArrowLeft,
  BrainCircuit,
  Bug,
  Keyboard,
  MessageCircle,
  Globe2,
  Moon,
  Palette,
  Search,
  Sun
} from "lucide-react";
import type { GatewayClient } from "@psychevo/client";
import type { ChannelWechatQrPollResult, ChannelWechatQrStartResult, ModelOptionView, RuntimeProfileView, SessionSummary } from "@psychevo/protocol";
import { Switch } from "@psychevo/components";
import type {
  Appearance,
  SessionBrowserWorkspaceState,
  SettingsSection,
  WorkbenchChannel,
  WorkbenchChannelDoctor,
  WorkbenchChannelSource,
  WorkbenchUsageStats
} from "./types";
import { ChannelsSettingsPanel } from "./settings-panels/channels";
import { ModelsSettingsPanel } from "./settings-panels/models";
import { SlashCommandsSettingsPanel } from "./settings-panels/slash";
import { WebSearchSettingsPanel } from "./settings-panels/web-search";
import { ArchivedSessionsPanel, UsageSettingsPanel } from "./settings-panels/usage";
import type { ChannelSettingsControls, ChannelUpdateDraft } from "./settings-panels/types";

const SETTINGS_SECTIONS: Array<{ id: SettingsSection; label: string; description: string }> = [
  { id: "appearance", label: "Appearance", description: "Theme" },
  { id: "models", label: "Models", description: "Providers and auxiliary models" },
  { id: "web-search", label: "Web Search", description: "Execution, backends, and hosted controls" },
  { id: "slash", label: "Slash Commands", description: "Aliases and TUI shortcuts" },
  { id: "usage", label: "Usage", description: "Tokens and cost" },
  { id: "debug", label: "Debug", description: "Developer diagnostics" },
  { id: "channels", label: "Channels", description: "Messaging connections" },
  { id: "archived", label: "Archived sessions", description: "Restore or delete" }
];
export function SettingsPage({
  appearance,
  archivedSessions,
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
  onDebugChange,
  onDeleteArchivedSession,
  onDeleteChannel,
  onDoctorChannel,
  onDoctorChannels,
  onOpenTranscript,
  onLoadChannelSources,
  onModelAssignmentSaved,
  onModelCatalogLoaded,
  onPollWechatQrSetup,
  onRestoreArchivedSession,
  onSectionChange,
  onSlashSettingsSaved,
  onRefreshUsageStats,
  onSetChannelEnabled,
  onStartWechatQrSetup,
  onUpdateChannel,
  runtimeProfiles,
  sessionBrowserWorkspaces,
  cwd
}: {
  appearance: Appearance;
  archivedSessions: SessionSummary[];
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
  onDebugChange(value: boolean): void;
  onDeleteArchivedSession(threadId: string): void;
  onDeleteChannel(channel: WorkbenchChannel): Promise<void>;
  onDoctorChannel(channel: WorkbenchChannel): void;
  onDoctorChannels(): void;
  onOpenTranscript(): void;
  onLoadChannelSources(channel: WorkbenchChannel): Promise<WorkbenchChannelSource[]>;
  onModelAssignmentSaved(): Promise<void>;
  onModelCatalogLoaded(options: ModelOptionView[]): void;
  onPollWechatQrSetup(sessionId: string): Promise<ChannelWechatQrPollResult>;
  onRestoreArchivedSession(threadId: string): void;
  onSectionChange(value: SettingsSection): void;
  onSlashSettingsSaved(): Promise<void>;
  onRefreshUsageStats(): void;
  onSetChannelEnabled(channel: WorkbenchChannel, enabled: boolean): void;
  onStartWechatQrSetup(): Promise<ChannelWechatQrStartResult>;
  onUpdateChannel(channel: WorkbenchChannel, draft: ChannelUpdateDraft): Promise<WorkbenchChannel>;
  runtimeProfiles: RuntimeProfileView[];
  sessionBrowserWorkspaces: SessionBrowserWorkspaceState[];
  cwd: string;
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
              onDebugChange={onDebugChange}
              onDeleteArchivedSession={onDeleteArchivedSession}
              onDeleteChannel={onDeleteChannel}
              onDoctorChannel={onDoctorChannel}
              onDoctorChannels={onDoctorChannels}
              onLoadChannelSources={onLoadChannelSources}
              onModelAssignmentSaved={onModelAssignmentSaved}
              onModelCatalogLoaded={onModelCatalogLoaded}
              onPollWechatQrSetup={onPollWechatQrSetup}
              onRestoreArchivedSession={onRestoreArchivedSession}
              onSlashSettingsSaved={onSlashSettingsSaved}
              onRefreshUsageStats={onRefreshUsageStats}
              onSetChannelEnabled={onSetChannelEnabled}
              onStartWechatQrSetup={onStartWechatQrSetup}
              onUpdateChannel={onUpdateChannel}
              runtimeProfiles={runtimeProfiles}
              sessionBrowserWorkspaces={sessionBrowserWorkspaces}
              cwd={cwd}
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
  onDebugChange,
  onDeleteArchivedSession,
  onDeleteChannel,
  onDoctorChannel,
  onDoctorChannels,
  onLoadChannelSources,
  onModelAssignmentSaved,
  onModelCatalogLoaded,
  onPollWechatQrSetup,
  onRefreshUsageStats,
  onRestoreArchivedSession,
  onSetChannelEnabled,
  onSlashSettingsSaved,
  onStartWechatQrSetup,
  onUpdateChannel,
  runtimeProfiles,
  sessionBrowserWorkspaces,
  cwd
}: {
  appearance: Appearance;
  archivedSessions: SessionSummary[];
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
  onDebugChange(value: boolean): void;
  onDeleteArchivedSession(threadId: string): void;
  onDeleteChannel(channel: WorkbenchChannel): Promise<void>;
  onDoctorChannel(channel: WorkbenchChannel): void;
  onDoctorChannels(): void;
  onLoadChannelSources(channel: WorkbenchChannel): Promise<WorkbenchChannelSource[]>;
  onModelAssignmentSaved(): Promise<void>;
  onModelCatalogLoaded(options: ModelOptionView[]): void;
  onPollWechatQrSetup(sessionId: string): Promise<ChannelWechatQrPollResult>;
  onRefreshUsageStats(): void;
  onRestoreArchivedSession(threadId: string): void;
  onSetChannelEnabled(channel: WorkbenchChannel, enabled: boolean): void;
  onSlashSettingsSaved(): Promise<void>;
  onStartWechatQrSetup(): Promise<ChannelWechatQrStartResult>;
  onUpdateChannel(channel: WorkbenchChannel, draft: ChannelUpdateDraft): Promise<WorkbenchChannel>;
  runtimeProfiles: RuntimeProfileView[];
  sessionBrowserWorkspaces: SessionBrowserWorkspaceState[];
  cwd: string;
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
          cwd={cwd}
        />
      );
    case "slash":
      return (
        <SlashCommandsSettingsPanel
          client={client}
          disabled={disabled}
          onSaved={onSlashSettingsSaved}
          cwd={cwd}
        />
      );
    case "web-search":
      return <WebSearchSettingsPanel client={client} cwd={cwd} disabled={disabled} />;
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
            <Switch
              ariaLabel="Show debug tab"
              checked={debugEnabled}
              label="Show debug tab"
              onCheckedChange={onDebugChange}
              showLabel={false}
            />
          </SettingsOptionRow>
        </div>
      );
    case "channels":
      return (
        <ChannelsSettingsPanel
          channelDoctor={channelDoctor}
          channels={channels}
          client={client}
          disabled={disabled}
          onDeleteChannel={onDeleteChannel}
          onDoctorChannel={onDoctorChannel}
          onDoctorChannels={onDoctorChannels}
          onLoadChannelSources={onLoadChannelSources}
          onPollWechatQrSetup={onPollWechatQrSetup}
          onSetChannelEnabled={onSetChannelEnabled}
          onStartWechatQrSetup={onStartWechatQrSetup}
          onUpdateChannel={onUpdateChannel}
          runtimeProfiles={runtimeProfiles}
          sessionBrowserWorkspaces={sessionBrowserWorkspaces}
          cwd={cwd}
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


function settingsSectionIcon(section: SettingsSection, size: number): ReactNode {
  switch (section) {
    case "appearance":
      return <Sun size={size} />;
    case "models":
      return <BrainCircuit size={size} />;
    case "web-search":
      return <Globe2 size={size} />;
    case "slash":
      return <Keyboard size={size} />;
    case "usage":
      return <Activity size={size} />;
    case "archived":
      return <Archive size={size} />;
    case "debug":
      return <Bug size={size} />;
    case "channels":
      return <MessageCircle size={size} />;
  }
}
