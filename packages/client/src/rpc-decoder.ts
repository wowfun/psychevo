import {
  gatewayResponseResultSchema,
  RpcNotificationSchema,
  RpcResponseSchema,
  type GatewayMethod,
  type JsonRpcErrorResponse,
  type JsonRpcSuccess,
  type RpcNotification
} from "@psychevo/protocol";

export type DecodedRpcEnvelope =
  | { kind: "notification"; notification: RpcNotification }
  | { kind: "response"; response: JsonRpcSuccess | JsonRpcErrorResponse };

export function decodeRpcEnvelope(data: unknown): DecodedRpcEnvelope {
  const raw = typeof data === "string" ? data : String(data);
  const value = JSON.parse(raw) as unknown;
  const record = asRecord(value);
  if (record && !Object.prototype.hasOwnProperty.call(record, "id")) {
    return {
      kind: "notification",
      notification: RpcNotificationSchema.parse(value)
    };
  }
  return {
    kind: "response",
    response: RpcResponseSchema.parse(value)
  };
}

export function decodeRpcResult(method: GatewayMethod, value: unknown): unknown {
  return gatewayResponseResultSchema(method).parse(value);
}

function asRecord(value: unknown): Record<string, unknown> | null {
  return value !== null && typeof value === "object" && !Array.isArray(value)
    ? value as Record<string, unknown>
    : null;
}
