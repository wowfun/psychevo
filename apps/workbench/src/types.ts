import type {
  ContextReadResult,
  AutomationTaskView,
  ChannelConfigView,
  ChannelDoctorChannelView,
  ChannelSourceBindingView,
  GatewayEvent,
  GatewayInputPart,
  RuntimeHistoryFidelityView,
  SessionUsageSummaryView,
  TerminalExitedPayload,
  TerminalOutputPayload,
  ThreadTraceResult,
  ThreadBrowserCursor,
  UsageReadResult,
  WorkspaceDiffResult,
  WorkspaceFileReadResult
} from "@psychevo/protocol";

export type ContextUsageCategory = ContextReadResult["categories"][number];

export type AgentContribution = "instructions" | "tools" | "mcp" | "skills";

export type WorkbenchAgent = {
  name: string;
  description: string;
  enabled: boolean;
  source: string;
  sourceLabel: string;
  generated: boolean;
  target?: BackendConfigTarget | null;
  mutable: boolean;
  path?: string | null;
  entrypoints: string[];
  tools: string[];
  mcpServers: string[];
  contributions: AgentContribution[];
  optionalContributions: AgentContribution[];
  diagnostics: WorkbenchDiagnostic[];
  backend?: { ref?: string } | null;
};

export type WorkbenchBackend = {
  id: string;
  kind: string;
  enabled: boolean;
  label: string;
  description?: string | null;
  command?: string | null;
  args: string[];
  cwd: string;
  entrypoints: string[];
  clientCapabilities: string[];
  mcpServers: string[];
  envKeys: string[];
  sourceTargets: BackendConfigTarget[];
  diagnostics: WorkbenchDiagnostic[];
};

export type BackendConfigTarget = "project" | "profile";

export type WorkbenchDiagnostic = {
  kind: string;
  message: string;
};

export type WorkbenchBackendDoctor = {
  id: string;
  ok: boolean;
  checks: Array<{ name: string; ok: boolean; message: string; path: string | null }>;
};

export type WorkbenchChannel = ChannelConfigView;
export type WorkbenchChannelDoctor = ChannelDoctorChannelView;
export type WorkbenchChannelSource = ChannelSourceBindingView;

export type BackendDraft = {
  id: string;
  enabled: boolean;
  label: string;
  description: string;
  commandJsonText: string;
  cwd: string;
  entrypoints: string[];
  clientCapabilities: string[];
  mcpServersText: string;
};

export type BackendCommandJson = {
  command: string;
  args: string[];
  env: Record<string, string>;
};

export type WorkbenchCommand = {
  name: string;
  slash: string;
  usage: string;
  summary: string;
  aliases: string[];
  argumentKind: string;
  source: string;
  expandsTo: string | null;
  presentationKind: string;
  destination: string | null;
  feedbackAnchor: string | null;
  alternateAction: CommandAlternateAction | null;
};

export type RightWorkspacePreview = {
  content: string;
  kind: "html" | "markdown";
  path?: string | null;
  title: string;
};

export type RightWorkspaceBrowserState = {
  address: string;
  currentUrl: string | null;
  reloadKey: number;
};

export type RightWorkspaceTabKind =
  | "review"
  | "terminal"
  | "files"
  | "debug"
  | "sideConversation"
  | "agentSession"
  | "team"
  | "browser"
  | "preview";

export type RightWorkspaceTab = {
  id: string;
  kind: RightWorkspaceTabKind;
  title: string;
  threadId?: string | null;
  parentThreadId?: string | null;
  runtimeRef?: string | null;
  runtimeStatus?: string | null;
  runtimeReadOnly?: boolean;
  historyFidelity?: RuntimeHistoryFidelityView | null;
  pendingPrompt?: string | null;
  path?: string | null;
  diff?: WorkspaceDiffResult | null;
  file?: WorkspaceFileReadResult | null;
  browser?: RightWorkspaceBrowserState;
  preview?: RightWorkspacePreview | null;
  message?: string | null;
};

export type MainView = "transcript" | "search" | "settings" | "automations" | "capabilities";
export type CapabilityTab = "agents" | "skills" | "plugins" | "mcp" | "tools";
export type SettingsSection = "appearance" | "models" | "slash" | "usage" | "debug" | "channels" | "archived";
export type WorkbenchUsageStats = UsageReadResult;
export type WorkbenchAutomation = AutomationTaskView;
export type Appearance = "dark" | "light" | "warm";
export type CommandOverlay = "commands";
export type CommandTrigger = "composer" | "commandsPanel" | "commandOverlay";

export type CommandAlternateAction = {
  type: string;
  target: string;
  label: string;
};

export type CommandFeedback = {
  accepted: boolean;
  command: string;
  message: string;
  feedbackAnchor?: string | null;
  alternateAction?: CommandAlternateAction | null;
} | null;

export type DebugEvent = {
  id: string;
  at: number;
  method: string;
  payload: unknown;
};

export type TraceState = {
  error: string | null;
  loading: boolean;
  result: ThreadTraceResult | null;
  threadId: string | null;
};

export type SessionBrowserWorkspaceState = {
  cwd: string;
  hiddenCount: number;
  nextCursor: ThreadBrowserCursor | null;
};

export type PendingAttachment = {
  id: string;
  input: GatewayInputPart;
  kind: "file" | "image" | "text";
  name: string;
  previewUrl?: string | null;
  size: number;
  sizeLabel: string;
  error?: string | null;
};

export type SearchResult = {
  excerpt: string;
  id: string;
  kind: "message" | "session";
  subtitle: string;
  title: string;
};

export type WorkbenchPrefs = {
  appearance: Appearance;
  appearanceVersion: 1;
  debug: boolean;
  rightWidthPx: number;
};

export type TerminalNotificationEvent =
  | { method: "terminal/output"; params: TerminalOutputPayload; seq: number }
  | { method: "terminal/exited"; params: TerminalExitedPayload; seq: number };

export type GatewayEventFeed = {
  event: GatewayEvent;
  seq: number;
};

export type WorkspaceFileTreeItem = {
  badge?: string | null;
  disabled?: boolean;
  kind: "directory" | "file";
  name: string;
  path: string;
  depth: number;
  status?: string | null;
};
