import {
  Archive,
  Check,
  ChevronDown,
  ChevronRight,
  CircleSlash,
  Download,
  History,
  Pencil,
  Plus,
  RefreshCw,
  RotateCcw,
  Send,
  Share2,
  Square,
  Terminal,
  Trash2,
  X
} from "lucide-react";
import { useEffect, useMemo, useRef, useState, type FormEvent, type KeyboardEvent } from "react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import type {
  CompletionItem,
  CompletionListResult,
  GatewayActivity,
  GatewayMention,
  PendingClarify,
  PendingPermission,
  PermissionDecision,
  SessionSummary,
  SettingsReadResult,
  TranscriptBlock,
  TranscriptEntry
} from "@psychevo/protocol";

const IDLE_ACTIVITY: GatewayActivity = {
  running: false,
  activeTurnId: null,
  queuedTurns: 0
};

export interface HistoryPanelProps {
  archived: boolean;
  currentThreadId?: string | undefined;
  disabled?: boolean;
  sessions: SessionSummary[];
  onArchive(sessionId: string): void;
  onDelete(sessionId: string): void;
  onExport(sessionId: string): void;
  onNew(): void;
  onRename(sessionId: string, title: string): void;
  onRestore(sessionId: string): void;
  onResume(sessionId: string): void;
  onShare(sessionId: string): void;
  onToggleArchived(): void;
}

export function HistoryPanel(props: HistoryPanelProps) {
  const [editingId, setEditingId] = useState<string | null>(null);
  const [draft, setDraft] = useState("");
  const sessions = Array.isArray(props.sessions) ? props.sessions : [];

  return (
    <section className="pevo-panel pevo-history" aria-label="History">
      <header className="pevo-panelHeader">
        <div className="pevo-titleLine">
          <History size={17} aria-hidden />
          <h2>History</h2>
        </div>
        <div className="pevo-iconRow">
          <IconButton title="New thread" onClick={props.onNew} disabled={props.disabled}>
            <Plus size={17} />
          </IconButton>
          <IconButton title={props.archived ? "Show active" : "Show archived"} onClick={props.onToggleArchived}>
            {props.archived ? <RotateCcw size={17} /> : <Archive size={17} />}
          </IconButton>
        </div>
      </header>
      <div className="pevo-sessionList">
        {sessions.length === 0 ? (
          <div className="pevo-empty">No sessions</div>
        ) : (
          sessions.map((session) => {
            const active = session.id === props.currentThreadId;
            const running = session.activity?.running === true;
            const title = session.title?.trim() || shortId(session.id);
            const editing = editingId === session.id;
            return (
              <article className={`pevo-sessionRow ${active ? "is-active" : ""} ${running ? "is-running" : ""}`} key={session.id}>
                {editing ? (
                  <form
                    className="pevo-rename"
                    onSubmit={(event) => {
                      event.preventDefault();
                      const next = draft.trim();
                      if (next) {
                        props.onRename(session.id, next);
                      }
                      setEditingId(null);
                    }}
                  >
                    <input value={draft} onChange={(event) => setDraft(event.target.value)} autoFocus />
                    <IconButton title="Save title" type="submit">
                      <Check size={16} />
                    </IconButton>
                    <IconButton title="Cancel rename" type="button" onClick={() => setEditingId(null)}>
                      <X size={16} />
                    </IconButton>
                  </form>
                ) : (
                  <>
                    <button
                      className="pevo-sessionMain"
                      onClick={() => props.onResume(session.id)}
                      disabled={props.disabled}
                      type="button"
                    >
                      <span>{title}</span>
                      <small>
                        {session.source} · {session.messageCount ?? 0} msg · {dateLabel(session.updatedAtMs)}
                        {running && <b>running</b>}
                      </small>
                    </button>
                    <div className="pevo-sessionActions">
                      <IconButton
                        title="Rename"
                        onClick={() => {
                          setDraft(title);
                          setEditingId(session.id);
                        }}
                      >
                        <Pencil size={15} />
                      </IconButton>
                      <IconButton title="Export" onClick={() => props.onExport(session.id)}>
                        <Download size={15} />
                      </IconButton>
                      <IconButton title="Share" onClick={() => props.onShare(session.id)}>
                        <Share2 size={15} />
                      </IconButton>
                      {props.archived ? (
                        <IconButton title="Restore" onClick={() => props.onRestore(session.id)} disabled={props.disabled || running}>
                          <RotateCcw size={15} />
                        </IconButton>
                      ) : (
                        <IconButton title="Archive" onClick={() => props.onArchive(session.id)} disabled={props.disabled || running}>
                          <Archive size={15} />
                        </IconButton>
                      )}
                      <IconButton title="Delete" danger onClick={() => props.onDelete(session.id)} disabled={props.disabled || active || running}>
                        <Trash2 size={15} />
                      </IconButton>
                    </div>
                  </>
                )}
              </article>
            );
          })
        )}
      </div>
    </section>
  );
}

export interface TranscriptPanelProps {
  activity?: GatewayActivity;
  entries: TranscriptEntry[];
}

export function TranscriptPanel({ activity, entries }: TranscriptPanelProps) {
  const [followingBottom, setFollowingBottom] = useState(true);
  const scrollRef = useRef<HTMLDivElement | null>(null);
  const orderedEntries = useMemo(() => orderTranscriptEntries(entries), [entries]);
  const visibleEntries = useMemo(() => orderedEntries.filter((entry) => visibleBlocks(entry).length > 0), [orderedEntries]);
  const entryCountLabel = visibleEntries.length === 1 ? "1 entry" : `${visibleEntries.length} entries`;

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
      <header className="pevo-panelHeader pevo-transcriptHeader">
        <div className="pevo-titleLine">
          <h2>Transcript</h2>
        </div>
        <span className="pevo-countPill">{entryCountLabel}</span>
      </header>
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
          visibleEntries.map((entry) => <TranscriptEntryView entry={entry} key={entry.id} />)
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

export interface ComposerProps {
  completionProvider?: (text: string, cursor: number) => Promise<CompletionListResult>;
  disabled?: boolean;
  running: boolean;
  onCommand?(command: string): void;
  onInterrupt(): void;
  onShell?(command: string): void;
  onSteer(text: string): void;
  onSubmit(text: string, mentions: GatewayMention[]): void;
}

export function Composer({
  completionProvider,
  disabled,
  running,
  onCommand,
  onInterrupt,
  onShell,
  onSteer,
  onSubmit
}: ComposerProps) {
  const [draft, setDraft] = useState("");
  const [turnMode, setTurnMode] = useState<"turn" | "steer">("steer");
  const [inputMode, setInputMode] = useState<"prompt" | "shell">("prompt");
  const [completion, setCompletion] = useState<CompletionListResult | null>(null);
  const [activeCompletion, setActiveCompletion] = useState(0);
  const mentionsRef = useRef<GatewayMention[]>([]);
  const textareaRef = useRef<HTMLTextAreaElement | null>(null);
  const completionOptionRefs = useRef<Array<HTMLButtonElement | null>>([]);
  const completionTimer = useRef<number | null>(null);
  const completionSequence = useRef(0);
  const trimmed = draft.trim();
  const completionItems = completion?.items ?? [];
  const shellMode = inputMode === "shell";

  useEffect(() => () => {
    if (completionTimer.current !== null) {
      window.clearTimeout(completionTimer.current);
    }
  }, []);

  useEffect(() => {
    if (completionItems.length === 0) {
      return;
    }
    completionOptionRefs.current[activeCompletion]?.scrollIntoView?.({ block: "nearest" });
  }, [activeCompletion, completionItems.length]);

  function submit(event: FormEvent) {
    event.preventDefault();
    if (!trimmed || disabled) {
      return;
    }
    cancelCompletion();
    if (shellMode) {
      onShell?.(trimmed);
      setDraft("");
      setInputMode("prompt");
      updateMentions([]);
      return;
    }
    if (trimmed.startsWith("/") && !trimmed.includes("\n") && onCommand) {
      onCommand(trimmed);
      setDraft("");
      updateMentions([]);
      return;
    }
    if (running && turnMode === "steer") {
      onSteer(trimmed);
    } else {
      onSubmit(trimmed, activeMentionsForDraft(draft, mentionsRef.current));
    }
    setDraft("");
    updateMentions([]);
  }

  function cancelCompletion() {
    completionSequence.current += 1;
    if (completionTimer.current !== null) {
      window.clearTimeout(completionTimer.current);
      completionTimer.current = null;
    }
    setCompletion(null);
  }

  function scheduleCompletion(text: string, cursor: number, nextInputMode = inputMode) {
    if (!completionProvider) {
      return;
    }
    if (nextInputMode === "shell" && activeCompletionSigil(text, cursor) === "/") {
      cancelCompletion();
      return;
    }
    if (completionTimer.current !== null) {
      window.clearTimeout(completionTimer.current);
    }
    completionTimer.current = window.setTimeout(() => {
      const sequence = completionSequence.current + 1;
      completionSequence.current = sequence;
      void completionProvider(text, cursor)
        .then((result) => {
          if (sequence !== completionSequence.current) {
            return;
          }
          const items = completionItemsForResult(result);
          setCompletion(items.length > 0 ? { ...result, items } : null);
          setActiveCompletion(0);
        })
        .catch(() => {
          if (sequence === completionSequence.current) {
            setCompletion(null);
          }
        });
    }, 120);
}

function completionItemsForResult(result: CompletionListResult): CompletionItem[] {
  return Array.isArray(result.items) ? result.items : [];
}

function acceptCompletion(item = completionItems[activeCompletion]) {
    const replacement = completion?.replacement;
    if (!item || !replacement) {
      return;
    }
    const start = replacement.start;
    const end = replacement.end;
    const visibleText = completionInsertText(item);
    const insertText = completionInsertTextWithSpacing(item);
    const nextDraft = `${draft.slice(0, start)}${insertText}${draft.slice(end)}`;
    const cursor = start + insertText.length;
    setDraft(nextDraft);
    cancelCompletion();
    const target = item.target;
    if (target) {
      updateMentions((current) => [
        ...current.filter((mention) => nextDraft.slice(mention.range.start, mention.range.end) === mention.visibleText),
        {
          visibleText,
          range: { start, end: start + visibleText.length },
          target
        }
      ]);
    }
    window.requestAnimationFrame(() => {
      textareaRef.current?.focus();
      textareaRef.current?.setSelectionRange(cursor, cursor);
    });
  }

  function updateMentions(next: GatewayMention[] | ((current: GatewayMention[]) => GatewayMention[])) {
    mentionsRef.current = typeof next === "function" ? next(mentionsRef.current) : next;
  }

  function handleKeyDown(event: KeyboardEvent<HTMLTextAreaElement>) {
    if (event.nativeEvent.isComposing) {
      return;
    }
    if (!completionItems.length && shellMode && !draft && (event.key === "Escape" || event.key === "Backspace")) {
      event.preventDefault();
      setInputMode("prompt");
      cancelCompletion();
      return;
    }
    if (!completionItems.length && inputMode === "prompt" && !draft && event.key === "!") {
      event.preventDefault();
      setInputMode("shell");
      cancelCompletion();
      return;
    }
    if (completionItems.length > 0) {
      if (event.key === "ArrowDown" || (event.ctrlKey && event.key.toLowerCase() === "n")) {
        event.preventDefault();
        setActiveCompletion((index) => (index + 1) % completionItems.length);
        return;
      }
      if (event.key === "ArrowUp" || (event.ctrlKey && event.key.toLowerCase() === "p")) {
        event.preventDefault();
        setActiveCompletion((index) => (index + completionItems.length - 1) % completionItems.length);
        return;
      }
      if (event.key === "Tab" || event.key === "Enter") {
        event.preventDefault();
        acceptCompletion();
        return;
      }
      if (event.key === "Escape") {
        event.preventDefault();
        cancelCompletion();
        return;
      }
    }
    if (event.key === "Enter" && !event.shiftKey && !event.ctrlKey && !event.altKey && !event.metaKey) {
      event.preventDefault();
      event.currentTarget.form?.requestSubmit();
    }
  }

  return (
    <form className={`pevo-composer ${shellMode ? "is-shellMode" : ""}`} onSubmit={submit}>
      {running && (
        <div className="pevo-segmented" role="tablist" aria-label="Turn mode">
          <button className={turnMode === "turn" ? "is-selected" : ""} onClick={() => setTurnMode("turn")} type="button">
            Queue
          </button>
          <button className={turnMode === "steer" ? "is-selected" : ""} onClick={() => setTurnMode("steer")} type="button">
            Steer
          </button>
        </div>
      )}
      <div className="pevo-composerInput">
        {shellMode && <span className="pevo-shellMarker" aria-hidden>!</span>}
        <textarea
          ref={textareaRef}
          value={draft}
          onChange={(event) => {
            const nextRawDraft = event.target.value;
            const enteringShell = inputMode === "prompt" && nextRawDraft.startsWith("!");
            const nextMode = enteringShell ? "shell" : inputMode;
            const nextDraft = enteringShell ? nextRawDraft.slice(1) : nextRawDraft;
            if (enteringShell) {
              setInputMode("shell");
            }
            setDraft(nextDraft);
            updateMentions(activeMentionsForDraft(nextDraft, mentionsRef.current));
            scheduleCompletion(nextDraft, enteringShell ? Math.max(0, event.target.selectionStart - 1) : event.target.selectionStart, nextMode);
          }}
          onKeyDown={handleKeyDown}
          onSelect={(event) => scheduleCompletion(event.currentTarget.value, event.currentTarget.selectionStart)}
          placeholder={shellMode ? "shell command" : "Ask pevo..."}
          rows={3}
          disabled={disabled}
        />
      </div>
      {shellMode && !trimmed && (
        <div className="pevo-shellHelp">shell mode: type !&lt;command&gt; to run a local shell command</div>
      )}
      {completionItems.length > 0 && (
        <div className="pevo-completionPopover" role="listbox">
          {completionItems.map((item, index) => (
            <button
              aria-selected={index === activeCompletion}
              className={index === activeCompletion ? "is-active" : ""}
              key={item.id}
              ref={(node) => {
                completionOptionRefs.current[index] = node;
              }}
              onMouseDown={(event) => {
                event.preventDefault();
                acceptCompletion(item);
              }}
              role="option"
              type="button"
            >
              <strong>{item.label}</strong>
              <span>{item.kind}</span>
              {item.detail && <small>{item.detail}</small>}
            </button>
          ))}
        </div>
      )}
      <div className="pevo-composerActions">
        {running && (
          <IconButton title="Interrupt active turn" onClick={onInterrupt} type="button">
            <Square size={17} />
          </IconButton>
        )}
        <button className="pevo-primaryButton" disabled={!trimmed || disabled || (shellMode && !onShell)} type="submit">
          {shellMode ? <Terminal size={17} aria-hidden /> : <Send size={17} aria-hidden />}
          <span>{shellMode ? "Run" : running && turnMode === "steer" ? "Steer" : "Send"}</span>
        </button>
      </div>
    </form>
  );
}

export interface StatusPanelProps {
  activity?: GatewayActivity | undefined;
  pendingClarifies?: PendingClarify[] | undefined;
  pendingPermissions?: PendingPermission[] | undefined;
  settings?: SettingsReadResult | undefined;
  status: string;
  onClarify(requestId: string, answer: string): void;
  onPermission(requestId: string, decision: PermissionDecision): void;
  onRefresh(): void;
}

export function StatusPanel(props: StatusPanelProps) {
  const activity = normalizeActivity(props.activity);
  const pendingClarifies = Array.isArray(props.pendingClarifies) ? props.pendingClarifies : [];
  const pendingPermissions = Array.isArray(props.pendingPermissions) ? props.pendingPermissions : [];

  return (
    <section className="pevo-panel pevo-utility" aria-label="Status">
      <header className="pevo-panelHeader">
        <div className="pevo-titleLine">
          <CircleSlash size={17} aria-hidden />
          <h2>Status</h2>
        </div>
        <IconButton title="Refresh" onClick={props.onRefresh}>
          <RefreshCw size={17} />
        </IconButton>
      </header>

      <dl className="pevo-statusGrid">
        <div><dt>Connection</dt><dd>{props.status}</dd></div>
        <div><dt>Turn</dt><dd>{activity.running ? "running" : "idle"}</dd></div>
        <div><dt>Queued</dt><dd>{activity.queuedTurns}</dd></div>
      </dl>

      <div className="pevo-stack">
        <h3>Permissions</h3>
        {pendingPermissions.length === 0 ? (
          <p className="pevo-muted">None</p>
        ) : (
          pendingPermissions.map((permission) => (
            <div className="pevo-request" key={permission.requestId}>
              <strong>{permission.toolName}</strong>
              <p>{permission.reason}</p>
              <div className="pevo-buttonRow">
                <button onClick={() => props.onPermission(permission.requestId, "allowOnce")} type="button">Once</button>
                <button onClick={() => props.onPermission(permission.requestId, "allowSession")} type="button">Session</button>
                <button onClick={() => props.onPermission(permission.requestId, "deny")} type="button">Deny</button>
              </div>
            </div>
          ))
        )}
      </div>

      <div className="pevo-stack">
        <h3>Clarify</h3>
        {pendingClarifies.length === 0 ? (
          <p className="pevo-muted">None</p>
        ) : (
          pendingClarifies.map((clarify) => (
            <ClarifyRequest key={clarify.requestId} request={clarify} onSubmit={props.onClarify} />
          ))
        )}
      </div>

      <div className="pevo-stack">
        <h3>Settings</h3>
        <dl className="pevo-settings">
          <div><dt>Workdir</dt><dd>{props.settings?.workdir ?? "unknown"}</dd></div>
          <div><dt>Memory</dt><dd>{stringSetting(props.settings?.memoryResources?.mode, "status_only")}</dd></div>
          <div><dt>Secrets</dt><dd>{stringSetting(props.settings?.secrets?.frontendPersistence, "disabled")}</dd></div>
        </dl>
      </div>
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

function TranscriptEntryView({ entry }: { entry: TranscriptEntry }) {
  return (
    <>
      {visibleBlocks(entry).map((block) => (
        <TranscriptBlockView block={block} entry={entry} key={block.id} />
      ))}
    </>
  );
}

function TranscriptBlockView({ block, entry }: { block: TranscriptBlock; entry: TranscriptEntry }) {
  const [open, setOpen] = useState(defaultReasoningOpen(block));
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
      <article className="pevo-message is-user" {...transcriptBlockDataAttributes(entry, block)}>
        <MarkdownText text={text} />
      </article>
    );
  }
  if (block.kind === "text" && entry.role === "assistant") {
    return (
      <article
        className={`pevo-message is-assistant ${block.status === "running" ? "is-streaming" : ""}`}
        {...transcriptBlockDataAttributes(entry, block)}
      >
        <MarkdownText streaming={block.status === "running"} text={text} />
      </article>
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

function transcriptBlocks(entry: TranscriptEntry): TranscriptBlock[] {
  return Array.isArray(entry.blocks) ? entry.blocks : [];
}

function transcriptArtifactIds(block: TranscriptBlock): string[] {
  return Array.isArray(block.artifactIds) ? block.artifactIds : [];
}

function transcriptBlockText(block: TranscriptBlock): string {
  return block.body ?? block.detail ?? block.preview ?? "";
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

function completionInsertText(item: CompletionItem): string {
  return item.insertText || item.label;
}

function completionInsertTextWithSpacing(item: CompletionItem): string {
  const text = completionInsertText(item);
  if (item.kind === "directory") {
    return text;
  }
  return text.endsWith(" ") ? text : `${text} `;
}

function activeMentionsForDraft(draft: string, mentions: GatewayMention[]): GatewayMention[] {
  return mentions.filter((mention) => draft.slice(mention.range.start, mention.range.end) === mention.visibleText);
}

function activeCompletionSigil(text: string, cursor: number): "/" | "$" | "@" | null {
  const prefix = text.slice(0, Math.max(0, cursor));
  for (let index = prefix.length - 1; index >= 0; index -= 1) {
    const ch = prefix.charAt(index);
    if (/\s/.test(ch)) {
      return null;
    }
    if (ch === "/" || ch === "$" || ch === "@") {
      return ch;
    }
  }
  return null;
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

function asRecord(value: unknown): Record<string, unknown> {
  if (value && typeof value === "object" && !Array.isArray(value)) {
    return value as Record<string, unknown>;
  }
  if (typeof value === "string" && value.trim().startsWith("{")) {
    try {
      const parsed = JSON.parse(value) as unknown;
      if (parsed && typeof parsed === "object" && !Array.isArray(parsed)) {
        return parsed as Record<string, unknown>;
      }
    } catch {
      return {};
    }
  }
  return {};
}

function liveOrder(entry: TranscriptEntry): number | null {
  const metadata = asRecord(entry.metadata);
  const value = metadata.liveOrder ?? metadata.live_order;
  return typeof value === "number" && Number.isFinite(value) ? value : null;
}

function stringValue(value: unknown): string | null {
  return typeof value === "string" && value.trim() ? value : null;
}

function compactJson(value: unknown): string {
  try {
    return JSON.stringify(value);
  } catch {
    return String(value);
  }
}

function prettyJson(value: unknown): string {
  try {
    return JSON.stringify(value, null, 2);
  } catch {
    return String(value);
  }
}

function compactText(text: string, max: number): string {
  const normalized = text.replace(/\s+/g, " ").trim();
  if (normalized.length <= max) {
    return normalized;
  }
  return `${normalized.slice(0, Math.max(0, max - 3))}...`;
}

function ClarifyRequest({
  request,
  onSubmit
}: {
  request: PendingClarify;
  onSubmit(requestId: string, answer: string): void;
}) {
  const [answer, setAnswer] = useState("");
  return (
    <form
      className="pevo-request"
      onSubmit={(event) => {
        event.preventDefault();
        onSubmit(request.requestId, answer);
        setAnswer("");
      }}
    >
      <pre>{JSON.stringify(request.raw, null, 2)}</pre>
      <input value={answer} onChange={(event) => setAnswer(event.target.value)} />
      <button type="submit">Submit</button>
    </form>
  );
}

function IconButton({
  children,
  danger,
  ...props
}: React.ButtonHTMLAttributes<HTMLButtonElement> & { danger?: boolean }) {
  const label = props["aria-label"] ?? (typeof props.title === "string" ? props.title : undefined);
  return (
    <button
      {...props}
      aria-label={label}
      className={`pevo-iconButton ${danger ? "is-danger" : ""} ${props.className ?? ""}`.trim()}
    >
      {children}
    </button>
  );
}

function shortId(value: string): string {
  return value.length > 10 ? value.slice(0, 10) : value;
}

function dateLabel(value?: number | null): string {
  if (!value) {
    return "pending";
  }
  return new Intl.DateTimeFormat(undefined, {
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
    month: "short"
  }).format(new Date(value));
}

function stringSetting(value: unknown, fallback: string): string {
  return typeof value === "string" && value.trim() ? value : fallback;
}

function normalizeActivity(activity?: Partial<GatewayActivity> | null): GatewayActivity {
  return {
    running: activity?.running === true,
    activeTurnId: typeof activity?.activeTurnId === "string" ? activity.activeTurnId : null,
    queuedTurns: Number.isFinite(activity?.queuedTurns) ? Number(activity?.queuedTurns) : IDLE_ACTIVITY.queuedTurns
  };
}
