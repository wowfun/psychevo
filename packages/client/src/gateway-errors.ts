export type GatewayDelivery = "not_sent" | "unknown" | "acknowledged";

export type GatewayClientErrorCode =
  | "not_connected"
  | "connect_failed"
  | "connect_timeout"
  | "disconnected"
  | "request_timeout"
  | "request_aborted"
  | "protocol_fault"
  | "server_error";

export type GatewayClientErrorKind = "transport" | "server" | "protocol";

export interface GatewayClientErrorDetails {
  data?: unknown;
  kind?: GatewayClientErrorKind;
  rpcCode?: number | null;
}

export class GatewayClientError extends Error {
  readonly code: GatewayClientErrorCode;
  readonly data: unknown;
  readonly delivery: GatewayDelivery;
  readonly kind: GatewayClientErrorKind;
  readonly rpcCode: number | null;

  constructor(
    code: GatewayClientErrorCode,
    delivery: GatewayDelivery,
    message: string,
    details: GatewayClientErrorDetails = {}
  ) {
    super(message);
    this.name = "GatewayClientError";
    this.code = code;
    this.data = details.data;
    this.delivery = delivery;
    this.kind = details.kind ?? (code === "protocol_fault" ? "protocol" : "transport");
    this.rpcCode = details.rpcCode ?? null;
  }
}

export function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}
