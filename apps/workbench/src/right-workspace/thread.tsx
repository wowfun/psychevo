import { useEffect, useMemo, useRef, useState } from "react";
import { Bot, MessageSquare, RefreshCw } from "lucide-react";
import { Composer, TranscriptPanel, type TranscriptAgentSession } from "@psychevo/components";
import {
  appendOptimisticPrompt,
  applyLiveTranscriptEvent,
  parseThreadSnapshot,
  scopeForCwd
} from "@psychevo/client";
import type { GatewayClient } from "@psychevo/client";
import type {
  CompletionListResult,
  GatewayMention,
  GatewayEvent,
  GatewayRequestScope,
  ThreadSnapshot
} from "@psychevo/protocol";
import type { GatewayEventFeed } from "../types";
import { idleActivity, normalizeSnapshot } from "../session-utils";

type ThreadPanelProps = {
  client: GatewayClient | null;
  disabled: boolean;
  kind: "sideConversation" | "agentSession";
  latestGatewayEvent: GatewayEventFeed | null;
  parentThreadId?: string | null;
  pendingPrompt?: string | null;
  promptSubmitBlockReason?: string | undefined;
  promptSubmitDisabled?: boolean | undefined;
  scope: GatewayRequestScope | null;
  threadId: string | null;
  title: string;
  onCopyText?: ((text: string) => void | Promise<void>) | undefined;
  onOpenAgentSession?: ((session: TranscriptAgentSession) => void) | undefined;
  onPendingPromptConsumed?: (() => void) | undefined;
  onSubmitThreadTurn(threadId: string, text: string, mentions: GatewayMention[]): Promise<void>;
};

export function ThreadPanel({
  client,
  disabled,
  kind,
  latestGatewayEvent,
  parentThreadId,
  pendingPrompt,
  promptSubmitBlockReason,
  promptSubmitDisabled = false,
  scope,
  threadId,
  title,
  onCopyText,
  onOpenAgentSession,
  onPendingPromptConsumed,
  onSubmitThreadTurn
}: ThreadPanelProps) {
  const [snapshot, setSnapshot] = useState<ThreadSnapshot | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const pendingEventsRef = useRef<GatewayEventFeed[]>([]);
  const consumedPendingPromptRef = useRef<string | null>(null);
  const snapshotMatchesThread = (snapshot?.thread?.id ?? null) === threadId;
  const visibleSnapshot = snapshotMatchesThread ? snapshot : null;
  const activity = normalizeSnapshot(visibleSnapshot ?? emptyThreadSnapshot(threadId)).activity;
  const entries = visibleSnapshot?.entries ?? [];
  const running = activity.running;
  const icon = kind === "agentSession" ? <Bot size={16} /> : <MessageSquare size={16} />;
  const threadLabel = useMemo(() => threadId ?? "unavailable", [threadId]);

  async function refresh() {
    if (!client || !threadId) {
      setSnapshot(null);
      return;
    }
    setLoading(true);
    setError(null);
    try {
      setSnapshot(normalizeSnapshot(parseThreadSnapshot(await client.request("thread/read", { threadId }))));
    } catch (refreshError) {
      setError(refreshError instanceof Error ? refreshError.message : String(refreshError));
    } finally {
      setLoading(false);
    }
  }

  useEffect(() => {
    void refresh();
    pendingEventsRef.current = [];
    consumedPendingPromptRef.current = null;
  }, [client, threadId]);

  useEffect(() => {
    if (!latestGatewayEvent || !eventMatchesThread(latestGatewayEvent.event, threadId)) {
      return;
    }
    setSnapshot((current) => {
      if (!current) {
        appendPendingGatewayEvent(pendingEventsRef.current, latestGatewayEvent);
        return current;
      }
      return normalizeSnapshot(applyLiveTranscriptEvent(current, latestGatewayEvent.event));
    });
  }, [latestGatewayEvent?.seq, threadId]);

  useEffect(() => {
    if (!snapshot || pendingEventsRef.current.length === 0) {
      return;
    }
    const pending = pendingEventsRef.current;
    pendingEventsRef.current = [];
    let next = snapshot;
    for (const feed of pending) {
      if (!eventMatchesThread(feed.event, threadId)) {
        appendPendingGatewayEvent(pendingEventsRef.current, feed);
        continue;
      }
      next = normalizeSnapshot(applyLiveTranscriptEvent(next, feed.event));
    }
    if (next !== snapshot) {
      setSnapshot(next);
    }
  }, [snapshot?.thread?.id, threadId]);

  useEffect(() => {
    const prompt = pendingPrompt?.trim();
    if (!prompt || !threadId || !snapshot || consumedPendingPromptRef.current === prompt) {
      return;
    }
    consumedPendingPromptRef.current = prompt;
    onPendingPromptConsumed?.();
    if (promptSubmitDisabled) {
      setError(promptSubmitBlockReason ?? "Select a provider/model before starting a conversation.");
      return;
    }
    setSnapshot((current) => current ? normalizeSnapshot(appendOptimisticPrompt(current, prompt)) : current);
    void onSubmitThreadTurn(threadId, prompt, []).catch((submitError) => {
      setError(submitError instanceof Error ? submitError.message : String(submitError));
    });
  }, [onPendingPromptConsumed, onSubmitThreadTurn, pendingPrompt, promptSubmitBlockReason, promptSubmitDisabled, snapshot?.thread?.id, threadId]);

  async function submit(text: string, mentions: GatewayMention[]) {
    if (!threadId || !text.trim()) {
      return;
    }
    if (promptSubmitDisabled) {
      setError(promptSubmitBlockReason ?? "Select a provider/model before starting a conversation.");
      return;
    }
    setSnapshot((current) => current ? normalizeSnapshot(appendOptimisticPrompt(current, text.trim())) : current);
    try {
      await onSubmitThreadTurn(threadId, text, mentions);
    } catch (submitError) {
      setError(submitError instanceof Error ? submitError.message : String(submitError));
    }
  }

  async function steer(text: string) {
    if (!client || !threadId || !activity.activeTurnId || !text.trim()) {
      return;
    }
    setSnapshot((current) => current ? normalizeSnapshot(appendOptimisticPrompt(current, text.trim())) : current);
    try {
      await client.request("turn/steer", {
        expectedTurnId: activity.activeTurnId,
        threadId,
        text
      });
    } catch (steerError) {
      setError(steerError instanceof Error ? steerError.message : String(steerError));
    }
  }

  async function interrupt() {
    if (!client || !threadId) {
      return;
    }
    try {
      await client.request("turn/interrupt", { threadId });
      await refresh();
    } catch (interruptError) {
      setError(interruptError instanceof Error ? interruptError.message : String(interruptError));
    }
  }

  async function completionProvider(text: string, cursor: number): Promise<CompletionListResult> {
    if (!client || !scope || !threadId) {
      return { items: [], replacement: null };
    }
    return client.request("completion/list", {
      cursor,
      scope,
      text,
      threadId
    });
  }

  return (
    <section className="threadPanel" aria-label={title}>
      <header>
        <div className="threadPanelTitle">
          {icon}
          <div>
            <h2>{title}</h2>
            <p title={threadLabel}>{threadLabel}</p>
          </div>
        </div>
        <button
          aria-label="Refresh thread"
          disabled={disabled || loading || !threadId}
          onClick={() => void refresh()}
          title="Refresh"
          type="button"
        >
          <RefreshCw size={15} />
        </button>
      </header>
      {parentThreadId && <p className="threadPanelParent" title={parentThreadId}>Parent {parentThreadId}</p>}
      {error && <p className="threadPanelError">{error}</p>}
      <div className="threadPanelTranscript">
        <TranscriptPanel
          activity={activity}
          entries={entries}
          onCopyText={onCopyText}
          onOpenAgentSession={onOpenAgentSession}
          threadId={threadId}
        />
      </div>
      <div className="threadPanelComposerDock">
        <Composer
          completionProvider={completionProvider}
          disabled={disabled || !threadId}
          mode="default"
          promptSubmitBlockReason={promptSubmitBlockReason}
          promptSubmitDisabled={promptSubmitDisabled}
          running={running}
          runningStartedAtMs={activity.startedAtMs ?? null}
          onInterrupt={() => void interrupt()}
          onSteer={(text) => void steer(text)}
          onSubmit={(text, mentions) => void submit(text, mentions)}
        />
      </div>
    </section>
  );
}

function emptyThreadSnapshot(threadId: string | null): ThreadSnapshot {
  return {
    source: { kind: "web", rawId: "right-thread", lifetime: "persistent", rawIdentity: null, visibleName: null },
    scope: scopeForCwd(""),
    thread: threadId
      ? { id: threadId, backend: { kind: "psychevo", nativeId: threadId }, sourceKey: null }
      : null,
    entries: [],
    activity: idleActivity(),
    pendingPermissions: [],
    pendingClarifies: []
  };
}

function appendPendingGatewayEvent(pending: GatewayEventFeed[], feed: GatewayEventFeed) {
  if (pending.some((candidate) => candidate.seq === feed.seq)) {
    return;
  }
  pending.push(feed);
}

function eventMatchesThread(event: GatewayEvent, threadId: string | null): boolean {
  const eventThreadId = eventThreadIdForEvent(event);
  if (!threadId) {
    return eventThreadId === null;
  }
  return eventThreadId === threadId;
}

function eventThreadIdForEvent(event: GatewayEvent): string | null {
  switch (event.type) {
    case "turnStarted":
    case "turnQueued":
      return event.threadId || null;
    case "turnCompleted":
      return event.threadId ||
        event.turn.threadId ||
        event.committedEntries.find((entry) => entry.threadId)?.threadId ||
        null;
    case "entryStarted":
    case "entryUpdated":
    case "entryCompleted":
      return event.entry.threadId || null;
    case "activityChanged":
      return event.threadId || null;
    case "titleChanged":
      return event.threadId || null;
    default:
      return null;
  }
}
