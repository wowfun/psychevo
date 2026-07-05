import { ArrowDownToLine, Check, ChevronDown, ChevronRight, Copy, ExternalLink } from "lucide-react";
import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import {
  sideInheritedMetadataHidden,
  type GatewayActivity,
  type TranscriptBlock,
  type TranscriptEntry
} from "@psychevo/protocol";
import { MarkdownText } from "./markdown";
import { asRecord, stringValue } from "./shared";
import { evidenceDisplay, type EvidenceDisplay } from "./toolEvidence";
import { ToolDetail } from "./transcript/tool-detail";

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
  onCopyText?: ((text: string) => void | Promise<void>) | undefined;
  onOpenAgentSession?: ((session: TranscriptAgentSession) => void) | undefined;
  threadId?: string | null;
}

type CopyTextHandler = ((text: string) => void | Promise<void>) | undefined;
type OpenAgentSessionHandler = ((session: TranscriptAgentSession) => void) | undefined;

const ACTIVITY_SPINNER = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧"];
const BOTTOM_THRESHOLD_PX = 48;
const TRANSCRIPT_SCROLL_MEMORY_LIMIT = 64;
const useIsomorphicLayoutEffect = typeof window === "undefined" ? useEffect : useLayoutEffect;

type TranscriptScrollMemory = {
  atBottom: boolean;
  top: number;
};

export function TranscriptPanel({ activity, entries, onCopyText, onOpenAgentSession, threadId }: TranscriptPanelProps) {
  const [followingBottom, setFollowingBottom] = useState(true);
  const [scrolling, setScrolling] = useState(false);
  const [activityTick, setActivityTick] = useState(0);
  const scrollRef = useRef<HTMLDivElement | null>(null);
  const scrollIdleTimer = useRef<ReturnType<typeof globalThis.setTimeout> | null>(null);
  const scrollMemoryRef = useRef<Map<string, TranscriptScrollMemory>>(new Map());
  const activeThreadKeyRef = useRef<string | null>(null);
  const orderedEntries = useMemo(() => orderTranscriptEntries(entries), [entries]);
  const visibleEntries = useMemo(() => orderedEntries.filter((entry) => visibleBlocks(entry).length > 0), [orderedEntries]);
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
        {visibleEntries.length === 0 ? (
          <div className="pevo-empty pevo-emptyThread">No messages yet</div>
        ) : (
          visibleEntries.map((entry) => (
            <TranscriptEntryView
              activityTick={activityTick}
              entry={entry}
              key={entry.id}
              onCopyText={onCopyText}
              onOpenAgentSession={onOpenAgentSession}
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
  onOpenAgentSession
}: {
  activityTick: number;
  entry: TranscriptEntry;
  onCopyText: CopyTextHandler;
  onOpenAgentSession: OpenAgentSessionHandler;
}) {
  return (
    <>
      {visibleBlocks(entry).map((block) => (
        <TranscriptBlockView
          activityTick={activityTick}
          block={block}
          entry={entry}
          key={block.id}
          onCopyText={onCopyText}
          onOpenAgentSession={onOpenAgentSession}
        />
      ))}
    </>
  );
}

function TranscriptBlockView({
  activityTick,
  block,
  entry,
  onCopyText,
  onOpenAgentSession
}: {
  activityTick: number;
  block: TranscriptBlock;
  entry: TranscriptEntry;
  onCopyText: CopyTextHandler;
  onOpenAgentSession: OpenAgentSessionHandler;
}) {
  const text = transcriptBlockText(block);
  const display = evidenceDisplay(block, text);
  const shouldDefaultOpen = defaultBlockOpen(block, display);
  const [open, setOpen] = useState(shouldDefaultOpen);
  const [copied, setCopied] = useState(false);
  const status = statusLabel(block);
  const artifactIds = transcriptArtifactIds(block);
  useEffect(() => {
    if (shouldDefaultOpen) {
      setOpen(true);
    }
  }, [block.id, shouldDefaultOpen]);

  if (block.kind === "text" && entry.role === "user") {
    return (
      <div className="pevo-messageFrame is-user">
        <article className="pevo-message is-user" {...transcriptBlockDataAttributes(entry, block)}>
          <MarkdownText text={text} />
        </article>
        {onCopyText && (
          <MessageMeta
            block={block}
            copied={copied}
            showElapsed={false}
            onCopy={async () => {
              try {
                await onCopyText(text);
                setCopied(true);
                globalThis.setTimeout(() => setCopied(false), 1_200);
              } catch {
                setCopied(false);
              }
            }}
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
          <MarkdownText streaming={block.status === "running"} text={text} />
        </article>
        {onCopyText && (
          <MessageMeta
            block={block}
            copied={copied}
            showElapsed
            onCopy={async () => {
              try {
                await onCopyText(text);
                setCopied(true);
                globalThis.setTimeout(() => setCopied(false), 1_200);
              } catch {
                setCopied(false);
              }
            }}
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
        <button className={`pevo-reasoningHeader ${runningReasoning ? "is-runningActivity" : ""}`} onClick={() => setOpen((value) => !value)} type="button">
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
  const agentSession = agentSessionFromBlock(block, display);
  const canOpenAgentSession = Boolean(agentSession && onOpenAgentSession);
  const runningTool = isRunningToolActivityBlock(block);
  const elapsed = runningTool ? liveToolBlockElapsed(block) : transcriptToolBlockElapsed(block);
  const evidenceLineClass = [
    "pevo-evidenceLine",
    display.singleTitle ? "is-singleTitle" : "",
    runningTool ? "is-runningTool" : "",
    canOpenAgentSession ? "is-openTarget" : ""
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
      <code>{display.title}</code>
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
  showElapsed
}: {
  block: TranscriptBlock;
  copied: boolean;
  onCopy(): void | Promise<void>;
  showElapsed: boolean;
}) {
  const timestamp = transcriptBlockTimestamp(block);
  const elapsed = showElapsed ? transcriptBlockElapsed(block) : null;
  return (
    <div className="pevo-messageMeta" aria-label="Message actions">
      <button className="pevo-messageCopy" onClick={() => void onCopy()} title="Copy" type="button">
        {copied ? <Check size={14} aria-hidden /> : <Copy size={14} aria-hidden />}
        <span className="pevo-srOnly">{copied ? "Copied" : "Copy message"}</span>
      </button>
      {elapsed && <span className="pevo-messageElapsed">{elapsed}</span>}
      {timestamp && (
        <time className="pevo-messageTime" dateTime={timestamp.iso}>
          {timestamp.label}
        </time>
      )}
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
    "data-source": block.source || entry.source
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
