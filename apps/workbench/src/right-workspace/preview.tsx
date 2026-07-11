import { useEffect, useState } from "react";
import { FileText, ShieldCheck } from "lucide-react";
import { MarkdownText } from "@psychevo/components";
import type { RightWorkspacePreview } from "../types";

const LOCKED_HTML_PREVIEW_CSP = [
  "default-src 'none'",
  "base-uri 'none'",
  "connect-src 'none'",
  "form-action 'none'",
  "frame-src 'none'",
  "object-src 'none'",
  "script-src 'none'",
  "style-src 'unsafe-inline'",
  "img-src data: blob:",
  "font-src data:",
  "media-src data: blob:",
  "worker-src 'none'",
  "navigate-to 'none'"
].join("; ");
const INTERACTIVE_HTML_PREVIEW_CSP = [
  "base-uri 'none'",
  "form-action 'none'",
  "frame-src 'none'",
  "object-src 'none'"
].join("; ");

export function PreviewPanel({
  htmlExecutionActive,
  onCopyText,
  preview
}: {
  htmlExecutionActive: boolean;
  onCopyText?: ((text: string) => void | Promise<void>) | undefined;
  preview: RightWorkspacePreview;
}) {
  return (
    <section className="previewPanel" aria-label="Preview">
      <header>
        <FileText size={17} />
        <div>
          <h2>{preview.title || "Preview"}</h2>
          {preview.path && <p title={preview.path}>{preview.path}</p>}
        </div>
      </header>
      {preview.kind === "html" ? (
        <HtmlStaticPreview
          active={htmlExecutionActive}
          content={preview.content}
          documentId={preview.path ?? preview.title}
          title={preview.title}
        />
      ) : (
        <div className="previewMarkdown">
          <MarkdownText
            copyLabel="Copy Markdown preview"
            copyText={preview.content}
            onCopyText={onCopyText}
            text={preview.content}
          />
        </div>
      )}
    </section>
  );
}

export function HtmlStaticPreview({
  active = true,
  content,
  documentId,
  title
}: {
  active?: boolean;
  content: string;
  documentId: string;
  title: string;
}) {
  const [trust, setTrust] = useState({ content, documentId, trusted: false });
  const interactive = trust.trusted && trust.content === content && trust.documentId === documentId;

  useEffect(() => {
    setTrust((current) => (
      current.content === content && current.documentId === documentId
        ? current
        : { content, documentId, trusted: false }
    ));
  }, [content, documentId]);

  return (
    <div className={`htmlStaticPreview ${interactive ? "is-interactive" : "is-locked"}`}>
      <div className="htmlPreviewNotice">
        <ShieldCheck size={14} aria-hidden />
        <span>HTML preview</span>
        <small>
          {interactive ? "Trusted · scripts + network on" : "Locked · run enables scripts + network"}
        </small>
        <button
          onClick={() => setTrust({ content, documentId, trusted: !interactive })}
          title={interactive ? "Stop scripts" : "Trust and run this HTML with scripts and network access"}
          type="button"
        >
          {interactive ? "Stop interactive preview" : "Run interactive preview"}
        </button>
      </div>
      {active && (
        <iframe
          aria-hidden={interactive ? undefined : true}
          inert={!interactive}
          key={interactive ? "interactive" : "locked"}
          sandbox={interactive ? "allow-scripts" : ""}
          srcDoc={htmlPreviewDocument(content, interactive)}
          tabIndex={interactive ? 0 : -1}
          title={title || "HTML preview"}
        />
      )}
    </div>
  );
}

function htmlPreviewDocument(content: string, interactive: boolean): string {
  const csp = interactive ? INTERACTIVE_HTML_PREVIEW_CSP : LOCKED_HTML_PREVIEW_CSP;
  const policy = `<meta http-equiv="Content-Security-Policy" content="${csp}">`;
  const doctype = content.match(/^\s*<!doctype\s+html[^>]*>/i);
  if (!doctype) {
    return `${policy}${content}`;
  }
  return `${content.slice(0, doctype[0].length)}${policy}${content.slice(doctype[0].length)}`;
}
