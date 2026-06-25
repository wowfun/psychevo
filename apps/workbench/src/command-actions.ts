import type { Dispatch, MutableRefObject, SetStateAction } from "react";
import {
  appendOptimisticPrompt,
  scopeForWorkdir,
  type GatewayClient
} from "@psychevo/client";
import {
  WorkspaceDiffResultSchema,
  type GatewayMention,
  type GatewayRequestScope,
  type RuntimeOptionsResult,
  type SessionSummary,
  type SettingsReadResult,
  type ThreadSnapshot,
  type WorkspaceDiffResult
} from "@psychevo/protocol";
import { downloadUrl, type GatewayEndpoint, type PsychevoHost } from "@psychevo/host";
import {
  asRecord,
  commandFeedbackFromResult,
  optionalStringField,
  stringArray,
  stringField
} from "./data";
import {
  formatRuntimeModeValues,
  isRuntimeModeOption,
  normalizeRequestedRuntimeMode,
  projectRuntimeModeOption,
  resolvePeerRuntimeMode,
  runtimeModeCommandValues
} from "./runtime-controls";
import type {
  CommandAlternateAction,
  CommandFeedback,
  CommandOverlay,
  CommandTrigger,
  MainView,
  PendingAttachment,
  RightWorkspaceTab,
  RightWorkspaceTabKind
} from "./types";
import type { PendingDetachedShell } from "./viewGuard";

type RefreshSnapshot = (
  nextClient?: GatewayClient | null,
  threadId?: string,
  scope?: GatewayRequestScope,
  readOnly?: boolean,
  expectedEpoch?: number | null,
  allowDetachedAdoption?: boolean
) => Promise<void>;

type RefreshWorkspaceSurface = (
  nextClient?: GatewayClient | null,
  scope?: GatewayRequestScope,
  threadId?: string | null,
  expectedEpoch?: number | null
) => Promise<void>;

type CommandActionsParams = {
  activeScope: GatewayRequestScope | null;
  activity: ThreadSnapshot["activity"];
  client: GatewayClient | null;
  endpoint: GatewayEndpoint | null;
  host: PsychevoHost | null;
  initScope: GatewayRequestScope | null;
  pendingDetachedShellRef: MutableRefObject<PendingDetachedShell | null>;
  runtimeModeOption: RuntimeOptionsResult["options"][number] | null;
  runtimeOptionsError: string | null;
  runtimeSessionId: string | null;
  selectedRuntimeMode: string;
  selectedRuntimeRef: string;
  settings: SettingsReadResult | undefined;
  snapshot: ThreadSnapshot;
  viewEpochRef: MutableRefObject<number>;
  workMode: string;
  workspaceDiff: WorkspaceDiffResult | null;
  beginExplicitViewSwitch(): number;
  clearCommandTransientUi(): void;
  handleAttachment(): Promise<void>;
  openReviewTab(diff: WorkspaceDiffResult, path?: string | null): void;
  openRightWorkspaceTab(kind: RightWorkspaceTabKind, patch?: Partial<RightWorkspaceTab>, forceNew?: boolean): void;
  patchComposerDraft(text: string): void;
  openCommandOverlay(kind: CommandOverlay): void;
  refreshHistory(nextClient?: GatewayClient | null, includeArchived?: boolean, workdir?: string | null): Promise<SessionSummary[]>;
  refreshRevertedThreadSnapshot(nextClient: GatewayClient | null, threadId: string | null): Promise<void>;
  refreshSnapshot: RefreshSnapshot;
  refreshWorkspaceSurface: RefreshWorkspaceSurface;
  revealHistoryPanel(): void;
  revealRightWorkspace(tabId?: string | null): void;
  setActiveCommandOverlay: Dispatch<SetStateAction<CommandOverlay | null>>;
  setAttachments: Dispatch<SetStateAction<PendingAttachment[]>>;
  setCommandFeedback: Dispatch<SetStateAction<CommandFeedback>>;
  setDraftSession(value: null): void;
  setError: Dispatch<SetStateAction<string | null>>;
  setMobilePanel: Dispatch<SetStateAction<"history" | "transcript" | "status">>;
  setRuntimeOptionsError: Dispatch<SetStateAction<string | null>>;
  setRuntimeOptionsResult: Dispatch<SetStateAction<RuntimeOptionsResult | null>>;
  setRuntimeSessionId: Dispatch<SetStateAction<string | null>>;
  setSelectedRuntimeMode: Dispatch<SetStateAction<string>>;
  setSnapshot: Dispatch<SetStateAction<ThreadSnapshot>>;
  setWorkMode: Dispatch<SetStateAction<string>>;
  setWorkspaceDiff: Dispatch<SetStateAction<WorkspaceDiffResult | null>>;
  startNewThread(workdir?: string): Promise<void>;
  submitThreadTurn(threadId: string, text: string, mentions: GatewayMention[]): Promise<void>;
  submitTurn(text: string, mentions: GatewayMention[], displayText?: string | null): Promise<void>;
  updateMainView(value: MainView): void;
};

export function createCommandActions(params: CommandActionsParams) {
  function commandScope(): GatewayRequestScope {
    return params.activeScope
      ?? params.initScope
      ?? scopeForWorkdir(params.settings?.workdir ?? window.location.pathname);
  }

  function revealCommandsPanel(_trigger: CommandTrigger = "commandsPanel") {
    params.openCommandOverlay("commands");
  }

  function revealHostPanel(panel: string, trigger: CommandTrigger = "commandsPanel") {
    switch (panel) {
      case "history":
      case "sessions":
        params.revealHistoryPanel();
        return;
      case "commands":
      case "help":
        revealCommandsPanel(trigger);
        return;
      case "preview":
        params.openRightWorkspaceTab("review", { diff: params.workspaceDiff, title: "Review" });
        return;
      case "files":
        params.openRightWorkspaceTab("files");
        return;
      case "debug":
        params.openRightWorkspaceTab("debug");
        return;
      case "status":
      default:
        params.revealRightWorkspace(null);
    }
  }

  function routeCommandFeedback(feedback: CommandFeedback, trigger: CommandTrigger) {
    const anchor = feedback?.feedbackAnchor;
    if (trigger === "commandsPanel" || anchor === "commandsPanel") {
      revealCommandsPanel(trigger);
      return;
    }
    if (anchor === "status") {
      params.revealRightWorkspace(null);
    }
  }

  async function runCommandAlternateAction(action: CommandAlternateAction | null | undefined) {
    if (!action) {
      return;
    }
    if (action.type === "openPanel") {
      switch (action.target) {
        case "history":
        case "sessions":
        case "commands":
        case "status":
        case "preview":
          revealHostPanel(action.target);
          return;
        default:
          return;
      }
    }
    if (action.type === "openComposerControl") {
      if (action.target === "attachments") {
        await params.handleAttachment();
        return;
      }
      params.revealRightWorkspace(null);
    }
  }

  async function executeCommand(command: string, trigger: CommandTrigger = "composer") {
    if (await handleLocalRuntimeCommand(command, trigger)) {
      return;
    }
    const result = await params.client?.request("command/execute", {
      command,
      scope: commandScope(),
      threadId: params.snapshot.thread?.id ?? null
    });
    if (!result) {
      return;
    }
    const record = asRecord(result);
    const action = asRecord(record.action);
    const downloadThreadId = action.type === "downloadSession"
      ? optionalStringField(action.threadId) ?? params.snapshot.thread?.id ?? null
      : null;
    const feedback = commandFeedbackFromResult(command, record, trigger, {
      downloadAvailable: action.type !== "downloadSession" || Boolean(params.endpoint && downloadThreadId)
    });
    if (action.type === "passThroughPrompt") {
      await runHostAction(record.action, trigger);
      return;
    }
    if (record.accepted !== true && record.known === false) {
      await params.submitTurn(command, []);
      return;
    }
    if (record.accepted !== true) {
      params.setCommandFeedback(feedback ?? {
        accepted: false,
        command,
        message: `Unsupported command: ${command}`,
        feedbackAnchor: trigger
      });
      routeCommandFeedback(feedback, trigger);
      return;
    }
    params.setCommandFeedback(feedback);
    if (feedback) {
      routeCommandFeedback(feedback, trigger);
    }
    await runHostAction(record.action, trigger);
  }

  async function handleLocalRuntimeCommand(command: string, trigger: CommandTrigger): Promise<boolean> {
    const match = command.trim().match(/^\/mode(?:\s+(.+))?$/);
    if (!match) {
      return false;
    }
    const requested = match[1]?.trim() ?? "";
    if (params.selectedRuntimeRef === "native") {
      if (!requested) {
        const feedback = {
          accepted: true,
          command,
          message: `Current Psychevo mode: ${params.workMode}. Available: default, plan.`,
          feedbackAnchor: trigger
        } satisfies CommandFeedback;
        params.setCommandFeedback(feedback);
        routeCommandFeedback(feedback, trigger);
        return true;
      }
      if (!["default", "plan"].includes(requested)) {
        const feedback = {
          accepted: false,
          command,
          message: `Unknown Psychevo mode: ${requested}. Available: default, plan.`,
          feedbackAnchor: trigger
        } satisfies CommandFeedback;
        params.setCommandFeedback(feedback);
        routeCommandFeedback(feedback, trigger);
        return true;
      }
      params.setWorkMode(requested);
      const feedback = {
        accepted: true,
        command,
        message: `Psychevo mode set to ${requested}.`,
        feedbackAnchor: trigger
      } satisfies CommandFeedback;
      params.setCommandFeedback(feedback);
      routeCommandFeedback(feedback, trigger);
      return true;
    }

    let modeOption = params.runtimeModeOption;
    if (!modeOption && params.client) {
      try {
        const result = await params.client.request("runtime/options", {
          runtimeRef: params.selectedRuntimeRef,
          runtimeSessionId: params.runtimeSessionId,
          scope: commandScope(),
          threadId: params.snapshot.thread?.id ?? null
        });
        params.setRuntimeOptionsResult(result);
        params.setRuntimeSessionId(result.runtimeSessionId ?? null);
        modeOption = result.options.find(isRuntimeModeOption) ?? null;
      } catch (error) {
        const message = error instanceof Error ? error.message : String(error);
        const feedback = {
          accepted: false,
          command,
          message: `Unable to load ${params.selectedRuntimeRef} modes: ${message}`,
          feedbackAnchor: trigger
        } satisfies CommandFeedback;
        params.setCommandFeedback(feedback);
        routeCommandFeedback(feedback, trigger);
        return true;
      }
    }
    if (!modeOption) {
      const feedback = {
        accepted: false,
        command,
        message: `${params.selectedRuntimeRef} does not expose runtime modes.`,
        feedbackAnchor: trigger
      } satisfies CommandFeedback;
      params.setCommandFeedback(feedback);
      routeCommandFeedback(feedback, trigger);
      return true;
    }
    const projected = projectRuntimeModeOption(modeOption);
    const values = runtimeModeCommandValues(projected);
    const currentMode = resolvePeerRuntimeMode(projected, params.workMode, params.selectedRuntimeMode);
    if (!requested) {
      const feedback = {
        accepted: true,
        command,
        message: `Current ${params.selectedRuntimeRef} mode: ${currentMode || "none"}. Available: ${formatRuntimeModeValues(projected)}.`,
        feedbackAnchor: trigger
      } satisfies CommandFeedback;
      params.setCommandFeedback(feedback);
      routeCommandFeedback(feedback, trigger);
      return true;
    }
    const requestedMode = normalizeRequestedRuntimeMode(projected, requested);
    if (!requestedMode || !values.includes(requestedMode)) {
      const feedback = {
        accepted: false,
        command,
        message: `Unknown ${params.selectedRuntimeRef} mode: ${requested}. Available: ${formatRuntimeModeValues(projected)}.`,
        feedbackAnchor: trigger
      } satisfies CommandFeedback;
      params.setCommandFeedback(feedback);
      routeCommandFeedback(feedback, trigger);
      return true;
    }
    if (projected.supportsPlan && (requestedMode === "plan" || requestedMode === projected.defaultValue)) {
      params.setWorkMode(requestedMode === "plan" ? "plan" : "default");
      params.setSelectedRuntimeMode("");
    } else {
      params.setWorkMode("default");
      params.setSelectedRuntimeMode(requestedMode);
    }
    const feedback = {
      accepted: true,
      command,
      message: `${params.selectedRuntimeRef} mode set to ${requestedMode}.`,
      feedbackAnchor: trigger
    } satisfies CommandFeedback;
    params.setCommandFeedback(feedback);
    routeCommandFeedback(feedback, trigger);
    return true;
  }

  async function runHostAction(action: unknown, trigger: CommandTrigger = "commandsPanel") {
    const record = asRecord(action);
    switch (record.type) {
      case "threadStart": {
        await params.startNewThread();
        break;
      }
      case "sideConversationStart": {
        const threadId = optionalStringField(record.threadId);
        if (!threadId) {
          params.setError("Side chat did not include a thread id.");
          break;
        }
        const prompt = optionalStringField(record.prompt)?.trim() ?? null;
        params.openRightWorkspaceTab("sideConversation", {
          parentThreadId: optionalStringField(record.parentThreadId) ?? params.snapshot.thread?.id ?? null,
          pendingPrompt: prompt,
          threadId,
          title: "Side chat"
        }, true);
        break;
      }
      case "threadArchive":
        if (params.snapshot.thread?.id) {
          params.setDraftSession(null);
          await params.client?.request("thread/archive", { threadId: params.snapshot.thread.id });
          await params.refreshHistory();
        }
        break;
      case "threadDelete":
        if (params.snapshot.thread?.id) {
          params.setDraftSession(null);
          await params.client?.request("thread/delete", { threadId: params.snapshot.thread.id });
          await params.refreshHistory();
        }
        break;
      case "turnInterrupt":
        {
          const threadId = params.snapshot.thread?.id ?? null;
          await params.client?.request("turn/interrupt", { threadId });
          await params.refreshSnapshot(params.client, threadId ?? undefined, undefined, true, params.viewEpochRef.current);
        }
        break;
      case "sessionUndo": {
        const threadId = optionalStringField(record.threadId) ?? params.snapshot.thread?.id ?? null;
        await params.refreshRevertedThreadSnapshot(params.client, threadId);
        await params.refreshHistory();
        await params.refreshWorkspaceSurface(params.client, params.activeScope ?? params.initScope ?? undefined, threadId);
        params.setAttachments([]);
        params.patchComposerDraft(stringField(record.prompt));
        params.updateMainView("transcript");
        params.setMobilePanel("transcript");
        break;
      }
      case "sessionRedo": {
        const threadId = optionalStringField(record.threadId) ?? params.snapshot.thread?.id ?? null;
        await params.refreshRevertedThreadSnapshot(params.client, threadId);
        await params.refreshHistory();
        await params.refreshWorkspaceSurface(params.client, params.activeScope ?? params.initScope ?? undefined, threadId);
        params.setAttachments([]);
        params.patchComposerDraft("");
        params.updateMainView("transcript");
        params.setMobilePanel("transcript");
        break;
      }
      case "queuePrompt": {
        const text = stringField(record.text).trim();
        const displayText = optionalStringField(record.displayText);
        if (text) {
          await params.submitTurn(text, [], displayText);
        }
        break;
      }
      case "passThroughPrompt":
      case "submitPrompt": {
        const text = stringField(record.text).trim();
        const displayText = optionalStringField(record.displayText);
        if (text) {
          await params.submitTurn(text, [], displayText);
        }
        break;
      }
      case "steerPrompt": {
        const text = stringField(record.text).trim();
        if (text && params.activity.activeTurnId) {
          params.setSnapshot((current) => appendOptimisticPrompt(current, text));
          await params.client?.request("turn/steer", {
            expectedTurnId: params.activity.activeTurnId,
            threadId: params.snapshot.thread?.id ?? null,
            text
          });
          await params.refreshHistory();
        } else if (text) {
          params.setCommandFeedback({
            accepted: false,
            command: "/steer",
            message: "/steer is only available while a turn is running.",
            feedbackAnchor: "composer"
          });
          params.setMobilePanel("transcript");
        }
        break;
      }
      case "downloadSession":
        {
          const threadId = optionalStringField(record.threadId) ?? params.snapshot.thread?.id ?? null;
          if (params.endpoint && threadId) {
            const kind = stringField(record.kind) === "share" ? "share" : "export";
            void params.host?.open.openDownload(downloadUrl(params.endpoint, threadId, kind, {
              filename: optionalStringField(record.filename),
              format: optionalStringField(record.format),
              include: stringArray(record.include)
            }));
          }
        }
        break;
      case "workspaceDiff": {
        const diff = WorkspaceDiffResultSchema.parse(record.diff);
        params.setActiveCommandOverlay(null);
        params.setWorkspaceDiff(diff);
        params.openReviewTab(diff, diff.selectedPath);
        break;
      }
      case "showPanel":
        revealHostPanel(stringField(record.panel), trigger);
        break;
      default:
        if (record.type) {
          params.setError(`Unsupported host action: ${String(record.type)}`);
        }
    }
  }

  return {
    executeCommand,
    runCommandAlternateAction
  };
}
