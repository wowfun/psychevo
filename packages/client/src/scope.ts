import type { GatewayRequestScope } from "@psychevo/protocol";

export function gatewayScopeKey(scope: GatewayRequestScope | null): string {
  if (!scope) {
    return "";
  }
  return JSON.stringify([
    scope.cwd,
    scope.source.kind,
    scope.source.rawId ?? null,
    scope.source.lifetime
  ]);
}
