import { useEffect, useMemo, useRef, useState } from "react";
import { Bot, MessageSquare, RefreshCw } from "lucide-react";
import { Composer, TranscriptPanel, type TranscriptAgentSession, type WorkspaceFileLinkContext } from "@psychevo/components";
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
  GatewayRequestScope,
  RuntimeHistoryFidelityView,
  ThreadSnapshot
} from "@psychevo/protocol";
import {
  gatewayEventsForThread,
  type GatewayEventFeedItem,
  type GatewayThreadEventFeed
} from "../gateway-event-feed";
import { parseRuntimeContext, runtimeSessionHistoryFidelity } from "../runtime-context";
import { idleActivity, normalizeSnapshot } from "../session-utils";

type ThreadPanelProps = {
  client: GatewayClient | null;
  disabled: boolean;
  gatewayEventFeed: GatewayThreadEventFeed;
  kind: "sideConversation" | "agentSession";
  historyFidelity?: RuntimeHistoryFidelityView | null;
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
  workspaceFileLinks?: WorkspaceFileLinkContext | undefined;
};

export function ThreadPanel({
  client,
  disabled,
  gatewayEventFeed,
  kind,
  historyFidelity: registeredHistoryFidelity = null,
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
  onSubmitThreadTurn,
  workspaceFileLinks
}: ThreadPanelProps) {
  const [snapshot, setSnapshot] = useState<ThreadSnapshot | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [observedHistoryFidelity, setObservedHistoryFidelity] = useState<RuntimeHistoryFidelityView | null>(
    registeredHistoryFidelity
  );
  const [access, setAccess] = useState<"checking" | "readOnly" | "readWrite">(
    kind === "sideConversation" ? "readWrite" : "checking"
  );
  const [steerAvailable, setSteerAvailable] = useState(kind === "sideConversation");
  const gatewayEventFeedRef = useRef(gatewayEventFeed);
  const lastAppliedGatewayEventSeqRef = useRef(gatewayEventFeed.latestSeq);
  const refreshGenerationRef = useRef(0);
  const consumedPendingPromptRef = useRef<string | null>(null);
  gatewayEventFeedRef.current = gatewayEventFeed;
  const snapshotMatchesThread = (snapshot?.thread?.id ?? null) === threadId;
  const visibleSnapshot = snapshotMatchesThread ? snapshot : null;
  const activity = normalizeSnapshot(visibleSnapshot ?? emptyThreadSnapshot(threadId)).activity;
  const entries = visibleSnapshot?.entries ?? [];
  const running = activity.running;
  const writable = kind === "sideConversation" || access === "readWrite";
  const readOnly = kind === "agentSession" && access === "readOnly";
  const icon = kind === "agentSession" ? <Bot size={16} /> : <MessageSquare size={16} />;
  const threadLabel = useMemo(() => threadId ?? "unavailable", [threadId]);

  async function refresh() {
    const refreshGeneration = ++refreshGenerationRef.current;
    const barrierSeq = gatewayEventFeedRef.current.latestSeq;
    lastAppliedGatewayEventSeqRef.current = barrierSeq;
    if (!client || !threadId) {
      setSnapshot(null);
      setObservedHistoryFidelity(registeredHistoryFidelity);
      setSteerAvailable(kind === "sideConversation");
      setLoading(false);
      return;
    }
    setLoading(true);
    setError(null);
    setAccess(kind === "sideConversation" ? "readWrite" : "checking");
    setSteerAvailable(kind === "sideConversation");
    try {
      let nextAccess: "readOnly" | "readWrite" = "readWrite";
      let accessError: string | null = null;
      let nextHistoryFidelity = registeredHistoryFidelity;
      let nextSteerAvailable = kind === "sideConversation";
      if (kind === "agentSession") {
        try {
          const context = parseRuntimeContext(await client.request("runtime/context/read", {
            threadId,
            runtimeRef: null,
            scope
          }));
          const binding = context.binding?.threadId === threadId ? context.binding : null;
          nextSteerAvailable = Boolean(binding) && (
            binding?.nativeKind === "native"
            || binding?.nativeKind === "acp"
            || context.capabilities.some(
              (capability) => capability.id === "turn.steer" && capability.enabled
            )
          );
          nextAccess = binding?.ownership === "readWrite"
            ? "readWrite"
            : "readOnly";
          const activeSession = context.activeSession;
          if (activeSession && binding && activeSession.sessionHandle === binding.sessionHandle) {
            nextHistoryFidelity = activeSession.fidelity;
          }
          if (binding?.ownership === "readOnly" && binding.sessionHandle) {
            try {
              const history = await client.request("runtime/session/read", {
                runtimeRef: binding.runtimeRef,
                sessionHandle: binding.sessionHandle,
                scope
              });
              nextHistoryFidelity = runtimeSessionHistoryFidelity(history) ?? nextHistoryFidelity;
            } catch (historyError) {
              accessError = historyError instanceof Error ? historyError.message : String(historyError);
            }
          }
        } catch (contextError) {
          nextAccess = "readOnly";
          accessError = contextError instanceof Error ? contextError.message : String(contextError);
        }
      }
      let next = normalizeSnapshot(parseThreadSnapshot(await client.request("thread/read", { threadId })));
      if (refreshGenerationRef.current !== refreshGeneration) {
        return;
      }
      const pending = gatewayEventsForThread(gatewayEventFeedRef.current, threadId)
        .filter((record) => record.seq > barrierSeq);
      next = applyGatewayFeed(next, pending);
      lastAppliedGatewayEventSeqRef.current = pending.at(-1)?.seq ?? barrierSeq;
      setSnapshot(next);
      setAccess(nextAccess);
      setSteerAvailable(nextSteerAvailable);
      setObservedHistoryFidelity(nextHistoryFidelity);
      setError(accessError);
    } catch (refreshError) {
      if (refreshGenerationRef.current === refreshGeneration) {
        setError(refreshError instanceof Error ? refreshError.message : String(refreshError));
      }
    } finally {
      if (refreshGenerationRef.current === refreshGeneration) {
        setLoading(false);
      }
    }
  }

  useEffect(() => {
    void refresh();
    consumedPendingPromptRef.current = null;
    return () => {
      refreshGenerationRef.current += 1;
    };
  }, [client, kind, registeredHistoryFidelity, threadId]);

  useEffect(() => {
    const pending = gatewayEventsForThread(gatewayEventFeed, threadId)
      .filter((record) => record.seq > lastAppliedGatewayEventSeqRef.current);
    if (pending.length === 0) {
      return;
    }
    setSnapshot((current) => {
      if (!current || (current.thread?.id ?? null) !== threadId) {
        return current;
      }
      lastAppliedGatewayEventSeqRef.current = pending.at(-1)?.seq ?? lastAppliedGatewayEventSeqRef.current;
      return applyGatewayFeed(current, pending);
    });
  }, [gatewayEventFeed.latestSeq, threadId]);

  useEffect(() => {
    const prompt = pendingPrompt?.trim();
    if (!prompt || !threadId || !snapshot || consumedPendingPromptRef.current === prompt) {
      return;
    }
    consumedPendingPromptRef.current = prompt;
    onPendingPromptConsumed?.();
    if (!writable) {
      setError("This runtime child is read-only.");
      return;
    }
    if (promptSubmitDisabled) {
      setError(promptSubmitBlockReason ?? "Select a provider/model before starting a conversation.");
      return;
    }
    setSnapshot((current) => current ? normalizeSnapshot(appendOptimisticPrompt(current, prompt)) : current);
    void onSubmitThreadTurn(threadId, prompt, []).catch((submitError) => {
      setError(submitError instanceof Error ? submitError.message : String(submitError));
    });
  }, [onPendingPromptConsumed, onSubmitThreadTurn, pendingPrompt, promptSubmitBlockReason, promptSubmitDisabled, snapshot?.thread?.id, threadId, writable]);

  async function submit(text: string, mentions: GatewayMention[]) {
    if (!writable || !threadId || !text.trim()) {
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
    if (!writable || !steerAvailable || !client || !threadId || !activity.activeTurnId || !text.trim()) {
      return;
    }
    try {
      const result = await client.request("turn/steer", {
        expectedTurnId: activity.activeTurnId,
        threadId,
        text
      });
      if (!result.accepted) {
        setError("The selected Runtime Profile does not support steering this turn.");
        return;
      }
      setSnapshot((current) => current ? normalizeSnapshot(appendOptimisticPrompt(current, text.trim())) : current);
    } catch (steerError) {
      setError(steerError instanceof Error ? steerError.message : String(steerError));
    }
  }

  async function interrupt() {
    if (!writable || !client || !threadId) {
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
      <div className="threadPanelNotices">
        {parentThreadId && <p className="threadPanelParent" title={parentThreadId}>Parent {parentThreadId}</p>}
        {readOnly && (
          <p
            className="threadPanelRuntimeNotice"
            data-history-fidelity={observedHistoryFidelity ?? "unknown"}
            role="note"
          >
            Read-only runtime child
            {observedHistoryFidelity && ` · ${runtimeHistoryFidelityNotice(observedHistoryFidelity)}`}
          </p>
        )}
        {error && <p className="threadPanelError">{error}</p>}
      </div>
      <div className="threadPanelTranscript">
        <TranscriptPanel
          activity={activity}
          entries={entries}
          onCopyText={onCopyText}
          onOpenAgentSession={onOpenAgentSession}
          threadId={threadId}
          {...(workspaceFileLinks ? { workspaceFileLinks } : {})}
        />
      </div>
      {writable && (
        <div className="threadPanelComposerDock">
          <Composer
            completionProvider={completionProvider}
            disabled={disabled || !threadId}
            mode="default"
            promptSubmitBlockReason={promptSubmitBlockReason}
            promptSubmitDisabled={promptSubmitDisabled}
            running={running}
            runningStartedAtMs={activity.startedAtMs ?? null}
            steerAvailable={steerAvailable}
            onInterrupt={() => void interrupt()}
            onSteer={(text) => void steer(text)}
            onSubmit={(text, mentions) => void submit(text, mentions)}
          />
        </div>
      )}
    </section>
  );
}

function runtimeHistoryFidelityNotice(fidelity: RuntimeHistoryFidelityView): string {
  if (fidelity === "full") return "Full history";
  if (fidelity === "summary") return "Summary history; only a condensed record is available.";
  return "Partial history; some messages or detail may be missing.";
}

function emptyThreadSnapshot(threadId: string | null): ThreadSnapshot {
  return {
    source: { kind: "web", rawId: "right-thread", lifetime: "persistent", rawIdentity: null, visibleName: null },
    scope: scopeForCwd(""),
    thread: threadId
      ? { id: threadId, backend: { kind: "psychevo", sessionHandle: threadId, runtimeRef: "native" }, sourceKey: null }
      : null,
    entries: [],
    activity: idleActivity(),
    pendingActions: []
  };
}

function applyGatewayFeed(
  snapshot: ThreadSnapshot,
  records: GatewayEventFeedItem[]
): ThreadSnapshot {
  return records.reduce(
    (current, record) => normalizeSnapshot(applyLiveTranscriptEvent(current, record.event)),
    snapshot
  );
}
