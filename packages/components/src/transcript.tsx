import { Check, ChevronDown, ChevronRight, Copy } from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import type { GatewayActivity, TranscriptBlock, TranscriptEntry } from "@psychevo/protocol";
import { asRecord, compactJson, compactText, prettyJson, stringValue } from "./shared";

export interface TranscriptPanelProps {
  activity?: GatewayActivity;
  entries: TranscriptEntry[];
  onCopyText?(text: string): void | Promise<void>;
}

type CopyTextHandler = ((text: string) => void | Promise<void>) | undefined;

export function TranscriptPanel({ activity, entries, onCopyText }: TranscriptPanelProps) {
  const [followingBottom, setFollowingBottom] = useState(true);
  const scrollRef = useRef<HTMLDivElement | null>(null);
  const orderedEntries = useMemo(() => orderTranscriptEntries(entries), [entries]);
  const visibleEntries = useMemo(() => orderedEntries.filter((entry) => visibleBlocks(entry).length > 0), [orderedEntries]);

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

  return (
    <section className="pevo-panel pevo-transcript" aria-label="Transcript">
      <div
        className="pevo-threadItems"
        ref={scrollRef}
        onScroll={(event) => {
          const target = event.currentTarget;
          const atBottom = target.scrollHeight - target.scrollTop - target.clientHeight < 48;
          setFollowingBottom(atBottom);
        }}
      >
        {visibleEntries.length === 0 ? (
          <div className="pevo-empty pevo-emptyThread">No messages yet</div>
        ) : (
          visibleEntries.map((entry) => <TranscriptEntryView entry={entry} key={entry.id} onCopyText={onCopyText} />)
        )}
      </div>
      {!followingBottom && (
        <button className="pevo-jumpBottom" onClick={() => {
          const scroller = scrollRef.current;
          scroller?.scrollTo({ top: scroller.scrollHeight, behavior: "smooth" });
          setFollowingBottom(true);
        }} type="button">
          Jump to latest
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
  return (
    <article className={`pevo-evidence is-${block.status}`} {...transcriptBlockDataAttributes(entry, block)}>
      <button className="pevo-evidenceLine" onClick={() => setOpen((value) => !value)} type="button">
        {open ? <ChevronDown size={15} aria-hidden /> : <ChevronRight size={15} aria-hidden />}
        <code>{block.title ?? block.kind}</code>
        <span>{display.summary}</span>
        {status && <em>{status}</em>}
      </button>
      {open && display.detail && <pre>{display.detail}</pre>}
      {artifactIds.length > 0 && (
        <div className="pevo-artifactRefs">
          {artifactIds.map((artifactId) => <span key={artifactId}>{artifactId}</span>)}
        </div>
      )}
    </article>
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

function MarkdownText({ streaming, text }: { streaming?: boolean; text: string }) {
  return (
    <div className={`pevo-markdown ${streaming ? "is-streaming" : ""}`}>
      <ReactMarkdown remarkPlugins={[remarkGfm]}>{text}</ReactMarkdown>
    </div>
  );
}

function evidenceDisplay(block: TranscriptBlock, fallbackText: string): { detail: string | null; summary: string } {
  const metadata = asRecord(block.metadata);
  if (metadata.projection === "tool") {
    const args = metadata.args ?? metadata.arguments;
    const result = block.result?.content ?? metadata.result;
    const summary = toolArgsSummary(block.title ?? block.kind, args, block.preview ?? fallbackText);
    const detail = [
      args === undefined ? null : `args\n${prettyJson(args)}`,
      result === undefined ? null : `result\n${prettyJson(result)}`,
      block.result?.isError ? "status\nerror" : null,
      metadata.outcome && metadata.outcome !== "normal" ? `outcome\n${String(metadata.outcome)}` : null
    ].filter((value): value is string => value !== null).join("\n\n");
    return { summary, detail: detail || null };
  }
  const summary = block.preview ?? compactText(fallbackText, 180);
  const detail = [block.detail, block.body, block.result?.content, block.preview]
    .filter((value): value is string => Boolean(value?.trim()))
    .filter((value) => value.trim() !== summary.trim())[0] ?? null;
  return { summary, detail };
}

function toolArgsSummary(toolName: string, args: unknown, fallback: string): string {
  const record = asRecord(args);
  const path = stringValue(record.path) ?? stringValue(record.file) ?? stringValue(record.file_path);
  if (path) {
    return path;
  }
  const command = stringValue(record.cmd) ?? stringValue(record.command);
  if (command) {
    return compactText(firstEffectiveCommand(command), 180);
  }
  const query = stringValue(record.query) ?? stringValue(record.pattern);
  if (query) {
    return compactText(query, 180);
  }
  if (args !== undefined) {
    return compactText(compactJson(args), 180);
  }
  return compactText(fallback || toolName, 180);
}

function firstEffectiveCommand(command: string): string {
  return command
    .split(/\r?\n/)
    .map((line) => line.trim())
    .find((line) => line && !line.startsWith("#")) ?? command.trim();
}

function liveOrder(entry: TranscriptEntry): number | null {
  const metadata = asRecord(entry.metadata);
  const value = metadata.liveOrder ?? metadata.live_order;
  return typeof value === "number" && Number.isFinite(value) ? value : null;
}
