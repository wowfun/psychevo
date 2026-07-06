import { ArrowUp, Bot, Command, FileText, Folder, Settings2, Sparkles, X, Plus } from "lucide-react";
import { useEffect, useLayoutEffect, useRef, useState, type FormEvent, type KeyboardEvent, type ReactNode } from "react";
import type { CompletionItem, CompletionListResult, GatewayMention, PendingAction } from "@psychevo/protocol";
import { IconButton, Switch } from "./primitives";

export interface ComposerProps {
  addMenuOptions?: ReactNode;
  attachments?: ComposerAttachmentView[] | undefined;
  completionProvider?: (text: string, cursor: number) => Promise<CompletionListResult>;
  disabled?: boolean;
  draftPatch?: ComposerDraftPatch | undefined;
  leftControls?: ReactNode;
  mode?: string;
  planModeAvailable?: boolean;
  preActionControls?: ReactNode;
  promptSubmitBlockReason?: string | undefined;
  promptSubmitDisabled?: boolean;
  requestPanel?: ReactNode;
  rightControls?: ReactNode;
  running: boolean;
  runningStartedAtMs?: number | null;
  onCommand?(command: string): void;
  onAttach?(): void;
  onInterrupt(): void;
  onModeChange?(mode: string): void;
  onRemoveAttachment?(id: string): void;
  onShell?(command: string): void;
  onSteer(text: string): void;
  onSubmit(text: string, mentions: GatewayMention[]): void;
}

export interface ComposerDraftPatch {
  id: number;
  text: string;
}

export interface ComposerAttachmentView {
  id: string;
  kind: "file" | "image" | "text";
  name: string;
  sizeLabel: string;
}

export function Composer({
  addMenuOptions,
  attachments,
  completionProvider,
  disabled,
  draftPatch,
  leftControls,
  mode = "default",
  planModeAvailable = true,
  preActionControls,
  promptSubmitBlockReason,
  promptSubmitDisabled = false,
  requestPanel,
  rightControls,
  running,
  runningStartedAtMs,
  onAttach,
  onCommand,
  onInterrupt,
  onModeChange,
  onRemoveAttachment,
  onShell,
  onSteer,
  onSubmit
}: ComposerProps) {
  const [draft, setDraft] = useState("");
  const [turnMode, setTurnMode] = useState<"turn" | "steer">("steer");
  const [inputMode, setInputMode] = useState<"prompt" | "shell">("prompt");
  const [attachMenuOpen, setAttachMenuOpen] = useState(false);
  const [completion, setCompletion] = useState<CompletionListResult | null>(null);
  const [activeCompletion, setActiveCompletion] = useState(0);
  const [elapsedNowMs, setElapsedNowMs] = useState(() => Date.now());
  const [observedRunningStartedAtMs, setObservedRunningStartedAtMs] = useState<number | null>(null);
  const mentionsRef = useRef<GatewayMention[]>([]);
  const textareaRef = useRef<HTMLTextAreaElement | null>(null);
  const completionOptionRefs = useRef<Array<HTMLButtonElement | null>>([]);
  const completionTimer = useRef<number | null>(null);
  const completionSequence = useRef(0);
  const attachMenuRef = useRef<HTMLDivElement | null>(null);
  const trimmed = draft.trim();
  const completionItems = completion?.items ?? [];
  const completionRows = completionRowsForItems(completionItems);
  const shellMode = inputMode === "shell";
  const planMode = planModeAvailable && mode === "plan";
  const attachmentItems = attachments ?? [];
  const hasPromptPayload = Boolean(trimmed) || attachmentItems.length > 0;
  const slashCommandCandidate = Boolean(trimmed)
    && trimmed.startsWith("/")
    && !trimmed.includes("\n")
    && Boolean(onCommand);
  const showTurnModeControls = running && !shellMode && Boolean(trimmed);
  const effectiveRunningStartedAtMs = isPositiveTimestamp(runningStartedAtMs)
    ? Number(runningStartedAtMs)
    : observedRunningStartedAtMs;
  const runningElapsed = running ? compactElapsedLabel(effectiveRunningStartedAtMs, elapsedNowMs) : null;
  const runningSpinner = running ? activitySpinnerFrame(effectiveRunningStartedAtMs, elapsedNowMs) : null;

  useEffect(() => {
    if (!running) {
      setObservedRunningStartedAtMs(null);
      return;
    }
    setObservedRunningStartedAtMs((current) => current ?? Date.now());
  }, [running]);

  useEffect(() => {
    if (!running || !isPositiveTimestamp(effectiveRunningStartedAtMs)) {
      return;
    }
    setElapsedNowMs(Date.now());
    const timer = window.setInterval(() => {
      setElapsedNowMs(Date.now());
    }, 120);
    return () => window.clearInterval(timer);
  }, [running, effectiveRunningStartedAtMs]);

  useLayoutEffect(() => {
    resizeTextarea(textareaRef.current);
  }, [draft, shellMode]);

  useEffect(() => () => {
    if (completionTimer.current !== null) {
      window.clearTimeout(completionTimer.current);
    }
  }, []);

  useEffect(() => {
    if (!draftPatch) {
      return;
    }
    cancelCompletion();
    setInputMode("prompt");
    setDraft(draftPatch.text);
    updateMentions([]);
    window.requestAnimationFrame(() => {
      textareaRef.current?.focus();
      const cursor = draftPatch.text.length;
      textareaRef.current?.setSelectionRange(cursor, cursor);
    });
  }, [draftPatch?.id]);

  useEffect(() => {
    if (completionItems.length === 0) {
      return;
    }
    completionOptionRefs.current[activeCompletion]?.scrollIntoView?.({ block: "nearest" });
  }, [activeCompletion, completionItems.length]);

  useEffect(() => {
    if (!attachMenuOpen) {
      return;
    }
    function onPointerDown(event: MouseEvent) {
      if (attachMenuRef.current?.contains(event.target as Node)) {
        return;
      }
      setAttachMenuOpen(false);
    }
    function onKeyDown(event: globalThis.KeyboardEvent) {
      if (event.key === "Escape") {
        setAttachMenuOpen(false);
      }
    }
    document.addEventListener("mousedown", onPointerDown);
    document.addEventListener("keydown", onKeyDown);
    return () => {
      document.removeEventListener("mousedown", onPointerDown);
      document.removeEventListener("keydown", onKeyDown);
    };
  }, [attachMenuOpen]);

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
    if (attachmentItems.length === 0 && slashCommandCandidate && onCommand) {
      onCommand(trimmed);
      setDraft("");
      updateMentions([]);
      return;
    }
    if (running && turnMode === "steer" && attachmentItems.length === 0) {
      onSteer(trimmed);
    } else if (promptSubmitDisabled) {
      return;
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

  const actionButton = running ? (
    <button
      aria-label="Interrupt active turn"
      className="pevo-primaryButton pevo-sendButton is-interrupt"
      onClick={onInterrupt}
      type="button"
    >
      <span className="pevo-stopGlyph" aria-hidden />
    </button>
  ) : (
    <button
      aria-label={shellMode ? "Run shell command" : "Send message"}
      className="pevo-primaryButton pevo-sendButton"
      disabled={disabled || (shellMode ? (!trimmed || !onShell) : !hasPromptPayload || (promptSubmitDisabled && !slashCommandCandidate))}
      title={!shellMode && promptSubmitDisabled && !slashCommandCandidate ? promptSubmitBlockReason : undefined}
      type="submit"
    >
      <ArrowUp size={17} aria-hidden />
    </button>
  );

  return (
    <form className={`pevo-composer ${shellMode ? "is-shellMode" : ""} ${running ? "is-running" : ""} ${planMode ? "is-planMode" : ""}`} onSubmit={submit}>
      {requestPanel && <div className="pevo-composerRequestPanel">{requestPanel}</div>}
      {showTurnModeControls && (
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
          {completionRows.map((row) => {
            if (row.type === "header") {
              return (
                <div className="pevo-completionGroup" key={row.key} role="presentation">
                  {row.label}
                </div>
              );
            }
            const item = row.item;
            const rightLabel = completionRightLabel(item);
            return (
              <button
                aria-selected={row.index === activeCompletion}
                className={row.index === activeCompletion ? "is-active" : ""}
                key={item.id}
                ref={(node) => {
                  completionOptionRefs.current[row.index] = node;
                }}
                onMouseDown={(event) => {
                  event.preventDefault();
                  acceptCompletion(item);
                }}
                role="option"
                type="button"
              >
                <span className="pevo-completionIcon" aria-hidden>
                  <CompletionItemIcon item={item} />
                </span>
                <span className="pevo-completionCopy">
                  <strong>{item.label}</strong>
                  {item.detail && <small>{item.detail}</small>}
                </span>
                {rightLabel && <span className="pevo-completionScope">{rightLabel}</span>}
              </button>
            );
          })}
        </div>
      )}
      <div className="pevo-composerFooter">
        <div className="pevo-composerLeftControls">
          <div className="pevo-addMenu" ref={attachMenuRef}>
            <IconButton
              title="Add attachments and options"
              onClick={() => setAttachMenuOpen((open) => !open)}
              disabled={disabled}
              type="button"
              aria-expanded={attachMenuOpen}
            >
              <Plus size={18} />
            </IconButton>
            {attachMenuOpen && (
              <div className="pevo-addPopover" role="menu">
                <button
                  className="pevo-addFileRow"
                  disabled={!onAttach}
                  onClick={() => {
                    setAttachMenuOpen(false);
                    onAttach?.();
                  }}
                  role="menuitem"
                  type="button"
                >
                  Add images and files
                </button>
                <Switch
                  checked={planMode}
                  className="pevo-modeSwitchRow"
                  disabled={!planModeAvailable}
                  label="Plan mode"
                  onCheckedChange={(checked) => {
                    onModeChange?.(checked ? "plan" : "default");
                  }}
                  size="compact"
                />
                {addMenuOptions}
              </div>
            )}
          </div>
          {leftControls}
          {planMode && (
            <div className="pevo-planChip" tabIndex={0}>
              <span>Plan</span>
              <button
                aria-label="Disable Plan mode"
                onClick={() => onModeChange?.("default")}
                type="button"
              >
                <X size={12} aria-hidden />
              </button>
            </div>
          )}
        </div>
        {runningElapsed && (
          <span className="pevo-composerTurnStatus" aria-label="Active turn elapsed">
            {runningSpinner && (
              <span className="pevo-composerTurnSpinner" aria-hidden="true">
                {runningSpinner}
              </span>
            )}
            <span>{runningElapsed}</span>
          </span>
        )}
        <div className="pevo-composerRightControls">
          {rightControls && <div className="pevo-composerInlineStatus">{rightControls}</div>}
          <div className="pevo-composerActions">
            {preActionControls}
            {actionButton}
          </div>
        </div>
      </div>
    </form>
  );
}

function resizeTextarea(textarea: HTMLTextAreaElement | null) {
  if (!textarea) {
    return;
  }
  textarea.style.height = "auto";
  const styles = window.getComputedStyle(textarea);
  const maxHeight = parseFloat(styles.maxHeight);
  const minHeight = parseFloat(styles.minHeight);
  const measuredHeight = textarea.scrollHeight || minHeight || 0;
  const clampedHeight = Number.isFinite(maxHeight)
    ? Math.min(measuredHeight, maxHeight)
    : measuredHeight;
  const nextHeight = Math.max(clampedHeight, Number.isFinite(minHeight) ? minHeight : 0);
  textarea.style.height = `${nextHeight}px`;
  textarea.style.overflowY = Number.isFinite(maxHeight) && measuredHeight > maxHeight ? "auto" : "hidden";
}

function completionItemsForResult(result: CompletionListResult): CompletionItem[] {
  return orderCompletionItems(Array.isArray(result.items) ? result.items : []);
}

type CompletionGroupId = "commands" | "skills" | "agents" | "directories" | "files" | "capabilities" | "options";

type CompletionGroupMeta = {
  id: string;
  label: string;
};

type CompletionDisplayRow =
  | { type: "header"; key: string; id: string; label: string }
  | { type: "item"; key: string; item: CompletionItem; index: number };

const COMPLETION_GROUP_LABELS: Record<CompletionGroupId, string> = {
  commands: "Commands",
  skills: "Skills",
  agents: "Agents",
  directories: "Directories",
  files: "Files",
  capabilities: "Capabilities",
  options: "Options"
};

const COMPLETION_GROUP_ORDER: Record<CompletionGroupId, number> = {
  commands: 0,
  skills: 1,
  agents: 2,
  directories: 3,
  files: 4,
  capabilities: 5,
  options: 6
};

function orderCompletionItems(items: CompletionItem[]): CompletionItem[] {
  return items
    .map((item, index) => ({ item, index, group: completionGroupForItem(item).id }))
    .sort((left, right) => (
      completionGroupRank(left.group) - completionGroupRank(right.group)
      || left.index - right.index
    ))
    .map(({ item }) => item);
}

function completionRowsForItems(items: CompletionItem[]): CompletionDisplayRow[] {
  const rows: CompletionDisplayRow[] = [];
  let previousGroup: string | null = null;
  items.forEach((item, index) => {
    const group = completionGroupForItem(item);
    if (group.id !== previousGroup) {
      rows.push({
        type: "header",
        key: `header:${group.id}:${index}`,
        id: group.id,
        label: group.label
      });
      previousGroup = group.id;
    }
    rows.push({
      type: "item",
      key: `item:${item.id}:${index}`,
      item,
      index
    });
  });
  return rows;
}

function completionGroupForItem(item: CompletionItem): CompletionGroupMeta {
  const explicitGroup = optionalString((item as CompletionItem & { group?: string | null }).group);
  const explicitLabel = optionalString((item as CompletionItem & { groupLabel?: string | null }).groupLabel);
  if (explicitGroup) {
    const knownGroup = knownCompletionGroup(explicitGroup);
    return {
      id: explicitGroup,
      label: explicitLabel ?? (knownGroup ? COMPLETION_GROUP_LABELS[knownGroup] : titleCaseIdentifier(explicitGroup))
    };
  }
  const targetKind = completionTargetKind(item);
  const kind = item.kind.toLowerCase();
  if (kind === "command") {
    return completionKnownGroup("commands");
  }
  if (kind === "skill" || targetKind === "skill") {
    return completionKnownGroup("skills");
  }
  if (kind === "agent" || targetKind === "agent") {
    return completionKnownGroup("agents");
  }
  if (kind === "directory") {
    return completionKnownGroup("directories");
  }
  if (kind === "file" || targetKind === "file") {
    return completionKnownGroup("files");
  }
  if (kind === "capability" || targetKind === "capability") {
    return completionKnownGroup("capabilities");
  }
  return completionKnownGroup("options");
}

function completionKnownGroup(id: CompletionGroupId): CompletionGroupMeta {
  return { id, label: COMPLETION_GROUP_LABELS[id] };
}

function knownCompletionGroup(id: string): CompletionGroupId | null {
  return id in COMPLETION_GROUP_LABELS ? id as CompletionGroupId : null;
}

function completionGroupRank(id: string): number {
  const knownGroup = knownCompletionGroup(id);
  return knownGroup ? COMPLETION_GROUP_ORDER[knownGroup] : COMPLETION_GROUP_ORDER.options;
}

function completionRightLabel(item: CompletionItem): string | null {
  const group = completionGroupForItem(item).id;
  const explicit = optionalString((item as CompletionItem & { scopeLabel?: string | null }).scopeLabel);
  const target = item.target as ({ kind?: unknown; source?: unknown } | null | undefined);
  if (group === "skills") {
    return skillScopeDisplayLabel(explicit);
  }
  if (group === "agents") {
    return agentScopeDisplayLabel(explicit)
      ?? (typeof target?.source === "string" ? agentScopeDisplayLabel(target.source) : null);
  }
  if (explicit) {
    return explicit;
  }
  if (target?.kind === "agent" && typeof target.source === "string" && target.source.trim()) {
    return target.source.trim();
  }
  if (group === "directories" || group === "files") {
    return item.kind;
  }
  return null;
}

function skillScopeDisplayLabel(value: string | null | undefined): string | null {
  switch (value?.trim()) {
    case "project":
    case "agents":
    case "Project":
      return "Project";
    case "explicit":
    case "global":
    case "agents_global":
    case "config":
    case "install_source":
    case "dynamic":
    case "User":
      return "User";
    case "plugin":
    case "system":
    case "builtin":
    case "built_in":
    case "core":
    case "System":
      return "System";
    default:
      return null;
  }
}

function agentScopeDisplayLabel(value: string | null | undefined): string | null {
  switch (value?.trim()) {
    case "project":
    case "claude_project":
    case "Project":
      return "Project";
    case "explicit":
    case "global":
    case "claude_global":
    case "generated":
    case "User":
      return "User";
    case "built_in":
    case "builtin":
    case "system":
    case "core":
    case "System":
      return "System";
    default:
      return null;
  }
}

function completionTargetKind(item: CompletionItem): string | null {
  const target = item.target as ({ kind?: unknown } | null | undefined);
  return typeof target?.kind === "string" ? target.kind : null;
}

function optionalString(value: string | null | undefined): string | null {
  return typeof value === "string" && value.trim() ? value.trim() : null;
}

function titleCaseIdentifier(value: string): string {
  return value
    .split(/[-_\s]+/)
    .filter(Boolean)
    .map((part) => `${part.charAt(0).toUpperCase()}${part.slice(1)}`)
    .join(" ") || "Options";
}

function CompletionItemIcon({ item }: { item: CompletionItem }) {
  const group = completionGroupForItem(item).id;
  if (group === "skills") {
    return <Sparkles size={14} />;
  }
  if (group === "agents") {
    return <Bot size={14} />;
  }
  if (group === "directories") {
    return <Folder size={14} />;
  }
  if (group === "files") {
    return <FileText size={14} />;
  }
  if (group === "capabilities") {
    return <Settings2 size={14} />;
  }
  return <Command size={14} />;
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

function compactElapsedLabel(startedAtMs: number | null | undefined, nowMs: number): string | null {
  if (!isPositiveTimestamp(startedAtMs)) {
    return null;
  }
  const elapsedSeconds = Math.max(0, Math.floor((nowMs - Number(startedAtMs)) / 1_000));
  if (elapsedSeconds < 60) {
    return `${elapsedSeconds}s`;
  }
  return `${Math.floor(elapsedSeconds / 60)}m${String(elapsedSeconds % 60).padStart(2, "0")}s`;
}

function activitySpinnerFrame(startedAtMs: number | null | undefined, nowMs: number): string | null {
  if (!isPositiveTimestamp(startedAtMs)) {
    return null;
  }
  const frames = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧"];
  const elapsedMs = Math.max(0, nowMs - Number(startedAtMs));
  return frames[Math.floor(elapsedMs / 120) % frames.length] ?? "⠋";
}

function isPositiveTimestamp(value: number | null | undefined): boolean {
  return Number.isFinite(value) && Number(value) > 0;
}

function ClarifyRequest({
  request,
  onSubmit
}: {
  request: PendingAction;
  onSubmit(requestId: string, answer: string): void;
}) {
  const [answer, setAnswer] = useState("");
  const payload = request.payload && typeof request.payload === "object"
    ? request.payload as Record<string, unknown>
    : {};
  const raw = payload.raw ?? request.payload;
  return (
    <form
      className="pevo-request"
      onSubmit={(event) => {
        event.preventDefault();
        onSubmit(request.actionId, answer);
        setAnswer("");
      }}
    >
      <pre>{JSON.stringify(raw, null, 2)}</pre>
      <input value={answer} onChange={(event) => setAnswer(event.target.value)} />
      <button type="submit">Submit</button>
    </form>
  );
}
