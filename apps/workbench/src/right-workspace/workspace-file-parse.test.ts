import { afterEach, describe, expect, it, vi } from "vitest";
import { runWorkspaceFileParseTask } from "./workspace-file-parse";

class MockWorker {
  static instances: MockWorker[] = [];

  readonly postMessage = vi.fn();
  readonly terminate = vi.fn();
  onerror: ((event: ErrorEvent) => void) | null = null;
  onmessage: ((event: MessageEvent<unknown>) => void) | null = null;

  constructor() {
    MockWorker.instances.push(this);
  }
}

afterEach(() => {
  MockWorker.instances = [];
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

describe("workspace preview parse worker", () => {
  it("terminates the task worker immediately when its signal aborts", async () => {
    vi.stubGlobal("Worker", MockWorker);
    const controller = new AbortController();
    const pending = runWorkspaceFileParseTask({
      bytes: new Uint8Array([0x61, 0x2c, 0x62]),
      delimiter: ",",
      kind: "table"
    }, controller.signal);
    const worker = MockWorker.instances[0];

    expect(worker).toBeTruthy();
    expect(worker?.postMessage).toHaveBeenCalledOnce();
    controller.abort();

    expect(worker?.terminate).toHaveBeenCalledOnce();
    await expect(pending).rejects.toMatchObject({ name: "AbortError" });
  });

  it("terminates the task worker after success", async () => {
    vi.stubGlobal("Worker", MockWorker);
    const pending = runWorkspaceFileParseTask({
      bytes: new TextEncoder().encode("name,score\nAda,42\n"),
      delimiter: ",",
      kind: "table"
    }, new AbortController().signal);
    const worker = MockWorker.instances[0];

    worker?.onmessage?.({
      data: {
        ok: true,
        result: {
          kind: "table",
          limits: { maxCells: 20_000, maxColumns: 100, maxRows: 2_000 },
          rows: [["name", "score"], ["Ada", "42"]],
          truncated: false
        }
      }
    } as MessageEvent<unknown>);

    await expect(pending).resolves.toEqual({
      kind: "table",
      limits: { maxCells: 20_000, maxColumns: 100, maxRows: 2_000 },
      rows: [["name", "score"], ["Ada", "42"]],
      truncated: false
    });
    expect(worker?.terminate).toHaveBeenCalledOnce();
  });

  it("terminates the task worker after failure", async () => {
    vi.stubGlobal("Worker", MockWorker);
    const pending = runWorkspaceFileParseTask({
      bytes: new Uint8Array([0x50, 0x4b]),
      kind: "zip"
    }, new AbortController().signal);
    const worker = MockWorker.instances[0];

    worker?.onmessage?.({
      data: {
        error: { message: "Invalid ZIP", name: "Error" },
        ok: false
      }
    } as MessageEvent<unknown>);

    await expect(pending).rejects.toThrow("Invalid ZIP");
    expect(worker?.terminate).toHaveBeenCalledOnce();
  });

  it("uses the same pure parser when Worker is unavailable", async () => {
    vi.stubGlobal("Worker", undefined);

    await expect(runWorkspaceFileParseTask({
      bytes: new TextEncoder().encode("name,score\nAda,42\n"),
      delimiter: ",",
      kind: "table"
    }, new AbortController().signal)).resolves.toEqual({
      kind: "table",
      limits: { maxCells: 20_000, maxColumns: 100, maxRows: 2_000 },
      rows: [["name", "score"], ["Ada", "42"]],
      truncated: false
    });
  });
});
