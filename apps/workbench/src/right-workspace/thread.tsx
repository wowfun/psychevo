import { useEffect, useMemo, useRef, useState, useSyncExternalStore } from "react";
import { Bot, MessageSquare, RefreshCw } from "lucide-react";
import { Composer, TranscriptPanel, type TranscriptAgentSession, type WorkspaceFileLinkContext } from "@psychevo/components";
import {
  appendOptimisticPrompt,
  parseThreadSnapshot,
  scopeForCwd,
  ThreadController
} from "@psychevo/client";
import type { GatewayClient } from "@psychevo/client";
import type {
  CompletionListResult,
  GatewayMention,
  GatewayRequestScope,
  ThreadActionDescriptorView,
  ThreadHistoryFidelityView,
  ThreadSnapshot
} from "@psychevo/protocol";
import {
  gatewayEventsForThread,
  type GatewayEventFeedItem,
  type GatewayThreadEventFeed
} from "../gateway-event-feed";
import { parseThreadContext } from "../runtime-context";
import { idleActivity, normalizeSnapshot } from "../session-utils";
import {
  hydrateThreadSnapshotHistory,
  threadApplicationTarget
} from "../thread-application";

type ThreadPanelProps = {
  client: GatewayClient | null;
  disabled: boolean;
  gatewayEventFeed: GatewayThreadEventFeed;
  kind: "sideConversation" | "agentSession";
  historyFidelity?: ThreadHistoryFidelityView | null;
  parentThreadId?: string | null;
  pendingPrompt?: string | null;
  scope: GatewayRequestScope | null;
  threadId: string | null;
  title: string;
  onCopyText?: ((text: string) => void | Promise<void>) | undefined;
  onOpenAgentSession?: ((session: TranscriptAgentSession) => void) | undefined;
  onPendingPromptConsumed?: (() => void) | undefined;
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
  scope,
  threadId,
  title,
  onCopyText,
  onOpenAgentSession,
  onPendingPromptConsumed,
  workspaceFileLinks
}: ThreadPanelProps) {
  const controller = useMemo(() => new ThreadController(null), [threadId]);
  const snapshotStore = useMemo(() => ({
    getSnapshot: () => controller.snapshot(),
    subscribe: (listener: () => void) => controller.subscribe(listener)
  }), [controller]);
  const snapshot = useSyncExternalStore(
    snapshotStore.subscribe,
    snapshotStore.getSnapshot,
    snapshotStore.getSnapshot
  );
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [threadContext, setThreadContext] = useState<ReturnType<typeof parseThreadContext> | null>(null);
  const [observedHistoryFidelity, setObservedHistoryFidelity] = useState<ThreadHistoryFidelityView | null>(
    registeredHistoryFidelity
  );
  const [access, setAccess] = useState<"checking" | "readOnly" | "readWrite">(
    kind === "sideConversation" ? "readWrite" : "checking"
  );
  const [threadActions, setThreadActions] = useState<ThreadActionDescriptorView[]>([]);
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
  const promptSubmitDisabled = loading
    || !visibleSnapshot
    || !threadContext
    || !threadContext.sendability.allowed;
  const promptSubmitBlockReason = loading
    ? "Loading Thread Context."
    : !visibleSnapshot
      ? "Loading thread."
      : threadContext?.sendability.reason
        ?? (!threadContext ? "Thread Context is required before starting a turn." : undefined);
  const icon = kind === "agentSession" ? <Bot size={16} /> : <MessageSquare size={16} />;
  const threadLabel = useMemo(() => threadId ?? "unavailable", [threadId]);

  async function refresh() {
    const refreshGeneration = ++refreshGenerationRef.current;
    let refreshAfterTerminal = false;
    const barrierSeq = gatewayEventFeedRef.current.latestSeq;
    lastAppliedGatewayEventSeqRef.current = barrierSeq;
    if (!client || !threadId) {
      controller.reset(null);
      controller.setContext(null);
      setThreadContext(null);
      setObservedHistoryFidelity(registeredHistoryFidelity);
      setThreadActions([]);
      setSteerAvailable(false);
      setLoading(false);
      return;
    }
    setLoading(true);
    setError(null);
    controller.setContext(null);
    setThreadContext(null);
    setAccess(kind === "sideConversation" ? "readWrite" : "checking");
    setThreadActions([]);
    setSteerAvailable(false);
    try {
      let nextAccess: "readOnly" | "readWrite" = "readWrite";
      let accessError: string | null = null;
      let nextHistoryFidelity = registeredHistoryFidelity;
      let nextActions: ThreadActionDescriptorView[] = [];
      let nextSteerAvailable = false;
      let nextContext: ReturnType<typeof parseThreadContext> | null = null;
      const bootstrap = parseThreadSnapshot(await client.request("thread/read", { threadId }));
      const next = normalizeSnapshot(await hydrateThreadSnapshotHistory(client, bootstrap));
      const threadScope = scope ?? next.scope;
      try {
        nextContext = parseThreadContext(await client.request("thread/context/read", {
          threadId,
          target: null,
          scope: threadScope
        }));
        const binding = nextContext.binding?.threadId === threadId ? nextContext.binding : null;
        nextActions = nextContext.actions;
        nextSteerAvailable = nextContext.actions.some(
          (action) => action.id === "steer" && action.enabled
        );
        if (kind === "agentSession") {
          nextAccess = binding?.ownership === "readWrite"
            ? "readWrite"
            : "readOnly";
        }
        nextHistoryFidelity = nextContext.history.fidelity;
      } catch (contextError) {
        if (kind === "agentSession") {
          nextAccess = "readOnly";
        }
        accessError = contextError instanceof Error ? contextError.message : String(contextError);
      }
      if (refreshGenerationRef.current !== refreshGeneration) {
        return;
      }
      const pending = gatewayEventsForThread(gatewayEventFeedRef.current, threadId)
        .filter((record) => record.seq > barrierSeq);
      controller.reset(next);
      controller.setContext(nextContext);
      setThreadContext(nextContext);
      refreshAfterTerminal = applyGatewayFeed(controller, pending);
      nextHistoryFidelity = controller.snapshot()?.history.fidelity ?? nextHistoryFidelity;
      lastAppliedGatewayEventSeqRef.current = pending.at(-1)?.seq ?? barrierSeq;
      setAccess(nextAccess);
      setThreadActions(nextActions);
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
      if (refreshAfterTerminal && refreshGenerationRef.current === refreshGeneration) {
        void refresh();
      }
    }
  }

  useEffect(() => {
    void refresh();
    consumedPendingPromptRef.current = null;
    return () => {
      refreshGenerationRef.current += 1;
    };
  }, [client, kind, registeredHistoryFidelity, scope, threadId]);

  useEffect(() => {
    const pending = gatewayEventsForThread(gatewayEventFeed, threadId)
      .filter((record) => record.seq > lastAppliedGatewayEventSeqRef.current);
    if (pending.length === 0) {
      return;
    }
    if ((controller.snapshot()?.thread?.id ?? null) !== threadId) {
      return;
    }
    const refreshAfterTerminal = applyGatewayFeed(controller, pending);
    lastAppliedGatewayEventSeqRef.current = pending.at(-1)?.seq ?? lastAppliedGatewayEventSeqRef.current;
    if (refreshAfterTerminal) {
      void refresh();
    }
  }, [controller, gatewayEventFeed.latestSeq, threadId]);

  useEffect(() => {
    const prompt = pendingPrompt?.trim();
    if (
      !prompt
      || !threadId
      || !snapshot
      || loading
      || !threadContext
      || consumedPendingPromptRef.current === prompt
    ) {
      return;
    }
    consumedPendingPromptRef.current = prompt;
    onPendingPromptConsumed?.();
    if (!writable) {
      setError("This runtime child is read-only.");
      return;
    }
    void submit(prompt, []);
  }, [loading, onPendingPromptConsumed, pendingPrompt, snapshot?.thread?.id, threadContext, threadId, writable]);

  async function submit(text: string, mentions: GatewayMention[]) {
    if (!writable || !threadId || !text.trim()) {
      return;
    }
    if (!client || !snapshot) {
      return;
    }
    const context = controller.context();
    if (!context) {
      setError("Thread Context is required before starting a turn.");
      return;
    }
    const input = [{ type: "text" as const, text: text.trim() }];
    const controls = controller.turnControls(context.targetId, {});
    const admission = controller.admitTurn({ controls, input, mentions });
    if (!admission.allowed) {
      setError(admission.reason ?? "This Agent target cannot start a turn.");
      return;
    }
    const plan = controller.beginTurn({
      controls,
      input,
      mentions,
      optimisticText: text.trim(),
      scope: snapshot.scope,
      threadId
    });
    try {
      const result = await client.request("turn/start", plan.params);
      controller.acceptTurnStart(result, plan.prepared);
    } catch (submitError) {
      controller.rejectTurnStart(plan.prepared);
      setError(submitError instanceof Error ? submitError.message : String(submitError));
    }
  }

  async function steer(text: string) {
    const target = threadApplicationTarget(scope ?? snapshot?.scope, threadId);
    if (!writable || !steerAvailable || !client || !target || !activity.activeTurnId || !text.trim()) {
      return;
    }
    try {
      const result = await client.request("thread/action/run", {
        ...target,
        action: { kind: "steer", expectedTurnId: activity.activeTurnId, text }
      });
      if (result.kind !== "steer" || !result.accepted) {
        setError("The selected Runtime Profile does not support steering this turn.");
        return;
      }
      const current = controller.snapshot();
      if (current) {
        controller.reset(normalizeSnapshot(appendOptimisticPrompt(current, text.trim())));
      }
    } catch (steerError) {
      setError(steerError instanceof Error ? steerError.message : String(steerError));
    }
  }

  async function interrupt() {
    const target = threadApplicationTarget(scope ?? snapshot?.scope, threadId);
    const interruptAvailable = threadActions.some((action) => action.id === "interrupt" && action.enabled);
    if (!writable || !client || !target || !interruptAvailable) {
      return;
    }
    try {
      await client.request("thread/action/run", {
        ...target,
        action: { kind: "interrupt" }
      });
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

function runtimeHistoryFidelityNotice(fidelity: ThreadHistoryFidelityView): string {
  if (fidelity === "full") return "Full history";
  if (fidelity === "summary") return "Summary history; only a condensed record is available.";
  if (fidelity === "unavailable") return "History unavailable; earlier messages could not be restored.";
  return "Partial history; some messages or detail may be missing.";
}

function emptyThreadSnapshot(threadId: string | null): ThreadSnapshot {
  return {
    source: { kind: "web", rawId: "right-thread", lifetime: "persistent", rawIdentity: null, visibleName: null },
    scope: scopeForCwd(""),
    thread: null,
    history: { owner: "psychevo", fidelity: "full", cursor: null, hint: null },
    entries: [],
    activity: idleActivity(),
    pendingActions: []
  };
}

function applyGatewayFeed(
  controller: ThreadController,
  records: GatewayEventFeedItem[]
): boolean {
  let terminalObserved = false;
  for (const record of records) {
    controller.applyGatewayEvent(record.event);
    terminalObserved ||= record.event.type === "turnCompleted";
  }
  return terminalObserved;
}
