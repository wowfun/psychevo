import { gatewayScopeKey, type GatewayClient } from "@psychevo/client";
import {
  WorkspaceChangesResultSchema,
  WorkspaceDiffResultSchema,
  WorkspaceFilesResultSchema,
  type GatewayRequestScope,
  type WorkspaceChangesResult,
  type WorkspaceDiffResult,
  type WorkspaceFilesResult,
  type WorkspaceGitBranchesResult
} from "@psychevo/protocol";

export type WorkspaceFacet = "branch" | "changes" | "diff" | "files" | "linkFiles";

export type WorkspaceSnapshot = {
  branch: WorkspaceGitBranchesResult | null | undefined;
  changes: WorkspaceChangesResult | null;
  diff: WorkspaceDiffResult | null;
  files: WorkspaceFilesResult | null;
  linkFiles: WorkspaceFilesResult | null;
  scopeEpoch: number;
};

type ValueUpdate<T> = T | ((current: T) => T);

function resolveUpdate<T>(current: T, update: ValueUpdate<T>): T {
  return typeof update === "function"
    ? (update as (current: T) => T)(current)
    : update;
}

export class WorkspaceApplication {
  private client: GatewayClient | null = null;
  private scope: GatewayRequestScope | null = null;
  private scopeKey = "";
  private selectedFilesRoot: string | null = null;
  private pendingFilesRoot: string | null = null;
  private pendingFilesRead: {
    beforeCommit: (() => boolean | Promise<boolean>) | null;
    controller: AbortController;
    promise: Promise<WorkspaceFilesResult | null>;
    root: string;
  } | null = null;
  private filesAuthorityKey = "";
  private allowedFilesRoots: string[] = [];
  private readonly revisions: Record<WorkspaceFacet, number> = {
    branch: 0,
    changes: 0,
    diff: 0,
    files: 0,
    linkFiles: 0
  };
  private readonly committedEpochs: Record<WorkspaceFacet, number> = {
    branch: -1,
    changes: -1,
    diff: -1,
    files: -1,
    linkFiles: -1
  };
  private readonly flights = new Map<WorkspaceFacet, Promise<unknown>>();
  private readonly fileInventoryFlights = new Map<string, {
    owners: Set<WorkspaceFacet>;
    request: Promise<WorkspaceFilesResult>;
  }>();
  private readonly listeners = new Set<() => void>();
  private snapshot: WorkspaceSnapshot = {
    branch: undefined,
    changes: null,
    diff: null,
    files: null,
    linkFiles: null,
    scopeEpoch: 0
  };

  getSnapshot = (): WorkspaceSnapshot => this.snapshot;

  currentFilesRoot = (): string | null => this.selectedFilesRoot;

  currentFilesRootIntent = (): string | null => this.pendingFilesRoot ?? this.selectedFilesRoot;

  subscribe = (listener: () => void): (() => void) => {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  };

  bind(client: GatewayClient | null, scope: GatewayRequestScope | null): void {
    const nextScopeKey = gatewayScopeKey(scope);
    if (this.client === client && this.scopeKey === nextScopeKey) {
      return;
    }
    this.client = client;
    this.scope = scope;
    this.scopeKey = nextScopeKey;
    this.selectedFilesRoot = null;
    this.pendingFilesRoot = null;
    this.pendingFilesRead?.controller.abort();
    this.pendingFilesRead = null;
    this.filesAuthorityKey = "";
    this.allowedFilesRoots = [];
    for (const facet of Object.keys(this.revisions) as WorkspaceFacet[]) {
      this.revisions[facet] += 1;
    }
    this.flights.clear();
    this.fileInventoryFlights.clear();
    this.commitSnapshot({
      changes: null,
      diff: null,
      files: null,
      linkFiles: null,
      scopeEpoch: this.snapshot.scopeEpoch + 1
    });
  }

  setBranch = (update: ValueUpdate<WorkspaceGitBranchesResult | null | undefined>): void => {
    this.replaceFacet("branch", resolveUpdate(this.snapshot.branch, update));
  };

  setChanges = (update: ValueUpdate<WorkspaceChangesResult | null>): void => {
    this.replaceFacet("changes", resolveUpdate(this.snapshot.changes, update));
  };

  setDiff = (update: ValueUpdate<WorkspaceDiffResult | null>): void => {
    this.replaceFacet("diff", resolveUpdate(this.snapshot.diff, update));
  };

  setFiles = (update: ValueUpdate<WorkspaceFilesResult | null>): void => {
    this.replaceFacet("files", resolveUpdate(this.snapshot.files, update));
  };

  bindFilesAuthority(authorityKey: string, roots: string[]): void {
    if (
      this.filesAuthorityKey === authorityKey
      && roots.length === this.allowedFilesRoots.length
      && roots.every((root, index) => root === this.allowedFilesRoots[index])
    ) return;
    const authorityChanged = this.filesAuthorityKey !== authorityKey;
    this.filesAuthorityKey = authorityKey;
    this.allowedFilesRoots = [...roots];
    const nextRoot = !authorityChanged && this.selectedFilesRoot && roots.includes(this.selectedFilesRoot)
      ? this.selectedFilesRoot
      : roots[0] ?? null;
    this.selectedFilesRoot = nextRoot;
    this.pendingFilesRoot = null;
    this.pendingFilesRead?.controller.abort();
    this.pendingFilesRead = null;
    this.revisions.files += 1;
    this.flights.delete("files");
    if (this.snapshot.files && this.snapshot.files.root !== nextRoot) {
      this.commitSnapshot({ files: null });
    }
  }

  ensure(
    facet: WorkspaceFacet,
    client: GatewayClient | null = this.client,
    scope: GatewayRequestScope | null = this.scope
  ): Promise<void> {
    if (client && scope) {
      this.bind(client, scope);
    }
    const value = this.snapshot[facet];
    if (
      this.committedEpochs[facet] === this.snapshot.scopeEpoch
      && (facet === "branch" ? value !== undefined : value !== null)
    ) {
      return Promise.resolve();
    }
    const existing = this.flights.get(facet);
    if (existing) {
      return existing as Promise<void>;
    }
    return this.startFacetRead(facet, client, scope);
  }

  refresh(
    facet: WorkspaceFacet,
    client: GatewayClient | null = this.client,
    scope: GatewayRequestScope | null = this.scope
  ): Promise<void> {
    if (!client || !scope) {
      return Promise.resolve();
    }
    this.bind(client, scope);
    if (facet === "files" && this.pendingFilesRead) {
      return this.pendingFilesRead.promise.then(() => undefined);
    }
    return this.startFacetRead(facet, client, scope);
  }

  private startFacetRead(
    facet: WorkspaceFacet,
    client: GatewayClient | null,
    scope: GatewayRequestScope | null
  ): Promise<void> {
    if (!client || !scope) {
      return Promise.resolve();
    }
    const revision = this.revisions[facet] + 1;
    this.revisions[facet] = revision;
    const epoch = this.snapshot.scopeEpoch;
    const request = this.requestFacet(facet, client, scope).then((value) => {
      if (
        epoch === this.snapshot.scopeEpoch
        && revision === this.revisions[facet]
      ) {
        this.commitFacet(facet, value);
      }
    });
    this.flights.set(facet, request);
    const clear = () => {
      if (this.flights.get(facet) === request) {
        this.flights.delete(facet);
      }
    };
    request.then(clear, clear);
    return request;
  }

  async refreshSurface(
    client: GatewayClient | null = this.client,
    scope: GatewayRequestScope | null = this.scope
  ): Promise<void> {
    await Promise.all([
      this.refresh("files", client, scope),
      this.refresh("diff", client, scope),
      this.refresh("changes", client, scope)
    ]);
  }

  async readDiff(
    path: string | null,
    client: GatewayClient | null = this.client,
    scope: GatewayRequestScope | null = this.scope
  ): Promise<WorkspaceDiffResult | null> {
    if (!client || !scope) {
      throw new Error("Workspace is unavailable");
    }
    this.bind(client, scope);
    const revision = this.revisions.diff + 1;
    this.revisions.diff = revision;
    this.flights.delete("diff");
    const epoch = this.snapshot.scopeEpoch;
    const result = WorkspaceDiffResultSchema.parse(
      await client.request("workspace/diff", { scope, path })
    );
    if (
      epoch !== this.snapshot.scopeEpoch
      || revision !== this.revisions.diff
    ) {
      return null;
    }
    if (path === null) {
      this.commitFacet("diff", result);
    }
    return result;
  }

  async readBranches(
    client: GatewayClient | null = this.client,
    scope: GatewayRequestScope | null = this.scope
  ): Promise<WorkspaceGitBranchesResult | null> {
    if (!client || !scope) {
      throw new Error("Workspace is unavailable");
    }
    this.bind(client, scope);
    const revision = this.revisions.branch + 1;
    this.revisions.branch = revision;
    this.flights.delete("branch");
    const epoch = this.snapshot.scopeEpoch;
    const result = await client.request("workspace/git/branches", { scope });
    if (
      epoch === this.snapshot.scopeEpoch
      && revision === this.revisions.branch
    ) {
      this.commitFacet("branch", result);
      return result;
    }
    return null;
  }

  async readFilesRoot(
    root: string,
    client: GatewayClient | null = this.client,
    scope: GatewayRequestScope | null = this.scope,
    options: {
      beforeCommit?: () => boolean | Promise<boolean>;
      force?: boolean;
    } = {}
  ): Promise<WorkspaceFilesResult | null> {
    if (!client || !scope) throw new Error("Workspace is unavailable");
    this.bind(client, scope);
    if (this.allowedFilesRoots.length > 0 && !this.allowedFilesRoots.includes(root)) {
      throw new Error("The selected directory is outside the current Thread Workspace.");
    }
    if (options.force !== true && this.pendingFilesRead?.root === root) {
      if (!this.pendingFilesRead.beforeCommit && options.beforeCommit) {
        this.pendingFilesRead.beforeCommit = options.beforeCommit;
      }
      return this.pendingFilesRead.promise;
    }
    if (
      options.force !== true
      && this.pendingFilesRead
      && this.selectedFilesRoot === root
      && this.snapshot.files?.root === root
      && this.committedEpochs.files === this.snapshot.scopeEpoch
    ) {
      this.revisions.files += 1;
      this.pendingFilesRoot = null;
      this.pendingFilesRead.controller.abort();
      this.pendingFilesRead = null;
      return this.snapshot.files;
    }
    if (
      options.force !== true
      && this.pendingFilesRoot === null
      && this.selectedFilesRoot === root
      && this.snapshot.files?.root === root
      && this.committedEpochs.files === this.snapshot.scopeEpoch
    ) {
      return this.snapshot.files;
    }
    this.pendingFilesRead?.controller.abort();
    this.pendingFilesRoot = root;
    const revision = this.revisions.files + 1;
    this.revisions.files = revision;
    this.flights.delete("files");
    const epoch = this.snapshot.scopeEpoch;
    const controller = new AbortController();
    const pending = {
      beforeCommit: options.beforeCommit ?? null,
      controller,
      promise: Promise.resolve(null) as Promise<WorkspaceFilesResult | null>,
      root
    };
    const operation = (async (): Promise<WorkspaceFilesResult | null> => {
      try {
        const result = await this.readFileInventory(
          client,
          { ...scope, cwd: root },
          "files",
          controller.signal
        );
        if (epoch !== this.snapshot.scopeEpoch || revision !== this.revisions.files) {
          return null;
        }
        if (pending.beforeCommit && !await pending.beforeCommit()) {
          if (epoch === this.snapshot.scopeEpoch && revision === this.revisions.files) {
            this.pendingFilesRoot = null;
          }
          return null;
        }
        if (epoch !== this.snapshot.scopeEpoch || revision !== this.revisions.files) {
          return null;
        }
        this.selectedFilesRoot = root;
        this.pendingFilesRoot = null;
        this.commitFacet("files", result);
        return result;
      } catch (error) {
        if (controller.signal.aborted || epoch !== this.snapshot.scopeEpoch || revision !== this.revisions.files) {
          return null;
        }
        if (epoch === this.snapshot.scopeEpoch && revision === this.revisions.files) {
          this.pendingFilesRoot = null;
        }
        throw error;
      }
    })();
    pending.promise = operation;
    this.pendingFilesRead = pending;
    try {
      return await operation;
    } finally {
      if (this.pendingFilesRead === pending) {
        this.pendingFilesRead = null;
      }
    }
  }

  private async requestFacet(
    facet: WorkspaceFacet,
    client: GatewayClient,
    scope: GatewayRequestScope
  ): Promise<WorkspaceSnapshot[WorkspaceFacet]> {
    switch (facet) {
      case "files":
        return this.readFileInventory(client, this.selectedFilesRoot
          ? { ...scope, cwd: this.selectedFilesRoot }
          : scope, "files");
      case "linkFiles":
        return this.readFileInventory(client, scope, "linkFiles");
      case "diff":
        return WorkspaceDiffResultSchema.parse(
          await client.request("workspace/diff", { scope, path: null })
        );
      case "changes":
        return WorkspaceChangesResultSchema.parse(
          await client.request("workspace/changes", { scope })
        );
      case "branch": {
        return client.request("workspace/git/branches", { scope });
      }
    }
  }

  private readFileInventory(
    client: GatewayClient,
    scope: GatewayRequestScope,
    owner: "files" | "linkFiles",
    signal?: AbortSignal
  ): Promise<WorkspaceFilesResult> {
    const key = gatewayScopeKey(scope);
    const existing = this.fileInventoryFlights.get(key);
    if (existing && !existing.owners.has(owner)) {
      existing.owners.add(owner);
      return existing.request;
    }
    const request = client.request(
      "workspace/files",
      { scope },
      signal ? { signal } : {}
    ).then((value) => (
      WorkspaceFilesResultSchema.parse(value)
    ));
    const flight = { owners: new Set<WorkspaceFacet>([owner]), request };
    this.fileInventoryFlights.set(key, flight);
    const clear = () => {
      if (this.fileInventoryFlights.get(key) === flight) {
        this.fileInventoryFlights.delete(key);
      }
    };
    request.then(clear, clear);
    return request;
  }

  private replaceFacet(
    facet: WorkspaceFacet,
    value: WorkspaceSnapshot[WorkspaceFacet]
  ): void {
    this.revisions[facet] += 1;
    this.flights.delete(facet);
    this.commitFacet(facet, value);
  }

  private commitFacet(
    facet: WorkspaceFacet,
    value: WorkspaceSnapshot[WorkspaceFacet]
  ): void {
    this.committedEpochs[facet] = this.snapshot.scopeEpoch;
    if (facet === "files" && value) {
      this.selectedFilesRoot = (value as WorkspaceFilesResult).root;
      if ((value as WorkspaceFilesResult).root === this.scope?.cwd) {
        this.revisions.linkFiles += 1;
        this.flights.delete("linkFiles");
        this.committedEpochs.linkFiles = this.snapshot.scopeEpoch;
        this.commitSnapshot({ files: value as WorkspaceFilesResult, linkFiles: value as WorkspaceFilesResult });
        return;
      }
    }
    this.commitSnapshot({ [facet]: value });
  }

  private commitSnapshot(patch: Partial<WorkspaceSnapshot>): void {
    this.snapshot = { ...this.snapshot, ...patch };
    for (const listener of this.listeners) {
      listener();
    }
  }
}
