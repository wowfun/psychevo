import { executeWorkspaceFileParseTask } from "./workspace-file-parse-executor";
import type {
  WorkspaceFileParseResponse,
  WorkspaceFileParseTask
} from "./workspace-file-parse-types";

const workerScope = self as unknown as {
  onmessage: ((event: MessageEvent<WorkspaceFileParseTask>) => void) | null;
  postMessage(message: WorkspaceFileParseResponse, transfer?: Transferable[]): void;
};

workerScope.onmessage = (event) => {
  void executeWorkspaceFileParseTask(event.data, new AbortController().signal).then(
    (result) => {
      const transfer = result.kind === "office"
        ? [exactArrayBuffer(result.bytes)]
        : [];
      const response = result.kind === "office"
        ? { ...result, bytes: new Uint8Array(transfer[0] as ArrayBuffer) }
        : result;
      workerScope.postMessage({ ok: true, result: response }, transfer);
    },
    (error) => {
      workerScope.postMessage({
        error: serializedError(error),
        ok: false
      });
    }
  );
};

function serializedError(error: unknown): { message: string; name: string } {
  return error instanceof Error
    ? { message: error.message, name: error.name }
    : { message: String(error), name: "Error" };
}

function exactArrayBuffer(bytes: Uint8Array): ArrayBuffer {
  return bytes.buffer.slice(bytes.byteOffset, bytes.byteOffset + bytes.byteLength) as ArrayBuffer;
}
