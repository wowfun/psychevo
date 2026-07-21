import { useState, type ReactNode } from "react";
import {
  Activity,
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
import type { ChannelWechatQrPollResult, ChannelWechatQrStartResult, ModelOptionView, RuntimeProfileView } from "@psychevo/protocol";
import { ActionButton, NavItem, Switch } from "@psychevo/components";
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
import { UsageSettingsPanel } from "./settings-panels/usage";
import type { ChannelSettingsControls, ChannelUpdateDraft } from "./settings-panels/types";

const SETTINGS_SECTIONS: Array<{ id: SettingsSection; label: string; description: string }> = [
  { id: "appearance", label: "Appearance", description: "Theme" },
  { id: "models", label: "Models", description: "Providers and auxiliary models" },
  { id: "web-search", label: "Web Search", description: "Execution, backends, and hosted controls" },
  { id: "slash", label: "Slash Commands", description: "Aliases and TUI shortcuts" },
  { id: "usage", label: "Usage", description: "Tokens and cost" },
  { id: "debug", label: "Debug", description: "Developer diagnostics" },
  { id: "channels", label: "Channels", description: "Messaging connections" }
];
export function SettingsPage({
  appearance,
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
  onDeleteChannel,
  onDoctorChannel,
  onDoctorChannels,
  onOpenTranscript,
  onLoadChannelSources,
  onModelAssignmentSaved,
  onModelCatalogLoaded,
  onPollWechatQrSetup,
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
  onDeleteChannel(channel: WorkbenchChannel): Promise<void>;
  onDoctorChannel(channel: WorkbenchChannel): void;
  onDoctorChannels(): void;
  onOpenTranscript(): void;
  onLoadChannelSources(channel: WorkbenchChannel): Promise<WorkbenchChannelSource[]>;
  onModelAssignmentSaved(): Promise<void>;
  onModelCatalogLoaded(options: ModelOptionView[]): void;
  onPollWechatQrSetup(sessionId: string): Promise<ChannelWechatQrPollResult>;
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
  const sectionMatches = (item: (typeof SETTINGS_SECTIONS)[number]) => (
    !normalizedQuery
    || item.label.toLowerCase().includes(normalizedQuery)
    || item.description.toLowerCase().includes(normalizedQuery)
  );
  const visibleSections = SETTINGS_SECTIONS.filter(sectionMatches);
  return (
    <section className="centerPage settingsPage" aria-label="Settings">
      <div className="settingsShell">
        <aside className="settingsNav" aria-label="Settings sections">
          <div className="settingsNavTop">
            <ActionButton block className="settingsBackButton" icon={<ArrowLeft size={15} />} onClick={onOpenTranscript} type="button" variant="ghost">Back to app</ActionButton>
            <label className="settingsSearch pevo-searchField">
              <Search size={14} aria-hidden />
              <input
                aria-label="Search settings"
                className="pevo-fieldControl pevo-fieldControl--search"
                onChange={(event) => setQuery(event.currentTarget.value)}
                placeholder="Search settings"
                type="search"
                value={query}
              />
            </label>
          </div>
          <div className="settingsNavGroups">
            {visibleSections.map((item) => (
              <NavItem
                className={item.id === section ? "is-selected" : ""}
                current={item.id === section}
                icon={settingsSectionIcon(item.id, 15)}
                key={item.id}
                label={item.label}
                onSelect={() => onSectionChange(item.id)}
              />
            ))}
            {visibleSections.length === 0 && (
              <p className="settingsNavEmpty">No settings found</p>
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
              onDeleteChannel={onDeleteChannel}
              onDoctorChannel={onDoctorChannel}
              onDoctorChannels={onDoctorChannels}
              onLoadChannelSources={onLoadChannelSources}
              onModelAssignmentSaved={onModelAssignmentSaved}
              onModelCatalogLoaded={onModelCatalogLoaded}
              onPollWechatQrSetup={onPollWechatQrSetup}
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
  onDeleteChannel,
  onDoctorChannel,
  onDoctorChannels,
  onLoadChannelSources,
  onModelAssignmentSaved,
  onModelCatalogLoaded,
  onPollWechatQrSetup,
  onRefreshUsageStats,
  onSetChannelEnabled,
  onSlashSettingsSaved,
  onStartWechatQrSetup,
  onUpdateChannel,
  runtimeProfiles,
  sessionBrowserWorkspaces,
  cwd
}: {
  appearance: Appearance;
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
  onDeleteChannel(channel: WorkbenchChannel): Promise<void>;
  onDoctorChannel(channel: WorkbenchChannel): void;
  onDoctorChannels(): void;
  onLoadChannelSources(channel: WorkbenchChannel): Promise<WorkbenchChannelSource[]>;
  onModelAssignmentSaved(): Promise<void>;
  onModelCatalogLoaded(options: ModelOptionView[]): void;
  onPollWechatQrSetup(sessionId: string): Promise<ChannelWechatQrPollResult>;
  onRefreshUsageStats(): void;
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
    case "debug":
      return (
        <div className="settingsRows">
          <SettingsOptionRow title="Show debug tab" description="Recent Gateway notifications in the right inspector.">
            <Switch
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
    case "debug":
      return <Bug size={size} />;
    case "channels":
      return <MessageCircle size={size} />;
  }
}
