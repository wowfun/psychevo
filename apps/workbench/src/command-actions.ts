import type { Dispatch, MutableRefObject, SetStateAction } from "react";
import {
  appendOptimisticPrompt,
  scopeForCwd,
  type GatewayClient
} from "@psychevo/client";
import {
  WorkspaceDiffResultSchema,
  type GatewayMention,
  type GatewayRequestScope,
  type SessionSummary,
  type SettingsReadResult,
  type ThreadContextReadResult,
  type ThreadSnapshot,
  type WorkspaceDiffResult
} from "@psychevo/protocol";
import type { GatewayEndpoint, PsychevoHost } from "@psychevo/host";
import {
  asRecord,
  commandFeedbackFromResult,
  optionalStringField,
  stringArray,
  stringField
} from "./data";
import {
  formatRuntimeModeValues,
  normalizeRequestedRuntimeMode,
  projectRuntimeModeOption,
  runtimeModeCommandValues,
  type RuntimeModeOption
} from "./runtime-controls";
import type {
  CommandAlternateAction,
  CommandFeedback,
  CommandOverlay,
  CommandTrigger,
  CapabilityTab,
  MainView,
  PendingAttachment,
  RightWorkspaceTab,
  RightWorkspaceTabKind
} from "./types";
import type { PendingDetachedShell } from "./viewGuard";
import {
  enabledThreadAction,
  runThreadInterrupt,
  snapshotThreadApplicationTarget,
  threadActionDescriptor
} from "./thread-application";

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
  fallbackCwd: string;
  host: PsychevoHost | null;
  initScope: GatewayRequestScope | null;
  pendingDetachedShellRef: MutableRefObject<PendingDetachedShell | null>;
  runtimeModeOption: RuntimeModeOption | null;
  runtimeContext: ThreadContextReadResult | null;
  settings: SettingsReadResult | undefined;
  snapshot: ThreadSnapshot;
  viewEpochRef: MutableRefObject<number>;
  workspaceDiff: WorkspaceDiffResult | null;
  beginExplicitViewSwitch(): number;
  clearCommandTransientUi(): void;
  changeRuntimeMode(value: string): Promise<void>;
  handleAttachment(): Promise<void>;
  openReviewTab(diff: WorkspaceDiffResult, path?: string | null): void;
  openRightWorkspaceTab(kind: RightWorkspaceTabKind, patch?: Partial<RightWorkspaceTab>, forceNew?: boolean): void;
  patchComposerDraft(text: string): void;
  openCommandOverlay(kind: CommandOverlay): void;
  openCapabilitiesTab(tab?: CapabilityTab): void;
  refreshHistory(nextClient?: GatewayClient | null, includeArchived?: boolean, cwd?: string | null): Promise<SessionSummary[]>;
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
  setSnapshot: Dispatch<SetStateAction<ThreadSnapshot>>;
  setWorkspaceDiff: Dispatch<SetStateAction<WorkspaceDiffResult | null>>;
  startNewThread(cwd?: string): Promise<void>;
  submitThreadTurn(threadId: string, text: string, mentions: GatewayMention[], displayText?: string | null): Promise<void>;
  submitTurn(text: string, mentions: GatewayMention[], displayText?: string | null): Promise<unknown>;
  updateMainView(value: MainView): void;
};

export function createCommandActions(params: CommandActionsParams) {
  function commandScope(): GatewayRequestScope {
    return params.activeScope
      ?? params.initScope
      ?? scopeForCwd(params.settings?.cwd || params.fallbackCwd);
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
      case "agents":
        params.openCapabilitiesTab("agents");
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
        case "agents":
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
    const modeOption = params.runtimeModeOption;
    if (!modeOption) {
      const feedback = {
        accepted: false,
        command,
        message: "The active Thread does not expose a mode control.",
        feedbackAnchor: trigger
      } satisfies CommandFeedback;
      params.setCommandFeedback(feedback);
      routeCommandFeedback(feedback, trigger);
      return true;
    }
    const projected = projectRuntimeModeOption(modeOption);
    const values = runtimeModeCommandValues(projected);
    const currentMode = modeOption.currentValue || projected.defaultValue;
    if (!requested) {
      const feedback = {
        accepted: true,
        command,
        message: `Current mode: ${currentMode || "none"}. Available: ${formatRuntimeModeValues(projected)}.`,
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
        message: `Unknown mode: ${requested}. Available: ${formatRuntimeModeValues(projected)}.`,
        feedbackAnchor: trigger
      } satisfies CommandFeedback;
      params.setCommandFeedback(feedback);
      routeCommandFeedback(feedback, trigger);
      return true;
    }
    await params.changeRuntimeMode(requestedMode);
    const feedback = {
      accepted: true,
      command,
      message: `Mode set to ${requestedMode}.`,
      feedbackAnchor: trigger
    } satisfies CommandFeedback;
    params.setCommandFeedback(feedback);
    routeCommandFeedback(feedback, trigger);
    return true;
  }

  async function runHostAction(action: unknown, trigger: CommandTrigger = "commandsPanel") {
    const record = asRecord(action);
    switch (record.type) {
      case "newSession": {
        await params.startNewThread();
        break;
      }
      case "threadCompactStart": {
        const target = snapshotThreadApplicationTarget(params.snapshot);
        const descriptor = threadActionDescriptor(params.runtimeContext, "compact");
        if (!params.client || !target || !descriptor?.enabled) {
          const feedback = {
            accepted: false,
            command: "/compact",
            message: descriptor?.unavailableReason ?? "Context compaction is not available for the active Thread.",
            feedbackAnchor: "composer"
          } satisfies NonNullable<CommandFeedback>;
          params.setCommandFeedback(feedback);
          params.setMobilePanel("transcript");
          break;
        }
        const actionResult = await params.client.request("thread/action/run", {
          ...target,
          action: {
            kind: "compact",
            instructions: optionalStringField(record.instructions)
          }
        });
        const compact = actionResult.kind === "compact" ? asRecord(actionResult.result) : {};
        const compactThreadId = actionResult.kind === "compact" ? actionResult.threadId : null;
        const feedback = {
          accepted: compact.accepted === true && compact.error == null,
          command: "/compact",
          message: optionalStringField(compact.message) ?? "Context compaction did not return a result.",
          feedbackAnchor: "composer"
        } satisfies NonNullable<CommandFeedback>;
        params.setCommandFeedback(feedback);
        if (compactThreadId) {
          await params.refreshSnapshot(params.client, compactThreadId, undefined, true, params.viewEpochRef.current);
        }
        await params.refreshHistory();
        params.setMobilePanel("transcript");
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
          const target = snapshotThreadApplicationTarget(params.snapshot);
          if (!params.client || !target) {
            params.setCommandFeedback({
              accepted: false,
              command: "interrupt",
              message: "Interrupt is not available for the active Thread.",
              feedbackAnchor: "composer"
            });
            break;
          }
          await runThreadInterrupt(params.client, target);
          await params.refreshSnapshot(params.client, target.threadId, undefined, true, params.viewEpochRef.current);
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
        const threadId = optionalStringField(record.threadId);
        if (text) {
          if (threadId) {
            await params.submitThreadTurn(threadId, text, [], displayText);
          } else {
            await params.submitTurn(text, [], displayText);
          }
        }
        break;
      }
      case "passThroughPrompt":
      case "submitPrompt": {
        const text = stringField(record.text).trim();
        const displayText = optionalStringField(record.displayText);
        const threadId = optionalStringField(record.threadId);
        if (text) {
          if (threadId) {
            await params.submitThreadTurn(threadId, text, [], displayText);
          } else {
            await params.submitTurn(text, [], displayText);
          }
        }
        break;
      }
      case "steerPrompt": {
        const text = stringField(record.text).trim();
        const target = snapshotThreadApplicationTarget(params.snapshot);
        const steer = threadActionDescriptor(params.runtimeContext, "steer");
        if (text && params.activity.activeTurnId && params.client && target && steer?.enabled) {
          const result = await params.client.request("thread/action/run", {
            ...target,
            action: { kind: "steer", expectedTurnId: params.activity.activeTurnId, text }
          });
          if (result.kind === "steer" && result.accepted) {
            params.setSnapshot((current) => appendOptimisticPrompt(current, text));
            await params.refreshHistory();
          } else {
            params.setCommandFeedback({
              accepted: false,
              command: "/steer",
              message: "The selected Runtime Profile does not support steering this turn.",
              feedbackAnchor: "composer"
            });
            params.setMobilePanel("transcript");
          }
        } else if (text) {
          params.setCommandFeedback({
            accepted: false,
            command: "/steer",
            message: !params.activity.activeTurnId
              ? "/steer is only available while a turn is running."
              : steer?.unavailableReason ?? "The selected Runtime Profile does not support steering this turn.",
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
            void params.host?.open.downloadSession(params.endpoint, threadId, kind, {
              filename: optionalStringField(record.filename),
              format: optionalStringField(record.format),
              include: stringArray(record.include)
            });
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
