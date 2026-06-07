import { Paperclip, Send, Square, Terminal, X } from "lucide-react";
import { useEffect, useRef, useState, type FormEvent, type KeyboardEvent, type ReactNode } from "react";
import type { CompletionItem, CompletionListResult, GatewayMention, PendingClarify } from "@psychevo/protocol";
import { IconButton } from "./primitives";

export interface ComposerProps {
  attachments?: ComposerAttachmentView[] | undefined;
  completionProvider?: (text: string, cursor: number) => Promise<CompletionListResult>;
  disabled?: boolean;
  leftControls?: ReactNode;
  requestPanel?: ReactNode;
  running: boolean;
  onCommand?(command: string): void;
  onAttach?(): void;
  onInterrupt(): void;
  onRemoveAttachment?(id: string): void;
  onShell?(command: string): void;
  onSteer(text: string): void;
  onSubmit(text: string, mentions: GatewayMention[]): void;
}

export interface ComposerAttachmentView {
  id: string;
  kind: "file" | "image" | "text";
  name: string;
  sizeLabel: string;
}

export function Composer({
  attachments,
  completionProvider,
  disabled,
  leftControls,
  requestPanel,
  running,
  onAttach,
  onCommand,
  onInterrupt,
  onRemoveAttachment,
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
  const attachmentItems = attachments ?? [];
  const hasPromptPayload = Boolean(trimmed) || attachmentItems.length > 0;

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
    if (disabled || (shellMode ? !trimmed : !hasPromptPayload)) {
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
    if (attachmentItems.length === 0 && trimmed.startsWith("/") && !trimmed.includes("\n") && onCommand) {
      onCommand(trimmed);
      setDraft("");
      updateMentions([]);
      return;
    }
    if (running && turnMode === "steer" && attachmentItems.length === 0) {
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
    <form className={`pevo-composer ${shellMode ? "is-shellMode" : ""} ${running ? "is-running" : ""}`} onSubmit={submit}>
      {requestPanel && <div className="pevo-composerRequestPanel">{requestPanel}</div>}
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
          placeholder={shellMode ? "shell command" : "Ask Psychevo..."}
          rows={1}
          disabled={disabled}
        />
        <div className="pevo-composerActions">
          {running && (
            <IconButton title="Interrupt active turn" onClick={onInterrupt} type="button">
              <Square size={17} />
            </IconButton>
          )}
          <button
            aria-label={shellMode ? "Run shell command" : running && turnMode === "steer" ? "Steer active turn" : "Send message"}
            className="pevo-primaryButton pevo-sendButton"
            disabled={disabled || (shellMode ? (!trimmed || !onShell) : !hasPromptPayload)}
            type="submit"
          >
            {shellMode ? <Terminal size={17} aria-hidden /> : <Send size={17} aria-hidden />}
          </button>
        </div>
      </div>
      {shellMode && !trimmed && (
        <div className="pevo-shellHelp">shell mode: type !&lt;command&gt; to run a local shell command</div>
      )}
      {attachmentItems.length > 0 && (
        <div className="pevo-attachmentList" aria-label="Attachments">
          {attachmentItems.map((attachment) => (
            <span className="pevo-attachmentChip" key={attachment.id}>
              <strong>{attachment.name}</strong>
              <small>{attachment.kind} · {attachment.sizeLabel}</small>
              <button
                aria-label={`Remove ${attachment.name}`}
                onClick={() => onRemoveAttachment?.(attachment.id)}
                type="button"
              >
                <X size={13} />
              </button>
            </span>
          ))}
        </div>
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
      <div className="pevo-composerFooter">
        <div className="pevo-composerLeftControls">
          <IconButton title="Add attachment" onClick={onAttach} disabled={disabled || !onAttach} type="button">
            <Paperclip size={17} />
          </IconButton>
          {leftControls}
        </div>
      </div>
    </form>
  );
}

function completionItemsForResult(result: CompletionListResult): CompletionItem[] {
  return Array.isArray(result.items) ? result.items : [];
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
