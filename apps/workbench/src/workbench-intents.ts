import type { Dispatch, MutableRefObject, SetStateAction } from "react";
import {
  appendOptimisticPrompt,
  scopeForCwd,
  type GatewayClient
} from "@psychevo/client";
import type {
  CompletionListResult,
  GatewayRequestScope,
  PendingActionView,
  PermissionDecision,
  SessionSummary,
  SettingsReadResult,
  ThreadEditableDraft,
  ThreadEditableInputPart,
  ThreadImportProfileView,
  ThreadSnapshot,
  TranscriptEntry
} from "@psychevo/protocol";
import type { HistoryDraftSession } from "@psychevo/components";
import type { ReturnTypeOfAppActions } from "./app-actions";
import type { ReturnTypeOfSurfaceActions } from "./surface-actions";
import type { CommandFeedback, MainView, PendingAttachment } from "./types";
import {
  runThreadInterrupt,
  snapshotThreadApplicationTarget
} from "./thread-application";

type RefreshHistory = ReturnTypeOfSurfaceActions["refreshHistory"];
type RefreshSnapshot = ReturnTypeOfSurfaceActions["refreshSnapshot"];
type StartNewThread = ReturnTypeOfAppActions["startNewThread"];

export type WorkbenchIntentOwner = ReturnType<typeof createWorkbenchIntentOwner>;

export type WorkbenchIntentOwnerParams = {
  activeScope: GatewayRequestScope | null;
  agentMentionsEnabled: boolean;
  archivedSessions: SessionSummary[];
  beginExplicitViewSwitch(): number;
  clearCommandTransientUi(): void;
  client: GatewayClient | null;
  currentThreadId: string | null;
  fallbackCwd: string;
  importScope: GatewayRequestScope;
  initScope: GatewayRequestScope | null;
  patchComposerDraft(text: string, inputParts?: ThreadEditableInputPart[]): void;
  refreshHistory: RefreshHistory;
  refreshSnapshot: RefreshSnapshot;
  sessions: SessionSummary[];
  setAttachments: Dispatch<SetStateAction<PendingAttachment[]>>;
  setCommandFeedback: Dispatch<SetStateAction<CommandFeedback>>;
  setDraftSession: Dispatch<SetStateAction<HistoryDraftSession | null>>;
  setMobilePanel: Dispatch<SetStateAction<"history" | "transcript" | "status">>;
  setSnapshot: Dispatch<SetStateAction<ThreadSnapshot>>;
  settings: SettingsReadResult | undefined;
  snapshot: ThreadSnapshot;
  startNewThread: StartNewThread;
  steerAvailable: boolean;
  steerTurnId: string | null;
  submitThreadTurn(
    threadId: string,
    text: string,
    mentions: [],
    displayText?: string | null,
    inputOverride?: ThreadEditableInputPart[]
  ): Promise<void>;
  updateMainView(value: MainView): void;
  viewEpochRef: MutableRefObject<number>;
};

export function createWorkbenchIntentOwner(params: WorkbenchIntentOwnerParams) {
  function requireClient(): GatewayClient {
    if (!params.client) {
      throw new Error("Gateway client is unavailable.");
    }
    return params.client;
  }

  async function openThread(
    threadId: string,
    options: { allowDetachedAdoption?: boolean; readOnly?: boolean } = {}
  ) {
    const epoch = params.beginExplicitViewSwitch();
    await params.refreshSnapshot(
      params.client,
      threadId,
      undefined,
      options.readOnly ?? false,
      epoch,
      options.allowDetachedAdoption ?? false
    );
    params.updateMainView("transcript");
    params.setMobilePanel("transcript");
  }

  async function deleteSession(session: SessionSummary) {
    const client = requireClient();
    const deletingCurrent = session.id === params.currentThreadId;
    params.setDraftSession(null);
    await client.request("thread/delete", { threadId: session.id });
    if (deletingCurrent) {
      await params.startNewThread(undefined, { refreshHistory: false });
    }
    await Promise.all([
      params.refreshHistory(client),
      params.refreshHistory(client, true)
    ]);
  }

  async function archiveSession(threadId: string) {
    const client = requireClient();
    params.setDraftSession(null);
    await client.request("thread/archive", { threadId });
    await Promise.all([
      params.refreshHistory(client),
      params.refreshHistory(client, true)
    ]);
  }

  async function restoreSession(threadId: string) {
    const client = requireClient();
    params.setDraftSession(null);
    await client.request("thread/restore", { threadId });
    await Promise.all([
      params.refreshHistory(client),
      params.refreshHistory(client, true)
    ]);
  }

  async function activateArchived(threadId: string) {
    const client = requireClient();
    await client.request("thread/restore", { threadId });
    await Promise.all([
      params.refreshHistory(client),
      params.refreshHistory(client, true)
    ]);
    await openThread(threadId);
  }

  async function importSession(
    profile: ThreadImportProfileView,
    candidateId: string,
    targetId: string,
    activate: boolean
  ) {
    const client = requireClient();
    const imported = await client.request("thread/import", {
      archived: !activate,
      candidateId,
      scope: params.importScope,
      targetId
    });
    const threadId = imported.snapshot.thread?.id;
    if (!threadId) {
      throw new Error(`Imported ${profile.profileLabel} session did not publish a Thread.`);
    }
    await Promise.all([
      params.refreshHistory(client),
      params.refreshHistory(client, true)
    ]);
    await openThread(threadId, {
      allowDetachedAdoption: true,
      readOnly: !activate
    });
  }

  async function forkSession(threadId: string) {
    const client = requireClient();
    const session = params.sessions.find((candidate) => candidate.id === threadId);
    if (!session) return;
    const result = await client.request("thread/action/run", {
      action: { kind: "fork" },
      scope: scopeForCwd(session.cwd),
      threadId
    });
    const forkedThreadId = result.kind === "fork" ? result.snapshot.thread?.id : null;
    if (!forkedThreadId) return;
    await openThread(forkedThreadId);
    await params.refreshHistory(client);
  }

  async function renameSession(threadId: string, title: string) {
    const client = requireClient();
    await client.request("thread/rename", { threadId, title });
    await params.refreshHistory(client);
  }

  async function readUserMessageDraft(entry: TranscriptEntry) {
    const client = requireClient();
    const target = snapshotThreadApplicationTarget(params.snapshot);
    if (!target) throw new Error("The active Thread is unavailable.");
    return client.request("thread/history/draft/read", {
      ...target,
      messageId: entry.id
    });
  }

  async function updateUserMessage(entry: TranscriptEntry, draft: ThreadEditableDraft) {
    const client = requireClient();
    const target = snapshotThreadApplicationTarget(params.snapshot);
    if (!target) throw new Error("The active Thread is unavailable.");
    const result = await client.request("thread/action/run", {
      ...target,
      action: { kind: "revertConversation", messageId: entry.id, draft }
    });
    if (result.kind !== "revertConversation") return;
    if (result.noOp) {
      params.setSnapshot(result.snapshot);
      return;
    }
    const parts = draft.parts ?? [];
    const text = editableDraftText(parts);
    await params.submitThreadTurn(target.threadId, text, [], text, parts);
  }

  async function forkUserMessage(entry: TranscriptEntry, draft: ThreadEditableDraft) {
    const client = requireClient();
    const target = snapshotThreadApplicationTarget(params.snapshot);
    if (!target) throw new Error("The active Thread is unavailable.");
    const result = await client.request("thread/action/run", {
      ...target,
      action: { kind: "forkBefore", messageId: entry.id }
    });
    if (result.kind !== "forkBefore" || !result.snapshot.thread?.id) return;
    const epoch = params.beginExplicitViewSwitch();
    params.setSnapshot(result.snapshot);
    await params.refreshSnapshot(
      client,
      result.snapshot.thread.id,
      undefined,
      false,
      epoch
    );
    prefillEditableDraft(draft.parts ?? [], params.patchComposerDraft, params.setAttachments);
    await params.refreshHistory(client);
    params.updateMainView("transcript");
    params.setMobilePanel("transcript");
  }

  async function restoreEditedHistory() {
    const client = requireClient();
    const target = snapshotThreadApplicationTarget(params.snapshot);
    if (!target) return;
    const result = await client.request("thread/action/run", {
      ...target,
      action: { kind: "unrevertConversation" }
    });
    if (result.kind !== "unrevertConversation") return;
    params.setSnapshot(result.snapshot);
    prefillEditableDraft(
      result.draft.parts ?? [],
      params.patchComposerDraft,
      params.setAttachments
    );
  }

  async function completion(text: string, cursor: number): Promise<CompletionListResult> {
    const client = params.client;
    if (!client) return { items: [], replacement: null };
    const scope = params.activeScope
      ?? params.initScope
      ?? scopeForCwd(params.settings?.cwd ?? params.fallbackCwd);
    const result = await client.request("completion/list", {
      cursor,
      scope,
      text,
      threadId: params.snapshot.thread?.id ?? null
    });
    return {
      ...result,
      items: result.items.filter((item) => (
        params.agentMentionsEnabled || item.target?.kind !== "agent"
      ))
    };
  }

  async function respondClarify(
    request: PendingActionView,
    answers: string[][] | null,
    cancel: boolean
  ) {
    const client = requireClient();
    const target = snapshotThreadApplicationTarget(params.snapshot, request.threadId);
    if (!target) {
      setInteractionFeedback(params.setCommandFeedback, false, "Clarify response does not belong to the active Thread.");
      return;
    }
    const response = await client.request("thread/interaction/respond", {
      ...target,
      interactionId: request.actionId,
      response: cancel
        ? { kind: "cancelClarify" }
        : { kind: "clarify", answers: answers ?? [] }
    });
    setInteractionFeedback(
      params.setCommandFeedback,
      response.accepted,
      response.accepted ? "Clarify response accepted." : "Clarify response was not accepted."
    );
    await params.refreshSnapshot(client, target.threadId, undefined, true);
  }

  async function respondPermission(
    request: PendingActionView,
    decision: PermissionDecision,
    directory?: string
  ) {
    const client = requireClient();
    const target = snapshotThreadApplicationTarget(params.snapshot, request.threadId);
    if (!target) {
      setInteractionFeedback(params.setCommandFeedback, false, "Permission response does not belong to the active Thread.");
      return;
    }
    const response = await client.request("thread/interaction/respond", {
      ...target,
      interactionId: request.actionId,
      response: { kind: "permission", decision, ...(directory ? { directory } : {}) }
    });
    setInteractionFeedback(
      params.setCommandFeedback,
      response.accepted,
      response.accepted ? "Permission response accepted." : "Permission response was not accepted."
    );
    await params.refreshSnapshot(client, target.threadId, undefined, true);
  }

  async function interrupt() {
    const client = requireClient();
    const target = snapshotThreadApplicationTarget(params.snapshot);
    if (!target) {
      params.setCommandFeedback({
        accepted: false,
        command: "interrupt",
        message: "Interrupt is not available for the active Thread.",
        feedbackAnchor: "composer"
      });
      return;
    }
    await runThreadInterrupt(client, target);
    await params.refreshSnapshot(
      client,
      target.threadId,
      undefined,
      true,
      params.viewEpochRef.current
    );
  }

  async function steer(text: string) {
    if (!params.steerTurnId || !params.steerAvailable) return;
    const client = requireClient();
    const target = snapshotThreadApplicationTarget(params.snapshot);
    if (!target) return;
    params.clearCommandTransientUi();
    const result = await client.request("thread/action/run", {
      ...target,
      action: { kind: "steer", expectedTurnId: params.steerTurnId, text }
    });
    if (result.kind === "steer" && result.accepted) {
      params.setSnapshot((current) => appendOptimisticPrompt(current, text));
      await params.refreshHistory(client);
      return;
    }
    params.setCommandFeedback({
      accepted: false,
      command: "/steer",
      message: "The selected Runtime Profile does not support steering this turn.",
      feedbackAnchor: "composer"
    });
  }

  return {
    activateArchived,
    archiveSession,
    completion,
    deleteSession,
    forkSession,
    forkUserMessage,
    importSession,
    interrupt,
    openThread,
    readUserMessageDraft,
    renameSession,
    respondClarify,
    respondPermission,
    restoreEditedHistory,
    restoreSession,
    steer,
    updateUserMessage
  };
}

function editableDraftText(parts: ThreadEditableInputPart[]): string {
  return parts
    .filter((part): part is Extract<ThreadEditableInputPart, { type: "text" }> => part.type === "text")
    .map((part) => part.text)
    .join("\n");
}

function prefillEditableDraft(
  parts: ThreadEditableInputPart[],
  patchComposerDraft: (text: string, parts?: ThreadEditableInputPart[]) => void,
  setAttachments: Dispatch<SetStateAction<PendingAttachment[]>>
) {
  patchComposerDraft(editableDraftText(parts), parts);
  const attachments = parts.flatMap((part, index): PendingAttachment[] => {
    if (part.type !== "image") return [];
    const source = part.input.kind === "localPath" ? part.input.path : part.input.url;
    const name = source.split(/[\\/]/).pop()?.split(/[?#]/)[0] || `image-${index + 1}`;
    return [{
      id: `history:${index}:${source}`,
      input: part,
      kind: "image",
      name,
      ...(part.input.kind === "url" ? { previewUrl: part.input.url } : {}),
      size: 0,
      sizeLabel: "From history"
    }];
  });
  setAttachments(attachments);
}

function setInteractionFeedback(
  setCommandFeedback: Dispatch<SetStateAction<CommandFeedback>>,
  accepted: boolean,
  message: string
) {
  setCommandFeedback({
    accepted,
    command: "thread/interaction/respond",
    message,
    feedbackAnchor: "composer"
  });
}
