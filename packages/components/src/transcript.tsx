import { ArrowDownToLine, Check, ChevronDown, ChevronRight, Copy, Download, ExternalLink, GitFork, Image as ImageIcon, Maximize2, Pencil, Volume2, X } from "lucide-react";
import { useEffect, useLayoutEffect, useMemo, useRef, useState, type CSSProperties } from "react";
import {
  sideInheritedMetadataHidden,
  type GatewayActivity,
  type ThreadEditableDraft,
  type ThreadHistoryDraftReadResult,
  type ThreadHistoryView,
  type TranscriptBlock,
  type TranscriptEntry
} from "@psychevo/protocol";
import { MarkdownText } from "./markdown";
import { asRecord, stringValue } from "./shared";
import { evidenceDisplay, type EvidenceDisplay } from "./toolEvidence";
import { ToolDetail } from "./transcript/tool-detail";
import { resolveWorkspaceFilePath, type WorkspaceFileLinkContext } from "./workspaceFileLinks";

export type TranscriptAgentSession = {
  agentName?: string | null;
  childSessionId: string;
  parentSessionId?: string | null;
  task?: string | null;
  taskName?: string | null;
  title?: string | null;
};

export interface TranscriptPanelProps {
  activity?: GatewayActivity;
  entries: TranscriptEntry[];
  history?: TranscriptHistoryView | null | undefined;
  onCopyText?: ((text: string) => void | Promise<void>) | undefined;
  onForkUserMessage?: ((entry: TranscriptEntry, draft: ThreadEditableDraft) => void | Promise<void>) | undefined;
  onReadUserMessageDraft?: ((entry: TranscriptEntry) => Promise<ThreadHistoryDraftReadResult>) | undefined;
  onUpdateUserMessage?: ((entry: TranscriptEntry, draft: ThreadEditableDraft) => void | Promise<void>) | undefined;
  onOpenAgentSession?: ((session: TranscriptAgentSession) => void) | undefined;
  onReadAloudText?: ((text: string) => void | Promise<void>) | undefined;
  threadId?: string | null;
  workspaceFileLinks?: WorkspaceFileLinkContext;
}

export type TranscriptHistoryView = ThreadHistoryView;

type CopyTextHandler = ((text: string) => void | Promise<void>) | undefined;
type ReadAloudTextHandler = ((text: string) => void | Promise<void>) | undefined;
type OpenAgentSessionHandler = ((session: TranscriptAgentSession) => void) | undefined;
type ReadUserMessageDraftHandler = ((entry: TranscriptEntry) => Promise<ThreadHistoryDraftReadResult>) | undefined;
type MutateUserMessageHandler = ((entry: TranscriptEntry, draft: ThreadEditableDraft) => void | Promise<void>) | undefined;

const ACTIVITY_SPINNER = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧"];
const BOTTOM_THRESHOLD_PX = 48;
const TRANSCRIPT_SCROLL_MEMORY_LIMIT = 64;
const useIsomorphicLayoutEffect = typeof window === "undefined" ? useEffect : useLayoutEffect;

type TranscriptScrollMemory = {
  atBottom: boolean;
  top: number;
};

export function TranscriptPanel({ activity, entries, history, onCopyText, onForkUserMessage, onOpenAgentSession, onReadAloudText, onReadUserMessageDraft, onUpdateUserMessage, threadId, workspaceFileLinks }: TranscriptPanelProps) {
  const [followingBottom, setFollowingBottom] = useState(true);
  const [scrolling, setScrolling] = useState(false);
  const [activityTick, setActivityTick] = useState(0);
  const scrollRef = useRef<HTMLDivElement | null>(null);
  const scrollIdleTimer = useRef<ReturnType<typeof globalThis.setTimeout> | null>(null);
  const scrollMemoryRef = useRef<Map<string, TranscriptScrollMemory>>(new Map());
  const activeThreadKeyRef = useRef<string | null>(null);
  const orderedEntries = useMemo(() => orderTranscriptEntries(entries), [entries]);
  const visibleEntries = useMemo(() => visibleTranscriptEntries(orderedEntries), [orderedEntries]);
  const threadKey = useMemo(() => transcriptThreadKey(threadId, visibleEntries), [threadId, visibleEntries]);
  const hasRunningActivityBlock = useMemo(
    () => visibleEntries.some((entry) => visibleBlocks(entry).some(isRunningActivityBlock)),
    [visibleEntries]
  );
  const threadItemsClass = `pevo-threadItems ${scrolling ? "is-scrolling" : ""}`.trim();

  function updateFollowingBottom(value: boolean) {
    setFollowingBottom(value);
  }

  useIsomorphicLayoutEffect(() => {
    const scroller = scrollRef.current;
    if (!scroller) {
      return;
    }
    const threadChanged = activeThreadKeyRef.current !== threadKey;
    activeThreadKeyRef.current = threadKey;
    if (threadChanged) {
      const remembered = threadKey ? recallTranscriptScroll(scrollMemoryRef.current, threadKey) : null;
      const atBottom = remembered?.atBottom ?? true;
      const top = atBottom ? scroller.scrollHeight : clampScrollTop(scroller, remembered?.top ?? 0);
      scrollTranscript(scroller, top);
      updateFollowingBottom(atBottom);
      rememberCurrentTranscriptScroll(scrollMemoryRef.current, threadKey, top, atBottom);
      return;
    }
    if (!followingBottom) {
      return;
    }
    scrollTranscript(scroller, scroller.scrollHeight);
    rememberCurrentTranscriptScroll(scrollMemoryRef.current, threadKey, scroller.scrollHeight, true);
  }, [followingBottom, threadKey, visibleEntries, activity?.running]);

  useEffect(() => () => {
    if (scrollIdleTimer.current !== null) {
      globalThis.clearTimeout(scrollIdleTimer.current);
    }
  }, []);

  useEffect(() => {
    if (!hasRunningActivityBlock) {
      return;
    }
    const timer = window.setInterval(() => setActivityTick((value) => value + 1), 120);
    return () => window.clearInterval(timer);
  }, [hasRunningActivityBlock]);

  return (
    <section className="pevo-panel pevo-transcript" aria-label="Transcript">
      <div
        className={threadItemsClass}
        ref={scrollRef}
        onScroll={(event) => {
          const target = event.currentTarget;
          const atBottom = transcriptAtBottom(target);
          updateFollowingBottom(atBottom);
          rememberCurrentTranscriptScroll(scrollMemoryRef.current, activeThreadKeyRef.current ?? threadKey, target.scrollTop, atBottom);
          setScrolling(true);
          if (scrollIdleTimer.current !== null) {
            globalThis.clearTimeout(scrollIdleTimer.current);
          }
          scrollIdleTimer.current = globalThis.setTimeout(() => {
            setScrolling(false);
            scrollIdleTimer.current = null;
          }, 900);
        }}
      >
        {historyFidelityNotice(history)}
        {visibleEntries.length === 0 ? (
          <div className="pevo-empty pevo-emptyThread">No messages yet</div>
        ) : (
          visibleEntries.map((entry) => (
            <TranscriptEntryView
              activityTick={activityTick}
              entry={entry}
              key={entry.id}
              onCopyText={onCopyText}
              onForkUserMessage={onForkUserMessage}
              onOpenAgentSession={onOpenAgentSession}
              onReadAloudText={onReadAloudText}
              onReadUserMessageDraft={onReadUserMessageDraft}
              onUpdateUserMessage={onUpdateUserMessage}
              workspaceFileLinks={workspaceFileLinks}
            />
          ))
        )}
      </div>
      {!followingBottom && (
        <button
          aria-label="Jump to latest"
          className="pevo-jumpBottom"
          data-tooltip="Jump to latest"
          onClick={() => {
            const scroller = scrollRef.current;
            if (scroller) {
              scrollTranscript(scroller, scroller.scrollHeight);
              rememberCurrentTranscriptScroll(scrollMemoryRef.current, activeThreadKeyRef.current ?? threadKey, scroller.scrollHeight, true);
            }
            updateFollowingBottom(true);
          }}
          title="Jump to latest"
          type="button"
        >
          <ArrowDownToLine size={17} aria-hidden />
        </button>
      )}
    </section>
  );
}

function historyFidelityNotice(history: TranscriptHistoryView | null | undefined) {
  if (!history || history.owner === "psychevo" || history.fidelity === "full") {
    return null;
  }
  const ownerLabel = history.owner === "agent" ? "agent" : "process";
  const message = history.hint?.trim() || (() => {
    switch (history.fidelity) {
      case "summary": return `Only a summarized history is available from this ${ownerLabel}.`;
      case "partial": return `Only part of the history is available from this ${ownerLabel}.`;
      case "unavailable": return `Earlier history is unavailable from this ${ownerLabel}.`;
      default: return `This ${ownerLabel} owns the session history.`;
    }
  })();
  return (
    <div
      className="pevo-historyNotice"
      data-history-fidelity={history.fidelity}
      data-history-owner={history.owner}
      role="note"
    >
      {message}
    </div>
  );
}

function transcriptThreadKey(threadId: string | null | undefined, entries: TranscriptEntry[]): string | null {
  const explicit = threadId?.trim();
  if (explicit) {
    return explicit;
  }
  for (const entry of entries) {
    if (entry.threadId.trim()) {
      return entry.threadId;
    }
  }
  return null;
}

function transcriptAtBottom(scroller: HTMLElement): boolean {
  return scroller.scrollHeight - scroller.scrollTop - scroller.clientHeight < BOTTOM_THRESHOLD_PX;
}

function scrollTranscript(scroller: HTMLElement, top: number): void {
  scroller.scrollTo({ top, behavior: "auto" });
}

function clampScrollTop(scroller: HTMLElement, top: number): number {
  const maxTop = Math.max(0, scroller.scrollHeight - scroller.clientHeight);
  return Math.min(Math.max(0, top), maxTop);
}

function recallTranscriptScroll(
  memory: Map<string, TranscriptScrollMemory>,
  threadKey: string
): TranscriptScrollMemory | null {
  const remembered = memory.get(threadKey);
  if (!remembered) {
    return null;
  }
  memory.delete(threadKey);
  memory.set(threadKey, remembered);
  return remembered;
}

function rememberCurrentTranscriptScroll(
  memory: Map<string, TranscriptScrollMemory>,
  threadKey: string | null,
  top: number,
  atBottom: boolean
): void {
  if (!threadKey) {
    return;
  }
  memory.delete(threadKey);
  memory.set(threadKey, { atBottom, top });
  while (memory.size > TRANSCRIPT_SCROLL_MEMORY_LIMIT) {
    const oldest = memory.keys().next().value;
    if (typeof oldest !== "string") {
      break;
    }
    memory.delete(oldest);
  }
}

function orderTranscriptEntries(entries: TranscriptEntry[]): TranscriptEntry[] {
  return [...entries].sort((left, right) => {
    if (left.messageSeq !== null && right.messageSeq !== null && left.messageSeq !== right.messageSeq) {
      return left.messageSeq - right.messageSeq;
    }
    if (left.messageSeq !== right.messageSeq) {
      const timelineComparison = compareTimelineMs(left, right);
      if (timelineComparison !== 0) {
        return timelineComparison;
      }
      return left.messageSeq !== null ? -1 : 1;
    }
    const sameTurn = Boolean(left.turnId) && left.turnId === right.turnId;
    if (sameTurn) {
      const leftLiveOrder = liveOrder(left);
      const rightLiveOrder = liveOrder(right);
      if (leftLiveOrder !== null && rightLiveOrder !== null && leftLiveOrder !== rightLiveOrder) {
        return leftLiveOrder - rightLiveOrder;
      }
      if (leftLiveOrder !== null && rightLiveOrder === null) {
        return -1;
      }
      if (leftLiveOrder === null && rightLiveOrder !== null) {
        return 1;
      }
    }
    if (left.createdAtMs !== right.createdAtMs) {
      return left.createdAtMs - right.createdAtMs;
    }
    return left.id.localeCompare(right.id);
  });
}

function visibleBlocks(entry: TranscriptEntry): TranscriptBlock[] {
  if (isHiddenTranscriptEntry(entry)) {
    return [];
  }
  return transcriptBlocks(entry)
    .sort((left, right) => {
      if (left.order !== right.order) {
        return left.order - right.order;
      }
      if (left.createdAtMs !== right.createdAtMs) {
        return left.createdAtMs - right.createdAtMs;
      }
      return left.id.localeCompare(right.id);
    })
    .filter((block) => !isHiddenTranscriptBlock(entry, block));
}

function visibleTranscriptEntries(entries: TranscriptEntry[]): TranscriptEntry[] {
  const visibleEntries: TranscriptEntry[] = [];
  let previousGeneratedImage: GeneratedImageArtifact | null = null;
  for (const entry of entries) {
    const blocks = visibleBlocks(entry);
    const nextBlocks: TranscriptBlock[] = [];
    for (const block of blocks) {
      if (isDuplicateGeneratedImageProse(block, previousGeneratedImage)) {
        continue;
      }
      nextBlocks.push(block);
      const generatedImage = generatedImageArtifact(block);
      if (generatedImage) {
        previousGeneratedImage = generatedImage;
      } else if (transcriptBlockText(block).trim()) {
        previousGeneratedImage = null;
      }
    }
    if (nextBlocks.length > 0) {
      visibleEntries.push(nextBlocks.length === entry.blocks.length ? entry : { ...entry, blocks: nextBlocks });
    }
  }
  return visibleEntries;
}

function isHiddenTranscriptBlock(entry: TranscriptEntry, block: TranscriptBlock): boolean {
  if (metadataHidden(block.metadata)) {
    return true;
  }
  if (block.kind === "reasoning") {
    return !transcriptBlockText(block).trim();
  }
  if (block.kind === "text" && (entry.role === "user" || entry.role === "assistant")) {
    return !transcriptBlockText(block).trim();
  }
  return false;
}

function isHiddenTranscriptEntry(entry: TranscriptEntry): boolean {
  return metadataHidden(entry.metadata);
}

function metadataHidden(metadata: unknown): boolean {
  return asRecord(metadata).hidden === true || sideInheritedMetadataHidden(metadata);
}

function TranscriptEntryView({
  activityTick,
  entry,
  onCopyText,
  onForkUserMessage,
  onOpenAgentSession,
  onReadAloudText,
  onReadUserMessageDraft,
  onUpdateUserMessage,
  workspaceFileLinks
}: {
  activityTick: number;
  entry: TranscriptEntry;
  onCopyText: CopyTextHandler;
  onForkUserMessage: MutateUserMessageHandler;
  onOpenAgentSession: OpenAgentSessionHandler;
  onReadAloudText: ReadAloudTextHandler;
  onReadUserMessageDraft: ReadUserMessageDraftHandler;
  onUpdateUserMessage: MutateUserMessageHandler;
  workspaceFileLinks: WorkspaceFileLinkContext | undefined;
}) {
  const blocks = visibleBlocks(entry);
  const phaseOrdinals = Array.from(new Set(blocks.flatMap((block) => (
    Number.isInteger(block.phaseOrdinal) && Number(block.phaseOrdinal) > 0
      ? [Number(block.phaseOrdinal)]
      : []
  ))));
  const hasNativePhases = phaseOrdinals.length > 1;
  const [showNativePhases, setShowNativePhases] = useState(false);
  const [editor, setEditor] = useState<ThreadHistoryDraftReadResult | null>(null);
  const [editorLoading, setEditorLoading] = useState(false);
  const [editorError, setEditorError] = useState<string | null>(null);
  const canEdit = entry.role === "user"
    && entry.status === "completed"
    && entry.messageSeq !== null
    && Boolean(onReadUserMessageDraft && onUpdateUserMessage && onForkUserMessage);
  async function beginEdit() {
    if (!canEdit || !onReadUserMessageDraft) return;
    setEditorLoading(true);
    setEditorError(null);
    try {
      const next = await onReadUserMessageDraft(entry);
      if (next.unavailableReason) {
        setEditorError(next.unavailableReason);
        return;
      }
      setEditor(next);
    } catch (cause) {
      setEditorError(cause instanceof Error ? cause.message : String(cause));
    } finally {
      setEditorLoading(false);
    }
  }
  const renderedBlocks = (items: TranscriptBlock[]) => items.map((block) => (
    <TranscriptBlockView
      activityTick={activityTick}
      block={block}
      entry={entry}
      key={block.id}
      onCopyText={onCopyText}
      onEditUserMessage={canEdit ? beginEdit : undefined}
      onOpenAgentSession={onOpenAgentSession}
      onReadAloudText={onReadAloudText}
      workspaceFileLinks={workspaceFileLinks}
    />
  ));
  if (editor) {
    return (
      <HistoryMessageEditor
        draft={editor}
        error={editorError}
        onCancel={() => {
          setEditor(null);
          setEditorError(null);
        }}
        onFork={async (draft) => {
          try {
            await onForkUserMessage?.(entry, draft);
            setEditor(null);
          } catch (cause) {
            setEditorError(cause instanceof Error ? cause.message : String(cause));
          }
        }}
        onUpdate={async (draft) => {
          try {
            await onUpdateUserMessage?.(entry, draft);
            setEditor(null);
          } catch (cause) {
            setEditorError(cause instanceof Error ? cause.message : String(cause));
          }
        }}
      />
    );
  }
  if (editorLoading || editorError) {
    return (
      <div className="pevo-messageFrame is-user">
        <article className="pevo-message is-user pevo-historyEditorStatus" role={editorError ? "alert" : "status"}>
          {editorError ?? "Loading editable message…"}
        </article>
      </div>
    );
  }
  if (!hasNativePhases) {
    return <>{renderedBlocks(blocks)}</>;
  }
  return (
    <div className="pevo-nativePhases">
      <button
        aria-expanded={showNativePhases}
        className="pevo-nativePhasesToggle"
        onClick={() => setShowNativePhases((current) => !current)}
        type="button"
      >
        {showNativePhases ? "Hide native phases" : "Show native phases"}
      </button>
      {showNativePhases
        ? nativePhaseGroups(blocks).map((group, index) => group.ordinal == null ? (
          <div className="pevo-nativePhaseUngrouped" key={`unphased:${index}`}>
            {renderedBlocks(group.blocks)}
          </div>
        ) : (
          <section className="pevo-nativePhase" key={`phase:${group.ordinal}:${index}`} aria-label={`Phase ${group.ordinal}`}>
            <header>Phase {group.ordinal}</header>
            {renderedBlocks(group.blocks)}
          </section>
        ))
        : renderedBlocks(blocks)}
    </div>
  );
}

function nativePhaseGroups(blocks: TranscriptBlock[]): Array<{ ordinal: number | null; blocks: TranscriptBlock[] }> {
  return blocks.reduce<Array<{ ordinal: number | null; blocks: TranscriptBlock[] }>>((groups, block) => {
    const ordinal = Number.isInteger(block.phaseOrdinal) && Number(block.phaseOrdinal) > 0
      ? Number(block.phaseOrdinal)
      : null;
    const previous = groups.at(-1);
    if (previous?.ordinal === ordinal) {
      previous.blocks.push(block);
      return groups;
    }
    groups.push({ ordinal, blocks: [block] });
    return groups;
  }, []);
}

function TranscriptBlockView({
  activityTick,
  block,
  entry,
  onCopyText,
  onEditUserMessage,
  onOpenAgentSession,
  onReadAloudText,
  workspaceFileLinks
}: {
  activityTick: number;
  block: TranscriptBlock;
  entry: TranscriptEntry;
  onCopyText: CopyTextHandler;
  onEditUserMessage: (() => void | Promise<void>) | undefined;
  onOpenAgentSession: OpenAgentSessionHandler;
  onReadAloudText: ReadAloudTextHandler;
  workspaceFileLinks: WorkspaceFileLinkContext | undefined;
}) {
  const text = transcriptBlockText(block);
  const display = evidenceDisplay(block, text);
  const shouldDefaultOpen = defaultBlockOpen(block, display);
  const [open, setOpen] = useState(shouldDefaultOpen);
  const [copied, setCopied] = useState(false);
  const wasRunningReasoningRef = useRef(block.kind === "reasoning" && block.status === "running");
  const status = statusLabel(block);
  const artifactIds = transcriptArtifactIds(block);
  useEffect(() => {
    if (shouldDefaultOpen) {
      setOpen(true);
    }
  }, [block.id, shouldDefaultOpen]);
  useEffect(() => {
    const runningReasoning = block.kind === "reasoning" && block.status === "running";
    if (wasRunningReasoningRef.current && !runningReasoning) {
      setOpen(false);
    }
    wasRunningReasoningRef.current = runningReasoning;
  }, [block.id, block.kind, block.status]);

  if (block.kind === "text" && entry.role === "user") {
    return (
      <div className="pevo-messageFrame is-user">
        <article className="pevo-message is-user" {...transcriptBlockDataAttributes(entry, block)}>
          <MarkdownText text={text} />
        </article>
        {(onCopyText || onEditUserMessage) && (
          <MessageMeta
            block={block}
            copied={copied}
            showElapsed={false}
            {...(onCopyText ? { onCopy: async () => {
              try {
                await onCopyText(text);
                setCopied(true);
                globalThis.setTimeout(() => setCopied(false), 1_200);
              } catch {
                setCopied(false);
              }
            }} : {})}
            {...(onEditUserMessage ? { onEdit: onEditUserMessage } : {})}
          />
        )}
      </div>
    );
  }
  if (block.kind === "text" && entry.role === "assistant") {
    return (
      <div className="pevo-messageFrame is-assistant">
        <article
          className={`pevo-message is-assistant ${block.status === "running" ? "is-streaming" : ""}`}
          {...transcriptBlockDataAttributes(entry, block)}
        >
          <MarkdownText
            streaming={block.status === "running"}
            text={text}
            {...(workspaceFileLinks ? { workspaceFileLinks } : {})}
          />
        </article>
        {(onCopyText || onReadAloudText) && (
          <MessageMeta
            block={block}
            copied={copied}
            onReadAloud={onReadAloudText ? async () => onReadAloudText(text) : undefined}
            showElapsed
            onCopy={onCopyText ? async () => {
              try {
                await onCopyText(text);
                setCopied(true);
                globalThis.setTimeout(() => setCopied(false), 1_200);
              } catch {
                setCopied(false);
              }
            } : undefined}
          />
        )}
      </div>
    );
  }
  if (block.kind === "reasoning") {
    const runningReasoning = block.status === "running";
    const runningElapsed = runningReasoning ? liveBlockElapsed(block) : null;
    return (
      <article
        className={`pevo-reasoning ${block.status === "running" ? "is-streaming" : ""}`}
        data-has-body={text.trim() ? "true" : "false"}
        {...transcriptBlockDataAttributes(entry, block)}
      >
        <button
          aria-expanded={open}
          className={`pevo-reasoningHeader ${runningReasoning ? "is-runningActivity" : ""}`}
          onClick={() => setOpen((value) => !value)}
          type="button"
        >
          {runningReasoning ? (
            <span className="pevo-evidenceSpinner" aria-hidden="true">
              {ACTIVITY_SPINNER[activityTick % ACTIVITY_SPINNER.length]}
            </span>
          ) : (
            open ? <ChevronDown size={15} aria-hidden /> : <ChevronRight size={15} aria-hidden />
          )}
          <span>{reasoningTitle()}</span>
          {runningElapsed ? <em className="pevo-evidenceElapsed">{runningElapsed}</em> : status && <em>{status}</em>}
        </button>
        {open && <MarkdownText streaming={block.status === "running"} text={text} />}
      </article>
    );
  }
  const webCitation = webCitationFromBlock(block);
  if (webCitation) {
    return (
      <article className="pevo-webCitation" {...transcriptBlockDataAttributes(entry, block)}>
        <ExternalLink size={13} aria-hidden />
        <a href={webCitation.url} rel="noreferrer" target="_blank">{webCitation.title || webCitation.url}</a>
      </article>
    );
  }
  const webImage = webImageFromBlock(block);
  if (webImage) return <WebSearchImageView block={block} entry={entry} image={webImage} />;
  const generatedImage = generatedImageArtifact(block);
  if (generatedImage) {
    return (
      <GeneratedImageArtifactView
        artifact={generatedImage}
        block={block}
        entry={entry}
      />
    );
  }
  const agentSession = agentSessionFromBlock(block, display);
  const canOpenAgentSession = Boolean(agentSession && onOpenAgentSession);
  const workspaceFileTarget = workspaceFileLinks
    ? completedToolWorkspaceFileTarget(block, workspaceFileLinks)
    : null;
  const runningTool = isRunningToolActivityBlock(block);
  const elapsed = runningTool ? liveToolBlockElapsed(block) : transcriptToolBlockElapsed(block);
  const evidenceLineClass = [
    "pevo-evidenceLine",
    display.singleTitle ? "is-singleTitle" : "",
    runningTool ? "is-runningTool" : "",
    canOpenAgentSession || workspaceFileTarget ? "is-openTarget" : ""
  ].filter(Boolean).join(" ");
  const lineButton = (
    <button
      aria-expanded={display.sections.length > 0 ? open : undefined}
      className={evidenceLineClass}
      onClick={() => setOpen((value) => !value)}
      type="button"
    >
      {runningTool ? (
        <span className="pevo-evidenceSpinner" aria-hidden="true">
          {ACTIVITY_SPINNER[activityTick % ACTIVITY_SPINNER.length]}
        </span>
      ) : (
        open ? <ChevronDown size={15} aria-hidden /> : <ChevronRight size={15} aria-hidden />
      )}
      <code title={display.title}>{display.title}</code>
      {display.summary && <span>{display.summary}</span>}
      {elapsed ? <em className="pevo-evidenceElapsed">{elapsed}</em> : !runningTool && status && <em>{status}</em>}
    </button>
  );
  return (
    <article className={`pevo-evidence is-${block.status} is-tool-${display.category}`} {...transcriptBlockDataAttributes(entry, block)}>
      {canOpenAgentSession && agentSession ? (
        <div className="pevo-evidenceActionRow">
          {lineButton}
          <button
            aria-label={`Open ${agentSession.title ?? display.title} agent session`}
            className="pevo-evidenceOpenButton"
            onClick={() => {
              onOpenAgentSession?.(agentSession);
            }}
            title="Open agent session"
            type="button"
          >
            <ExternalLink size={13} aria-hidden />
            <span>Open</span>
          </button>
        </div>
      ) : workspaceFileTarget && workspaceFileLinks ? (
        <div className="pevo-evidenceActionRow">
          {lineButton}
          <button
            aria-label={`Open file ${workspaceFileTarget.label}`}
            className="pevo-evidenceOpenButton"
            onClick={() => {
              void workspaceFileLinks.onOpen(workspaceFileTarget.path);
            }}
            title="Open file preview"
            type="button"
          >
            <ExternalLink size={13} aria-hidden />
            <span>Open</span>
          </button>
        </div>
      ) : lineButton}
      {open && display.sections.length > 0 && <ToolDetail display={display} />}
      {artifactIds.length > 0 && (
        <div className="pevo-artifactRefs">
          {artifactIds.map((artifactId) => <span key={artifactId}>{artifactId}</span>)}
        </div>
      )}
    </article>
  );
}

function completedToolWorkspaceFileTarget(
  block: TranscriptBlock,
  context: WorkspaceFileLinkContext
): { label: string; path: string } | null {
  const metadata = asRecord(block.metadata);
  const toolName = stringValue(metadata.tool_name ?? metadata.toolName);
  const status = block.result?.status ?? block.status;
  if (
    metadata.projection !== "tool"
    || (toolName !== "read" && toolName !== "edit" && toolName !== "write")
    || status !== "completed"
    || block.result?.isError === true
  ) {
    return null;
  }
  const args = asRecord(metadata.args ?? metadata.arguments);
  const label = stringValue(args.path);
  if (!label) {
    return null;
  }
  const path = resolveWorkspaceFilePath(context, label);
  return path ? { label, path } : null;
}

type WebCitation = { title: string; url: string };
type WebSearchImage = { caption: string | null; imageUrl: string; sourceUrl: string; thumbnailUrl: string | null };

function webCitationFromBlock(block: TranscriptBlock): WebCitation | null {
  const metadata = asRecord(block.metadata);
  if (metadata.projection !== "url_citation") return null;
  const url = stringValue(metadata.url);
  if (!url) return null;
  return { title: stringValue(metadata.title) ?? "", url };
}

function webImageFromBlock(block: TranscriptBlock): WebSearchImage | null {
  const metadata = asRecord(block.metadata);
  if (metadata.projection !== "web_image_source") return null;
  const imageUrl = stringValue(metadata.image_url);
  const sourceUrl = stringValue(metadata.source_website_url);
  if (!imageUrl || !sourceUrl) return null;
  return { caption: stringValue(metadata.caption), imageUrl, sourceUrl, thumbnailUrl: stringValue(metadata.thumbnail_url) };
}

function WebSearchImageView({ block, entry, image }: { block: TranscriptBlock; entry: TranscriptEntry; image: WebSearchImage }) {
  const [expanded, setExpanded] = useState(false);
  const [failed, setFailed] = useState(false);
  const [loaded, setLoaded] = useState(false);
  const src = image.thumbnailUrl ?? image.imageUrl;
  useEffect(() => { setExpanded(false); setFailed(false); setLoaded(false); }, [block.id, src]);
  return (
    <article className="pevo-webImageSource" {...transcriptBlockDataAttributes(entry, block)}>
      <button aria-expanded={expanded} onClick={() => setExpanded((value) => !value)} type="button">
        {expanded ? <ChevronDown size={14} aria-hidden /> : <ChevronRight size={14} aria-hidden />}
        <span>{image.caption ?? "Image result"}</span>
      </button>
      <a href={image.sourceUrl} rel="noreferrer" target="_blank">Source</a>
      {expanded && !failed && <div className={`pevo-webImageThumbnail ${loaded ? "is-loaded" : "is-loading"}`}>
        {!loaded && <span>Loading image…</span>}
        <img alt={image.caption ?? "Web search image result"} onError={() => setFailed(true)} onLoad={() => setLoaded(true)} src={src} />
      </div>}
      {expanded && failed && <p>Image preview unavailable.</p>}
    </article>
  );
}

type GeneratedImageArtifact = {
  agentVisibleSource: string | null;
  artifactId: string | null;
  display: string | null;
  displayUrl: string | null;
  error: string | null;
  height: number | null;
  mimeType: string | null;
  model: string | null;
  phase: "pending" | "loaded" | "failed";
  prompt: string | null;
  provider: string | null;
  revisedPrompt: string | null;
  savedPath: string | null;
  width: number | null;
};

function GeneratedImageArtifactView({
  artifact,
  block,
  entry
}: {
  artifact: GeneratedImageArtifact;
  block: TranscriptBlock;
  entry: TranscriptEntry;
}) {
  const [lightboxOpen, setLightboxOpen] = useState(false);
  const [loaded, setLoaded] = useState(false);
  const imageVisible = Boolean(artifact.displayUrl && artifact.phase !== "failed");
  const frameStyle: CSSProperties = { aspectRatio: generatedImageAspectRatio(artifact) };
  const title = artifact.display ?? "Generated image";
  const providerModel = [artifact.provider, artifact.model].filter(Boolean).join(" / ");
  const downloadName = generatedImageDownloadName(artifact);

  useEffect(() => {
    setLoaded(false);
    setLightboxOpen(false);
  }, [artifact.displayUrl, block.id]);

  return (
    <article
      className={`pevo-generatedImageArtifact is-${artifact.phase}`}
      {...transcriptBlockDataAttributes(entry, block)}
    >
      <div className="pevo-generatedImageFrame" style={frameStyle}>
        {imageVisible && artifact.displayUrl ? (
          <button
            aria-label="Preview generated image"
            className="pevo-generatedImageButton"
            onClick={() => setLightboxOpen(true)}
            type="button"
          >
            <img
              alt={artifact.prompt ? `Generated image: ${artifact.prompt}` : "Generated image"}
              className={loaded ? "is-loaded" : ""}
              onLoad={() => setLoaded(true)}
              src={artifact.displayUrl}
            />
          </button>
        ) : (
          <div className="pevo-generatedImagePlaceholder">
            <ImageIcon size={22} aria-hidden />
            <span>{artifact.phase === "failed" ? "Image unavailable" : "Generating"}</span>
          </div>
        )}
      </div>
      <div className="pevo-generatedImageMeta">
        <div>
          <strong>{title}</strong>
          {artifact.prompt && <span>{artifact.prompt}</span>}
          {artifact.savedPath && <code>{artifact.savedPath}</code>}
          {artifact.phase === "failed" && artifact.error && <em>{artifact.error}</em>}
        </div>
        <div className="pevo-generatedImageActions" aria-label="Generated image actions">
          {imageVisible && artifact.displayUrl && (
            <>
              <button
                aria-label="Open generated image preview"
                onClick={() => setLightboxOpen(true)}
                title="Preview"
                type="button"
              >
                <Maximize2 size={14} aria-hidden />
              </button>
              <a
                aria-label="Download generated image"
                download={downloadName}
                href={artifact.displayUrl}
                title="Download"
              >
                <Download size={14} aria-hidden />
              </a>
              <a
                aria-label="Open generated image"
                href={artifact.displayUrl}
                rel="noreferrer"
                target="_blank"
                title="Open"
              >
                <ExternalLink size={14} aria-hidden />
              </a>
            </>
          )}
        </div>
      </div>
      {providerModel && <small className="pevo-generatedImageProvider">{providerModel}</small>}
      {lightboxOpen && imageVisible && artifact.displayUrl && (
        <div
          aria-label="Generated image preview"
          aria-modal="true"
          className="pevo-generatedImageLightbox"
          onClick={() => setLightboxOpen(false)}
          role="dialog"
        >
          <button
            aria-label="Close image preview"
            className="pevo-generatedImageLightboxClose"
            onClick={() => setLightboxOpen(false)}
            type="button"
          >
            <X size={18} aria-hidden />
          </button>
          <img
            alt={artifact.prompt ? `Generated image: ${artifact.prompt}` : "Generated image"}
            onClick={(event) => event.stopPropagation()}
            src={artifact.displayUrl}
          />
        </div>
      )}
    </article>
  );
}

function generatedImageArtifact(block: TranscriptBlock): GeneratedImageArtifact | null {
  const metadata = asRecord(block.metadata);
  const result = generatedImageResultRecord(block, metadata);
  const toolName = stringValue(metadata.tool_name ?? metadata.toolName ?? result.tool_name ?? result.toolName);
  const mediaKind = stringValue(result.mediaKind ?? result.media_kind ?? metadata.mediaKind ?? metadata.media_kind);
  const isGenerated = mediaKind === "generated_image" || imageGenerationToolName(toolName);
  if (!isGenerated || (block.kind !== "artifact" && !imageGenerationToolName(toolName))) {
    return null;
  }
  const artifactIds = transcriptArtifactIds(block);
  const artifactId = firstStringField([result, metadata], ["artifactId", "artifact_id"]) ?? artifactIds[0] ?? null;
  const displayUrl = firstStringField([result, metadata], ["displayUrl", "display_url", "hostUrl", "host_url"]);
  const savedPath = firstStringField([result, metadata], ["savedPath", "saved_path", "path"]);
  const phase = generatedImagePhase(block, result, displayUrl, artifactId);
  return {
    agentVisibleSource: firstStringField([result, metadata], ["agentVisibleSource", "agent_visible_source"]),
    artifactId,
    display: firstStringField([result, metadata], ["display", "title"]),
    displayUrl,
    error: firstStringField([result, metadata], ["error", "message"]),
    height: firstNumberField([result, metadata], ["height"]),
    mimeType: firstStringField([result, metadata], ["mimeType", "mime_type"]),
    model: firstStringField([result, metadata], ["model"]),
    phase,
    prompt: firstStringField([result, metadata, asRecord(metadata.args ?? metadata.arguments)], ["prompt"]),
    provider: firstStringField([result, metadata], ["provider"]),
    revisedPrompt: firstStringField([result, metadata], ["revisedPrompt", "revised_prompt"]),
    savedPath,
    width: firstNumberField([result, metadata], ["width"])
  };
}

function generatedImageResultRecord(block: TranscriptBlock, metadata: Record<string, unknown>): Record<string, unknown> {
  const result = asRecord(metadata.result);
  if (Object.keys(result).length > 0) {
    return result;
  }
  const resultMetadata = asRecord(block.result?.metadata);
  const nestedResult = asRecord(resultMetadata.result);
  if (Object.keys(nestedResult).length > 0) {
    return nestedResult;
  }
  const contentResult = jsonRecord(block.result?.content);
  if (Object.keys(contentResult).length > 0) {
    return contentResult;
  }
  return jsonRecord(block.body);
}

function generatedImagePhase(
  block: TranscriptBlock,
  result: Record<string, unknown>,
  displayUrl: string | null,
  artifactId: string | null
): GeneratedImageArtifact["phase"] {
  const status = (stringValue(result.status) ?? block.status).toLowerCase();
  if (block.status === "failed" || status === "failed" || status === "error") {
    return "failed";
  }
  if (displayUrl || artifactId) {
    return "loaded";
  }
  return "pending";
}

function generatedImageAspectRatio(artifact: GeneratedImageArtifact): string {
  if (artifact.width && artifact.height && artifact.width > 0 && artifact.height > 0) {
    return `${artifact.width} / ${artifact.height}`;
  }
  return "1 / 1";
}

function generatedImageDownloadName(artifact: GeneratedImageArtifact): string {
  const extension = imageExtensionForMime(artifact.mimeType) ?? extensionFromPath(artifact.savedPath) ?? "png";
  return `${artifact.artifactId ?? "generated-image"}.${extension}`;
}

function imageExtensionForMime(mimeType: string | null): string | null {
  switch (mimeType) {
    case "image/jpeg":
      return "jpg";
    case "image/png":
      return "png";
    case "image/webp":
      return "webp";
    case "image/gif":
      return "gif";
    default:
      return null;
  }
}

function extensionFromPath(path: string | null): string | null {
  const match = path?.match(/\.([a-z0-9]+)(?:$|[?#])/i);
  const extension = match?.[1];
  return extension ? extension.toLowerCase() : null;
}

function imageGenerationToolName(toolName: string | null): boolean {
  return toolName === "image_generate"
    || toolName === "image_generation.generate"
    || toolName === "image_generation__generate";
}

function firstNumberField(records: Array<Record<string, unknown>>, keys: string[]): number | null {
  for (const record of records) {
    for (const key of keys) {
      const value = record[key];
      if (typeof value === "number" && Number.isFinite(value)) {
        return value;
      }
    }
  }
  return null;
}

function isDuplicateGeneratedImageProse(block: TranscriptBlock, artifact: GeneratedImageArtifact | null): boolean {
  if (!artifact || block.kind !== "text") {
    return false;
  }
  const text = normalizeGeneratedImageProse(transcriptBlockText(block));
  if (!text) {
    return false;
  }
  const duplicateValues = [
    artifact.savedPath,
    artifact.displayUrl,
    artifact.agentVisibleSource
  ].map(normalizeGeneratedImageProse).filter((value): value is string => Boolean(value));
  return duplicateValues.some((value) => text === value || text === `saved ${value}` || text === `saved to ${value}`);
}

function normalizeGeneratedImageProse(value: string | null | undefined): string | null {
  const trimmed = value?.trim();
  if (!trimmed) {
    return null;
  }
  return trimmed
    .replace(/^`+|`+$/g, "")
    .replace(/^saved(?: image)?(?::|\s+to)\s+/i, "")
    .replace(/^generated image(?: saved)?(?::|\s+to)\s+/i, "")
    .trim()
    .toLowerCase();
}

function agentSessionFromBlock(block: TranscriptBlock, display: EvidenceDisplay): TranscriptAgentSession | null {
  if (block.kind !== "agent") {
    return null;
  }
  const metadata = asRecord(block.metadata);
  const metadataResult = asRecord(metadata.result);
  const metadataResultChildSession = asRecord(metadataResult.child_session ?? metadataResult.childSession);
  const resultMetadata = asRecord(block.result?.metadata);
  const resultMetadataResult = asRecord(resultMetadata.result);
  const resultMetadataResultChildSession = asRecord(
    resultMetadataResult.child_session ?? resultMetadataResult.childSession
  );
  const resultMetadataChildSession = asRecord(resultMetadata.child_session ?? resultMetadata.childSession);
  const blockResult = asRecord(block.result);
  const blockResultContent = jsonRecord(block.result?.content);
  const blockBody = jsonRecord(block.body);
  const records = [
    metadata,
    metadataResult,
    metadataResultChildSession,
    resultMetadata,
    resultMetadataResult,
    resultMetadataResultChildSession,
    resultMetadataChildSession,
    blockResult,
    blockResultContent,
    blockBody
  ];
  const childSessionId = firstStringField(
    records,
    ["child_thread_id", "childThreadId", "child_session_id", "childSessionId", "session_id", "sessionId"]
  );
  if (!childSessionId) {
    return null;
  }
  const agentName = firstStringField(records, ["agent_name", "agentName", "name"]);
  const taskName = firstStringField(records, ["task_name", "taskName"]);
  return {
    agentName,
    childSessionId,
    parentSessionId: firstStringField(records, ["parent_thread_id", "parentThreadId", "parent_session_id", "parentSessionId"]),
    task: firstStringField(records, ["message", "task", "prompt"]),
    taskName,
    title: taskName ?? agentName ?? display.title,
  };
}

function firstStringField(records: Array<Record<string, unknown>>, keys: string[]): string | null {
  for (const record of records) {
    for (const key of keys) {
      const value = stringValue(record[key]);
      if (value) {
        return value;
      }
    }
  }
  return null;
}

function jsonRecord(value: unknown): Record<string, unknown> {
  if (typeof value !== "string") {
    return {};
  }
  try {
    return asRecord(JSON.parse(value));
  } catch {
    return {};
  }
}

function MessageMeta({
  block,
  copied,
  onCopy,
  onEdit,
  onReadAloud,
  showElapsed
}: {
  block: TranscriptBlock;
  copied: boolean;
  onCopy?: (() => void | Promise<void>) | undefined;
  onEdit?: (() => void | Promise<void>) | undefined;
  onReadAloud?: (() => void | Promise<void>) | undefined;
  showElapsed: boolean;
}) {
  const timestamp = transcriptBlockTimestamp(block);
  const elapsed = showElapsed ? transcriptBlockElapsed(block) : null;
  return (
    <div className="pevo-messageMeta" aria-label="Message actions">
      {onEdit && (
        <button className="pevo-messageCopy" onClick={() => void onEdit()} title="Edit message" type="button">
          <Pencil size={14} aria-hidden />
          <span className="pevo-srOnly">Edit this message in the same thread or fork a new thread</span>
        </button>
      )}
      {onCopy && (
        <button className="pevo-messageCopy" onClick={() => void onCopy()} title="Copy" type="button">
          {copied ? <Check size={14} aria-hidden /> : <Copy size={14} aria-hidden />}
          <span className="pevo-srOnly">{copied ? "Copied" : "Copy message"}</span>
        </button>
      )}
      {onReadAloud && (
        <button className="pevo-messageCopy" onClick={() => void onReadAloud()} title="Read aloud" type="button">
          <Volume2 size={14} aria-hidden />
          <span className="pevo-srOnly">Read aloud</span>
        </button>
      )}
      {elapsed && <span className="pevo-messageElapsed">{elapsed}</span>}
      {timestamp && (
        <time className="pevo-messageTime" dateTime={timestamp.iso}>
          {timestamp.label}
        </time>
      )}
    </div>
  );
}

function HistoryMessageEditor({
  draft,
  error,
  onCancel,
  onFork,
  onUpdate
}: {
  draft: ThreadHistoryDraftReadResult;
  error: string | null;
  onCancel(): void;
  onFork(draft: ThreadEditableDraft): void | Promise<void>;
  onUpdate(draft: ThreadEditableDraft): void | Promise<void>;
}) {
  const [parts, setParts] = useState(() => draft.parts.map((part) => ({ ...part })));
  const [pending, setPending] = useState<"update" | "fork" | null>(null);
  const hasPayload = parts.some((part) => part.type === "image" || part.text.trim());
  async function commit(kind: "update" | "fork") {
    if (!hasPayload || pending) return;
    setPending(kind);
    try {
      const next = { parts } satisfies ThreadEditableDraft;
      if (kind === "update") await onUpdate(next);
      else await onFork(next);
    } finally {
      setPending(null);
    }
  }
  return (
    <div className="pevo-messageFrame is-user">
      <article className="pevo-message is-user pevo-historyEditor" aria-label="Edit user message">
        <div className="pevo-historyEditorParts">
          {parts.map((part, index) => part.type === "text" ? (
            <textarea
              aria-label={`Message text ${index + 1}`}
              autoFocus={index === 0}
              disabled={Boolean(pending)}
              key={`text:${index}`}
              onChange={(event) => setParts((current) => current.map((candidate, candidateIndex) => (
                candidateIndex === index && candidate.type === "text"
                  ? { ...candidate, text: event.target.value }
                  : candidate
              )))}
              rows={Math.max(2, Math.min(10, part.text.split("\n").length + 1))}
              value={part.text}
            />
          ) : (
            <div className="pevo-historyEditorImage" key={`image:${index}`}>
              <ImageIcon size={15} aria-hidden />
              <span>{part.input.kind === "localPath" ? part.input.path : part.input.url}</span>
            </div>
          ))}
        </div>
        {draft.warning && <p className="pevo-historyEditorWarning" role="note">{draft.warning}</p>}
        {error && <p className="pevo-historyEditorError" role="alert">{error}</p>}
        <div className="pevo-historyEditorActions">
          <button disabled={Boolean(pending)} onClick={onCancel} type="button">Cancel</button>
          <button
            aria-label="Fork a new thread before this message"
            disabled={!hasPayload || Boolean(pending)}
            onClick={() => void commit("fork")}
            type="button"
          >
            <GitFork size={14} aria-hidden /> {pending === "fork" ? "Forking…" : "Fork"}
          </button>
          <button
            aria-label="Update this message and run in the same thread"
            disabled={!hasPayload || Boolean(pending)}
            onClick={() => void commit("update")}
            type="button"
          >
            {pending === "update" ? "Updating…" : "Update & run"}
          </button>
        </div>
      </article>
    </div>
  );
}

function transcriptBlocks(entry: TranscriptEntry): TranscriptBlock[] {
  return Array.isArray(entry.blocks) ? entry.blocks : [];
}

function transcriptArtifactIds(block: TranscriptBlock): string[] {
  return Array.isArray(block.artifactIds) ? block.artifactIds : [];
}

function transcriptBlockText(block: TranscriptBlock): string {
  return block.body ?? block.detail ?? block.preview ?? "";
}

function transcriptBlockTimestamp(block: TranscriptBlock): { iso: string; label: string } | null {
  const value = block.updatedAtMs || block.createdAtMs;
  if (!Number.isFinite(value) || value <= 0) {
    return null;
  }
  const date = new Date(value);
  return {
    iso: date.toISOString(),
    label: new Intl.DateTimeFormat(undefined, {
      hour: "2-digit",
      minute: "2-digit"
    }).format(date)
  };
}

function transcriptBlockElapsed(block: TranscriptBlock): string | null {
  return transcriptBlockElapsedWithThreshold(block, 0);
}

function transcriptToolBlockElapsed(block: TranscriptBlock): string | null {
  return transcriptBlockElapsedWithThreshold(block, 1_000);
}

function transcriptBlockElapsedWithThreshold(block: TranscriptBlock, minVisibleMs: number): string | null {
  const metadata = asRecord(block.metadata);
  const resultMetadata = asRecord(metadata.result_metadata);
  const blockResultMetadata = asRecord(block.result?.metadata);
  const messageMetadata = asRecord(metadata.message_metadata);
  const value = metadata.elapsed_ms
    ?? metadata.elapsedMs
    ?? resultMetadata.elapsed_ms
    ?? resultMetadata.elapsedMs
    ?? blockResultMetadata.elapsed_ms
    ?? blockResultMetadata.elapsedMs
    ?? messageMetadata.elapsed_ms
    ?? messageMetadata.elapsedMs;
  const elapsedMs = typeof value === "number" ? value : typeof value === "string" ? Number(value) : NaN;
  if (!Number.isFinite(elapsedMs) || elapsedMs < 0) {
    return null;
  }
  return compactElapsedMs(elapsedMs, minVisibleMs);
}

function liveBlockElapsed(block: TranscriptBlock): string | null {
  return liveBlockElapsedWithThreshold(block, 0);
}

function liveToolBlockElapsed(block: TranscriptBlock): string | null {
  return liveBlockElapsedWithThreshold(block, 1_000);
}

function liveBlockElapsedWithThreshold(block: TranscriptBlock, minVisibleMs: number): string | null {
  const startedAtMs = isPlausibleTimestampMs(block.createdAtMs)
    ? block.createdAtMs
    : block.updatedAtMs;
  const fallbackNowMs = Date.now();
  const effectiveStartedAtMs = isPlausibleTimestampMs(startedAtMs) ? startedAtMs : fallbackNowMs;
  return compactElapsedMs(fallbackNowMs - effectiveStartedAtMs, minVisibleMs);
}

function isPlausibleTimestampMs(value: number): boolean {
  return Number.isFinite(value) && value >= 946_684_800_000;
}

function compactElapsedMs(elapsedMs: number, minVisibleMs = 0): string | null {
  if (elapsedMs < minVisibleMs) {
    return null;
  }
  const seconds = Math.max(0, Math.floor(elapsedMs / 1_000));
  if (seconds < 60) {
    return `${seconds}s`;
  }
  return `${Math.floor(seconds / 60)}m${String(seconds % 60).padStart(2, "0")}s`;
}

function isRunningToolActivityBlock(block: TranscriptBlock): boolean {
  if (block.status !== "running") {
    return false;
  }
  return [
    "tool",
    "toolCall",
    "toolResult",
    "shell",
    "file",
    "web",
    "mcp",
    "agent"
  ].includes(block.kind);
}

function isRunningActivityBlock(block: TranscriptBlock): boolean {
  return block.status === "running" && (block.kind === "reasoning" || isRunningToolActivityBlock(block));
}

function defaultBlockOpen(block: TranscriptBlock, display: EvidenceDisplay): boolean {
  if (display.defaultOpen) {
    return true;
  }
  return block.kind === "reasoning" && block.status === "running";
}

function transcriptBlockDataAttributes(entry: TranscriptEntry, block: TranscriptBlock) {
  return {
    "data-entry-id": entry.id,
    "data-block-id": block.id,
    "data-block-kind": block.kind,
    "data-turn-id": entry.turnId ?? "",
    "data-source": block.source || entry.source,
    "data-phase-ordinal": block.phaseOrdinal ?? ""
  };
}

function reasoningTitle(): string {
  return "Thinking";
}

function statusLabel(block: TranscriptBlock): string | null {
  switch (block.status) {
    case "completed":
      return null;
    case "needsInput":
      return "needs input";
    default:
      return block.status;
  }
}

function liveOrder(entry: TranscriptEntry): number | null {
  const metadata = asRecord(entry.metadata);
  const value = metadata.liveOrder ?? metadata.live_order;
  return typeof value === "number" && Number.isFinite(value) ? value : null;
}

function compareTimelineMs(left: TranscriptEntry, right: TranscriptEntry): number {
  const leftTime = timelineMs(left);
  const rightTime = timelineMs(right);
  return leftTime !== null && rightTime !== null && leftTime !== rightTime ? leftTime - rightTime : 0;
}

function timelineMs(entry: TranscriptEntry): number | null {
  const value = entry.createdAtMs || entry.updatedAtMs;
  return typeof value === "number" && Number.isFinite(value) && value > 0 ? value : null;
}
