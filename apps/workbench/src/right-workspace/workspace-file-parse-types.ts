import type { ExcalidrawDocument } from "./workspace-file-excalidraw-data";
import type { DelimitedTableLimits } from "./workspace-file-delimited";
import type { ZipDirectoryEntry } from "./workspace-file-zip";

type ParseTaskBase = { bytes: Uint8Array };

export type WorkspaceFileOfficeParseTask = ParseTaskBase & {
  filename: string;
  kind: "office";
};

export type WorkspaceFileTableParseTask = ParseTaskBase & {
  delimiter: string;
  kind: "table";
};

export type WorkspaceFileZipParseTask = ParseTaskBase & { kind: "zip" };
export type WorkspaceFileExcalidrawParseTask = ParseTaskBase & { kind: "excalidraw" };

export type WorkspaceFileParseTask =
  | WorkspaceFileOfficeParseTask
  | WorkspaceFileTableParseTask
  | WorkspaceFileZipParseTask
  | WorkspaceFileExcalidrawParseTask;

export type WorkspaceFileOfficeParseResult = { bytes: Uint8Array; kind: "office" };
export type WorkspaceFileTableParseResult = {
  kind: "table";
  limits: DelimitedTableLimits;
  rows: string[][];
  truncated: boolean;
};
export type WorkspaceFileZipParseResult = { entries: ZipDirectoryEntry[]; kind: "zip" };
export type WorkspaceFileExcalidrawParseResult = {
  document: ExcalidrawDocument;
  kind: "excalidraw";
};

export type WorkspaceFileParseResult =
  | WorkspaceFileOfficeParseResult
  | WorkspaceFileTableParseResult
  | WorkspaceFileZipParseResult
  | WorkspaceFileExcalidrawParseResult;

export type WorkspaceFileParseResponse =
  | { ok: true; result: WorkspaceFileParseResult }
  | { error: { message: string; name: string }; ok: false };
