import type {
  WorkspaceFileExcalidrawParseResult,
  WorkspaceFileExcalidrawParseTask,
  WorkspaceFileOfficeParseResult,
  WorkspaceFileOfficeParseTask,
  WorkspaceFileParseResponse,
  WorkspaceFileParseResult,
  WorkspaceFileParseTask,
  WorkspaceFileTableParseResult,
  WorkspaceFileTableParseTask,
  WorkspaceFileZipParseResult,
  WorkspaceFileZipParseTask
} from "./workspace-file-parse-types";

export function runWorkspaceFileParseTask(
  task: WorkspaceFileOfficeParseTask,
  signal: AbortSignal
): Promise<WorkspaceFileOfficeParseResult>;
export function runWorkspaceFileParseTask(
  task: WorkspaceFileTableParseTask,
  signal: AbortSignal
): Promise<WorkspaceFileTableParseResult>;
export function runWorkspaceFileParseTask(
  task: WorkspaceFileZipParseTask,
  signal: AbortSignal
): Promise<WorkspaceFileZipParseResult>;
export function runWorkspaceFileParseTask(
  task: WorkspaceFileExcalidrawParseTask,
  signal: AbortSignal
): Promise<WorkspaceFileExcalidrawParseResult>;
export function runWorkspaceFileParseTask(
  task: WorkspaceFileParseTask,
  signal: AbortSignal
): Promise<WorkspaceFileParseResult>;
export function runWorkspaceFileParseTask(
  task: WorkspaceFileParseTask,
  signal: AbortSignal
): Promise<WorkspaceFileParseResult> {
  if (signal.aborted) {
    return Promise.reject(abortError());
  }
  if (typeof Worker === "undefined") {
    return import("./workspace-file-parse-executor").then(({ executeWorkspaceFileParseTask }) => (
      executeWorkspaceFileParseTask(task, signal)
    ));
  }

  const worker = new Worker(
    new URL("./workspace-file-parse.worker.ts", import.meta.url),
    { type: "module" }
  );
  return new Promise((resolve, reject) => {
    let settled = false;
    const finish = (callback: () => void) => {
      if (settled) return;
      settled = true;
      signal.removeEventListener("abort", onAbort);
      worker.terminate();
      callback();
    };
    const onAbort = () => finish(() => reject(abortError()));
    signal.addEventListener("abort", onAbort, { once: true });
    worker.onmessage = (event: MessageEvent<WorkspaceFileParseResponse>) => {
      const response = event.data;
      if (response.ok) {
        finish(() => resolve(response.result));
      } else {
        finish(() => reject(restoredError(response.error)));
      }
    };
    worker.onerror = (event) => {
      finish(() => reject(new Error(event.message || "Workspace preview parser worker failed.")));
    };

    const buffer = exactArrayBuffer(task.bytes);
    const payload = { ...task, bytes: new Uint8Array(buffer) } as WorkspaceFileParseTask;
    try {
      worker.postMessage(payload, [buffer]);
    } catch (error) {
      finish(() => reject(error));
    }
  });
}

function abortError(): DOMException {
  return new DOMException("Aborted", "AbortError");
}

function restoredError(source: { message: string; name: string }): Error {
  const error = new Error(source.message);
  error.name = source.name;
  return error;
}

function exactArrayBuffer(bytes: Uint8Array): ArrayBuffer {
  return bytes.buffer.slice(bytes.byteOffset, bytes.byteOffset + bytes.byteLength) as ArrayBuffer;
}
