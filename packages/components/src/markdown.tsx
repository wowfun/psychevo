import { useEffect, useMemo, useRef, useState } from "react";
import { Check, Copy } from "lucide-react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { isMap, isScalar, isSeq, parseDocument } from "yaml";

export interface MarkdownTextProps {
  copyLabel?: string;
  copyText?: string;
  onCopyText?: ((text: string) => void | Promise<void>) | undefined;
  streaming?: boolean;
  text: string;
}

type MarkdownFrontmatter = {
  body: string;
  entries: FrontmatterEntry[] | null;
};

type FrontmatterEntry = {
  key: string;
  value: FrontmatterDisplayValue;
};

type FrontmatterDisplayValue =
  | { kind: "scalar"; text: string }
  | { kind: "chips"; items: string[] }
  | { kind: "code"; text: string };

const STREAM_REVEAL_INITIAL_CHARS = 24;
const STREAM_REVEAL_INTERVAL_MS = 24;
const STREAM_REVEAL_MAX_STEP_CHARS = 16;
const FRONTMATTER_BOUNDARY = "---";
const FRONTMATTER_CODE_MAX_CHARS = 1200;

export function MarkdownText({ copyLabel = "Copy Markdown", copyText, onCopyText, streaming, text }: MarkdownTextProps) {
  const [copying, setCopying] = useState(false);
  const [copied, setCopied] = useState(false);
  const copiedTimeoutRef = useRef<ReturnType<typeof globalThis.setTimeout> | null>(null);
  const visibleText = useStreamingReveal(text, streaming === true);
  const frontmatter = useMemo(() => markdownFrontmatter(visibleText), [visibleText]);

  useEffect(() => () => {
    if (copiedTimeoutRef.current) {
      globalThis.clearTimeout(copiedTimeoutRef.current);
    }
  }, []);

  async function handleCopy() {
    if (!onCopyText || copying) {
      return;
    }
    setCopying(true);
    if (copiedTimeoutRef.current) {
      globalThis.clearTimeout(copiedTimeoutRef.current);
      copiedTimeoutRef.current = null;
    }
    try {
      await onCopyText(copyText ?? text);
      setCopied(true);
      copiedTimeoutRef.current = globalThis.setTimeout(() => {
        setCopied(false);
        copiedTimeoutRef.current = null;
      }, 1_200);
    } catch {
      setCopied(false);
    } finally {
      setCopying(false);
    }
  }

  const markdown = (
    <div className={`pevo-markdown ${streaming ? "is-streaming" : ""}`}>
      {frontmatter.entries && <FrontmatterTable entries={frontmatter.entries} />}
      <ReactMarkdown remarkPlugins={[remarkGfm]}>{frontmatter.body}</ReactMarkdown>
    </div>
  );
  if (!onCopyText) {
    return markdown;
  }
  return (
    <div className="pevo-markdownFrame">
      <button
        aria-label={copyLabel}
        className="pevo-markdownCopy"
        disabled={copying}
        onClick={() => void handleCopy()}
        title={copyLabel}
        type="button"
      >
        {copied ? <Check size={14} aria-hidden /> : <Copy size={14} aria-hidden />}
        <span className="pevo-srOnly">{copied ? "Copied" : copyLabel}</span>
      </button>
      {markdown}
    </div>
  );
}

function FrontmatterTable({ entries }: { entries: FrontmatterEntry[] }) {
  if (entries.length === 0) {
    return null;
  }
  return (
    <table className="pevo-frontmatterTable">
      <caption className="pevo-srOnly">YAML frontmatter</caption>
      <tbody>
        {entries.map((entry, index) => (
          <tr key={`${entry.key}-${index}`}>
            <th className="pevo-frontmatterKey" scope="row">
              {entry.key}
            </th>
            <td className="pevo-frontmatterValue">
              <FrontmatterValue value={entry.value} />
            </td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}

function FrontmatterValue({ value }: { value: FrontmatterDisplayValue }) {
  switch (value.kind) {
    case "chips":
      return (
        <span className="pevo-frontmatterChips">
          {value.items.map((item, index) => (
            <code className="pevo-frontmatterChip" key={`${item}-${index}`}>
              {item}
            </code>
          ))}
        </span>
      );
    case "code":
      return <code className="pevo-frontmatterCode">{value.text}</code>;
    case "scalar":
      return <span>{value.text}</span>;
  }
}

function markdownFrontmatter(text: string): MarkdownFrontmatter {
  const split = splitFrontmatter(text);
  if (!split) {
    return { body: text, entries: null };
  }
  const entries = parseFrontmatterEntries(split.yaml);
  if (!entries) {
    return { body: text, entries: null };
  }
  return { body: split.body, entries };
}

function splitFrontmatter(text: string): { body: string; yaml: string } | null {
  const firstLineEnd = text.indexOf("\n");
  if (firstLineEnd < 0) {
    return null;
  }
  const firstLine = trimLineEnding(text.slice(0, firstLineEnd));
  if (firstLine !== FRONTMATTER_BOUNDARY) {
    return null;
  }
  let cursor = firstLineEnd + 1;
  while (cursor <= text.length) {
    const nextLineEnd = text.indexOf("\n", cursor);
    const lineEnd = nextLineEnd < 0 ? text.length : nextLineEnd;
    const line = trimLineEnding(text.slice(cursor, lineEnd));
    if (line === FRONTMATTER_BOUNDARY) {
      const bodyStart = nextLineEnd < 0 ? text.length : nextLineEnd + 1;
      return {
        body: text.slice(bodyStart),
        yaml: text.slice(firstLineEnd + 1, cursor)
      };
    }
    if (nextLineEnd < 0) {
      break;
    }
    cursor = nextLineEnd + 1;
  }
  return null;
}

function trimLineEnding(line: string): string {
  return line.endsWith("\r") ? line.slice(0, -1) : line;
}

function parseFrontmatterEntries(source: string): FrontmatterEntry[] | null {
  if (!source.trim()) {
    return [];
  }
  let yamlDocument: ReturnType<typeof parseDocument>;
  try {
    yamlDocument = parseDocument(source);
  } catch {
    return null;
  }
  if (yamlDocument.errors.length > 0) {
    return null;
  }
  const contents = yamlDocument.contents;
  if (!isMap(contents)) {
    return null;
  }
  const entries: FrontmatterEntry[] = [];
  for (const item of contents.items) {
    const key = frontmatterKey(item.key);
    if (key === null) {
      return null;
    }
    entries.push({
      key,
      value: frontmatterDisplayValue(item.value)
    });
  }
  return entries;
}

function frontmatterKey(node: unknown): string | null {
  if (!isScalar(node)) {
    return null;
  }
  const value = node.value;
  if (typeof value === "string" || typeof value === "number" || typeof value === "boolean") {
    return String(value);
  }
  if (value === null || value === undefined) {
    return "";
  }
  return null;
}

function frontmatterDisplayValue(node: unknown): FrontmatterDisplayValue {
  if (isSeq(node) && node.items.length > 0) {
    const items = node.items.map(frontmatterScalarText);
    if (items.every((item): item is string => item !== null)) {
      return { kind: "chips", items };
    }
  }
  const scalar = frontmatterScalarText(node);
  if (scalar !== null) {
    return { kind: "scalar", text: scalar };
  }
  return { kind: "code", text: boundedJson(yamlNodeToJson(node)) };
}

function frontmatterScalarText(node: unknown): string | null {
  if (node === null) {
    return "null";
  }
  if (!isScalar(node)) {
    return null;
  }
  const value = node.value;
  if (value === null || value === undefined) {
    return "null";
  }
  if (typeof value === "string" || typeof value === "number" || typeof value === "boolean") {
    return String(value);
  }
  return null;
}

function yamlNodeToJson(node: unknown): unknown {
  if (node === null) {
    return null;
  }
  if (isScalar(node)) {
    return node.value ?? null;
  }
  if (isSeq(node)) {
    return node.items.map(yamlNodeToJson);
  }
  if (isMap(node)) {
    const record: Record<string, unknown> = {};
    for (const item of node.items) {
      const key = frontmatterKey(item.key);
      record[key ?? String(recordKeyFallback(item.key))] = yamlNodeToJson(item.value);
    }
    return record;
  }
  return String(node);
}

function recordKeyFallback(value: unknown): string {
  if (isScalar(value)) {
    return frontmatterScalarText(value) ?? "";
  }
  return "";
}

function boundedJson(value: unknown): string {
  const text = JSON.stringify(value, null, 2) ?? String(value);
  if (text.length <= FRONTMATTER_CODE_MAX_CHARS) {
    return text;
  }
  return `${text.slice(0, FRONTMATTER_CODE_MAX_CHARS - 3)}...`;
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
      const step = Math.min(STREAM_REVEAL_MAX_STEP_CHARS, Math.max(1, Math.ceil(remaining / 5)));
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
