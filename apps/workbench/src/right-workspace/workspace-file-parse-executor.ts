import { parseDelimitedText } from "./workspace-file-delimited";
import { readExcalidrawScene } from "./workspace-file-excalidraw-data";
import { sanitizeOfficePreview } from "./workspace-file-office";
import type {
  WorkspaceFileParseResult,
  WorkspaceFileParseTask
} from "./workspace-file-parse-types";
import { readZipDirectory } from "./workspace-file-zip";

export async function executeWorkspaceFileParseTask(
  task: WorkspaceFileParseTask,
  signal: AbortSignal
): Promise<WorkspaceFileParseResult> {
  throwIfAborted(signal);
  switch (task.kind) {
    case "office": {
      const bytes = await sanitizeOfficePreview(task.bytes, task.filename, signal);
      throwIfAborted(signal);
      return { bytes, kind: "office" };
    }
    case "table": {
      const parsed = parseDelimitedText(new TextDecoder().decode(task.bytes), task.delimiter);
      throwIfAborted(signal);
      return { kind: "table", ...parsed };
    }
    case "zip": {
      const entries = await readZipDirectory(task.bytes, signal);
      throwIfAborted(signal);
      return { entries, kind: "zip" };
    }
    case "excalidraw": {
      const scene = readExcalidrawScene(task.bytes);
      throwIfAborted(signal);
      return { kind: "excalidraw", scene };
    }
  }
}

function throwIfAborted(signal: AbortSignal) {
  if (signal.aborted) {
    throw new DOMException("Aborted", "AbortError");
  }
}
