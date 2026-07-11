import {
  Children,
  isValidElement,
  useEffect,
  useId,
  useMemo,
  useRef,
  useState,
  type CSSProperties,
  type ReactNode
} from "react";
import { Check, Copy, FileText, Maximize2, RotateCcw, X, ZoomIn, ZoomOut } from "lucide-react";
import ReactMarkdown, { type Components } from "react-markdown";
import remarkGfm from "remark-gfm";
import { isMap, isScalar, isSeq, parseDocument } from "yaml";
import {
  workspaceFileRemarkPlugin,
  workspacePathFromLinkNode,
  type WorkspaceFileLinkContext
} from "./workspaceFileLinks";

export interface MarkdownTextProps {
  copyLabel?: string;
  copyText?: string;
  mermaidLoader?: MermaidLoader;
  onCopyText?: ((text: string) => void | Promise<void>) | undefined;
  streaming?: boolean;
  text: string;
  workspaceFileLinks?: WorkspaceFileLinkContext;
}

export type MermaidLoader = () => Promise<MermaidModule>;

type MermaidModule = MermaidApi | { default: MermaidApi };

type MermaidApi = {
  initialize?: (options: Record<string, unknown>) => void;
  render: (id: string, source: string) => Promise<{ svg: string } | string> | { svg: string } | string;
};

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

export function MarkdownText({
  copyLabel = "Copy Markdown",
  copyText,
  mermaidLoader = loadMermaid,
  onCopyText,
  streaming,
  text,
  workspaceFileLinks
}: MarkdownTextProps) {
  const [copying, setCopying] = useState(false);
  const [copied, setCopied] = useState(false);
  const copiedTimeoutRef = useRef<ReturnType<typeof globalThis.setTimeout> | null>(null);
  const visibleText = useStreamingReveal(text, streaming === true);
  const frontmatter = useMemo(() => markdownFrontmatter(visibleText), [visibleText]);
  const completeMermaidOccurrences = useMemo(
    () => extractCompleteMermaidOccurrences(frontmatter.body),
    [frontmatter.body]
  );
  const workspaceRemarkPlugin = useMemo(
    () => workspaceFileLinks ? workspaceFileRemarkPlugin(workspaceFileLinks, streaming === true) : null,
    [streaming, workspaceFileLinks]
  );
  const markdownComponents = useMemo<Components>(() => ({
    a({ children, node, ...props }) {
      const workspacePath = workspacePathFromLinkNode(node);
      if (workspacePath && workspaceFileLinks) {
        const visibleLabel = markdownInlineText(children) || workspacePath;
        return (
          <button
            aria-label={`Open file ${visibleLabel}`}
            className="pevo-workspaceFileLink"
            onClick={() => void workspaceFileLinks.onOpen(workspacePath)}
            title={workspacePath}
            type="button"
          >
            <FileText size={13} aria-hidden />
            <span>{children}</span>
          </button>
        );
      }
      return <a {...props}>{children}</a>;
    },
    code({ children, className, node, ...props }) {
      const language = markdownCodeLanguage(className);
      const source = normalizeMermaidSource(String(children).replace(/\n$/, ""));
      const openingLine = node?.position?.start.line;
      if (
        language === "mermaid" &&
        typeof openingLine === "number" &&
        completeMermaidOccurrences.get(openingLine) === source
      ) {
        return (
          <MermaidBlock
            mermaidLoader={mermaidLoader}
            onCopyText={onCopyText}
            source={source}
          />
        );
      }
      return (
        <code className={className} {...props}>
          {children}
        </code>
      );
    },
    pre({ children, node: _node, ...props }) {
      if (containsOnlyMermaidBlock(children)) {
        return <>{children}</>;
      }
      return <pre {...props}>{children}</pre>;
    }
  }), [completeMermaidOccurrences, mermaidLoader, onCopyText, workspaceFileLinks]);

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
      <ReactMarkdown
        components={markdownComponents}
        remarkPlugins={workspaceRemarkPlugin ? [remarkGfm, workspaceRemarkPlugin] : [remarkGfm]}
      >
        {frontmatter.body}
      </ReactMarkdown>
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

function markdownInlineText(children: ReactNode): string {
  return Children.toArray(children).map((child) => {
    if (typeof child === "string" || typeof child === "number") {
      return String(child);
    }
    if (!isValidElement<{ children?: ReactNode }>(child)) {
      return "";
    }
    return markdownInlineText(child.props.children);
  }).join("");
}

type MermaidBlockProps = {
  mermaidLoader: MermaidLoader;
  onCopyText: ((text: string) => void | Promise<void>) | undefined;
  source: string;
};

type MermaidRenderState =
  | { kind: "loading" }
  | { kind: "rendered"; size: MermaidSvgSize | null; svg: string }
  | { error: string; kind: "error" };

type MermaidSvgSize = {
  height: number;
  width: number;
};

type MermaidViewMode = "fit" | "actual";

const MERMAID_ZOOM_MIN = 0.5;
const MERMAID_ZOOM_MAX = 3;
const MERMAID_ZOOM_STEP = 0.25;

function MermaidBlock({ mermaidLoader, onCopyText, source }: MermaidBlockProps) {
  const stableId = useId().replace(/[^A-Za-z0-9_-]/g, "");
  const renderId = useMemo(() => `pevo-mermaid-${stableId}-${hashString(source)}`, [source, stableId]);
  const [state, setState] = useState<MermaidRenderState>({ kind: "loading" });
  const [viewMode, setViewMode] = useState<MermaidViewMode>("fit");
  const [zoom, setZoom] = useState(1);
  const [expanded, setExpanded] = useState(false);
  const [copying, setCopying] = useState(false);
  const [copied, setCopied] = useState(false);
  const copiedTimeoutRef = useRef<ReturnType<typeof globalThis.setTimeout> | null>(null);

  useEffect(() => () => {
    if (copiedTimeoutRef.current) {
      globalThis.clearTimeout(copiedTimeoutRef.current);
    }
  }, []);

  useEffect(() => {
    if (!expanded) {
      return;
    }
    function handleKeyDown(event: KeyboardEvent) {
      if (event.key === "Escape") {
        setExpanded(false);
      }
    }
    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [expanded]);

  useEffect(() => {
    let canceled = false;
    setState({ kind: "loading" });
    setViewMode("fit");
    setZoom(1);
    setExpanded(false);
    void mermaidLoader()
      .then((module) => {
        const mermaid = resolveMermaidApi(module);
        mermaid.initialize?.({
          fontFamily: "inherit",
          securityLevel: "strict",
          startOnLoad: false,
          theme: "base"
        });
        return mermaid.render(renderId, source);
      })
      .then((result) => {
        if (canceled) {
          return;
        }
        const svg = typeof result === "string" ? result : result.svg;
        setState({
          kind: "rendered",
          size: parseMermaidSvgSize(svg),
          svg
        });
      })
      .catch((error: unknown) => {
        if (canceled) {
          return;
        }
        setState({
          error: error instanceof Error ? error.message : String(error),
          kind: "error"
        });
      });
    return () => {
      canceled = true;
    };
  }, [mermaidLoader, renderId, source]);

  async function copySource() {
    if (!onCopyText || copying) {
      return;
    }
    setCopying(true);
    if (copiedTimeoutRef.current) {
      globalThis.clearTimeout(copiedTimeoutRef.current);
      copiedTimeoutRef.current = null;
    }
    try {
      await onCopyText(source);
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

  function zoomBy(delta: number) {
    setZoom((value) => clampMermaidZoom(value + delta));
  }

  function resetView() {
    setViewMode("fit");
    setZoom(1);
  }

  const rendered = state.kind === "rendered";
  const zoomLabel = mermaidZoomLabel(zoom);

  function renderCanvas(expandedCanvas = false) {
    const canvasClassName = [
      "pevo-mermaidCanvas",
      `is-${viewMode}`,
      expandedCanvas ? "is-expandedCanvas" : ""
    ].filter(Boolean).join(" ");
    return (
      <div className={canvasClassName} aria-label="Mermaid diagram">
        {state.kind === "loading" && <span>Rendering diagram</span>}
        {state.kind === "rendered" && (
          <div
            className="pevo-mermaidViewport"
            style={mermaidViewportStyle(state.size, viewMode, zoom)}
          >
            <div
              className="pevo-mermaidSvg"
              dangerouslySetInnerHTML={{ __html: state.svg }}
              style={mermaidSvgStyle(state.size, viewMode, zoom)}
            />
          </div>
        )}
        {state.kind === "error" && (
          <div className="pevo-mermaidError" role="alert">
            <strong>Diagram error</strong>
            <span>{state.error}</span>
            <pre><code>{source}</code></pre>
          </div>
        )}
      </div>
    );
  }

  return (
    <figure className={`pevo-mermaidBlock is-${state.kind}`}>
      <figcaption>
        <span>Diagram</span>
        <div className="pevo-mermaidToolbar" aria-label="Mermaid diagram controls">
          <div className="pevo-mermaidMode" role="group" aria-label="Mermaid diagram size">
            <button
              aria-label="Fit Mermaid diagram to width"
              aria-pressed={viewMode === "fit"}
              disabled={!rendered}
              onClick={() => setViewMode("fit")}
              title="Fit to width"
              type="button"
            >
              Fit
            </button>
            <button
              aria-label="View Mermaid diagram at original size"
              aria-pressed={viewMode === "actual"}
              disabled={!rendered}
              onClick={() => setViewMode("actual")}
              title="Original size"
              type="button"
            >
              1:1
            </button>
          </div>
          <button
            aria-label="Zoom out Mermaid diagram"
            disabled={!rendered || zoom <= MERMAID_ZOOM_MIN}
            onClick={() => zoomBy(-MERMAID_ZOOM_STEP)}
            title="Zoom out"
            type="button"
          >
            <ZoomOut size={13} aria-hidden />
          </button>
          <span className="pevo-mermaidZoomValue" aria-label="Mermaid zoom">{zoomLabel}</span>
          <button
            aria-label="Zoom in Mermaid diagram"
            disabled={!rendered || zoom >= MERMAID_ZOOM_MAX}
            onClick={() => zoomBy(MERMAID_ZOOM_STEP)}
            title="Zoom in"
            type="button"
          >
            <ZoomIn size={13} aria-hidden />
          </button>
          <button
            aria-label="Reset Mermaid view"
            disabled={!rendered || (viewMode === "fit" && zoom === 1)}
            onClick={resetView}
            title="Reset view"
            type="button"
          >
            <RotateCcw size={13} aria-hidden />
          </button>
          <button
            aria-label="Expand Mermaid diagram"
            disabled={!rendered}
            onClick={() => setExpanded(true)}
            title="Expand diagram"
            type="button"
          >
            <Maximize2 size={13} aria-hidden />
          </button>
          {onCopyText && (
            <button
              aria-label="Copy Mermaid source"
              disabled={copying}
              onClick={() => void copySource()}
              title="Copy Mermaid source"
              type="button"
            >
              {copied ? <Check size={13} aria-hidden /> : <Copy size={13} aria-hidden />}
            </button>
          )}
        </div>
      </figcaption>
      {renderCanvas()}
      {expanded && state.kind === "rendered" && (
        <div className="pevo-mermaidOverlay" role="dialog" aria-label="Mermaid diagram" aria-modal="true">
          <div className="pevo-mermaidOverlayPanel">
            <header>
              <strong>Diagram</strong>
              <button
                aria-label="Close Mermaid diagram"
                onClick={() => setExpanded(false)}
                title="Close"
                type="button"
              >
                <X size={15} aria-hidden />
              </button>
            </header>
            {renderCanvas(true)}
          </div>
        </div>
      )}
    </figure>
  );
}

function clampMermaidZoom(value: number): number {
  return Math.min(MERMAID_ZOOM_MAX, Math.max(MERMAID_ZOOM_MIN, Math.round(value * 100) / 100));
}

function mermaidZoomLabel(zoom: number): string {
  return `${Math.round(zoom * 100)}%`;
}

function mermaidViewportStyle(
  size: MermaidSvgSize | null,
  viewMode: MermaidViewMode,
  zoom: number
): CSSProperties {
  if (viewMode === "actual" && size) {
    return {
      height: `${size.height * zoom}px`,
      width: `${size.width * zoom}px`
    };
  }
  return {
    minWidth: 0,
    width: mermaidZoomLabel(zoom)
  };
}

function mermaidSvgStyle(
  size: MermaidSvgSize | null,
  viewMode: MermaidViewMode,
  zoom: number
): CSSProperties {
  if (viewMode !== "actual" || !size) {
    return {};
  }
  return {
    height: `${size.height}px`,
    transform: `scale(${zoom})`,
    transformOrigin: "top left",
    width: `${size.width}px`
  };
}

function parseMermaidSvgSize(svg: string): MermaidSvgSize | null {
  const width = parseSvgNumericAttribute(svg, "width");
  const height = parseSvgNumericAttribute(svg, "height");
  if (width && height) {
    return { height, width };
  }
  const viewBox = /\bviewBox\s*=\s*["']\s*([+-]?(?:\d+\.?\d*|\.\d+)(?:[,\s]+[+-]?(?:\d+\.?\d*|\.\d+)){3})\s*["']/u.exec(svg);
  if (!viewBox?.[1]) {
    return null;
  }
  const numbers = viewBox[1].trim().split(/[\s,]+/u).map(Number);
  const viewBoxWidth = numbers.at(2);
  const viewBoxHeight = numbers.at(3);
  if (
    typeof viewBoxWidth !== "number" ||
    typeof viewBoxHeight !== "number" ||
    !Number.isFinite(viewBoxWidth) ||
    !Number.isFinite(viewBoxHeight) ||
    viewBoxWidth <= 0 ||
    viewBoxHeight <= 0
  ) {
    return null;
  }
  return {
    height: viewBoxHeight,
    width: viewBoxWidth
  };
}

function parseSvgNumericAttribute(svg: string, attribute: "height" | "width"): number | null {
  const pattern = new RegExp(`\\b${attribute}\\s*=\\s*["']\\s*([^"']+)\\s*["']`, "u");
  const match = pattern.exec(svg);
  const value = match?.[1];
  if (!value) {
    return null;
  }
  const number = Number.parseFloat(value);
  if (!Number.isFinite(number) || number <= 0) {
    return null;
  }
  return number;
}

function resolveMermaidApi(module: MermaidModule): MermaidApi {
  if ("default" in module) {
    return module.default;
  }
  return module;
}

function loadMermaid(): Promise<MermaidModule> {
  return import("mermaid");
}

function containsOnlyMermaidBlock(children: ReactNode): boolean {
  if (Children.count(children) !== 1) {
    return false;
  }
  const only = Children.only(children);
  return isValidElement(only) && only.type === MermaidBlock;
}

function markdownCodeLanguage(className: string | undefined): string | null {
  const match = /(?:^|\s)language-([A-Za-z0-9_-]+)/.exec(className ?? "");
  return match?.[1]?.toLowerCase() ?? null;
}

function extractCompleteMermaidOccurrences(text: string): Map<number, string> {
  const lines = text.replace(/\r\n/g, "\n").split("\n");
  const occurrences = new Map<number, string>();
  for (let index = 0; index < lines.length; index += 1) {
    const opening = /^([ \t]*)(`{3,}|~{3,})[ \t]*mermaid(?:[ \t].*)?$/.exec(lines[index] ?? "");
    if (!opening) {
      continue;
    }
    const fence = opening[2] ?? "```";
    const fenceChar = fence[0] ?? "`";
    const fenceLength = fence.length;
    const body: string[] = [];
    for (let cursor = index + 1; cursor < lines.length; cursor += 1) {
      const line = lines[cursor] ?? "";
      if (isClosingFence(line, fenceChar, fenceLength)) {
        occurrences.set(index + 1, normalizeMermaidSource(body.join("\n")));
        index = cursor;
        break;
      }
      body.push(line);
    }
  }
  return occurrences;
}

function isClosingFence(line: string, fenceChar: string, fenceLength: number): boolean {
  const trimmed = line.trim();
  return trimmed.length >= fenceLength && [...trimmed].every((char) => char === fenceChar);
}

function normalizeMermaidSource(source: string): string {
  return source.replace(/\r\n/g, "\n").trim();
}

function hashString(value: string): string {
  let hash = 5381;
  for (let index = 0; index < value.length; index += 1) {
    hash = ((hash << 5) + hash) ^ value.charCodeAt(index);
  }
  return (hash >>> 0).toString(36);
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
