import Ajv, { type ValidateFunction } from "ajv";
import {
  gatewaySchemaRefs,
  gatewaySchemas,
  type GatewaySchemaName
} from "./generated/schemas";
import {
  gatewayMethodContracts,
  type GatewayMethod,
  type GatewayRequestParams,
  type GatewayRequestResults,
  type GatewayResultValidation
} from "./generated/methods";
export * from "./generated";
export type {
  GatewayActivityView as GatewayActivity,
  JsonRpcNotification as RpcNotification,
  PendingActionView as PendingAction,
  SessionSummaryView as SessionSummary
} from "./generated";
import type {
  ClientRequest,
  GatewayEvent,
  AutomationDraftResult,
  AutomationListResult,
  AutomationMutationResult,
  AutomationRunResult,
  ContextReadResult,
  CompletionListResult,
  InitializeResult,
  JsonRpcErrorResponse,
  JsonRpcNotification,
  JsonRpcSuccess,
  ObservabilityReadResult,
  SettingsReadResult,
  TerminalExitedPayload,
  TerminalOutputPayload,
  ThreadBrowserResult,
  ThreadListResult,
  ThreadTraceResult,
  ThreadSnapshot,
  UsageReadResult,
  WorkspaceCreateResult,
  WorkspaceChangeMutationResult,
  WorkspaceChangesResult,
  WorkspaceDiffResult,
  WorkspaceFilePreviewOpenResult,
  WorkspaceFilePreviewReleaseResult,
  WorkspaceFileReadResult,
  WorkspaceFileWriteResult,
  WorkspaceFilesResult
} from "./generated";

const ajv = new Ajv({ allErrors: true, strict: false, validateFormats: false });
const compiled = new Map<GatewaySchemaName, ValidateFunction>();
let schemaRefsRegistered = false;

export const SIDE_INHERITED_METADATA_KEY = "side_inherited";

export function sideInheritedMetadataHidden(metadata: unknown): boolean {
  const record = recordForValue(metadata);
  const sideInherited = recordForValue(record[SIDE_INHERITED_METADATA_KEY]);
  return sideInherited.hidden === true;
}

export type SafeParseResult<T> =
  | { data: T; success: true }
  | { error: Error; success: false };

export interface RuntimeSchema<T> {
  parse(value: unknown): T;
  safeParse(value: unknown): SafeParseResult<T>;
}

export interface GatewayMethodValidation {
  params: "precise";
  result: GatewayResultValidation;
}

export const RpcNotificationSchema = schema<JsonRpcNotification>("JsonRpcNotification");
export const JsonRpcSuccessSchema = schema<JsonRpcSuccess>("JsonRpcSuccess");
export const JsonRpcErrorResponseSchema =
  schema<JsonRpcErrorResponse>("JsonRpcErrorResponse");
export const ClientRequestSchema = schema<ClientRequest>("ClientRequest");
export const GatewayEventSchema = schema<GatewayEvent>("GatewayEvent");
export const ThreadSnapshotSchema = schema<ThreadSnapshot>("ThreadSnapshot");
export const ThreadBrowserResultSchema =
  schema<ThreadBrowserResult>("ThreadBrowserResult");
export const ThreadListResultSchema = schema<ThreadListResult>("ThreadListResult");
export const ThreadTraceResultSchema = schema<ThreadTraceResult>("ThreadTraceResult");
export const CompletionListResultSchema =
  schema<CompletionListResult>("CompletionListResult");
export const TerminalOutputPayloadSchema =
  schema<TerminalOutputPayload>("TerminalOutputPayload");
export const TerminalExitedPayloadSchema =
  schema<TerminalExitedPayload>("TerminalExitedPayload");
export const InitializeResultSchema = schema<InitializeResult>("InitializeResult");
export const AutomationListResultSchema =
  schema<AutomationListResult>("AutomationListResult");
export const AutomationDraftResultSchema =
  schema<AutomationDraftResult>("AutomationDraftResult");
export const AutomationMutationResultSchema =
  schema<AutomationMutationResult>("AutomationMutationResult");
export const AutomationRunResultSchema =
  schema<AutomationRunResult>("AutomationRunResult");
export const SettingsReadResultSchema =
  schema<SettingsReadResult>("SettingsReadResult");
export const WorkspaceCreateResultSchema =
  schema<WorkspaceCreateResult>("WorkspaceCreateResult");
export const WorkspaceFilesResultSchema =
  schema<WorkspaceFilesResult>("WorkspaceFilesResult");
export const WorkspaceFileReadResultSchema =
  schema<WorkspaceFileReadResult>("WorkspaceFileReadResult");
export const WorkspaceFilePreviewOpenResultSchema =
  schema<WorkspaceFilePreviewOpenResult>("WorkspaceFilePreviewOpenResult");
export const WorkspaceFilePreviewReleaseResultSchema =
  schema<WorkspaceFilePreviewReleaseResult>("WorkspaceFilePreviewReleaseResult");
export const WorkspaceFileWriteResultSchema =
  schema<WorkspaceFileWriteResult>("WorkspaceFileWriteResult");
export const WorkspaceDiffResultSchema =
  schema<WorkspaceDiffResult>("WorkspaceDiffResult");
export const WorkspaceChangesResultSchema =
  schema<WorkspaceChangesResult>("WorkspaceChangesResult");
export const WorkspaceChangeMutationResultSchema =
  schema<WorkspaceChangeMutationResult>("WorkspaceChangeMutationResult");
export const ContextReadResultSchema =
  schema<ContextReadResult>("ContextReadResult");
export const ObservabilityReadResultSchema =
  schema<ObservabilityReadResult>("ObservabilityReadResult");
export const UsageReadResultSchema =
  schema<UsageReadResult>("UsageReadResult");

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

export function gatewayMethodValidation(
  method: GatewayMethod
): GatewayMethodValidation {
  return {
    params: "precise",
    result: gatewayMethodContracts[method].resultValidation
  };
}

export function gatewayRequestParamsSchema<M extends GatewayMethod>(
  method: M
): RuntimeSchema<GatewayRequestParams[M]> {
  return schema<GatewayRequestParams[M]>(
    gatewayMethodContracts[method].paramsSchema as GatewaySchemaName
  );
}

export function gatewayResponseResultSchema<M extends GatewayMethod>(
  method: M
): RuntimeSchema<GatewayRequestResults[M]> {
  const contract = gatewayMethodContracts[method];
  if (contract.resultSchema === null) {
    return opaqueObjectSchema as RuntimeSchema<GatewayRequestResults[M]>;
  }
  return schema<GatewayRequestResults[M]>(
    contract.resultSchema as GatewaySchemaName
  );
}

const opaqueObjectSchema: RuntimeSchema<Record<string, unknown>> = {
  parse(value) {
    if (typeof value === "object" && value !== null && !Array.isArray(value)) {
      return value as Record<string, unknown>;
    }
    throw new Error("opaque Gateway result validation failed: expected an object");
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

function recordForValue(value: unknown): Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value) ? value as Record<string, unknown> : {};
}

function validator(name: GatewaySchemaName): ValidateFunction {
  const existing = compiled.get(name);
  if (existing) {
    return existing;
  }
  registerSchemaRefs();
  const validate = ajv.compile(gatewaySchemas[name]);
  compiled.set(name, validate);
  return validate;
}

function registerSchemaRefs(): void {
  if (schemaRefsRegistered) {
    return;
  }
  for (const schemaRef of gatewaySchemaRefs) {
    ajv.addSchema(schemaRef);
  }
  schemaRefsRegistered = true;
}
