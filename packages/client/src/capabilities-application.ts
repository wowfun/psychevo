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

export interface CapabilityScopeEpoch {
  readonly epoch: number;
  readonly key: string;
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
  private activeKey: string | null = null;
  private activeState: ScopeState | null = null;
  private readonly reads = new Map<CapabilityDomain, Promise<unknown>>();
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

  activate(scope: GatewayRequestScope | null): void {
    const key = scope ? scopeKey(scope) : null;
    if (key === this.activeKey) return;
    this.lifecycleEpoch += 1;
    this.stopPoll();
    this.reads.clear();
    this.activeKey = key;
    this.activeState = scope ? createState(scope) : null;
    this.emit();
  }

  getSnapshot(): CapabilitiesSnapshot {
    const state = this.activeState;
    if (!state) return EMPTY_SNAPSHOT;
    state.snapshot ??= freezeSnapshot(state);
    return state.snapshot;
  }

  subscribe(listener: () => void): () => void {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  }

  select(domain: CapabilityDomain, id: string | null): void {
    const state = this.requireActiveState();
    if (state.selection[domain] === id) return;
    state.selection = { ...state.selection, [domain]: id };
    this.commit(state);
  }

  clearError(): void {
    const state = this.requireActiveState();
    if (!state.error) return;
    state.error = null;
    this.commit(state);
  }

  captureScope(): CapabilityScopeEpoch | null {
    return this.activeKey === null
      ? null
      : { epoch: this.lifecycleEpoch, key: this.activeKey };
  }

  reportError(error: unknown, scope: CapabilityScopeEpoch | null): void {
    const state = this.activeState;
    if (
      !state
      || !scope
      || scope.epoch !== this.lifecycleEpoch
      || scope.key !== this.activeKey
    ) return;
    state.error = errorMessage(error);
    this.commit(state);
  }

  async refresh(domain: CapabilityDomain): Promise<unknown> {
    return this.refreshDomain(domain, false);
  }

  private async refreshDomain(domain: CapabilityDomain, force: boolean): Promise<unknown> {
    const client = this.requireClient();
    const state = this.requireActiveState();
    const scope = state.scope;
    const pending = this.reads.get(domain);
    if (pending && !force) return pending;
    if (pending) {
      try {
        await pending;
      } catch {
        // The forced post-mutation read below is authoritative.
      }
      if (this.disposed || this.activeState !== state) return undefined;
    }
    const lifecycleEpoch = this.lifecycleEpoch;
    state.loading = { ...state.loading, [domain]: true };
    state.error = null;
    this.commit(state);
    const read = readDomain(client, domain, scope)
      .then((data) => {
        if (
          this.disposed
          || lifecycleEpoch !== this.lifecycleEpoch
          || this.activeState !== state
        ) return data;
        state.data = { ...state.data, [domain]: data };
        return data;
      })
      .catch((error) => {
        if (
          !this.disposed
          && lifecycleEpoch === this.lifecycleEpoch
          && this.activeState === state
        ) {
          state.error = errorMessage(error);
        }
        throw error;
      })
      .finally(() => {
        if (this.reads.get(domain) === read) this.reads.delete(domain);
        if (
          !this.disposed
          && lifecycleEpoch === this.lifecycleEpoch
          && this.activeState === state
        ) {
          state.loading = { ...state.loading, [domain]: false };
          this.commit(state);
        }
      });
    this.reads.set(domain, read);
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
    const state = scope && domain && scopeKey(scope) === this.activeKey
      ? this.activeState
      : null;
    const lifecycleEpoch = this.lifecycleEpoch;
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
        if (!this.isCurrent(state, lifecycleEpoch)) return result;
        await this.refreshDomain(domain, true);
        if (this.isCurrent(state, lifecycleEpoch)) {
          state.receipt = {
            domain,
            method,
            revision: state.revision + 1
          };
        }
      }
      return result;
    } catch (error) {
      if (state && this.isCurrent(state, lifecycleEpoch)) {
        state.error = errorMessage(error);
      }
      throw error;
    } finally {
      if (state && this.isCurrent(state, lifecycleEpoch)) {
        state.mutation = null;
        this.commit(state);
      }
    }
  }

  watchMcpOAuth(sessionId: string): void {
    this.startPoll({
      kind: "mcpOAuth",
      method: "mcp/oauth/status",
      params: (scope) => ({ scope, sessionId }),
      sessionId,
      successMessage: "OAuth login saved. Changes apply to the next run/session.",
      failureMessage: (result) => stringField(result, "error") || "OAuth login failed.",
      domain: "mcp"
    });
  }

  watchPluginConnect(sessionId: string): void {
    this.startPoll({
      kind: "pluginConnect",
      method: "plugin/connect/status",
      params: (scope) => ({ scope, sessionId }),
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
    config: {
      domain: CapabilityDomain;
      failureMessage(result: unknown): string;
      kind: CapabilityPollState["kind"];
      method: M;
      params(scope: GatewayRequestScope): GatewayRequestInit<M>;
      sessionId: string;
      successMessage: string;
    }
  ): void {
    this.stopPoll();
    const state = this.requireActiveState();
    const scope = state.scope;
    const lifecycleEpoch = this.lifecycleEpoch;
    state.poll = {
      kind: config.kind,
      message: null,
      sessionId: config.sessionId,
      status: "pending"
    };
    this.commit(state);
    const active: ActivePoll = {
      cancelled: false,
      key: this.activeKey ?? "",
      timer: null
    };
    this.poll = active;
    const tick = async () => {
      if (
        active.cancelled
        || this.disposed
        || !this.isCurrent(state, lifecycleEpoch)
        || active.key !== this.activeKey
      ) return;
      try {
        const result = await this.requireClient().request(config.method, config.params(scope));
        if (!this.isCurrent(state, lifecycleEpoch)) return;
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
        await this.refreshDomain(config.domain, true);
        if (!this.isCurrent(state, lifecycleEpoch)) return;
        this.commit(state);
        this.stopPoll(active);
      } catch (error) {
        if (!this.isCurrent(state, lifecycleEpoch)) return;
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

  private requireActiveState(): ScopeState {
    if (!this.activeState) {
      throw new Error("CapabilitiesApplication has no active scope.");
    }
    return this.activeState;
  }

  private isCurrent(state: ScopeState, lifecycleEpoch: number): boolean {
    return !this.disposed
      && lifecycleEpoch === this.lifecycleEpoch
      && this.activeState === state;
  }

  private commit(state: ScopeState): void {
    if (state !== this.activeState) return;
    state.revision += 1;
    state.snapshot = null;
    this.emit();
  }

  private emit(): void {
    for (const listener of this.listeners) listener();
  }

  private requireClient(): CapabilitiesClient {
    if (!this.client) throw new Error("CapabilitiesApplication is not attached to a Gateway client.");
    return this.client;
  }
}

function createState(scope: GatewayRequestScope): ScopeState {
  return {
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
