import { GatewayClient } from "@psychevo/client";
import {
  createBrowserHost,
  type GatewayEndpoint,
  type PsychevoHost
} from "@psychevo/host";

export interface WorkbenchRuntime {
  client: GatewayClient;
  dispose(): Promise<void> | void;
  endpoint: GatewayEndpoint;
  fallbackCwd: string;
  host: PsychevoHost;
  onOpenThreadRequest?(handler: (threadId: string) => void): Promise<() => void> | (() => void);
}

export type WorkbenchRuntimeFactory = () => Promise<WorkbenchRuntime> | WorkbenchRuntime;

export function createBrowserWorkbenchRuntime(): WorkbenchRuntime {
  const host = createBrowserHost(window.location, window.localStorage);
  const client = new GatewayClient(host.endpoint);
  return {
    client,
    dispose: () => client.close(),
    endpoint: host.endpoint,
    fallbackCwd: browserFallbackCwd(),
    host
  };
}

export function browserFallbackCwd(): string {
  return typeof window === "undefined" ? "/" : window.location.pathname || "/";
}
