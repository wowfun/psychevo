import { afterEach, describe, expect, it, vi } from "vitest";
import type {
  GatewayMethod,
  GatewayRequestScope
} from "@psychevo/protocol";

import {
  CapabilitiesApplication,
  type CapabilitiesClient
} from "./capabilities-application";

afterEach(() => {
  vi.useRealTimers();
});

describe("CapabilitiesApplication", () => {
  it("isolates state subscribers and reports a bounded diagnostic", () => {
    const diagnostics: Array<{ source: string; message: string }> = [];
    const application = new CapabilitiesApplication(
      null,
      (diagnostic) => diagnostics.push(diagnostic)
    );
    const later = vi.fn();
    application.subscribe(() => {
      throw new Error(`broken capability observer:${"x".repeat(1_100)}`);
    });
    application.subscribe(later);

    expect(() => application.activate(scope("/repo"))).not.toThrow();

    expect(later).toHaveBeenCalledOnce();
    expect(diagnostics).toHaveLength(1);
    expect(diagnostics[0]?.source).toBe("capabilities_listener");
    expect(diagnostics[0]?.message.length).toBe(1_000);
  });

  it("deduplicates concurrent reads for one canonical scope", async () => {
    const pending = deferred<unknown>();
    const client = fakeClient((method) => (
      method === "skill/list" ? pending.promise : Promise.reject(new Error(`unexpected ${method}`))
    ));
    const application = new CapabilitiesApplication(client);
    application.activate(scope("/repo"));

    const first = application.refresh("skills");
    const second = application.refresh("skills");

    expect(client.calls.filter(([method]) => method === "skill/list")).toHaveLength(1);
    pending.resolve({ skills: [{ name: "review" }] });
    await expect(Promise.all([first, second])).resolves.toHaveLength(2);
    expect(application.getSnapshot().data.skills).toEqual({
      skills: [{ name: "review" }]
    });
  });

  it("refreshes the authoritative domain before committing a mutation receipt", async () => {
    let enabled = false;
    const client = fakeClient(async (method) => {
      if (method === "skill/list") return { skills: [{ enabled, name: "review" }] };
      if (method === "skill/setEnabled") {
        enabled = true;
        return { enabled: true };
      }
      throw new Error(`unexpected ${method}`);
    });
    const application = new CapabilitiesApplication(client);
    const activeScope = scope("/repo");
    application.activate(activeScope);
    await application.refresh("skills");

    await application.request("skill/setEnabled", {
      enabled: true,
      name: "review",
      scope: activeScope
    });

    const snapshot = application.getSnapshot();
    expect(snapshot.data.skills).toEqual({
      skills: [{ enabled: true, name: "review" }]
    });
    expect(snapshot.receipt).toMatchObject({
      domain: "skills",
      method: "skill/setEnabled"
    });
    expect(snapshot.mutation).toBeNull();
    expect(client.calls.filter(([method]) => method === "skill/list")).toHaveLength(2);
  });

  it("owns Extension reads and refreshes after explicit enablement changes", async () => {
    let enabled = true;
    const client = fakeClient(async (method) => {
      if (method === "extension/list") {
        return { extensions: [{ id: "example.extension", enabled }], count: 1 };
      }
      if (method === "extension/setEnabled") {
        enabled = false;
        return { success: true, id: "example.extension", scope: "profile", enabled };
      }
      throw new Error(`unexpected ${method}`);
    });
    const application = new CapabilitiesApplication(client);
    const activeScope = scope("/repo");
    application.activate(activeScope);

    await application.refresh("extensions");
    await application.request("extension/setEnabled", {
      selector: "example.extension@profile",
      scopeName: "profile",
      enabled: false,
      scope: activeScope
    });

    expect(application.getSnapshot().data.extensions).toEqual({
      extensions: [{ id: "example.extension", enabled: false }],
      count: 1
    });
    expect(application.getSnapshot().receipt).toMatchObject({
      domain: "extensions",
      method: "extension/setEnabled"
    });
  });

  it("does not project a late result from an abandoned scope into the active scope", async () => {
    const oldRead = deferred<unknown>();
    const client = fakeClient(async (method, params) => {
      if (method !== "tool/list") throw new Error(`unexpected ${method}`);
      const cwd = (params as { scope: GatewayRequestScope }).scope.cwd;
      return cwd === "/old" ? oldRead.promise : { toolsets: [{ name: "new" }] };
    });
    const application = new CapabilitiesApplication(client);
    const oldScope = scope("/old");
    const newScope = scope("/new");
    application.activate(oldScope);
    const pending = application.refresh("tools");
    application.activate(newScope);
    await application.refresh("tools");

    oldRead.resolve({ toolsets: [{ name: "old" }] });
    await pending;

    expect(application.getSnapshot().data.tools).toEqual({
      toolsets: [{ name: "new" }]
    });
    application.activate(oldScope);
    expect(application.getSnapshot().data.tools).toBeNull();
  });

  it("reactivates the same owner without applying reads from the disposed generation", async () => {
    const staleRead = deferred<unknown>();
    let reads = 0;
    const client = fakeClient(async (method) => {
      if (method !== "skill/list") throw new Error(`unexpected ${method}`);
      reads += 1;
      return reads === 1
        ? staleRead.promise
        : { skills: [{ name: "current" }] };
    });
    const application = new CapabilitiesApplication(client);
    const activeScope = scope("/repo");
    application.activate(activeScope);
    const stale = application.refresh("skills");

    application.dispose();
    application.attachClient(client);
    await application.refresh("skills");
    staleRead.resolve({ skills: [{ name: "stale" }] });
    await stale;

    expect(application.getSnapshot().data.skills).toEqual({
      skills: [{ name: "current" }]
    });
  });

  it("does not refresh or publish a mutation that completes after its scope is abandoned", async () => {
    const mutation = deferred<unknown>();
    const client = fakeClient(async (method) => {
      if (method === "skill/setEnabled") return mutation.promise;
      if (method === "skill/list") return { skills: [{ name: "unexpected" }] };
      throw new Error(`unexpected ${method}`);
    });
    const application = new CapabilitiesApplication(client);
    const oldScope = scope("/old");
    application.activate(oldScope);
    const pending = application.request("skill/setEnabled", {
      enabled: true,
      name: "review",
      scope: oldScope
    });

    application.activate(scope("/new"));
    mutation.resolve({ enabled: true });
    await pending;

    expect(client.calls.filter(([method]) => method === "skill/list")).toHaveLength(0);
    expect(application.getSnapshot().mutation).toBeNull();
    expect(application.getSnapshot().receipt).toBeNull();
  });

  it("does not report an abandoned mutation error into a later scope epoch", () => {
    const application = new CapabilitiesApplication();
    const oldScope = scope("/old");
    application.activate(oldScope);
    const operation = application.captureScope();

    application.activate(scope("/new"));
    application.activate(oldScope);
    application.reportError(new Error("stale failure"), operation);

    expect(application.getSnapshot().error).toBeNull();
    application.activate(null);
    expect(() => application.reportError(new Error("stale failure"), operation)).not.toThrow();
  });

  it("owns OAuth polling and refreshes MCP state after success", async () => {
    vi.useFakeTimers();
    let statusReads = 0;
    const client = fakeClient(async (method) => {
      if (method === "mcp/oauth/status") {
        statusReads += 1;
        return { status: statusReads === 1 ? "pending" : "succeeded" };
      }
      if (method === "mcp/list") return { servers: [{ name: "docs" }] };
      throw new Error(`unexpected ${method}`);
    });
    const application = new CapabilitiesApplication(client);
    const activeScope = scope("/repo");
    application.activate(activeScope);

    application.watchMcpOAuth("oauth-1");
    await vi.runAllTimersAsync();

    expect(application.getSnapshot().poll).toEqual({
      kind: "mcpOAuth",
      message: "OAuth login saved. Changes apply to the next run/session.",
      sessionId: "oauth-1",
      status: "succeeded"
    });
    expect(application.getSnapshot().data.mcp).toEqual({
      servers: [{ name: "docs" }]
    });
  });
});

type FakeClient = CapabilitiesClient & {
  calls: Array<[GatewayMethod, unknown]>;
};

function fakeClient(
  handler: (method: GatewayMethod, params: unknown) => Promise<unknown>
): FakeClient {
  const calls: Array<[GatewayMethod, unknown]> = [];
  return {
    calls,
    request: (method, params) => {
      calls.push([method, params]);
      return handler(method, params) as never;
    }
  };
}

function scope(cwd: string): GatewayRequestScope {
  return {
    cwd,
    source: {
      kind: "web",
      lifetime: "persistent",
      rawId: `cwd:${cwd}`,
      rawIdentity: null,
      visibleName: null
    }
  };
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => {
    resolve = done;
  });
  return { promise, resolve };
}
