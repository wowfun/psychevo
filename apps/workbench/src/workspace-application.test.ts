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
  let reject!: (reason: unknown) => void;
  const promise = new Promise<T>((next, fail) => {
    resolve = next;
    reject = fail;
  });
  return { promise, reject, resolve };
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

    const a1 = application.ensure("files", client, scope("/a"));
    const a2 = application.ensure("files", client, scope("/a"));
    expect(request).toHaveBeenCalledTimes(1);

    const b = application.refresh("files", client, scope("/b"));
    first.resolve(files("/a"));
    await Promise.all([a1, a2]);
    expect(application.getSnapshot().files).toBeNull();

    second.resolve(files("/b"));
    await b;
    expect(application.getSnapshot().files?.root).toBe("/b");
  });

  it("invalidates an older refresh and returns null from stale diff and branch reads", async () => {
    const firstFiles = deferred<WorkspaceFilesResult>();
    const secondFiles = deferred<WorkspaceFilesResult>();
    const staleDiff = deferred<Record<string, unknown>>();
    const staleBranches = deferred<Record<string, unknown>>();
    let fileReads = 0;
    const request = vi.fn((method: string) => {
      if (method === "workspace/files") {
        fileReads += 1;
        return fileReads === 1 ? firstFiles.promise : secondFiles.promise;
      }
      if (method === "workspace/diff") {
        return staleDiff.promise;
      }
      if (method === "workspace/git/branches") {
        return staleBranches.promise;
      }
      throw new Error(`unexpected method: ${method}`);
    });
    const client = { request } as unknown as GatewayClient;
    const application = new WorkspaceApplication();
    const activeScope = scope("/a");

    const firstRefresh = application.refresh("files", client, activeScope);
    const secondRefresh = application.refresh("files", client, activeScope);
    firstFiles.resolve(files("/stale-refresh"));
    await firstRefresh;
    expect(application.getSnapshot().files).toBeNull();
    secondFiles.resolve(files("/fresh-refresh"));
    await secondRefresh;
    expect(application.getSnapshot().files?.root).toBe("/fresh-refresh");

    const diff = application.readDiff("src/main.ts", client, activeScope);
    const branches = application.readBranches(client, activeScope);
    application.bind(client, scope("/b"));
    staleDiff.resolve({
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
      selectedPath: "src/main.ts"
    });
    staleBranches.resolve({
      branches: ["main"],
      current: "main",
      detached: false,
      isGitRepo: true
    });

    expect(await diff).toBeNull();
    expect(await branches).toBeNull();
    expect(application.getSnapshot().branch).toBeUndefined();
    expect(application.getSnapshot().diff).toBeNull();
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

  it("keeps the selected Files root for subsequent background refreshes", async () => {
    const requestedRoots: string[] = [];
    const client = {
      request: vi.fn(async (method: string, params: unknown) => {
        if (method !== "workspace/files") throw new Error(`unexpected method: ${method}`);
        const root = (params as { scope: GatewayRequestScope }).scope.cwd;
        requestedRoots.push(root);
        return files(root);
      })
    } as unknown as GatewayClient;
    const application = new WorkspaceApplication();
    const primaryScope = scope("/primary");

    await application.refresh("files", client, primaryScope);
    await application.readFilesRoot("/secondary", client, primaryScope);
    await application.refresh("files", client, primaryScope);

    expect(requestedRoots).toEqual(["/primary", "/secondary", "/secondary"]);
    expect(application.getSnapshot().files?.root).toBe("/secondary");
  });

  it("commits a selected root only after the newest read succeeds", async () => {
    const secondary = deferred<WorkspaceFilesResult>();
    const client = {
      request: vi.fn(async (_method: string, params: unknown) => {
        const root = (params as { scope: GatewayRequestScope }).scope.cwd;
        return root === "/secondary" ? secondary.promise : files(root);
      })
    } as unknown as GatewayClient;
    const application = new WorkspaceApplication();
    const primaryScope = scope("/primary");
    application.bindFilesAuthority("thread:multi", ["/primary", "/secondary"]);
    await application.refresh("files", client, primaryScope);

    const pending = application.readFilesRoot("/secondary", client, primaryScope);
    expect(application.currentFilesRoot()).toBe("/primary");
    expect(application.currentFilesRootIntent()).toBe("/secondary");
    secondary.reject(new Error("offline"));

    await expect(pending).rejects.toThrow("offline");
    expect(application.currentFilesRoot()).toBe("/primary");
    expect(application.currentFilesRootIntent()).toBe("/primary");
    expect(application.getSnapshot().files?.root).toBe("/primary");
  });

  it("keeps an explicit root read authoritative over a background refresh", async () => {
    const secondary = deferred<WorkspaceFilesResult>();
    const client = {
      request: vi.fn(async (_method: string, params: unknown) => {
        const root = (params as { scope: GatewayRequestScope }).scope.cwd;
        return root === "/secondary" ? secondary.promise : files(root);
      })
    } as unknown as GatewayClient;
    const application = new WorkspaceApplication();
    const primaryScope = scope("/primary");
    application.bindFilesAuthority("thread:multi", ["/primary", "/secondary"]);
    await application.refresh("files", client, primaryScope);

    const selected = application.readFilesRoot("/secondary", client, primaryScope);
    const background = application.refresh("files", client, primaryScope);
    secondary.resolve(files("/secondary"));
    await Promise.all([selected, background]);

    expect(client.request).toHaveBeenCalledTimes(2);
    expect(application.currentFilesRootIntent()).toBe("/secondary");
    expect(application.getSnapshot().files?.root).toBe("/secondary");
  });

  it("rechecks the dirty transition immediately before committing a root", async () => {
    const secondary = deferred<WorkspaceFilesResult>();
    const beforeCommit = vi.fn(async () => false);
    const client = {
      request: vi.fn(async (_method: string, params: unknown) => secondary.promise)
    } as unknown as GatewayClient;
    const application = new WorkspaceApplication();
    const primaryScope = scope("/primary");
    application.bind(client, primaryScope);
    application.bindFilesAuthority("thread:multi", ["/primary", "/secondary"]);

    const selected = application.readFilesRoot("/secondary", client, primaryScope, { beforeCommit });
    secondary.resolve(files("/secondary"));

    await expect(selected).resolves.toBeNull();
    expect(beforeCommit).toHaveBeenCalledOnce();
    expect(application.currentFilesRoot()).toBe("/primary");
    expect(application.currentFilesRootIntent()).toBe("/primary");
  });

  it("joins a matching pending root read without duplicating its commit guard", async () => {
    const secondary = deferred<WorkspaceFilesResult>();
    const firstGuard = vi.fn(async () => true);
    const linkGuard = vi.fn(async () => true);
    const client = {
      request: vi.fn(async () => secondary.promise)
    } as unknown as GatewayClient;
    const application = new WorkspaceApplication();
    const primaryScope = scope("/primary");
    application.bindFilesAuthority("thread:multi", ["/primary", "/secondary"]);

    const selected = application.readFilesRoot("/secondary", client, primaryScope, {
      beforeCommit: firstGuard
    });
    const dependentLink = application.readFilesRoot("/secondary", client, primaryScope, {
      beforeCommit: linkGuard
    });
    secondary.resolve(files("/secondary"));

    await expect(Promise.all([selected, dependentLink])).resolves.toEqual([
      files("/secondary"),
      files("/secondary")
    ]);
    expect(client.request).toHaveBeenCalledOnce();
    expect(firstGuard).toHaveBeenCalledOnce();
    expect(linkGuard).not.toHaveBeenCalled();
  });

  it("aborts a superseded explicit Files-root inventory request", async () => {
    const requests: Array<{
      root: string;
      signal: AbortSignal | undefined;
      result: ReturnType<typeof deferred<WorkspaceFilesResult>>;
    }> = [];
    const client = {
      request: vi.fn((_method: string, params: { scope: { cwd: string } }, options?: { signal?: AbortSignal }) => {
        const result = deferred<WorkspaceFilesResult>();
        requests.push({ root: params.scope.cwd, signal: options?.signal, result });
        return result.promise;
      })
    } as unknown as GatewayClient;
    const application = new WorkspaceApplication();
    const primaryScope = scope("/primary");
    application.bindFilesAuthority("thread:multi", ["/primary", "/secondary", "/third"]);

    const secondary = application.readFilesRoot("/secondary", client, primaryScope);
    const third = application.readFilesRoot("/third", client, primaryScope);

    expect(requests).toHaveLength(2);
    expect(requests[0]?.signal?.aborted).toBe(true);
    requests[1]?.result.resolve(files("/third"));
    await expect(third).resolves.toEqual(files("/third"));
    requests[0]?.result.resolve(files("/secondary"));
    await expect(secondary).resolves.toBeNull();
  });

  it("coalesces identical Files and Transcript inventory reads", async () => {
    const primary = deferred<WorkspaceFilesResult>();
    const client = {
      request: vi.fn(async () => primary.promise)
    } as unknown as GatewayClient;
    const application = new WorkspaceApplication();
    const primaryScope = scope("/primary");
    application.bindFilesAuthority("thread:primary", ["/primary"]);

    const filesRead = application.refresh("files", client, primaryScope);
    const linkRead = application.refresh("linkFiles", client, primaryScope);
    primary.resolve(files("/primary"));
    await Promise.all([filesRead, linkRead]);

    expect(client.request).toHaveBeenCalledOnce();
    expect(application.getSnapshot().files?.root).toBe("/primary");
    expect(application.getSnapshot().linkFiles?.root).toBe("/primary");
  });

  it("does not reread an already committed Files root", async () => {
    const client = {
      request: vi.fn(async (_method: string, params: unknown) => (
        files((params as { scope: GatewayRequestScope }).scope.cwd)
      ))
    } as unknown as GatewayClient;
    const application = new WorkspaceApplication();
    const primaryScope = scope("/primary");
    application.bindFilesAuthority("thread:primary", ["/primary"]);

    await application.refresh("files", client, primaryScope);
    const current = await application.readFilesRoot("/primary", client, primaryScope);

    expect(current?.root).toBe("/primary");
    expect(client.request).toHaveBeenCalledTimes(1);
  });

  it("clamps a same-cwd Thread switch to the new authoritative root set", async () => {
    const requestedRoots: string[] = [];
    const client = {
      request: vi.fn(async (_method: string, params: unknown) => {
        const root = (params as { scope: GatewayRequestScope }).scope.cwd;
        requestedRoots.push(root);
        return files(root);
      })
    } as unknown as GatewayClient;
    const application = new WorkspaceApplication();
    const primaryScope = scope("/primary");

    application.bindFilesAuthority("thread:multi", ["/primary", "/secondary"]);
    await application.readFilesRoot("/secondary", client, primaryScope);
    application.bindFilesAuthority("thread:direct", ["/primary"]);

    expect(application.getSnapshot().files).toBeNull();
    await application.refresh("files", client, primaryScope);
    expect(requestedRoots).toEqual(["/secondary", "/primary"]);
  });

  it("clamps to primary when same-cwd Threads expose identical roots", async () => {
    const client = {
      request: vi.fn(async (_method: string, params: unknown) => (
        files((params as { scope: GatewayRequestScope }).scope.cwd)
      ))
    } as unknown as GatewayClient;
    const application = new WorkspaceApplication();
    const primaryScope = scope("/primary");

    application.bindFilesAuthority("thread:first", ["/primary", "/secondary"]);
    await application.readFilesRoot("/secondary", client, primaryScope);
    application.bindFilesAuthority("thread:second", ["/primary", "/secondary"]);

    expect(application.getSnapshot().files).toBeNull();
    await application.refresh("files", client, primaryScope);
    expect(application.getSnapshot().files?.root).toBe("/primary");
  });

  it("rejects an in-flight Files read when same-cwd authority shrinks", async () => {
    const secondaryRead = deferred<WorkspaceFilesResult>();
    const client = {
      request: vi.fn(async (_method: string, params: unknown) => {
        const root = (params as { scope: GatewayRequestScope }).scope.cwd;
        return root === "/secondary" ? secondaryRead.promise : files(root);
      })
    } as unknown as GatewayClient;
    const application = new WorkspaceApplication();
    const primaryScope = scope("/primary");

    application.bindFilesAuthority("thread:multi", ["/primary", "/secondary"]);
    await application.refresh("files", client, primaryScope);
    const pending = application.readFilesRoot("/secondary", client, primaryScope);

    application.bindFilesAuthority("thread:primary-only", ["/primary"]);
    secondaryRead.resolve(files("/secondary"));
    await pending;

    expect(application.getSnapshot().files?.root).toBe("/primary");
  });

  it("starts a fresh Files read after authority invalidates a pending root request", async () => {
    const secondaryRead = deferred<WorkspaceFilesResult>();
    const requestedRoots: string[] = [];
    const client = {
      request: vi.fn(async (_method: string, params: unknown) => {
        const root = (params as { scope: GatewayRequestScope }).scope.cwd;
        requestedRoots.push(root);
        return root === "/secondary" ? secondaryRead.promise : files(root);
      })
    } as unknown as GatewayClient;
    const application = new WorkspaceApplication();
    const primaryScope = scope("/primary");

    application.bindFilesAuthority("thread:multi", ["/primary", "/secondary"]);
    const obsolete = application.readFilesRoot("/secondary", client, primaryScope);
    application.bindFilesAuthority("thread:direct", ["/primary"]);
    await application.refresh("files", client, primaryScope);

    expect(requestedRoots).toEqual(["/secondary", "/primary"]);
    expect(application.getSnapshot().files?.root).toBe("/primary");
    secondaryRead.resolve(files("/secondary"));
    await obsolete;
    expect(application.getSnapshot().files?.root).toBe("/primary");
  });

  it("keeps Transcript link discovery on cwd while Files follows its selected root", async () => {
    const requestedRoots: string[] = [];
    const client = {
      request: vi.fn(async (_method: string, params: unknown) => {
        const root = (params as { scope: GatewayRequestScope }).scope.cwd;
        requestedRoots.push(root);
        return files(root);
      })
    } as unknown as GatewayClient;
    const application = new WorkspaceApplication();
    const primaryScope = scope("/primary");
    application.bindFilesAuthority("thread:multi", ["/primary", "/secondary"]);

    await application.readFilesRoot("/secondary", client, primaryScope);
    await application.ensure("linkFiles", client, primaryScope);

    expect(application.getSnapshot().files?.root).toBe("/secondary");
    expect(application.getSnapshot().linkFiles?.root).toBe("/primary");
    expect(requestedRoots).toEqual(["/secondary", "/primary"]);
  });
});
