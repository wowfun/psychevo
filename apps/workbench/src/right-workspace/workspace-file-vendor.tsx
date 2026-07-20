import { useCallback, useEffect, useMemo, useState } from "react";
import FileViewer, { type ViewerState } from "@file-viewer/react";
import {
  fileExtension,
  loadWorkspaceFileRenderers,
  type WorkspaceFileRendererPlugin
} from "./workspace-file-renderers";

export default function VendorFilePreview({
  active,
  buffer,
  filename,
  mediaType,
  onStateChange,
  size,
  url
}: {
  active: boolean;
  buffer?: ArrayBuffer;
  filename: string;
  mediaType: string;
  onStateChange(ready: boolean, error: unknown | null): void;
  size: number;
  url?: string;
}) {
  const [renderers, setRenderers] = useState<WorkspaceFileRendererPlugin[] | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    setRenderers(null);
    setError(null);
  }, [filename]);

  useEffect(() => {
    let disposed = false;
    if (!active || renderers || error) {
      return () => {
        disposed = true;
      };
    }
    void loadWorkspaceFileRenderers(filename).then(
      (configuredRenderers) => {
        if (!disposed) {
          setRenderers(configuredRenderers);
        }
      },
      (reason) => {
        if (!disposed) {
          const message = reason instanceof Error ? reason.message : String(reason);
          setError(message);
          onStateChange(false, reason);
        }
      }
    );
    return () => {
      disposed = true;
    };
  }, [active, error, filename, onStateChange, renderers]);

  const options = useMemo(() => ({
    autoRenderers: false,
    builtinRenderers: "none" as const,
    rendererMode: "replace" as const,
    renderers: renderers as never,
    styleIsolation: "scoped" as const,
    toolbar: {
      download: false,
      exportHtml: false,
      print: false,
      search: true,
      theme: false,
      zoom: true,
      permissions: {
        download: false,
        "export-html": false,
        print: false
      }
    },
    pdf: {
      rangeChunkSize: 16 * 1024,
      streaming: true,
      withCredentials: false
    },
    docx: {
      worker: true,
      workerJsZipUrl: "vendor/docx/jszip.min.js",
      workerUrl: "vendor/docx/docx.worker.js"
    },
    spreadsheet: {
      worker: "auto" as const,
      workerUrl: "vendor/xlsx/sheet.worker.js"
    },
    presentation: {
      workerUrl: "vendor/pptx/pptx.worker.js"
    }
  }), [renderers]);
  const handleViewerStateChange = useCallback((state: ViewerState) => {
    if (state.error) {
      onStateChange(false, state.error);
    } else if (state.ready) {
      onStateChange(true, null);
    }
  }, [onStateChange]);

  if (error) {
    return <p role="alert">The document renderer could not be loaded.</p>;
  }
  if (!renderers) {
    return <p role="status">Loading renderer…</p>;
  }
  return (
    <div
      className="workspaceFileVendor"
      data-media-type={mediaType}
      onClickCapture={(event) => {
        const anchor = (event.target as Element | null)?.closest?.("a[href]");
        if (anchor) {
          event.preventDefault();
          event.stopPropagation();
        }
      }}
    >
      <FileViewer
        filename={filename}
        size={size}
        type={fileExtension(filename)}
        {...(buffer ? { buffer } : {})}
        {...(url ? { url } : {})}
        options={options}
        onStateChange={handleViewerStateChange}
      />
    </div>
  );
}
