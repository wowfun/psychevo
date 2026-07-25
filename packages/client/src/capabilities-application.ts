import type {
  GatewayMethod,
  GatewayRequestScope,
  GatewayRequestResults
} from "@psychevo/protocol";

import type {
  GatewayRequestInit,
  GatewayRequestOptions
} from "./index";

export type CapabilityDomain = "agents" | "skills" | "plugins" | "mcp" | "tools";

export interface CapabilitiesClient {
  request<M extends GatewayMethod>(
    method: M,
    params?: GatewayRequestInit<M>,
    options?: GatewayRequestOptions
  ): Promise<GatewayRequestResults[M]>;
}

export interface CapabilityOperationReceipt {
  domain: CapabilityDomain;
  method: GatewayMethod;
  revision: number;
}

export interface CapabilityPollState {
  kind: "mcpOAuth" | "pluginConnect";
  sessionId: string;
  status: "pending" | "succeeded" | "failed";
  message: string | null;
}

export interface CapabilitiesSnapshot {
  data: Readonly<Record<CapabilityDomain, unknown | null>>;
  error: string | null;
  loading: Readonly<Record<CapabilityDomain, boolean>>;
  mutation: { domain: CapabilityDomain; method: GatewayMethod } | null;
  poll: CapabilityPollState | null;
  receipt: CapabilityOperationReceipt | null;
  revision: number;
  scope: GatewayRequestScope | null;
  selection: Readonly<Record<CapabilityDomain, string | null>>;
}

type ScopeState = {
  data: Record<CapabilityDomain, unknown | null>;
  error: string | null;
  loading: Record<CapabilityDomain, boolean>;
  mutation: { domain: CapabilityDomain; method: GatewayMethod } | null;
  poll: CapabilityPollState | null;
  receipt: CapabilityOperationReceipt | null;
  revision: number;
  scope: GatewayRequestScope;
  selection: Record<CapabilityDomain, string | null>;
  snapshot: CapabilitiesSnapshot | null;
};

type ActivePoll = {
  cancelled: boolean;
  key: string;
  timer: ReturnType<typeof setTimeout> | null;
};

const DOMAINS: CapabilityDomain[] = ["agents", "skills", "plugins", "mcp", "tools"];
const EMPTY_VALUES = Object.freeze({
  agents: null,
  skills: null,
  plugins: null,
  mcp: null,
  tools: null
});
const EMPTY_LOADING = Object.freeze({
  agents: false,
  skills: false,
  plugins: false,
  mcp: false,
  tools: false
});
const EMPTY_SELECTION = Object.freeze({
  agents: null,
  skills: null,
  plugins: null,
  mcp: null,
  tools: null
});
const EMPTY_SNAPSHOT: CapabilitiesSnapshot = Object.freeze({
  data: EMPTY_VALUES,
  error: null,
  loading: EMPTY_LOADING,
  mutation: null,
  poll: null,
  receipt: null,
  revision: 0,
  scope: null,
  selection: EMPTY_SELECTION
});

export class CapabilitiesApplication implements CapabilitiesClient {
  private client: CapabilitiesClient | null;
  private readonly listeners = new Set<() => void>();
  private readonly scopes = new Map<string, ScopeState>();
  private readonly reads = new Map<string, Promise<unknown>>();
  private poll: ActivePoll | null = null;
  private disposed = false;
  private lifecycleEpoch = 0;

  constructor(client: CapabilitiesClient | null = null) {
    this.client = client;
  }

  attachClient(client: CapabilitiesClient | null): void {
    this.client = client;
    if (client) this.disposed = false;
  }

  getSnapshot(scope: GatewayRequestScope | null): CapabilitiesSnapshot {
    if (!scope) {
      return EMPTY_SNAPSHOT;
    }
    const state = this.state(scope);
    state.snapshot ??= freezeSnapshot(state);
    return state.snapshot;
  }

  subscribe(listener: () => void): () => void {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  }

  select(scope: GatewayRequestScope, domain: CapabilityDomain, id: string | null): void {
    const state = this.state(scope);
    if (state.selection[domain] === id) return;
    state.selection = { ...state.selection, [domain]: id };
    this.commit(state);
  }

  clearError(scope: GatewayRequestScope): void {
    const state = this.state(scope);
    if (!state.error) return;
    state.error = null;
    this.commit(state);
  }

  reportError(scope: GatewayRequestScope, error: unknown): void {
    const state = this.state(scope);
    state.error = errorMessage(error);
    this.commit(state);
  }

  async refresh(
    scope: GatewayRequestScope,
    domain: CapabilityDomain
  ): Promise<unknown> {
    const client = this.requireClient();
    const state = this.state(scope);
    const key = `${scopeKey(scope)}:${domain}`;
    const pending = this.reads.get(key);
    if (pending) return pending;
    const lifecycleEpoch = this.lifecycleEpoch;
    state.loading = { ...state.loading, [domain]: true };
    state.error = null;
    this.commit(state);
    const read = readDomain(client, domain, scope)
      .then((data) => {
        if (
          this.disposed
          || lifecycleEpoch !== this.lifecycleEpoch
          || this.scopes.get(scopeKey(scope)) !== state
        ) return data;
        state.data = { ...state.data, [domain]: data };
        return data;
      })
      .catch((error) => {
        if (
          !this.disposed
          && lifecycleEpoch === this.lifecycleEpoch
          && this.scopes.get(scopeKey(scope)) === state
        ) {
          state.error = errorMessage(error);
        }
        throw error;
      })
      .finally(() => {
        this.reads.delete(key);
        if (
          !this.disposed
          && lifecycleEpoch === this.lifecycleEpoch
          && this.scopes.get(scopeKey(scope)) === state
        ) {
          state.loading = { ...state.loading, [domain]: false };
          this.commit(state);
        }
      });
    this.reads.set(key, read);
    return read;
  }

  async request<M extends GatewayMethod>(
    method: M,
    params?: GatewayRequestInit<M>,
    options?: GatewayRequestOptions
  ): Promise<GatewayRequestResults[M]> {
    const client = this.requireClient();
    const scope = requestScope(params);
    const domain = mutationDomain(method);
    const state = scope && domain ? this.state(scope) : null;
    if (state && domain) {
      state.mutation = { domain, method };
      state.error = null;
      this.commit(state);
    }
    try {
      const result = options === undefined
        ? await client.request(method, params)
        : await client.request(method, params, options);
      if (state && scope && domain) {
        await this.refresh(scope, domain);
        state.receipt = {
          domain,
          method,
          revision: state.revision + 1
        };
      }
      return result;
    } catch (error) {
      if (state) state.error = errorMessage(error);
      throw error;
    } finally {
      if (state) {
        state.mutation = null;
        this.commit(state);
      }
    }
  }

  watchMcpOAuth(scope: GatewayRequestScope, sessionId: string): void {
    this.startPoll(scope, {
      kind: "mcpOAuth",
      method: "mcp/oauth/status",
      params: () => ({ scope, sessionId }),
      sessionId,
      successMessage: "OAuth login saved. Changes apply to the next run/session.",
      failureMessage: (result) => stringField(result, "error") || "OAuth login failed.",
      domain: "mcp"
    });
  }

  watchPluginConnect(scope: GatewayRequestScope, sessionId: string): void {
    this.startPoll(scope, {
      kind: "pluginConnect",
      method: "plugin/connect/status",
      params: () => ({ scope, sessionId }),
      sessionId,
      successMessage: "Plugin connection is ready.",
      failureMessage: (result) => stringField(result, "reason") || "Plugin connection failed.",
      domain: "plugins"
    });
  }

  dispose(): void {
    this.disposed = true;
    this.lifecycleEpoch += 1;
    this.stopPoll();
    this.listeners.clear();
    this.reads.clear();
  }

  private startPoll<M extends "mcp/oauth/status" | "plugin/connect/status">(
    scope: GatewayRequestScope,
    config: {
      domain: CapabilityDomain;
      failureMessage(result: unknown): string;
      kind: CapabilityPollState["kind"];
      method: M;
      params(): GatewayRequestInit<M>;
      sessionId: string;
      successMessage: string;
    }
  ): void {
    this.stopPoll();
    const state = this.state(scope);
    state.poll = {
      kind: config.kind,
      message: null,
      sessionId: config.sessionId,
      status: "pending"
    };
    this.commit(state);
    const active: ActivePoll = {
      cancelled: false,
      key: scopeKey(scope),
      timer: null
    };
    this.poll = active;
    const tick = async () => {
      if (active.cancelled || this.disposed) return;
      try {
        const result = await this.requireClient().request(config.method, config.params());
        const status = stringField(result, "status");
        if (status === "pending") {
          active.timer = setTimeout(tick, 1_200);
          return;
        }
        state.poll = {
          kind: config.kind,
          message: status === "succeeded"
            ? config.successMessage
            : config.failureMessage(result),
          sessionId: config.sessionId,
          status: status === "succeeded" ? "succeeded" : "failed"
        };
        await this.refresh(scope, config.domain);
        this.commit(state);
        this.stopPoll(active);
      } catch (error) {
        state.error = errorMessage(error);
        state.poll = {
          kind: config.kind,
          message: state.error,
          sessionId: config.sessionId,
          status: "failed"
        };
        this.commit(state);
        this.stopPoll(active);
      }
    };
    active.timer = setTimeout(tick, 0);
  }

  private stopPoll(expected: ActivePoll | null = null): void {
    const active = this.poll;
    if (!active || (expected && active !== expected)) return;
    active.cancelled = true;
    if (active.timer !== null) clearTimeout(active.timer);
    this.poll = null;
  }

  private state(scope: GatewayRequestScope): ScopeState {
    const key = scopeKey(scope);
    const existing = this.scopes.get(key);
    if (existing) return existing;
    const created: ScopeState = {
      data: { ...EMPTY_VALUES },
      error: null,
      loading: { ...EMPTY_LOADING },
      mutation: null,
      poll: null,
      receipt: null,
      revision: 0,
      scope,
      selection: { ...EMPTY_SELECTION },
      snapshot: null
    };
    this.scopes.set(key, created);
    return created;
  }

  private commit(state: ScopeState): void {
    state.revision += 1;
    state.snapshot = null;
    for (const listener of this.listeners) listener();
  }

  private requireClient(): CapabilitiesClient {
    if (!this.client) throw new Error("CapabilitiesApplication is not attached to a Gateway client.");
    return this.client;
  }
}

function freezeSnapshot(state: ScopeState): CapabilitiesSnapshot {
  return Object.freeze({
    data: Object.freeze({ ...state.data }),
    error: state.error,
    loading: Object.freeze({ ...state.loading }),
    mutation: state.mutation ? Object.freeze({ ...state.mutation }) : null,
    poll: state.poll ? Object.freeze({ ...state.poll }) : null,
    receipt: state.receipt ? Object.freeze({ ...state.receipt }) : null,
    revision: state.revision,
    scope: state.scope,
    selection: Object.freeze({ ...state.selection })
  });
}

async function readDomain(
  client: CapabilitiesClient,
  domain: CapabilityDomain,
  scope: GatewayRequestScope
): Promise<unknown> {
  if (domain === "agents") {
    const backendList = await client.request("backend/list", { scope });
    const [agents, teams, runtimeProfiles] = await Promise.all([
      client.request("agent/list", { scope }),
      client.request("team/list", { scope }),
      client.request("runtime/profile/list", { scope })
    ]);
    return {
      ...objectValue(agents),
      ...objectValue(backendList),
      teams: objectValue(teams),
      runtimeProfiles: objectValue(runtimeProfiles)
    };
  }
  if (domain === "skills") return client.request("skill/list", { scope });
  if (domain === "plugins") return client.request("plugin/list", { scope });
  if (domain === "mcp") return client.request("mcp/list", { scope });
  return client.request("tool/list", { scope });
}

function mutationDomain(method: GatewayMethod): CapabilityDomain | null {
  if (
    method.startsWith("agent/")
    || method.startsWith("team/")
    || method.startsWith("backend/")
    || method.startsWith("runtime/profile/")
  ) {
    return readOnlyMethod(method) ? null : "agents";
  }
  if (method.startsWith("skill/")) return readOnlyMethod(method) ? null : "skills";
  if (method.startsWith("plugin/")) return readOnlyMethod(method) ? null : "plugins";
  if (method.startsWith("mcp/")) return readOnlyMethod(method) ? null : "mcp";
  if (method.startsWith("tool/")) return readOnlyMethod(method) ? null : "tools";
  return null;
}

function readOnlyMethod(method: GatewayMethod): boolean {
  return method.endsWith("/list")
    || method.endsWith("/read")
    || method.endsWith("/status")
    || method.endsWith("/doctor")
    || method.endsWith("/test")
    || method.endsWith("/inspect");
}

function requestScope<M extends GatewayMethod>(
  params: GatewayRequestInit<M> | undefined
): GatewayRequestScope | null {
  if (!params || typeof params !== "object" || !("scope" in params)) return null;
  const scope = (params as { scope?: unknown }).scope;
  if (!scope || typeof scope !== "object" || typeof (scope as { cwd?: unknown }).cwd !== "string") {
    return null;
  }
  return scope as GatewayRequestScope;
}

function scopeKey(scope: GatewayRequestScope): string {
  return JSON.stringify([
    scope.cwd,
    scope.source.kind,
    scope.source.rawId ?? null,
    scope.source.lifetime,
    scope.source.rawIdentity ?? null
  ]);
}

function objectValue(value: unknown): Record<string, unknown> {
  return value && typeof value === "object" && !Array.isArray(value)
    ? value as Record<string, unknown>
    : {};
}

function stringField(value: unknown, key: string): string {
  const field = objectValue(value)[key];
  return typeof field === "string" ? field : "";
}

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}
