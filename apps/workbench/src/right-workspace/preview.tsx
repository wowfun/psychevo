import { FileText } from "lucide-react";
import { MarkdownText } from "@psychevo/components";
import type { RightWorkspacePreview } from "../types";

const HTML_PREVIEW_CSP = [
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
  return (
    <div className="htmlStaticPreview">
      {active && (
        <iframe
          key={documentId}
          sandbox="allow-scripts"
          srcDoc={htmlPreviewDocument(content)}
          tabIndex={0}
          title={title || "HTML preview"}
        />
      )}
    </div>
  );
}

function htmlPreviewDocument(content: string): string {
  const policy = `<meta http-equiv="Content-Security-Policy" content="${HTML_PREVIEW_CSP}">`;
  const doctype = content.match(/^\s*<!doctype\s+html[^>]*>/i);
  if (!doctype) {
    return `${policy}${content}`;
  }
  return `${content.slice(0, doctype[0].length)}${policy}${content.slice(doctype[0].length)}`;
}
