// @vitest-environment jsdom

import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { GatewayClient } from "@psychevo/client";
import type { GatewayRequestScope } from "@psychevo/protocol";
import { WorkspaceFileGatewayAdapterProvider } from "./workspace-file-gateway-adapter";
import { WorkspaceFileSurface } from "./workspace-file-surface";

vi.mock("./workspace-file-excalidraw", async (importOriginal) => {
  const actual = await importOriginal<typeof import("./workspace-file-excalidraw")>();
  return {
    ...actual,
    ExcalidrawPreview: () => {
      throw new Error("renderer crashed");
    }
  };
});

vi.mock("./workspace-file-vendor", () => ({
  default: ({ filename }: { filename: string }) => (
    <div aria-label="Vendor preview" role="document">{filename}</div>
  )
}));

const scope: GatewayRequestScope = {
  cwd: "/workspace",
  source: {
    kind: "web",
    lifetime: "persistent",
    rawId: null,
    rawIdentity: null,
    visibleName: null
  }
};

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

describe("WorkspaceFileSurface", () => {
  it("shows only the latest target and releases a stale lease", async () => {
    const first = deferred<PreviewResult>();
    const requests: Array<{ method: string; params: unknown }> = [];
    const client = previewClient(async (method, params) => {
      requests.push({ method, params });
      if (method === "workspace/file/preview/open") {
        const path = (params as { path: string }).path;
        return path === "first.png" ? first.promise : previewResult("second.png", "image/png", "second");
      }
      if (method === "workspace/file/preview/release") {
        return { released: true };
      }
      throw new Error(`unexpected method ${method}`);
    });

    const view = renderSurface(client, "first.png");
    view.rerender(surfaceTree(client, "second.png"));

    const image = await screen.findByRole("img", { name: "Preview second.png" });
    expect(image.getAttribute("src")).toBe("http://gateway.test/_gateway/workspace-preview/second");

    await act(async () => {
      first.resolve(previewResult("first.png", "image/png", "first"));
      await first.promise;
    });

    await waitFor(() => {
      expect(requests).toContainEqual({
        method: "workspace/file/preview/release",
        params: { resourceId: "first" }
      });
    });
    expect(screen.queryByRole("img", { name: "Preview first.png" })).toBeNull();
  });

  it("pauses media while inactive and releases it on unmount", async () => {
    const pause = vi.spyOn(HTMLMediaElement.prototype, "pause").mockImplementation(() => undefined);
    const requests: Array<{ method: string; params: unknown }> = [];
    const client = previewClient(async (method, params) => {
      requests.push({ method, params });
      if (method === "workspace/file/preview/open") {
        return previewResult("clip.mp4", "video/mp4", "clip");
      }
      return { released: true };
    });
    const view = renderSurface(client, "clip.mp4");
    expect(await screen.findByLabelText("Preview clip.mp4")).toBeTruthy();

    view.rerender(surfaceTree(client, "clip.mp4", false));
    expect(pause).toHaveBeenCalled();
    view.unmount();

    await waitFor(() => {
      expect(requests).toContainEqual({
        method: "workspace/file/preview/release",
        params: { resourceId: "clip" }
      });
    });
  });

  it("terminates unfinished parsing when the Surface becomes inactive", async () => {
    const workers: Array<{ terminate: ReturnType<typeof vi.fn> }> = [];
    class PendingParseWorker {
      readonly postMessage = vi.fn();
      readonly terminate = vi.fn();
      onerror: ((event: ErrorEvent) => void) | null = null;
      onmessage: ((event: MessageEvent<unknown>) => void) | null = null;

      constructor() {
        workers.push(this);
      }
    }
    vi.stubGlobal("Worker", PendingParseWorker);
    vi.stubGlobal("fetch", vi.fn().mockResolvedValue(new Response("name,score\nAda,42\n", {
      status: 200,
      headers: { "content-length": "18", "content-type": "text/csv" }
    })));
    const client = previewClient(async (method) => {
      if (method === "workspace/file/preview/open") {
        return previewResult("scores.csv", "text/csv", "scores", { sizeBytes: 18 });
      }
      return { released: true };
    });

    const view = renderSurface(client, "scores.csv");
    await waitFor(() => expect(workers).toHaveLength(1));
    view.rerender(surfaceTree(client, "scores.csv", false));

    expect(workers[0]?.terminate).toHaveBeenCalledOnce();
  });

  it("routes native preview failures through the unified error state", async () => {
    const client = previewClient(async (method) => {
      if (method === "workspace/file/preview/open") {
        return previewResult("broken.png", "image/png", "broken");
      }
      if (method === "workspace/file/externalActions") {
        return externalActions("broken.png", ["systemDefault", "reveal"]);
      }
      return { released: true };
    });
    vi.stubGlobal("fetch", vi.fn().mockResolvedValue(new Response(null, { status: 200 })));

    renderSurface(client, "broken.png");
    fireEvent.error(await screen.findByRole("img", { name: "Preview broken.png" }));

    expect((await screen.findByRole("alert")).textContent).toContain(
      "The preview renderer could not open this file."
    );
    expect(await screen.findByRole("button", { name: "Open externally" })).toBeTruthy();
  });

  it("reopens a native streaming lease once after a 410 response", async () => {
    let opens = 0;
    const client = previewClient(async (method) => {
      if (method === "workspace/file/preview/open") {
        opens += 1;
        return previewResult("photo.png", "image/png", `photo-${opens}`);
      }
      return { released: true };
    });
    const fetchMock = vi.fn().mockResolvedValue(new Response(null, { status: 410 }));
    vi.stubGlobal("fetch", fetchMock);

    renderSurface(client, "photo.png");
    fireEvent.error(await screen.findByRole("img", { name: "Preview photo.png" }));

    await waitFor(() => expect(opens).toBe(2));
    const reopened = await screen.findByRole("img", { name: "Preview photo.png" });
    expect(reopened.getAttribute("src")).toContain("photo-2");
    fireEvent.error(reopened);

    expect((await screen.findByRole("alert")).textContent).toContain(
      "The preview renderer could not open this file."
    );
    expect(opens).toBe(2);
    expect(fetchMock).toHaveBeenCalledTimes(2);
  });

  it("reopens one expired lease and renders a bounded CSV as a table", async () => {
    let opens = 0;
    const client = previewClient(async (method) => {
      if (method === "workspace/file/preview/open") {
        opens += 1;
        return previewResult("scores.csv", "text/csv", `csv-${opens}`, {
          binary: false,
          content: "name,score\nAda,42\n",
          editable: true,
          editableReason: null,
          sizeBytes: 18
        });
      }
      return { released: true };
    });
    const fetchMock = vi.fn()
      .mockResolvedValueOnce(new Response(null, { status: 410 }))
      .mockResolvedValueOnce(new Response("name,score\nAda,42\n", {
        status: 200,
        headers: { "content-type": "text/csv" }
      }));
    vi.stubGlobal("fetch", fetchMock);

    renderSurface(client, "scores.csv");

    const table = await screen.findByRole("table", { name: "Preview scores.csv" });
    expect(table.textContent).toContain("Ada");
    expect(table.textContent).toContain("42");
    expect(opens).toBe(2);
    expect(fetchMock).toHaveBeenCalledTimes(2);
  });

  it("bounds delimited-table DOM nodes and reports truncation", async () => {
    const source = [
      "name,value",
      ...Array.from({ length: 2_100 }, (_, index) => `row-${index},${index}`)
    ].join("\n");
    const client = previewClient(async (method) => {
      if (method === "workspace/file/preview/open") {
        return previewResult("large.csv", "text/csv", "large-table", {
          sizeBytes: new TextEncoder().encode(source).byteLength
        });
      }
      return { released: true };
    });
    vi.stubGlobal("Worker", undefined);
    vi.stubGlobal("fetch", vi.fn().mockResolvedValue(new Response(source, {
      status: 200,
      headers: { "content-type": "text/csv" }
    })));

    renderSurface(client, "large.csv");

    const table = await screen.findByRole("table", { name: "Preview large.csv" });
    expect(table.querySelectorAll("tbody tr")).toHaveLength(1_999);
    expect(screen.getByText(/Table preview truncated/)).toBeTruthy();
  });

  it("uses the Gateway canonical path for renderer selection", async () => {
    const client = previewClient(async (method) => {
      if (method === "workspace/file/preview/open") {
        return previewResult("actual.pdf", "application/pdf", "canonical-pdf");
      }
      return { released: true };
    });

    renderSurface(client, "report");

    expect((await screen.findByRole("document", { name: "Vendor preview" })).textContent)
      .toBe("actual.pdf");
  });

  it("reports bounded whole-file loading progress before parsing", async () => {
    let streamController!: ReadableStreamDefaultController<Uint8Array>;
    const stream = new ReadableStream<Uint8Array>({
      start(controller) {
        streamController = controller;
      }
    });
    const client = previewClient(async (method) => {
      if (method === "workspace/file/preview/open") {
        return previewResult("scores.csv", "text/csv", "progress", { sizeBytes: 18 });
      }
      return { released: true };
    });
    vi.stubGlobal("Worker", undefined);
    vi.stubGlobal("fetch", vi.fn().mockResolvedValue(new Response(stream, {
      status: 200,
      headers: { "content-length": "18", "content-type": "text/csv" }
    })));

    renderSurface(client, "scores.csv");
    await act(async () => {
      streamController.enqueue(new TextEncoder().encode("name,"));
    });
    expect(await screen.findByText("Loading preview… 28%")).toBeTruthy();

    await act(async () => {
      streamController.enqueue(new TextEncoder().encode("score\nAda,42\n"));
      streamController.close();
    });
    expect(await screen.findByRole("table", { name: "Preview scores.csv" })).toBeTruthy();
  });

  it.each([
    ["large.docx", "application/vnd.openxmlformats-officedocument.wordprocessingml.document"],
    ["large.heic", "image/heic"]
  ])("rejects oversized whole-file bytes before rendering %s", async (path, mediaType) => {
    const client = previewClient(async (method) => {
      if (method === "workspace/file/preview/open") {
        return previewResult(path, mediaType, "large", {
          sizeBytes: 32 * 1024 * 1024 + 1
        });
      }
      return { released: true };
    });
    const fetchMock = vi.fn();
    vi.stubGlobal("fetch", fetchMock);

    renderSurface(client, path);

    expect(await screen.findByText("Preview requires the whole file and is limited to 32 MiB.")).toBeTruthy();
    expect(fetchMock).not.toHaveBeenCalled();
  });

  it("keeps the existing text editor controls and revision-aware save lane", async () => {
    const requests: Array<{ method: string; params: unknown }> = [];
    let content = "first\nsecond\n";
    const client = previewClient(async (method, params) => {
      requests.push({ method, params });
      if (method === "workspace/file/preview/open") {
        return previewResult("notes.txt", "text/plain", "notes", {
          binary: false,
          content,
          editable: true,
          editableReason: null,
          sizeBytes: content.length
        });
      }
      if (method === "workspace/file/write") {
        content = (params as { content: string }).content;
        return {
          lineEnding: "lf",
          path: "notes.txt",
          revision: "revision-saved",
          sizeBytes: 14
        };
      }
      return { released: true };
    });

    const view = renderSurface(client, "notes.txt");
    fireEvent.click(await screen.findByRole("button", { name: "Edit notes.txt" }));

    expect(screen.getByRole("searchbox", { name: "Find in file" })).toBeTruthy();
    expect(screen.getByRole("textbox", { name: "Go to line" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "Toggle word wrap" })).toBeTruthy();

    const editor = screen.getByRole("textbox", { name: "Edit notes.txt" });
    fireEvent.change(editor, { target: { value: "first\nchanged\n" } });
    fireEvent.keyDown(editor, { ctrlKey: true, key: "s" });

    await waitFor(() => {
      expect(requests).toContainEqual({
        method: "workspace/file/write",
        params: {
          content: "first\nchanged\n",
          expectedRevision: "revision-notes",
          force: false,
          path: "notes.txt",
          scope
        }
      });
    });
    await waitFor(() => {
      expect(view.container.querySelector(".rightCodePreview")?.textContent).toContain("changed");
    });
    expect(screen.queryByRole("textbox", { name: "Edit notes.txt" })).toBeNull();
    expect(screen.queryByText("unsaved")).toBeNull();
  });

  it("keeps explicitly unsupported legacy Office files in the unified error state", async () => {
    const requests: Array<{ method: string; params: unknown }> = [];
    const client = previewClient(async (method, params) => {
      requests.push({ method, params });
      if (method === "workspace/file/preview/open") {
        return previewResult("legacy.doc", "application/msword", "legacy", {
          binary: false,
          content: "plain text with a misleading extension",
          editable: true,
          editableReason: null
        });
      }
      if (method === "workspace/file/externalActions") {
        return externalActions("legacy.doc", ["systemDefault", "reveal"]);
      }
      if (method === "workspace/file/openExternal") {
        return { action: "systemDefault", path: "legacy.doc" };
      }
      return { released: true };
    });

    renderSurface(client, "legacy.doc");

    expect((await screen.findByRole("alert")).textContent).toContain(
      "Preview is not available for this file type."
    );
    fireEvent.click(await screen.findByRole("button", { name: "Open externally" }));
    await waitFor(() => {
      expect(requests).toContainEqual({
        method: "workspace/file/openExternal",
        params: { action: "systemDefault", path: "legacy.doc", scope }
      });
    });
  });

  it("treats the compound .draw.io suffix as explicitly unsupported", async () => {
    const client = previewClient(async (method) => {
      if (method === "workspace/file/preview/open") {
        return previewResult("diagram.draw.io", "application/xml", "draw-io", {
          binary: false,
          content: "<mxfile><diagram /></mxfile>",
          editable: true,
          editableReason: null
        });
      }
      if (method === "workspace/file/externalActions") {
        return externalActions("diagram.draw.io", []);
      }
      return { released: true };
    });

    renderSurface(client, "diagram.draw.io");

    expect((await screen.findByRole("alert")).textContent).toContain(
      "Preview is not available for this file type."
    );
  });

  it("does not offer external opening when the host only supports reveal", async () => {
    const client = previewClient(async (method) => {
      if (method === "workspace/file/preview/open") {
        return previewResult("legacy.ppt", "application/vnd.ms-powerpoint", "legacy-ppt");
      }
      if (method === "workspace/file/externalActions") {
        return externalActions("legacy.ppt", ["reveal"]);
      }
      return { released: true };
    });

    renderSurface(client, "legacy.ppt");

    expect(await screen.findByRole("alert")).toBeTruthy();
    await waitFor(() => {
      expect(screen.queryByRole("button", { name: "Open externally" })).toBeNull();
    });
  });

  it("contains lazy renderer crashes inside the file Surface", async () => {
    vi.spyOn(console, "error").mockImplementation(() => undefined);
    const client = previewClient(async (method) => {
      if (method === "workspace/file/preview/open") {
        return previewResult("drawing.excalidraw", "application/json", "drawing", {
          binary: false,
          content: "{\"elements\":[]}",
          sizeBytes: 15
        });
      }
      if (method === "workspace/file/externalActions") {
        return externalActions("drawing.excalidraw", []);
      }
      return { released: true };
    });
    const fetchMock = vi.fn()
      .mockResolvedValueOnce(new Response("{\"elements\":[]}", {
        status: 200,
        headers: { "content-length": "15" }
      }))
      .mockResolvedValueOnce(new Response(null, { status: 200 }));
    vi.stubGlobal("fetch", fetchMock);

    renderSurface(client, "drawing.excalidraw");

    expect((await screen.findByRole("alert")).textContent).toContain(
      "The preview renderer could not open this file."
    );
    expect(screen.getByRole("region", { name: "File preview drawing.excalidraw" })).toBeTruthy();
  });
});

type PreviewResult = {
  binary: boolean;
  content: string | null;
  editable: boolean;
  editableReason: string | null;
  expiresAtMs: number;
  lineEnding: "lf" | null;
  mediaType: string;
  path: string;
  resourceId: string;
  resourcePath: string;
  revision: string;
  sizeBytes: number;
  truncated: boolean;
  unreadable: string | null;
};

function previewResult(
  path: string,
  mediaType: string,
  resourceId: string,
  patch: Partial<PreviewResult> = {}
): PreviewResult {
  return {
    binary: true,
    content: null,
    editable: false,
    editableReason: "Binary files are read-only.",
    expiresAtMs: Date.now() + 60_000,
    lineEnding: null,
    mediaType,
    path,
    resourceId,
    resourcePath: `/_gateway/workspace-preview/${resourceId}`,
    revision: `revision-${resourceId}`,
    sizeBytes: 128,
    truncated: false,
    unreadable: null,
    ...patch
  };
}

function previewClient(
  request: (method: string, params: unknown) => Promise<unknown>
): GatewayClient {
  return {
    endpoint: {
      httpBase: "http://gateway.test",
      wsUrl: "ws://gateway.test/ws"
    },
    request
  } as unknown as GatewayClient;
}

function externalActions(
  path: string,
  availableActions: Array<"systemDefault" | "vscode" | "reveal">
) {
  return {
    path,
    category: "other" as const,
    textLike: false,
    platform: "linux" as const,
    preferredAction: availableActions[0] ?? "systemDefault",
    availableActions
  };
}

function renderSurface(client: GatewayClient, path: string, active = true) {
  return render(surfaceTree(client, path, active));
}

function surfaceTree(client: GatewayClient, path: string, active = true) {
  return (
    <WorkspaceFileGatewayAdapterProvider client={client}>
      <WorkspaceFileSurface
        active={active}
        onCompare={() => undefined}
        onDirtyChange={() => undefined}
        target={{ path, scope }}
        textEditing="enabled"
      />
    </WorkspaceFileGatewayAdapterProvider>
  );
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((next) => {
    resolve = next;
  });
  return { promise, resolve };
}
