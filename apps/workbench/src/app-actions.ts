import type { Dispatch, MutableRefObject, SetStateAction } from "react";
import {
  GatewayClientError,
  parseThreadSnapshot,
  scopeForCwd,
  type GatewayClient,
  type ThreadController,
  type ThreadTurnPreparation
} from "@psychevo/client";
import {
  SettingsReadResultSchema,
  WorkspaceChangeMutationResultSchema,
  WorkspaceCreateResultSchema,
  WorkspaceDiffResultSchema,
  WorkspaceFileWriteResultSchema,
  type ChannelUpdateParams,
  type ChannelSourceListResult,
  type ChannelWechatQrPollResult,
  type ChannelWechatQrStartResult,
  type ContextReadResult,
  type GatewayInputPart,
  type GatewayMention,
  type GatewayRequestScope,
  type ObservabilityReadResult,
  type ThreadControlDescriptorView,
  type ThreadContextReadResult,
  type SettingsReadResult,
  type ThreadSnapshot,
  type WorkspaceChangesResult,
  type WorkspaceDiffResult,
  type WorkspaceFolderListResult,
  type WorkspaceFileWriteResult,
  type WorkspaceGitBranchesResult
} from "@psychevo/protocol";
import type { PsychevoHost } from "@psychevo/host";
import { attachmentFromFile } from "./attachments";
import {
  asRecord,
  optionalStringField,
  parseBackendDoctor
} from "./data";
import { backendDraftFromBackend, parseBackendCommandJson } from "./capabilities-agents-config";
import { transcriptSearchText } from "./search-model";
import {
  multilineList,
  normalizeSnapshot
} from "./session-utils";
import { readProjectedThreadHistory } from "./thread-application";
import type {
  BackendDraft,
  CommandFeedback,
  PendingAttachment,
  RightWorkspaceTab,
  RightWorkspaceTabKind,
  TraceState,
  WorkbenchBackend,
  WorkbenchBackendDoctor,
  WorkbenchChannel,
  WorkbenchChannelDoctor,
  WorkbenchChannelSource,
  MainView
} from "./types";
import {
  createHistoryDraftSession,
  shouldAdoptDetachedShellResult,
  type PendingDetachedShell
} from "./viewGuard";
import {
  fileBasename
} from "./right-workspace-model";
import { parseThreadContext, runtimeControlSelections } from "./runtime-context";
import type { ComposerSessionCoordinator } from "./composer-session-coordinator";

type ChannelUpdateDraft = Partial<Omit<ChannelUpdateParams, "id" | "scope">>;

export type PendingUnknownTurnStart = {
  epoch: number;
  prepared: ThreadTurnPreparation;
  isInputCurrent: () => boolean;
  clearInput(): void;
};

type StartNewThreadOptions = {
  rejectProblem?: boolean;
  refreshHistory?: boolean;
  targetId?: string;
};

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

type AppActionsParams = {
  activeScope: GatewayRequestScope | null;
  attachments: PendingAttachment[];
  client: GatewayClient | null;
  composerSessionCoordinator: ComposerSessionCoordinator;
  currentThreadId: string | null;
  detachedShellTokenRef: MutableRefObject<number>;
  host: PsychevoHost | null;
  initScope: GatewayRequestScope | null;
  isThreadArchived(threadId: string): boolean;
  fallbackCwd: string;
  turnBlockReason: string;
  pendingDetachedShellRef: MutableRefObject<PendingDetachedShell | null>;
  pendingUnknownTurnStartRef: MutableRefObject<PendingUnknownTurnStart | null>;
  gatewayRecoveryRef: MutableRefObject<() => void>;
  firstTurnContextRefreshPendingRef: MutableRefObject<boolean>;
  runtimeControls: ThreadControlDescriptorView[];
  runtimeControlDrafts: Record<string, unknown>;
  selectedTargetId: string;
  selectedThreadIdRef: MutableRefObject<string | null>;
  settings: SettingsReadResult | undefined;
  snapshot: ThreadSnapshot;
  threadController: ThreadController;
  viewEpochRef: MutableRefObject<number>;
  adoptSnapshotScope(nextClient: GatewayClient, nextSnapshot: ThreadSnapshot): Promise<void>;
  beginExplicitViewSwitch(): number;
  clearCommandTransientUi(): void;
  openReviewTab(diff: WorkspaceDiffResult, path?: string | null): void;
  openRightWorkspaceTab(kind: RightWorkspaceTabKind, patch?: Partial<RightWorkspaceTab>, forceNew?: boolean): void;
  refreshAgentSurface(nextClient?: GatewayClient | null, scope?: GatewayRequestScope): Promise<void>;
  refreshHistory(nextClient?: GatewayClient | null, includeArchived?: boolean, cwd?: string | null): Promise<unknown>;
  refreshRuntimeContext(): void;
  refreshSnapshot: RefreshSnapshot;
  refreshWorkspaceSurface: RefreshWorkspaceSurface;
  setAttachments: Dispatch<SetStateAction<PendingAttachment[]>>;
  setBackendDoctor: Dispatch<SetStateAction<Record<string, WorkbenchBackendDoctor>>>;
  setBackendDraft: Dispatch<SetStateAction<BackendDraft | null>>;
  setChannelDoctor: Dispatch<SetStateAction<Record<string, WorkbenchChannelDoctor>>>;
  setCommandFeedback: Dispatch<SetStateAction<CommandFeedback>>;
  setContextUsage: Dispatch<SetStateAction<ContextReadResult | null>>;
  setDraftSession: Dispatch<SetStateAction<ReturnType<typeof createHistoryDraftSession> | null>>;
  setError: Dispatch<SetStateAction<string | null>>;
  setMobilePanel: Dispatch<SetStateAction<"history" | "transcript" | "status">>;
  setObservability: Dispatch<SetStateAction<ObservabilityReadResult | null>>;
  setRightTabs: Dispatch<SetStateAction<RightWorkspaceTab[]>>;
  setRuntimeOptionsError: Dispatch<SetStateAction<string | null>>;
  setRuntimeOptionsLoading: Dispatch<SetStateAction<boolean>>;
  setWorkspaceBranch: Dispatch<SetStateAction<string | null | undefined>>;
  setRuntimeContext: Dispatch<SetStateAction<ThreadContextReadResult | null>>;
  setRuntimeContextTargetId: Dispatch<SetStateAction<string>>;
  setSelectedTargetId: Dispatch<SetStateAction<string>>;
  setSnapshot: Dispatch<SetStateAction<ThreadSnapshot>>;
  setSettings: Dispatch<SetStateAction<SettingsReadResult | undefined>>;
  setTraceState: Dispatch<SetStateAction<TraceState>>;
  setWorkspaceChanges: Dispatch<SetStateAction<WorkspaceChangesResult | null>>;
  setWorkspaceDiff: Dispatch<SetStateAction<WorkspaceDiffResult | null>>;
  patchComposerDraft(text: string): void;
  updateMainView(value: MainView): void;
};

function upsertChannel(channels: WorkbenchChannel[], channel: WorkbenchChannel): WorkbenchChannel[] {
  const index = channels.findIndex((item) => item.id === channel.id);
  if (index === -1) {
    return [...channels, channel];
  }
  return channels.map((item) => (item.id === channel.id ? channel : item));
}

export function createAppActions(params: AppActionsParams) {
  function scope(): GatewayRequestScope {
    return params.activeScope
      ?? params.initScope
      ?? scopeForCwd(params.settings?.cwd || params.fallbackCwd);
  }

  function clearSessionObservability() {
    params.setObservability(null);
    params.setContextUsage(null);
    params.setTraceState({ error: null, loading: false, result: null, threadId: null });
  }

  async function startNewThread(cwd?: string, options: StartNewThreadOptions = {}) {
    if (!params.client) {
      return;
    }
    const epoch = params.beginExplicitViewSwitch();
    const previousScope = scope();
    const nextScope = cwd == null || cwd === previousScope.cwd
      ? previousScope
      : scopeForCwd(cwd);
    const inheritedTargetId = options.targetId
      ?? (cwd == null || cwd === previousScope.cwd ? params.selectedTargetId : "");
    clearSessionObservability();
    params.updateMainView("transcript");
    params.setMobilePanel("transcript");
    const optimisticDraft = normalizeSnapshot({
      source: params.snapshot.source,
      scope: nextScope,
      thread: null,
      history: params.snapshot.history,
      entries: [],
      activity: { running: false, activeTurnId: null, queuedTurns: 0 },
      pendingActions: []
    });
    params.selectedThreadIdRef.current = null;
    params.setSnapshot(optimisticDraft);
    params.setDraftSession(createHistoryDraftSession(epoch, nextScope.cwd));
    params.setRuntimeOptionsLoading(true);
    params.setRuntimeOptionsError(null);
    const openToken = params.composerSessionCoordinator.beginDraftOpen(epoch);
    const draftOpenRequest = params.client.request("thread/draft/open", {
      origin: nextScope,
      targetIntent: inheritedTargetId
        ? { kind: "exact", targetId: inheritedTargetId }
        : { kind: "default" }
    });
    const branchRequest = params.client.request("workspace/git/branches", {
      scope: nextScope
    }).then((result) => result.current?.trim() || null).catch(() => null);
    let opened;
    let workspaceBranch: string | null;
    try {
      [opened, workspaceBranch] = await Promise.all([draftOpenRequest, branchRequest]);
    } catch (error) {
      params.composerSessionCoordinator.failDraftOpen(openToken);
      if (params.viewEpochRef.current === epoch) {
        params.setRuntimeOptionsLoading(false);
      }
      throw error;
    }
    const nextSnapshot = parseThreadSnapshot(opened.snapshot);
    const nextContext = parseThreadContext(opened.context);
    const normalized = normalizeSnapshot(nextSnapshot);
    if (params.viewEpochRef.current === epoch) {
      await params.adoptSnapshotScope(params.client, nextSnapshot);
    }
    if (params.viewEpochRef.current === epoch) {
      params.selectedThreadIdRef.current = normalized.thread?.id ?? null;
      params.setSnapshot(normalized);
      params.setDraftSession(createHistoryDraftSession(epoch, nextScope.cwd));
      params.threadController.setContext(nextContext);
      params.setRuntimeContext(nextContext);
      params.setWorkspaceBranch(workspaceBranch);
      params.setRuntimeContextTargetId(nextContext.selectedTargetId ?? "");
      params.setSelectedTargetId(
        nextContext.selectedTargetId
        ?? nextContext.suggestedTargetId
        ?? ""
      );
      params.setRuntimeOptionsError(opened.problem?.message ?? null);
      if (opened.problem) {
        params.composerSessionCoordinator.failDraftOpen(openToken);
      } else {
        params.composerSessionCoordinator.completeDraftOpen(openToken);
      }
    }
    if (params.viewEpochRef.current === epoch) {
      params.setRuntimeOptionsLoading(false);
    }
    if (opened.problem && options.rejectProblem) {
      throw new Error(opened.problem.message);
    }
    if (options.refreshHistory === true) {
      await params.refreshHistory(params.client);
    }
    return normalized;
  }

  async function createWorkspace(name: string, parent: string | null = null) {
    if (!params.client) {
      return;
    }
    const created = WorkspaceCreateResultSchema.parse(await params.client.request("workspace/create", {
      name,
      parent
    }));
    await startNewThread(created.cwd);
    params.updateMainView("transcript");
    params.setMobilePanel("transcript");
  }

  async function readWorkspaceFolders(path: string | null = null): Promise<WorkspaceFolderListResult> {
    if (!params.client) {
      throw new Error("Gateway client is unavailable.");
    }
    return params.client.request("workspace/folders", { scope: scope(), path });
  }

  async function readWorkspaceGitBranches(): Promise<WorkspaceGitBranchesResult> {
    if (!params.client) {
      throw new Error("Gateway client is unavailable.");
    }
    const epoch = params.viewEpochRef.current;
    const result = await params.client.request("workspace/git/branches", { scope: scope() });
    if (params.viewEpochRef.current === epoch) {
      params.setWorkspaceBranch(result.current?.trim() || null);
    }
    return result;
  }

  async function checkoutWorkspaceGitBranch(
    branch: string,
    create: boolean
  ): Promise<WorkspaceGitBranchesResult> {
    if (!params.client) {
      throw new Error("Gateway client is unavailable.");
    }
    const epoch = params.viewEpochRef.current;
    const nextScope = scope();
    const result = await params.client.request("workspace/git/checkout", {
      scope: nextScope,
      branch,
      create
    });
    if (params.viewEpochRef.current !== epoch) {
      return result;
    }
    params.setWorkspaceBranch(result.current?.trim() || null);
    params.setSettings((current) => current
      ? {
          ...current,
          project: current.project
            ? { ...current.project, branch: result.current }
            : current.project
        }
      : current);
    try {
      const nextSettings = SettingsReadResultSchema.parse(await params.client.request("settings/read", {
        cwd: nextScope.cwd,
        threadId: params.currentThreadId
      }));
      if (params.viewEpochRef.current !== epoch) {
        return result;
      }
      params.setSettings(nextSettings);
      await params.refreshWorkspaceSurface(
        params.client,
        nextScope,
        params.currentThreadId,
        epoch
      );
    } catch (error) {
      if (params.viewEpochRef.current === epoch) {
        params.setError(error instanceof Error ? error.message : String(error));
      }
    }
    return result;
  }

  async function submitTurn(
    text: string,
    mentions: GatewayMention[],
    displayText?: string | null,
    inputOverride?: GatewayInputPart[],
    isInputCurrent: () => boolean = () => true
  ): Promise<boolean> {
    if (!params.client) return false;
    const submittedAtMs = Date.now();
    const nextInput: GatewayInputPart[] = inputOverride ?? [
      ...(text.trim() ? [{ type: "text" as const, text }] : []),
      ...params.attachments.map((attachment) => attachment.input)
    ];
    if (nextInput.length === 0) return false;
    const turnEpoch = params.viewEpochRef.current;
    if (params.composerSessionCoordinator.isReadinessPending(turnEpoch)) {
      const ready = await params.composerSessionCoordinator.waitToSubmit(turnEpoch, isInputCurrent);
      if (!ready) return false;
    }
    if (!isInputCurrent()) return false;
    const optimisticText = displayText?.trim()
      || text.trim()
      || params.attachments.map((attachment) => `[Attachment: ${attachment.name}]`).join(" ");
    const liveSnapshot = params.threadController.snapshot() ?? params.snapshot;
    const liveContext = params.threadController.context();
    const turnOverrides = runtimeControlSelections(
      liveContext?.controls ?? params.runtimeControls,
      params.runtimeControlDrafts
    );
    const selectedThreadId = liveSnapshot.thread?.id ?? null;
    let turnTargetId = liveContext?.selectedTargetId ?? params.selectedTargetId;
    if (selectedThreadId && params.isThreadArchived(selectedThreadId)) {
      await params.client.request("thread/restore", { threadId: selectedThreadId });
      await Promise.all([
        params.refreshHistory(params.client),
        params.refreshHistory(params.client, true)
      ]);
      const restoredContext = parseThreadContext(await params.client.request("thread/context/read", {
        threadId: selectedThreadId,
        target: null,
        scope: scope()
      }));
      params.threadController.setContext(restoredContext);
      turnTargetId = restoredContext.selectedTargetId ?? "";
      params.refreshRuntimeContext();
    }
    const turnControls = params.threadController.turnControls(
      turnTargetId,
      turnOverrides
    );
    const admission = params.threadController.admitTurn({ controls: turnControls, input: nextInput, mentions });
    if (!admission.allowed) {
      params.setCommandFeedback({
        accepted: false,
        command: "turn/start",
        message: admission.reason ?? params.turnBlockReason,
        feedbackAnchor: "composer"
      });
      return false;
    }
    params.pendingDetachedShellRef.current = null;
    params.clearCommandTransientUi();
    params.firstTurnContextRefreshPendingRef.current = selectedThreadId === null;
    const plan = params.threadController.beginTurn({
      controls: turnControls,
      input: nextInput,
      mentions,
      optimisticText,
      scope: liveSnapshot.scope,
      startedAtMs: submittedAtMs,
      threadId: liveSnapshot.thread?.id ?? null
    });
    const result = await params.client.request("turn/start", plan.params).catch((error) => {
      if (error instanceof GatewayClientError && error.delivery === "unknown") {
        params.pendingUnknownTurnStartRef.current = {
          epoch: turnEpoch,
          prepared: plan.prepared,
          isInputCurrent,
          clearInput: () => {
            if (!isInputCurrent()) return;
            params.patchComposerDraft("");
            params.setAttachments([]);
          }
        };
        params.setCommandFeedback({
          accepted: false,
          command: "turn/start",
          message: "Send status is unknown. Verifying the Thread; the draft is preserved.",
          feedbackAnchor: "composer"
        });
        params.gatewayRecoveryRef.current();
        return null;
      }
      if (params.viewEpochRef.current === turnEpoch) {
        params.threadController.rejectTurnStart(plan.prepared);
        params.firstTurnContextRefreshPendingRef.current = false;
        params.refreshRuntimeContext();
      }
      throw error;
    });
    if (!result) {
      return false;
    }
    if (params.viewEpochRef.current === turnEpoch) {
      const accepted = params.threadController.acceptTurnStart(result, plan.prepared);
      params.selectedThreadIdRef.current = accepted.threadId;
    }
    params.setAttachments([]);
    if (selectedThreadId === null) {
      await params.refreshHistory();
    }
    return true;
  }

  async function startShell(command: string) {
    params.clearCommandTransientUi();
    const pendingShell = params.snapshot.thread?.id
      ? null
      : {
          epoch: params.viewEpochRef.current,
          token: params.detachedShellTokenRef.current + 1
        };
    if (pendingShell) {
      params.detachedShellTokenRef.current = pendingShell.token;
      params.pendingDetachedShellRef.current = pendingShell;
    }
    const result = await params.client?.request("shell/start", {
      command,
      scope: scope(),
      threadId: params.snapshot.thread?.id ?? null
    });
    const record = asRecord(result);
    if (record.accepted !== true) {
      if (params.pendingDetachedShellRef.current?.token === pendingShell?.token) {
        params.pendingDetachedShellRef.current = null;
      }
      params.setCommandFeedback({
        accepted: false,
        command: `!${command}`,
        message: optionalStringField(record.message) ?? "Shell command was not accepted.",
        feedbackAnchor: "composer"
      });
      params.setMobilePanel("transcript");
      return;
    }
    const threadId = optionalStringField(record.threadId);
    if (threadId) {
      const adoptDetached = shouldAdoptDetachedShellResult(
        params.snapshot,
        threadId,
        params.viewEpochRef.current,
        params.pendingDetachedShellRef.current
      );
      if (adoptDetached || params.snapshot.thread?.id) {
        if (params.pendingDetachedShellRef.current?.token === pendingShell?.token) {
          params.pendingDetachedShellRef.current = null;
        }
        await params.refreshSnapshot(params.client, threadId, undefined, true, params.viewEpochRef.current, adoptDetached);
      }
    }
    await params.refreshHistory();
  }

  async function openFilePreview(path: string, options: { hideFileTree?: boolean } = {}) {
    const layoutPatch: Partial<RightWorkspaceTab> = options.hideFileTree
      ? { fileTreeOpen: false }
      : {};
    params.openRightWorkspaceTab("files", {
      ...layoutPatch,
      path,
      title: fileBasename(path)
    });
  }

  async function saveFileFromEditor(
    path: string,
    content: string,
    expectedRevision: string | null,
    force: boolean
  ): Promise<WorkspaceFileWriteResult> {
    const nextScope = scope();
    const result = WorkspaceFileWriteResultSchema.parse(await params.client?.request("workspace/file/write", {
      scope: nextScope,
      path,
      content,
      expectedRevision,
      force
    }));
    params.setRightTabs((current) => current.map((tab) => (
      tab.kind === "files" && tab.path === result.path
        ? { ...tab, message: null, title: fileBasename(result.path) }
        : tab
    )));
    await params.refreshWorkspaceSurface(params.client, nextScope, params.currentThreadId ?? null);
    return result;
  }

  async function acceptWorkspaceChange(turnId: string, path: string) {
    const result = WorkspaceChangeMutationResultSchema.parse(await params.client?.request("workspace/change/accept", {
      scope: scope(),
      turnId,
      path
    }));
    params.setWorkspaceChanges(result.changes);
  }

  async function rejectWorkspaceChange(turnId: string, path: string) {
    const nextScope = scope();
    const result = WorkspaceChangeMutationResultSchema.parse(await params.client?.request("workspace/change/reject", {
      scope: nextScope,
      turnId,
      path
    }));
    params.setWorkspaceChanges(result.changes);
    await params.refreshWorkspaceSurface(params.client, nextScope, params.currentThreadId ?? null);
  }

  async function openDiffPreview(path?: string | null) {
    const result = WorkspaceDiffResultSchema.parse(await params.client?.request("workspace/diff", { scope: scope(), path: path ?? null }));
    params.setWorkspaceDiff((current) => path ? current : result);
    params.openReviewTab(result, path ?? null);
  }

  async function loadThreadSearchText(threadId: string): Promise<string> {
    if (!params.client) {
      return "";
    }
    const history = await readProjectedThreadHistory(params.client, scope(), threadId);
    return transcriptSearchText(history.entries);
  }

  async function copyText(text: string) {
    const result = await params.host?.clipboard.writeText(text);
    if (!result || !result.ok) {
      const message = "Clipboard copy is not supported by this host.";
      params.setError(message);
      throw new Error(message);
    }
    params.setError(null);
  }

  async function handleAttachment() {
    const result = await params.host?.files.pickFile();
    if (!result || !result.ok) {
      params.setError("Attachments are not supported by this host yet.");
      return;
    }
    await handleAttachmentFiles([result.value]);
  }

  async function handleAttachmentFiles(files: File[]) {
    if (files.length === 0) {
      return;
    }
    const attachments = await Promise.all(files.map((file) => attachmentFromFile(file)));
    const admission = params.threadController.admitInput(attachments.map((attachment) => attachment.input));
    if (!admission.allowed) {
      params.setError(admission.reason ?? "The selected Agent target does not accept this attachment.");
      return;
    }
    params.setAttachments((current) => [...current, ...attachments]);
    params.setError(null);
  }

  async function writeBackendDraft(draft: BackendDraft) {
    if (!params.client) {
      throw new Error("Gateway client is unavailable.");
    }
    const commandConfig = parseBackendCommandJson(draft.commandJsonText);
    if (commandConfig.error) {
      throw new Error(commandConfig.error);
    }
    const nextScope = scope();
    await params.client.request("backend/write", {
      scope: nextScope,
      id: draft.id.trim(),
      target: "profile",
      enabled: draft.enabled,
      label: draft.label.trim() || null,
      description: draft.description.trim() || null,
      command: commandConfig.command.trim() || null,
      args: commandConfig.args,
      env: commandConfig.env,
      cwd: draft.cwd.trim() || "invocation",
      entrypoints: draft.entrypoints,
      clientCapabilities: draft.clientCapabilities,
      mcpServers: multilineList(draft.mcpServersText)
    });
    await params.refreshAgentSurface(params.client, nextScope);
  }

  async function saveBackendDraft(draft: BackendDraft) {
    await writeBackendDraft(draft);
    params.setBackendDraft(null);
  }

  async function updateBackendDraftFields(backend: WorkbenchBackend, patch: Partial<BackendDraft>) {
    await writeBackendDraft({ ...backendDraftFromBackend(backend), ...patch });
  }

  async function deleteBackend(backend: WorkbenchBackend) {
    if (!params.client) {
      throw new Error("Gateway client is unavailable.");
    }
    const nextScope = scope();
    await params.client.request("backend/delete", { scope: nextScope, id: backend.id, target: "profile" });
    await params.refreshAgentSurface(params.client, nextScope);
  }

  async function doctorBackend(backend: WorkbenchBackend) {
    if (!params.client) {
      throw new Error("Gateway client is unavailable.");
    }
    const result = await params.client.request("backend/doctor", { scope: scope(), id: backend.id });
    params.setBackendDoctor((current) => ({
      ...current,
      [backend.id]: parseBackendDoctor(result)
    }));
  }

  async function setChannelEnabled(channel: WorkbenchChannel, enabled: boolean) {
    if (!params.client) {
      throw new Error("Gateway client is unavailable.");
    }
    const result = await params.client.request("channel/enable", {
      scope: scope(),
      id: channel.id,
      enabled
    });
    params.setSettings((current) => current
      ? {
          ...current,
          channels: {
            channels: current.channels.channels.map((item) => (
              item.id === result.channel.id ? result.channel : item
            ))
          }
        }
      : current);
  }

  async function updateChannel(channel: WorkbenchChannel, draft: ChannelUpdateDraft): Promise<WorkbenchChannel> {
    if (!params.client) {
      throw new Error("Gateway client is unavailable.");
    }
    const result = await params.client.request("channel/update", {
      scope: scope(),
      id: channel.id,
      ...draft
    });
    const nextChannel = result.channel as WorkbenchChannel;
    params.setSettings((current) => current
      ? {
          ...current,
          channels: {
            channels: upsertChannel(current.channels.channels, nextChannel)
          }
        }
      : current);
    return nextChannel;
  }

  async function loadChannelSources(channel: WorkbenchChannel): Promise<WorkbenchChannelSource[]> {
    if (!params.client) {
      throw new Error("Gateway client is unavailable.");
    }
    const result = await params.client.request("channel/source/list", {
      scope: scope(),
      id: channel.id
    }) as ChannelSourceListResult;
    return result.sources as WorkbenchChannelSource[];
  }

  async function deleteChannel(channel: WorkbenchChannel) {
    if (!params.client) {
      throw new Error("Gateway client is unavailable.");
    }
    const result = await params.client.request("channel/delete", {
      scope: scope(),
      id: channel.id
    });
    params.setSettings((current) => current
      ? {
          ...current,
          channels: {
            channels: result.channels
          }
        }
      : current);
    params.setChannelDoctor((current) => {
      const next = { ...current };
      delete next[channel.id];
      return next;
    });
  }

  async function doctorChannel(channel: WorkbenchChannel) {
    if (!params.client) {
      throw new Error("Gateway client is unavailable.");
    }
    const result = await params.client.request("channel/doctor", {
      scope: scope(),
      id: channel.id,
      live: false
    });
    const checked = result.channels.find((item) => item.id === channel.id);
    if (checked) {
      params.setChannelDoctor((current) => ({
        ...current,
        [channel.id]: checked
      }));
    }
  }

  async function doctorChannels() {
    if (!params.client) {
      throw new Error("Gateway client is unavailable.");
    }
    const result = await params.client.request("channel/doctor", {
      scope: scope(),
      id: null,
      live: false
    });
    params.setChannelDoctor((current) => {
      const next = { ...current };
      for (const channel of result.channels) {
        next[channel.id] = channel;
      }
      return next;
    });
  }

  async function startWechatQrSetup(): Promise<ChannelWechatQrStartResult> {
    if (!params.client) {
      throw new Error("Gateway client is unavailable.");
    }
    return await params.client.request("channel/wechat-qr/start", {
      scope: scope(),
      id: "wechat",
      label: "WeChat"
    });
  }

  async function pollWechatQrSetup(sessionId: string): Promise<ChannelWechatQrPollResult> {
    if (!params.client) {
      throw new Error("Gateway client is unavailable.");
    }
    const result = await params.client.request("channel/wechat-qr/poll", {
      scope: scope(),
      sessionId,
      enable: true
    });
    const channel = result.channel;
    if (channel) {
      params.setSettings((current) => current
        ? {
            ...current,
            channels: {
              channels: upsertChannel(current.channels.channels, channel)
            }
          }
        : current);
    }
    return result;
  }

  return {
    acceptWorkspaceChange,
    copyText,
    checkoutWorkspaceGitBranch,
    createWorkspace,
    deleteBackend,
    deleteChannel,
    doctorChannel,
    doctorBackend,
    doctorChannels,
    handleAttachment,
    handleAttachmentFiles,
    loadThreadSearchText,
    openDiffPreview,
    openFilePreview,
    rejectWorkspaceChange,
    readWorkspaceFolders,
    readWorkspaceGitBranches,
    saveBackendDraft,
    saveFileFromEditor,
    pollWechatQrSetup,
    startWechatQrSetup,
    startNewThread,
    startShell,
    submitTurn,
    loadChannelSources,
    updateChannel,
    updateBackendDraftFields,
    setChannelEnabled
  };
}
