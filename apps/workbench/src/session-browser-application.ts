import { gatewayScopeKey, type GatewayClient } from "@psychevo/client";
import {
  ThreadBrowserResultSchema,
  ThreadListResultSchema,
  type GatewayEvent,
  type GatewayRequestScope,
  type NavigationStateView,
  type SessionSummary,
  type ThreadBrowserResult
} from "@psychevo/protocol";
import {
  isSessionSummaryPatchEvent,
  normalizeSessionSummary,
  patchSessionSummariesFromGatewayEvent,
  type SessionSummaryPatchEvent
} from "./session-utils";
import type { SessionBrowserWorkspaceState } from "./types";

export type SessionBrowserSnapshot = {
  archivedSessions: SessionSummary[];
  loadingOlderCwd: string | null;
  pinnedSessionIds: string[];
  pinnedWorkspaceIds: string[];
  scopeEpoch: number;
  sessions: SessionSummary[];
  workspaces: SessionBrowserWorkspaceState[];
};

type RefreshHistoryOptions = {
  activeScope: GatewayRequestScope | null;
  currentThreadId: string | null;
  cwd?: string | null;
  includeArchived?: boolean;
};

type LoadOlderOptions = {
  activeScope: GatewayRequestScope | null;
  currentThreadId: string | null;
  cwd: string;
};

type OlderFlight = {
  promise: Promise<void>;
  token: string;
};

type SessionSummaryEventWindow = {
  events: SessionSummaryPatchEvent[];
};

function mergeSessionSummaries(
  current: SessionSummary[],
  incoming: SessionSummary[]
): SessionSummary[] {
  const byId = new Map(current.map((session) => [session.id, session]));
  for (const session of incoming) {
    byId.set(session.id, session);
  }
  return Array.from(byId.values()).sort((left, right) => {
    const rightTime = right.updatedAtMs ?? right.startedAtMs ?? 0;
    const leftTime = left.updatedAtMs ?? left.startedAtMs ?? 0;
    return rightTime - leftTime || left.id.localeCompare(right.id);
  });
}

function mergeBrowserWorkspaces(
  current: SessionBrowserWorkspaceState[],
  incoming: SessionBrowserWorkspaceState[]
): SessionBrowserWorkspaceState[] {
  const byId = new Map(current.map((workspace) => [workspace.id, workspace]));
  for (const workspace of incoming) {
    const existing = byId.get(workspace.id);
    const pinned = existing?.pinned ?? workspace.pinned;
    byId.set(workspace.id, {
      ...workspace,
      ...(pinned === undefined ? {} : { pinned }),
      sessionIds: Array.from(new Set([
        ...(existing?.sessionIds ?? []),
        ...workspace.sessionIds
      ]))
    });
  }
  return Array.from(byId.values());
}

function sameOrderedIds(left: string[], right: string[]): boolean {
  return left.length === right.length && left.every((id, index) => id === right[index]);
}

export class SessionBrowserApplication {
  private client: GatewayClient | null = null;
  private clientEpoch = 0;
  private scopeKey = "";
  private recentRevision = 0;
  private archiveRevision = 0;
  private browseRevision = 0;
  private navigationRevision = -1;
  private olderFlight: OlderFlight | null = null;
  private readonly flights = new Map<string, Promise<unknown>>();
  private readonly listeners = new Set<() => void>();
  private readonly sessionSummaryEventWindows = new Set<SessionSummaryEventWindow>();
  private snapshot: SessionBrowserSnapshot;

  constructor(pinnedSessionIds: string[] = []) {
    this.snapshot = {
      archivedSessions: [],
      loadingOlderCwd: null,
      pinnedSessionIds: [...pinnedSessionIds],
      pinnedWorkspaceIds: [],
      scopeEpoch: 0,
      sessions: [],
      workspaces: []
    };
  }

  getSnapshot = (): SessionBrowserSnapshot => this.snapshot;

  subscribe = (listener: () => void): (() => void) => {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  };

  bind(client: GatewayClient | null, scope: GatewayRequestScope | null): void {
    const nextScopeKey = gatewayScopeKey(scope);
    if (this.client === client && this.scopeKey === nextScopeKey) {
      return;
    }
    const clientChanged = this.client !== client;
    this.client = client;
    this.scopeKey = nextScopeKey;
    this.browseRevision += 1;
    this.olderFlight = null;
    if (clientChanged) {
      this.clientEpoch += 1;
      this.navigationRevision = -1;
      this.recentRevision += 1;
      this.archiveRevision += 1;
      this.flights.clear();
    }
    this.update({
      loadingOlderCwd: null,
      scopeEpoch: this.snapshot.scopeEpoch + 1
    });
  }

  async refreshNavigation(client: GatewayClient | null = this.client): Promise<boolean> {
    if (!client) return false;
    const epoch = this.clientEpoch;
    const navigation = await client.request("navigation/read", {});
    return this.applyNavigation(navigation, epoch);
  }

  async togglePinnedSession(threadId: string): Promise<void> {
    if (!this.client) return;
    const epoch = this.clientEpoch;
    const navigation = await this.client.request("thread/pin/set", {
      threadId,
      pinned: !this.snapshot.pinnedSessionIds.includes(threadId)
    });
    this.applyNavigation(navigation, epoch);
  }

  async togglePinnedWorkspace(workspaceId: string): Promise<void> {
    if (!this.client) return;
    const epoch = this.clientEpoch;
    const navigation = await this.client.request("workspace/pin/set", {
      workspaceId,
      pinned: !this.snapshot.pinnedWorkspaceIds.includes(workspaceId)
    });
    this.applyNavigation(navigation, epoch);
  }

  private applyNavigation(navigation: NavigationStateView, epoch: number): boolean {
    if (epoch !== this.clientEpoch || navigation.revision < this.navigationRevision) {
      return false;
    }
    const changed = !sameOrderedIds(this.snapshot.pinnedSessionIds, navigation.pinnedThreadIds)
      || !sameOrderedIds(this.snapshot.pinnedWorkspaceIds, navigation.pinnedWorkspaceIds);
    this.navigationRevision = navigation.revision;
    this.update({
      pinnedSessionIds: navigation.pinnedThreadIds,
      pinnedWorkspaceIds: navigation.pinnedWorkspaceIds,
      workspaces: this.snapshot.workspaces.map((workspace) => ({
        ...workspace,
        pinned: navigation.pinnedWorkspaceIds.includes(workspace.id)
      }))
    });
    return changed;
  }

  patchGatewayEvent(event: GatewayEvent): void {
    if (isSessionSummaryPatchEvent(event)) {
      for (const window of this.sessionSummaryEventWindows) {
        window.events.push(event);
      }
    }
    const sessions = patchSessionSummariesFromGatewayEvent(this.snapshot.sessions, event);
    if (sessions !== this.snapshot.sessions) {
      this.update({ sessions });
    }
  }

  async refreshHistory(
    client: GatewayClient | null = this.client,
    options: RefreshHistoryOptions
  ): Promise<SessionSummary[]> {
    if (!client) {
      return [];
    }
    this.bind(client, options.activeScope);
    const cwd = options.cwd || null;
    if (options.includeArchived) {
      const key = `archive:${this.clientEpoch}:${cwd ?? ""}`;
      const existing = this.flights.get(key);
      if (existing) {
        return existing as Promise<SessionSummary[]>;
      }
      const revision = this.archiveRevision + 1;
      this.archiveRevision = revision;
      const epoch = this.clientEpoch;
      const request = client.request("thread/list", {
        archived: true,
        limit: 100,
        cwd
      }).then((value) => {
        const sessions = ThreadListResultSchema.parse(value).sessions.map(normalizeSessionSummary);
        if (epoch === this.clientEpoch && revision === this.archiveRevision) {
          this.update({ archivedSessions: sessions });
        }
        return sessions;
      });
      return this.trackFlight(key, request);
    }

    this.invalidateOlder();
    const includeSessionIds = this.includeSessionIds(options.currentThreadId);
    const key = `recent:${this.clientEpoch}:${JSON.stringify([cwd, includeSessionIds])}`;
    const existing = this.flights.get(key);
    if (existing) {
      return existing as Promise<SessionSummary[]>;
    }
    const revision = this.recentRevision + 1;
    this.recentRevision = revision;
    const epoch = this.clientEpoch;
    const eventWindow = this.openSessionSummaryEventWindow();
    const request = client.request("thread/browser", {
      archived: false,
      cursor: null,
      includeSessionIds,
      limit: 20,
      recentDays: 7,
      cwd
    }).then((value) => {
      const result = ThreadBrowserResultSchema.parse(value);
      const sessions = this.rebaseSessionSummaries(
        sessionsFromThreadBrowser(result),
        eventWindow
      );
      if (epoch === this.clientEpoch && revision === this.recentRevision) {
        this.update({
          sessions,
          workspaces: workspacesFromThreadBrowser(result).map((workspace) => ({
            ...workspace,
            pinned: this.snapshot.pinnedWorkspaceIds.includes(workspace.id)
          }))
        });
      }
      return sessions;
    }).finally(() => {
      this.sessionSummaryEventWindows.delete(eventWindow);
    });
    return this.trackFlight(key, request);
  }

  async loadOlder(
    client: GatewayClient | null = this.client,
    options: LoadOlderOptions
  ): Promise<void> {
    if (!client) {
      return;
    }
    this.bind(client, options.activeScope);
    const workspace = this.snapshot.workspaces.find((item) => item.cwd === options.cwd);
    const cursor = workspace?.nextCursor ?? null;
    if (!cursor) {
      return;
    }
    const token = JSON.stringify([
      this.clientEpoch,
      this.snapshot.scopeEpoch,
      this.browseRevision,
      options.cwd,
      cursor
    ]);
    if (this.olderFlight?.token === token) {
      await this.olderFlight.promise;
      return;
    }
    if (this.olderFlight) {
      return;
    }
    this.update({ loadingOlderCwd: options.cwd });
    const eventWindow = this.openSessionSummaryEventWindow();
    const request = client.request("thread/browser", {
      archived: false,
      cursor,
      includeSessionIds: this.includeSessionIds(options.currentThreadId),
      limit: 20,
      recentDays: 7,
      cwd: options.cwd
    }).then((value) => {
      const result = ThreadBrowserResultSchema.parse(value);
      const incomingSessions = this.rebaseSessionSummaries(
        sessionsFromThreadBrowser(result),
        eventWindow
      );
      if (this.olderFlight?.token === token) {
        this.update({
          sessions: mergeSessionSummaries(
            this.snapshot.sessions,
            incomingSessions
          ),
          workspaces: mergeBrowserWorkspaces(
            this.snapshot.workspaces,
            workspacesFromThreadBrowser(result)
          )
        });
      }
    }).finally(() => {
      this.sessionSummaryEventWindows.delete(eventWindow);
      if (this.olderFlight?.token === token) {
        this.olderFlight = null;
        this.update({ loadingOlderCwd: null });
      }
    });
    this.olderFlight = { promise: request, token };
    await request;
  }

  private includeSessionIds(currentThreadId: string | null): string[] {
    return Array.from(new Set([
      currentThreadId,
      ...this.snapshot.pinnedSessionIds
    ].filter((id): id is string => Boolean(id))));
  }

  private openSessionSummaryEventWindow(): SessionSummaryEventWindow {
    const window = { events: [] };
    this.sessionSummaryEventWindows.add(window);
    return window;
  }

  private rebaseSessionSummaries(
    sessions: SessionSummary[],
    window: SessionSummaryEventWindow
  ): SessionSummary[] {
    return window.events.reduce(patchSessionSummariesFromGatewayEvent, sessions);
  }

  private trackFlight<T>(key: string, request: Promise<T>): Promise<T> {
    this.flights.set(key, request);
    const clear = () => {
      if (this.flights.get(key) === request) {
        this.flights.delete(key);
      }
    };
    request.then(clear, clear);
    return request;
  }

  private invalidateOlder(): void {
    this.browseRevision += 1;
    this.olderFlight = null;
    if (this.snapshot.loadingOlderCwd !== null) {
      this.update({ loadingOlderCwd: null });
    }
  }

  private update(patch: Partial<SessionBrowserSnapshot>): void {
    this.snapshot = { ...this.snapshot, ...patch };
    for (const listener of this.listeners) {
      listener();
    }
  }
}

export function sessionsFromThreadBrowser(result: ThreadBrowserResult): SessionSummary[] {
  const seen = new Set<string>();
  const sessions: SessionSummary[] = [];
  for (const workspace of result.workspaces) {
    for (const session of workspace.sessions) {
      if (seen.has(session.id)) {
        continue;
      }
      seen.add(session.id);
      sessions.push(normalizeSessionSummary(session));
    }
  }
  return sessions;
}

export function workspacesFromThreadBrowser(
  result: ThreadBrowserResult
): SessionBrowserWorkspaceState[] {
  return result.workspaces.map((workspace) => ({
    id: workspace.workspace.id,
    name: workspace.workspace.name,
    roots: workspace.workspace.roots,
    sessionIds: workspace.sessions.map((session) => session.id),
    revision: workspace.workspace.revision,
    cwd: workspace.workspace.roots[0] ?? "",
    displayPath: workspace.workspace.name,
    hiddenCount: workspace.hiddenCount ?? 0,
    nextCursor: workspace.nextCursor ?? null
  }));
}
