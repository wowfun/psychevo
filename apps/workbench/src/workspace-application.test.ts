import { describe, expect, it, vi } from "vitest";
import type { GatewayClient } from "@psychevo/client";
import type {
  GatewayRequestScope,
  WorkspaceFilesResult
} from "@psychevo/protocol";
import { WorkspaceApplication } from "./workspace-application";

const scope = (cwd: string): GatewayRequestScope => ({
  cwd,
  source: { kind: "web", rawId: `scope:${cwd}`, lifetime: "persistent" }
});

const files = (root: string): WorkspaceFilesResult => ({
  root,
  entries: [],
  truncated: false
});

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((next) => {
    resolve = next;
  });
  return { promise, resolve };
}

describe("WorkspaceApplication", () => {
  it("single-flights a facet and rejects a response from the prior scope", async () => {
    const first = deferred<WorkspaceFilesResult>();
    const second = deferred<WorkspaceFilesResult>();
    const request = vi.fn((_method: string, params: unknown) => (
      (params as { scope: GatewayRequestScope }).scope.cwd === "/a"
        ? first.promise
        : second.promise
    ));
    const client = { request } as unknown as GatewayClient;
    const application = new WorkspaceApplication();

    const a1 = application.refresh("files", client, scope("/a"));
    const a2 = application.refresh("files", client, scope("/a"));
    expect(request).toHaveBeenCalledTimes(1);

    const b = application.refresh("files", client, scope("/b"));
    first.resolve(files("/a"));
    await Promise.all([a1, a2]);
    expect(application.getSnapshot().files).toBeNull();

    second.resolve(files("/b"));
    await b;
    expect(application.getSnapshot().files?.root).toBe("/b");
  });

  it("keeps facet revisions independent and mutations beat late reads", async () => {
    const pendingFiles = deferred<WorkspaceFilesResult>();
    const request = vi.fn(async (method: string) => {
      if (method === "workspace/files") {
        return pendingFiles.promise;
      }
      if (method === "workspace/diff") {
        return {
          isGitRepo: true,
          files: [],
          unifiedDiff: "",
          truncation: {
            truncated: false,
            maxBytes: 1,
            maxLines: 1,
            omittedBytes: 0,
            omittedLines: 0
          },
          selectedPath: null
        };
      }
      throw new Error(`unexpected method: ${method}`);
    });
    const client = { request } as unknown as GatewayClient;
    const application = new WorkspaceApplication();
    const activeScope = scope("/repo");

    const pending = application.refresh("files", client, activeScope);
    await application.refresh("diff", client, activeScope);
    application.setFiles(files("/mutation"));
    pendingFiles.resolve(files("/stale"));
    await pending;

    expect(application.getSnapshot().files?.root).toBe("/mutation");
    expect(application.getSnapshot().diff?.isGitRepo).toBe(true);
    await application.ensure("diff", client, activeScope);
    expect(request.mock.calls.filter(([method]) => method === "workspace/diff")).toHaveLength(1);
  });
});
