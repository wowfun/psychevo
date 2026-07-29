import {
  gatewaySchemaRefs,
  gatewaySchemas,
  type GatewaySchemaName
} from "./generated/schemas";

type JsonSchema = boolean | Record<string, unknown>;

export interface SchemaValidationError {
  instancePath: string;
  keyword: string;
  params?: {
    additionalProperty?: string;
    missingProperty?: string;
  };
}

const externalSchemas = new Map<string, JsonSchema>();
for (const candidate of gatewaySchemaRefs) {
  const schema = candidate as JsonSchema;
  if (typeof schema === "object" && typeof schema.$id === "string") {
    externalSchemas.set(schema.$id, schema);
  }
}

export function validateGatewaySchema(
  name: GatewaySchemaName,
  value: unknown
): SchemaValidationError | null {
  const schema = gatewaySchemas[name] as JsonSchema;
  return validateSchema(schema, value, "", schema, new Set());
}

function validateSchema(
  schema: JsonSchema,
  value: unknown,
  instancePath: string,
  documentRoot: JsonSchema,
  ancestors: Set<object>
): SchemaValidationError | null {
  if (schema === true) {
    return null;
  }
  if (schema === false) {
    return error(instancePath, "falseSchema");
  }

  if (typeof schema.$ref === "string") {
    const resolved = resolveReference(schema.$ref, documentRoot);
    if (!resolved) {
      return error(instancePath, "$ref");
    }
    return validateSchema(
      resolved.schema,
      value,
      instancePath,
      resolved.documentRoot,
      ancestors
    );
  }

  const allOfError = validateAllOf(schema.allOf, value, instancePath, documentRoot, ancestors);
  if (allOfError) {
    return allOfError;
  }
  if (!matchesAny(schema.anyOf, value, instancePath, documentRoot, ancestors)) {
    return error(instancePath, "anyOf");
  }
  if (!matchesExactlyOne(schema.oneOf, value, instancePath, documentRoot, ancestors)) {
    return error(instancePath, "oneOf");
  }
  if (!matchesType(schema.type, value)) {
    return error(instancePath, "type");
  }
  if (!matchesEnum(schema.enum, value)) {
    return error(instancePath, "enum");
  }

  if (typeof value === "number") {
    if (typeof schema.minimum === "number" && value < schema.minimum) {
      return error(instancePath, "minimum");
    }
    if (typeof schema.maximum === "number" && value > schema.maximum) {
      return error(instancePath, "maximum");
    }
  }

  if (Array.isArray(value)) {
    return validateArray(schema, value, instancePath, documentRoot, ancestors);
  }
  if (isObject(value)) {
    return validateObject(schema, value, instancePath, documentRoot, ancestors);
  }
  return null;
}

function validateAllOf(
  candidate: unknown,
  value: unknown,
  instancePath: string,
  documentRoot: JsonSchema,
  ancestors: Set<object>
): SchemaValidationError | null {
  if (!Array.isArray(candidate)) {
    return null;
  }
  for (const branch of candidate) {
    const branchError = validateSchema(
      branch as JsonSchema,
      value,
      instancePath,
      documentRoot,
      ancestors
    );
    if (branchError) {
      return branchError;
    }
  }
  return null;
}

function matchesAny(
  candidate: unknown,
  value: unknown,
  instancePath: string,
  documentRoot: JsonSchema,
  ancestors: Set<object>
): boolean {
  return !Array.isArray(candidate) || candidate.some((branch) => (
    validateSchema(branch as JsonSchema, value, instancePath, documentRoot, ancestors) === null
  ));
}

function matchesExactlyOne(
  candidate: unknown,
  value: unknown,
  instancePath: string,
  documentRoot: JsonSchema,
  ancestors: Set<object>
): boolean {
  if (!Array.isArray(candidate)) {
    return true;
  }
  let matches = 0;
  for (const branch of candidate) {
    if (validateSchema(branch as JsonSchema, value, instancePath, documentRoot, ancestors) === null) {
      matches += 1;
      if (matches > 1) {
        return false;
      }
    }
  }
  return matches === 1;
}

function matchesType(candidate: unknown, value: unknown): boolean {
  if (candidate == null) {
    return true;
  }
  const types = Array.isArray(candidate) ? candidate : [candidate];
  return types.some((type) => {
    switch (type) {
      case "array":
        return Array.isArray(value);
      case "boolean":
        return typeof value === "boolean";
      case "integer":
        return typeof value === "number" && Number.isFinite(value) && Number.isInteger(value);
      case "null":
        return value === null;
      case "number":
        return typeof value === "number" && Number.isFinite(value);
      case "object":
        return isObject(value);
      case "string":
        return typeof value === "string";
      default:
        return false;
    }
  });
}

function matchesEnum(candidate: unknown, value: unknown): boolean {
  return !Array.isArray(candidate) || candidate.some((item) => jsonEqual(item, value));
}

function validateArray(
  schema: Record<string, unknown>,
  value: unknown[],
  instancePath: string,
  documentRoot: JsonSchema,
  ancestors: Set<object>
): SchemaValidationError | null {
  if (ancestors.has(value)) {
    return error(instancePath, "cyclic");
  }
  if (schema.items == null) {
    return null;
  }
  ancestors.add(value);
  try {
    if (Array.isArray(schema.items)) {
      for (let index = 0; index < Math.min(schema.items.length, value.length); index += 1) {
        const itemError = validateSchema(
          schema.items[index] as JsonSchema,
          value[index],
          appendPath(instancePath, String(index)),
          documentRoot,
          ancestors
        );
        if (itemError) {
          return itemError;
        }
      }
      return null;
    }
    for (let index = 0; index < value.length; index += 1) {
      const itemError = validateSchema(
        schema.items as JsonSchema,
        value[index],
        appendPath(instancePath, String(index)),
        documentRoot,
        ancestors
      );
      if (itemError) {
        return itemError;
      }
    }
    return null;
  } finally {
    ancestors.delete(value);
  }
}

function validateObject(
  schema: Record<string, unknown>,
  value: Record<string, unknown>,
  instancePath: string,
  documentRoot: JsonSchema,
  ancestors: Set<object>
): SchemaValidationError | null {
  if (ancestors.has(value)) {
    return error(instancePath, "cyclic");
  }
  const properties = isObject(schema.properties) ? schema.properties : {};
  if (Array.isArray(schema.required)) {
    for (const property of schema.required) {
      if (
        typeof property === "string"
        && (!Object.hasOwn(value, property) || value[property] === undefined)
      ) {
        return {
          instancePath,
          keyword: "required",
          params: { missingProperty: property }
        };
      }
    }
  }

  ancestors.add(value);
  try {
    for (const [property, propertySchema] of Object.entries(properties)) {
      if (!Object.hasOwn(value, property) || value[property] === undefined) {
        continue;
      }
      const propertyError = validateSchema(
        propertySchema as JsonSchema,
        value[property],
        appendPath(instancePath, property),
        documentRoot,
        ancestors
      );
      if (propertyError) {
        return propertyError;
      }
    }
    for (const property of Object.keys(value)) {
      if (Object.hasOwn(properties, property)) {
        continue;
      }
      if (schema.additionalProperties === false) {
        return {
          instancePath,
          keyword: "additionalProperties",
          params: { additionalProperty: property }
        };
      }
      if (isSchema(schema.additionalProperties)) {
        const propertyError = validateSchema(
          schema.additionalProperties,
          value[property],
          appendPath(instancePath, property),
          documentRoot,
          ancestors
        );
        if (propertyError) {
          return propertyError;
        }
      }
    }
    return null;
  } finally {
    ancestors.delete(value);
  }
}

function resolveReference(
  reference: string,
  documentRoot: JsonSchema
): { documentRoot: JsonSchema; schema: JsonSchema } | null {
  if (reference.startsWith("#")) {
    const schema = resolveJsonPointer(documentRoot, reference.slice(1));
    return isSchema(schema) ? { documentRoot, schema } : null;
  }
  const hashIndex = reference.indexOf("#");
  const documentId = hashIndex >= 0 ? reference.slice(0, hashIndex) : reference;
  const external = externalSchemas.get(documentId);
  if (!external) {
    return null;
  }
  const schema = hashIndex >= 0
    ? resolveJsonPointer(external, reference.slice(hashIndex + 1))
    : external;
  return isSchema(schema) ? { documentRoot: external, schema } : null;
}

function resolveJsonPointer(root: JsonSchema, pointer: string): unknown {
  if (!pointer) {
    return root;
  }
  if (!pointer.startsWith("/")) {
    return undefined;
  }
  let current: unknown = root;
  for (const rawSegment of pointer.slice(1).split("/")) {
    if (!isObject(current)) {
      return undefined;
    }
    const segment = rawSegment.replace(/~1/g, "/").replace(/~0/g, "~");
    current = current[segment];
  }
  return current;
}

function isSchema(value: unknown): value is JsonSchema {
  return typeof value === "boolean" || isObject(value);
}

function isObject(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function appendPath(instancePath: string, segment: string): string {
  return `${instancePath}/${segment.replace(/~/g, "~0").replace(/\//g, "~1")}`;
}

function error(instancePath: string, keyword: string): SchemaValidationError {
  return { instancePath, keyword };
}

function jsonEqual(left: unknown, right: unknown): boolean {
  if (Object.is(left, right)) {
    return true;
  }
  if (Array.isArray(left) && Array.isArray(right)) {
    return left.length === right.length
      && left.every((item, index) => jsonEqual(item, right[index]));
  }
  if (isObject(left) && isObject(right)) {
    const leftKeys = Object.keys(left);
    const rightKeys = Object.keys(right);
    return leftKeys.length === rightKeys.length
      && leftKeys.every((key) => Object.hasOwn(right, key) && jsonEqual(left[key], right[key]));
  }
  return false;
}
