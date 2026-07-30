import { describe, expect, it, vi } from "vitest";
import type { GatewayClient } from "@psychevo/client";
import type {
  GatewayRequestScope,
  SessionSummary,
  ThreadBrowserResult
} from "@psychevo/protocol";
import { SessionBrowserApplication } from "./session-browser-application";

const scope = (cwd: string): GatewayRequestScope => ({
  cwd,
  source: { kind: "web", rawId: `scope:${cwd}`, lifetime: "persistent" }
});

const session = (id: string, cwd: string, updatedAtMs: number): SessionSummary => ({
  id,
  cwd,
  project: { cwd, label: cwd, displayPath: cwd },
  startedAtMs: 1,
  updatedAtMs,
  messageCount: 1,
  toolCallCount: 0,
  activity: { running: false, activeTurnId: null, queuedTurns: 0 }
});

const browserResult = (
  cwd: string,
  sessions: SessionSummary[],
  offset: number | null
): ThreadBrowserResult => ({
  workspaces: [{
    cwd,
    project: { cwd, label: cwd, displayPath: cwd },
    sessions,
    hiddenCount: 0,
    nextCursor: offset === null ? null : { cwd, offset }
  }]
});

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((next) => {
    resolve = next;
  });
  return { promise, resolve };
}

describe("SessionBrowserApplication", () => {
  it("keeps the overlapping cold-start global browse when initialize supplies the scope", async () => {
    const startup = deferred<ThreadBrowserResult>();
    const request = vi.fn(() => startup.promise);
    const client = { request } as unknown as GatewayClient;
    const application = new SessionBrowserApplication();

    const browse = application.refreshHistory(client, {
      activeScope: null,
      currentThreadId: null
    });
    application.bind(client, scope("/repo"));
    startup.resolve(browserResult("/repo", [session("persisted", "/repo", 2)], null));
    await browse;

    expect(request).toHaveBeenCalledTimes(1);
    expect(application.getSnapshot().sessions.map((item) => item.id)).toEqual([
      "persisted"
    ]);
  });

  it("single-flights identical reads and rejects a prior scope response", async () => {
    const first = deferred<ThreadBrowserResult>();
    const second = deferred<ThreadBrowserResult>();
    const request = vi.fn((_method: string, params: unknown) => {
      const cwd = (params as { cwd?: string | null }).cwd;
      return cwd === "/a" ? first.promise : second.promise;
    });
    const client = { request } as unknown as GatewayClient;
    const application = new SessionBrowserApplication(["pinned"]);

    const a1 = application.refreshHistory(client, {
      activeScope: scope("/a"),
      currentThreadId: "current",
      cwd: "/a"
    });
    const a2 = application.refreshHistory(client, {
      activeScope: scope("/a"),
      currentThreadId: "current",
      cwd: "/a"
    });
    expect(request).toHaveBeenCalledTimes(1);
    expect(request.mock.calls[0]?.[1]).toMatchObject({
      includeSessionIds: ["current", "pinned"]
    });

    const b = application.refreshHistory(client, {
      activeScope: scope("/b"),
      currentThreadId: null,
      cwd: "/b"
    });
    first.resolve(browserResult("/a", [session("a", "/a", 1)], null));
    await Promise.all([a1, a2]);
    expect(application.getSnapshot().sessions).toEqual([]);

    second.resolve(browserResult("/b", [session("b", "/b", 2)], null));
    await b;
    expect(application.getSnapshot().sessions.map((item) => item.id)).toEqual(["b"]);
  });

  it("owns pagination merge, loading state, and pin updates", async () => {
    const request = vi.fn(async (_method: string, params: unknown) => {
      const cursor = (params as { cursor?: { offset: number } | null }).cursor;
      return cursor
        ? browserResult("/repo", [session("older", "/repo", 1)], null)
        : browserResult("/repo", [session("newer", "/repo", 2)], 20);
    });
    const client = { request } as unknown as GatewayClient;
    const application = new SessionBrowserApplication();
    application.togglePinnedSession("newer");

    await application.refreshHistory(client, {
      activeScope: scope("/repo"),
      currentThreadId: null
    });
    const loading = application.loadOlder(client, {
      activeScope: scope("/repo"),
      currentThreadId: null,
      cwd: "/repo"
    });
    expect(application.getSnapshot().loadingOlderCwd).toBe("/repo");
    await loading;

    expect(application.getSnapshot().sessions.map((item) => item.id)).toEqual([
      "newer",
      "older"
    ]);
    expect(application.getSnapshot().loadingOlderCwd).toBeNull();
    expect(application.getSnapshot().pinnedSessionIds).toEqual(["newer"]);
  });

  it("does not let an old page merge or clear a newer scope loading state", async () => {
    const olderA = deferred<ThreadBrowserResult>();
    const olderB = deferred<ThreadBrowserResult>();
    const request = vi.fn((_method: string, params: unknown) => {
      const input = params as {
        cwd?: string | null;
        cursor?: { offset: number } | null;
      };
      if (input.cursor && input.cwd === "/a") {
        return olderA.promise;
      }
      if (input.cursor && input.cwd === "/b") {
        return olderB.promise;
      }
      const cwd = input.cwd ?? "/a";
      return Promise.resolve(browserResult(
        cwd,
        [session(`${cwd}-new`, cwd, 2)],
        20
      ));
    });
    const client = { request } as unknown as GatewayClient;
    const application = new SessionBrowserApplication();

    await application.refreshHistory(client, {
      activeScope: scope("/a"),
      currentThreadId: null,
      cwd: "/a"
    });
    const pageA = application.loadOlder(client, {
      activeScope: scope("/a"),
      currentThreadId: null,
      cwd: "/a"
    });
    expect(application.getSnapshot().loadingOlderCwd).toBe("/a");

    await application.refreshHistory(client, {
      activeScope: scope("/b"),
      currentThreadId: null,
      cwd: "/b"
    });
    expect(application.getSnapshot().loadingOlderCwd).toBeNull();
    const pageB = application.loadOlder(client, {
      activeScope: scope("/b"),
      currentThreadId: null,
      cwd: "/b"
    });
    expect(application.getSnapshot().loadingOlderCwd).toBe("/b");

    olderA.resolve(browserResult(
      "/a",
      [session("/a-old", "/a", 1)],
      null
    ));
    await pageA;
    expect(application.getSnapshot().loadingOlderCwd).toBe("/b");
    expect(application.getSnapshot().sessions.map((item) => item.id)).toEqual([
      "/b-new"
    ]);

    olderB.resolve(browserResult(
      "/b",
      [session("/b-old", "/b", 1)],
      null
    ));
    await pageB;
    expect(application.getSnapshot().loadingOlderCwd).toBeNull();
    expect(application.getSnapshot().sessions.map((item) => item.id)).toEqual([
      "/b-new",
      "/b-old"
    ]);
  });
});
