import Ajv, { type ValidateFunction } from "ajv";
import { gatewaySchemas, type GatewaySchemaName } from "./generated/schemas";
export * from "./generated";
export type {
  GatewayActivityView as GatewayActivity,
  JsonRpcNotification as RpcNotification,
  PendingClarifyView as PendingClarify,
  PendingPermissionView as PendingPermission,
  SessionSummaryView as SessionSummary,
  TurnResultPayload as TurnResultNotification
} from "./generated";
import type {
  GatewayEvent,
  ContextReadResult,
  CompletionListResult,
  InitializeResult,
  JsonRpcErrorResponse,
  JsonRpcNotification,
  JsonRpcSuccess,
  SettingsReadResult,
  ThreadListResult,
  ThreadSnapshot,
  TurnResultPayload,
  WorkspaceDiffResult,
  WorkspaceFileReadResult,
  WorkspaceFilesResult
} from "./generated";

const ajv = new Ajv({ allErrors: true, strict: false, validateFormats: false });
const compiled = new Map<GatewaySchemaName, ValidateFunction>();

export type SafeParseResult<T> =
  | { data: T; success: true }
  | { error: Error; success: false };

export interface RuntimeSchema<T> {
  parse(value: unknown): T;
  safeParse(value: unknown): SafeParseResult<T>;
}

export const RpcNotificationSchema = schema<JsonRpcNotification>("JsonRpcNotification");
export const JsonRpcSuccessSchema = schema<JsonRpcSuccess>("JsonRpcSuccess");
export const JsonRpcErrorResponseSchema =
  schema<JsonRpcErrorResponse>("JsonRpcErrorResponse");
export const GatewayEventSchema = schema<GatewayEvent>("GatewayEvent");
export const ThreadSnapshotSchema = schema<ThreadSnapshot>("ThreadSnapshot");
export const ThreadListResultSchema = schema<ThreadListResult>("ThreadListResult");
export const CompletionListResultSchema =
  schema<CompletionListResult>("CompletionListResult");
export const TurnResultNotificationSchema =
  schema<TurnResultPayload>("TurnResultPayload");
export const InitializeResultSchema = schema<InitializeResult>("InitializeResult");
export const SettingsReadResultSchema =
  schema<SettingsReadResult>("SettingsReadResult");
export const WorkspaceFilesResultSchema =
  schema<WorkspaceFilesResult>("WorkspaceFilesResult");
export const WorkspaceFileReadResultSchema =
  schema<WorkspaceFileReadResult>("WorkspaceFileReadResult");
export const WorkspaceDiffResultSchema =
  schema<WorkspaceDiffResult>("WorkspaceDiffResult");
export const ContextReadResultSchema =
  schema<ContextReadResult>("ContextReadResult");

export const RpcResponseSchema: RuntimeSchema<JsonRpcSuccess | JsonRpcErrorResponse> = {
  parse(value) {
    const success = JsonRpcSuccessSchema.safeParse(value);
    if (success.success) {
      return success.data;
    }
    return JsonRpcErrorResponseSchema.parse(value);
  },
  safeParse(value) {
    try {
      return { data: this.parse(value), success: true };
    } catch (error) {
      return {
        error: error instanceof Error ? error : new Error(String(error)),
        success: false
      };
    }
  }
};

function schema<T>(name: GatewaySchemaName): RuntimeSchema<T> {
  return {
    parse(value) {
      const validate = validator(name);
      if (validate(value)) {
        return value as T;
      }
      throw new Error(`${name} validation failed: ${ajv.errorsText(validate.errors)}`);
    },
    safeParse(value) {
      try {
        return { data: this.parse(value), success: true };
      } catch (error) {
        return {
          error: error instanceof Error ? error : new Error(String(error)),
          success: false
        };
      }
    }
  };
}

function validator(name: GatewaySchemaName): ValidateFunction {
  const existing = compiled.get(name);
  if (existing) {
    return existing;
  }
  const validate = ajv.compile(gatewaySchemas[name]);
  compiled.set(name, validate);
  return validate;
}
