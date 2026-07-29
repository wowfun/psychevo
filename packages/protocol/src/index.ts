import {
  gatewaySchemas,
  type GatewaySchemaName
} from "./generated/schemas";
import {
  strictlyCompiledGatewaySchemaNames
} from "./generated/validators";
import {
  validateGatewaySchema,
  type SchemaValidationError
} from "./schema-validator";
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
  return schema<GatewayRequestResults[M]>(
    contract.resultSchema as GatewaySchemaName
  );
}

/**
 * Compile the complete generated schema surface with strict AJV settings.
 *
 * Consumers normally compile validators lazily. This eager gate is exposed so
 * generation checks can prove that every emitted schema and reference is
 * internally coherent before a rarely used RPC reaches production.
 */
export function compileAllGatewaySchemas(): void {
  const generatedNames = new Set(strictlyCompiledGatewaySchemaNames);
  for (const name of Object.keys(gatewaySchemas) as GatewaySchemaName[]) {
    if (!generatedNames.has(name)) {
      throw new Error(`schema was not strictly compiled during generation: ${name}`);
    }
  }
}

function schema<T>(name: GatewaySchemaName): RuntimeSchema<T> {
  return {
    parse(value) {
      const validationError = validateGatewaySchema(name, value);
      if (!validationError) {
        return value as T;
      }
      throw new Error(`${name} validation failed: ${validationErrorsText([validationError])}`);
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

interface RuntimeValidationError extends SchemaValidationError {
  message?: string;
}

function validationErrorsText(
  errors: RuntimeValidationError[] | null | undefined
): string {
  if (!errors?.length) {
    return "unknown validation error";
  }
  return errors.slice(0, 5).map((error) => {
    const path = error.instancePath || "data";
    const property = error.keyword === "required"
      ? error.params?.missingProperty
      : error.params?.additionalProperty;
    if (typeof property === "string" && property) {
      const fieldPath = path === "data" ? `data.${property}` : `${path}/${property}`;
      return `${fieldPath} ${error.keyword === "required" ? "is required" : "is invalid"}`;
    }
    if (error.message) {
      return `${path} ${error.message}`;
    }
    return `${path} violates ${error.keyword ?? "schema"}`;
  }).join(", ");
}
