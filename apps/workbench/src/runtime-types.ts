import type { GatewayClient } from "@psychevo/client";
import type { GatewayRequestScope } from "@psychevo/protocol";

export type RefreshSnapshot = (
  nextClient?: GatewayClient | null,
  threadId?: string,
  scope?: GatewayRequestScope,
  readOnly?: boolean,
  expectedEpoch?: number | null,
  allowDetachedAdoption?: boolean
) => Promise<void>;

export type RefreshWorkspaceSurface = (
  nextClient?: GatewayClient | null,
  scope?: GatewayRequestScope,
  threadId?: string | null,
  expectedEpoch?: number | null
) => Promise<void>;
