import type {
  ContextReadResult,
  GatewayEvent,
  GatewayInputPart,
  SessionUsageSummaryView,
  TerminalExitedPayload,
  TerminalOutputPayload,
  ThreadTraceResult,
  ThreadBrowserCursor,
  WorkspaceDiffResult,
  WorkspaceFileReadResult
} from "@psychevo/protocol";

export type ContextUsageCategory = ContextReadResult["categories"][number];

export type WorkbenchAgent = {
  name: string;
  description: string;
  source: string;
  generated: boolean;
  path?: string | null;
  entrypoints: string[];
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
  presentationKind: string;
  destination: string | null;
  feedbackAnchor: string | null;
  alternateAction: CommandAlternateAction | null;
};

export type RightWorkspaceTabKind = "review" | "terminal" | "files" | "debug" | "sideConversation" | "agentSession";

export type RightWorkspaceTab = {
  id: string;
  kind: RightWorkspaceTabKind;
  title: string;
  threadId?: string | null;
  parentThreadId?: string | null;
  pendingPrompt?: string | null;
  path?: string | null;
  diff?: WorkspaceDiffResult | null;
  file?: WorkspaceFileReadResult | null;
  message?: string | null;
};

export type MainView = "transcript" | "search" | "settings";
export type SettingsSection = "appearance" | "debug" | "agents" | "archived";
export type Appearance = "dark" | "light";
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
  workdir: string;
  hiddenCount: number;
  nextCursor: ThreadBrowserCursor | null;
};

export type PendingAttachment = {
  id: string;
  input: GatewayInputPart;
  kind: "file" | "image" | "text";
  name: string;
  size: number;
  sizeLabel: string;
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

export type ParsedDiffLineKind = "add" | "delete" | "context" | "meta";

export type ParsedDiffLine = {
  kind: ParsedDiffLineKind;
  marker: string;
  newNumber: number | null;
  oldNumber: number | null;
  text: string;
};

export type ParsedDiffHunk = {
  header: string;
  lines: ParsedDiffLine[];
};

export type ParsedDiffFile = {
  headers: string[];
  hunks: ParsedDiffHunk[];
  path: string;
};
