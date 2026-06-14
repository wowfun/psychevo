import { ArrowDownToLine, Check, ChevronDown, ChevronRight, Copy } from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import type { GatewayActivity, TranscriptBlock, TranscriptEntry } from "@psychevo/protocol";
import { asRecord } from "./shared";
import { evidenceDisplay, type EvidenceDisplay, type ToolDetailSection } from "./toolEvidence";

export interface TranscriptPanelProps {
  activity?: GatewayActivity;
  entries: TranscriptEntry[];
  onCopyText?(text: string): void | Promise<void>;
}

export interface MarkdownTextProps {
  streaming?: boolean;
  text: string;
}

type CopyTextHandler = ((text: string) => void | Promise<void>) | undefined;

const STREAM_REVEAL_INITIAL_CHARS = 24;
const STREAM_REVEAL_INTERVAL_MS = 24;
const STREAM_REVEAL_MAX_STEP_CHARS = 16;

export function TranscriptPanel({ activity, entries, onCopyText }: TranscriptPanelProps) {
  const [followingBottom, setFollowingBottom] = useState(true);
  const [scrolling, setScrolling] = useState(false);
  const scrollRef = useRef<HTMLDivElement | null>(null);
  const scrollIdleTimer = useRef<ReturnType<typeof globalThis.setTimeout> | null>(null);
  const orderedEntries = useMemo(() => orderTranscriptEntries(entries), [entries]);
  const visibleEntries = useMemo(() => orderedEntries.filter((entry) => visibleBlocks(entry).length > 0), [orderedEntries]);
  const threadItemsClass = `pevo-threadItems ${scrolling ? "is-scrolling" : ""}`.trim();

  useEffect(() => {
    if (!followingBottom) {
      return;
    }
    const scroller = scrollRef.current;
    if (!scroller) {
      return;
    }
    scroller.scrollTo({ top: scroller.scrollHeight, behavior: "smooth" });
  }, [followingBottom, visibleEntries, activity?.running]);

  useEffect(() => () => {
    if (scrollIdleTimer.current !== null) {
      globalThis.clearTimeout(scrollIdleTimer.current);
    }
  }, []);

  return (
    <section className="pevo-panel pevo-transcript" aria-label="Transcript">
      <div
        className={threadItemsClass}
        ref={scrollRef}
        onScroll={(event) => {
          const target = event.currentTarget;
          const atBottom = target.scrollHeight - target.scrollTop - target.clientHeight < 48;
          setFollowingBottom(atBottom);
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
          visibleEntries.map((entry) => <TranscriptEntryView entry={entry} key={entry.id} onCopyText={onCopyText} />)
        )}
      </div>
      {!followingBottom && (
        <button
          aria-label="Jump to latest"
          className="pevo-jumpBottom"
          data-tooltip="Jump to latest"
          onClick={() => {
            const scroller = scrollRef.current;
            scroller?.scrollTo({ top: scroller.scrollHeight, behavior: "smooth" });
            setFollowingBottom(true);
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

function orderTranscriptEntries(entries: TranscriptEntry[]): TranscriptEntry[] {
  return [...entries].sort((left, right) => {
    if (left.messageSeq !== null && right.messageSeq !== null && left.messageSeq !== right.messageSeq) {
      return left.messageSeq - right.messageSeq;
    }
    if (left.messageSeq !== null && right.messageSeq === null) {
      return -1;
    }
    if (left.messageSeq === null && right.messageSeq !== null) {
      return 1;
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
  if (asRecord(block.metadata).hidden === true) {
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

function TranscriptEntryView({
  entry,
  onCopyText
}: {
  entry: TranscriptEntry;
  onCopyText: CopyTextHandler;
}) {
  return (
    <>
      {visibleBlocks(entry).map((block) => (
        <TranscriptBlockView block={block} entry={entry} key={block.id} onCopyText={onCopyText} />
      ))}
    </>
  );
}

function TranscriptBlockView({
  block,
  entry,
  onCopyText
}: {
  block: TranscriptBlock;
  entry: TranscriptEntry;
  onCopyText: CopyTextHandler;
}) {
  const [open, setOpen] = useState(defaultReasoningOpen(block));
  const [copied, setCopied] = useState(false);
  const text = transcriptBlockText(block);
  const status = statusLabel(block);
  const artifactIds = transcriptArtifactIds(block);
  useEffect(() => {
    if (defaultReasoningOpen(block)) {
      setOpen(true);
    }
  }, [block]);

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
    return (
      <article
        className={`pevo-reasoning ${block.status === "running" ? "is-streaming" : ""}`}
        data-has-body={text.trim() ? "true" : "false"}
        {...transcriptBlockDataAttributes(entry, block)}
      >
        <button className="pevo-reasoningHeader" onClick={() => setOpen((value) => !value)} type="button">
          {open ? <ChevronDown size={15} aria-hidden /> : <ChevronRight size={15} aria-hidden />}
          <span>{reasoningTitle()}</span>
          {status && <em>{status}</em>}
        </button>
        {open && <MarkdownText streaming={block.status === "running"} text={text} />}
      </article>
    );
  }
  const display = evidenceDisplay(block, text);
  const evidenceLineClass = `pevo-evidenceLine ${display.singleTitle ? "is-singleTitle" : ""}`.trim();
  return (
    <article className={`pevo-evidence is-${block.status} is-tool-${display.category}`} {...transcriptBlockDataAttributes(entry, block)}>
      <button className={evidenceLineClass} onClick={() => setOpen((value) => !value)} type="button">
        {open ? <ChevronDown size={15} aria-hidden /> : <ChevronRight size={15} aria-hidden />}
        <code>{display.title}</code>
        {display.summary && <span>{display.summary}</span>}
        {status && <em>{status}</em>}
      </button>
      {open && display.sections.length > 0 && <ToolDetail display={display} />}
      {artifactIds.length > 0 && (
        <div className="pevo-artifactRefs">
          {artifactIds.map((artifactId) => <span key={artifactId}>{artifactId}</span>)}
        </div>
      )}
    </article>
  );
}

function ToolDetail({ display }: { display: EvidenceDisplay }) {
  return (
    <div className="pevo-toolDetail">
      {display.sections.map((section, index) => <ToolDetailSectionView key={`${section.title}:${index}`} section={section} />)}
    </div>
  );
}

function ToolDetailSectionView({ section }: { section: ToolDetailSection }) {
  const toneClass = section.tone && section.tone !== "default" ? ` is-${section.tone}` : "";
  if (section.kind === "kv") {
    return (
      <section className={`pevo-toolSection is-kv${toneClass}`}>
        <h4>{section.title}</h4>
        <dl>
          {section.rows.map((row) => (
            <div key={`${row.label}:${row.value}`}>
              <dt>{row.label}</dt>
              <dd>{row.value}</dd>
            </div>
          ))}
        </dl>
      </section>
    );
  }
  return (
    <section className={`pevo-toolSection is-text${section.code ? " is-code" : ""}${toneClass}`}>
      <h4>{section.title}</h4>
      <pre>{section.text}</pre>
    </section>
  );
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
  const metadata = asRecord(block.metadata);
  const value = metadata.elapsed_ms ?? asRecord(metadata.message_metadata).elapsed_ms;
  const elapsedMs = typeof value === "number" ? value : typeof value === "string" ? Number(value) : NaN;
  if (!Number.isFinite(elapsedMs) || elapsedMs < 0) {
    return null;
  }
  const seconds = Math.floor(elapsedMs / 1_000);
  if (seconds < 60) {
    return `${seconds}s`;
  }
  return `${Math.floor(seconds / 60)}m${String(seconds % 60).padStart(2, "0")}s`;
}

function defaultReasoningOpen(block: TranscriptBlock): boolean {
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

export function MarkdownText({ streaming, text }: MarkdownTextProps) {
  const visibleText = useStreamingReveal(text, streaming === true);
  return (
    <div className={`pevo-markdown ${streaming ? "is-streaming" : ""}`}>
      <ReactMarkdown remarkPlugins={[remarkGfm]}>{visibleText}</ReactMarkdown>
    </div>
  );
}

function useStreamingReveal(text: string, streaming: boolean): string {
  const canReveal = canUseBrowserTextReveal();
  const [visibleText, setVisibleText] = useState(() => initialVisibleText(text, streaming && canReveal));
  const visibleRef = useRef(visibleText);
  const targetRef = useRef(text);
  const wasStreamingRef = useRef(streaming && canReveal);

  function setVisible(next: string) {
    visibleRef.current = next;
    setVisibleText(next);
  }

  useEffect(() => {
    if (!canReveal) {
      if (visibleRef.current !== text) {
        setVisible(text);
      }
      return;
    }

    targetRef.current = text;
    if (streaming) {
      wasStreamingRef.current = true;
      if (!text.startsWith(visibleRef.current) || visibleRef.current.length > text.length) {
        setVisible(initialVisibleText(text, true));
      }
      return;
    }

    if (
      wasStreamingRef.current &&
      text.startsWith(visibleRef.current) &&
      visibleRef.current.length < text.length
    ) {
      return;
    }

    wasStreamingRef.current = false;
    if (visibleRef.current !== text) {
      setVisible(text);
    }
  }, [canReveal, streaming, text]);

  useEffect(() => {
    if (!canReveal || (!streaming && !wasStreamingRef.current)) {
      return;
    }

    const timer = globalThis.setInterval(() => {
      const target = targetRef.current;
      const current = visibleRef.current;
      if (current === target) {
        if (!streaming) {
          wasStreamingRef.current = false;
          globalThis.clearInterval(timer);
        }
        return;
      }

      if (!target.startsWith(current) || current.length > target.length) {
        setVisible(target);
        return;
      }

      const remaining = target.length - current.length;
      const step = Math.max(1, Math.min(STREAM_REVEAL_MAX_STEP_CHARS, Math.ceil(remaining / 5)));
      setVisible(target.slice(0, current.length + step));
    }, STREAM_REVEAL_INTERVAL_MS);

    return () => globalThis.clearInterval(timer);
  }, [canReveal, streaming, text]);

  return visibleText;
}

function initialVisibleText(text: string, streaming: boolean): string {
  if (!streaming || text.length <= STREAM_REVEAL_INITIAL_CHARS) {
    return text;
  }
  return text.slice(0, STREAM_REVEAL_INITIAL_CHARS);
}

function canUseBrowserTextReveal(): boolean {
  if (typeof window === "undefined") {
    return false;
  }
  return !window.navigator.userAgent.toLowerCase().includes("jsdom");
}

function liveOrder(entry: TranscriptEntry): number | null {
  const metadata = asRecord(entry.metadata);
  const value = metadata.liveOrder ?? metadata.live_order;
  return typeof value === "number" && Number.isFinite(value) ? value : null;
}
