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

export type WorkspaceFacet = "branch" | "changes" | "diff" | "files";

export type WorkspaceSnapshot = {
  branch: WorkspaceGitBranchesResult | null | undefined;
  changes: WorkspaceChangesResult | null;
  diff: WorkspaceDiffResult | null;
  files: WorkspaceFilesResult | null;
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
  private readonly revisions: Record<WorkspaceFacet, number> = {
    branch: 0,
    changes: 0,
    diff: 0,
    files: 0
  };
  private readonly committedEpochs: Record<WorkspaceFacet, number> = {
    branch: -1,
    changes: -1,
    diff: -1,
    files: -1
  };
  private readonly flights = new Map<WorkspaceFacet, Promise<unknown>>();
  private readonly listeners = new Set<() => void>();
  private snapshot: WorkspaceSnapshot = {
    branch: undefined,
    changes: null,
    diff: null,
    files: null,
    scopeEpoch: 0
  };

  getSnapshot = (): WorkspaceSnapshot => this.snapshot;

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
    for (const facet of Object.keys(this.revisions) as WorkspaceFacet[]) {
      this.revisions[facet] += 1;
    }
    this.flights.clear();
    this.commitSnapshot({
      changes: null,
      diff: null,
      files: null,
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

  private async requestFacet(
    facet: WorkspaceFacet,
    client: GatewayClient,
    scope: GatewayRequestScope
  ): Promise<WorkspaceSnapshot[WorkspaceFacet]> {
    switch (facet) {
      case "files":
        return WorkspaceFilesResultSchema.parse(
          await client.request("workspace/files", { scope })
        );
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
    this.commitSnapshot({ [facet]: value });
  }

  private commitSnapshot(patch: Partial<WorkspaceSnapshot>): void {
    this.snapshot = { ...this.snapshot, ...patch };
    for (const listener of this.listeners) {
      listener();
    }
  }
}
