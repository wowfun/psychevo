import { describe, expect, it } from "vitest";
import publicPythonWireFixtures from "../fixtures/app-python-wire.json";
import {
  ClientRequestSchema,
  GatewayEventSchema,
  ThreadSnapshotSchema,
  compileAllGatewaySchemas,
  gatewayMethodValidation,
  gatewayMethodContracts,
  gatewayRequestParamsSchema,
  gatewayResponseResultSchema,
  gatewaySchemas
} from "./index";
import { validateGatewaySchema } from "./schema-validator";

describe("public Python wire decoder corpus", () => {
  it("accepts, rejects, and canonically preserves every shared wire shape", () => {
    expect(publicPythonWireFixtures.schemaVersion).toBe(1);
    expect(Object.keys(publicPythonWireFixtures.decoders)).toHaveLength(11);
    for (const [decoder, cases] of Object.entries(publicPythonWireFixtures.decoders)) {
      expect(Object.hasOwn(gatewaySchemas, cases.schema), decoder).toBe(true);
      const schema = cases.schema as keyof typeof gatewaySchemas;
      for (const fixture of cases.valid) {
        const decoded: unknown = JSON.parse(JSON.stringify(fixture.value));
        expect(
          validateGatewaySchema(schema, decoded),
          `${decoder}: ${fixture.name}`
        ).toBeNull();
        expect(decoded, `${decoder}: ${fixture.name}`).toEqual(fixture.value);
      }
      for (const fixture of cases.invalid) {
        expect(
          validateGatewaySchema(schema, fixture.value),
          `${decoder}: ${fixture.name}`
        ).not.toBeNull();
      }
    }
  });
});

describe("ClientRequestSchema", () => {
  it("validates every corrected generated request signature", () => {
    expect(ClientRequestSchema.safeParse({
      method: "thread/history/draft/read",
      params: {
        scope: {
          cwd: "/tmp/project",
          source: { kind: "web", rawId: "thread-test" }
        },
        threadId: "thread-1",
        messageId: "message:7"
      }
    }).success).toBe(true);
    expect(ClientRequestSchema.safeParse({
      method: "workspace/create",
      params: { name: "research", parent: "/tmp/workspaces" }
    }).success).toBe(true);
  });

  it("requires a client correlation id for turn/start", () => {
    const params = {
      scope: {
        cwd: "/tmp/project",
        source: { kind: "web", rawId: "thread-test" }
      },
      input: [{ type: "text", text: "hello" }]
    };
    expect(ClientRequestSchema.safeParse({
      method: "turn/start",
      params: { ...params, clientTurnId: "client-turn-1" }
    }).success).toBe(true);
    expect(ClientRequestSchema.safeParse({
      method: "turn/start",
      params
    }).success).toBe(false);
  });
});

describe("generated method validator registry", () => {
  it("strictly compiles every generated schema and reference", () => {
    expect(() => compileAllGatewaySchemas()).not.toThrow();
  });

  it("validates through the public runtime schema without dynamic code generation", () => {
    const originalFunction = globalThis.Function;
    const originalEval = globalThis.eval;
    Object.defineProperty(globalThis, "Function", {
      configurable: true,
      value: () => {
        throw new Error("dynamic Function is blocked by production CSP");
      }
    });
    Object.defineProperty(globalThis, "eval", {
      configurable: true,
      value: () => {
        throw new Error("eval is blocked by production CSP");
      }
    });
    try {
      expect(gatewayResponseResultSchema("plugin/list").safeParse({
        plugins: [],
        count: 0,
        codex_authority: { kind: "codex", readiness: "unavailable" },
        authorities: []
      }).success).toBe(true);
      expect(gatewayResponseResultSchema("plugin/list").safeParse([]).success).toBe(false);
    } finally {
      Object.defineProperty(globalThis, "Function", {
        configurable: true,
        value: originalFunction
      });
      Object.defineProperty(globalThis, "eval", {
        configurable: true,
        value: originalEval
      });
    }
  });

  it("has a precise result schema for every method", () => {
    expect(gatewayMethodValidation("thread/read")).toEqual({
      params: "precise",
      result: "precise"
    });
    expect(gatewayMethodValidation("plugin/list")).toEqual({
      params: "precise",
      result: "precise"
    });
    expect(Object.values(gatewayMethodContracts).every(
      (contract) => contract.resultValidation === "precise"
        && typeof contract.resultSchema === "string"
    )).toBe(true);
  });

  it("follows Rust optionality for params and validates result semantics", () => {
    expect(gatewayRequestParamsSchema("thread/list").safeParse({}).success).toBe(true);
    expect(gatewayRequestParamsSchema("thread/read").safeParse({}).success).toBe(false);
    expect(gatewayRequestParamsSchema("thread/read").safeParse({
      scope: undefined,
      threadId: "thread-1"
    }).success).toBe(true);
    expect(gatewayRequestParamsSchema("turn/start").safeParse({
      scope: {
        cwd: "/tmp/project",
        source: { kind: "web", rawId: "thread-test" }
      },
      clientTurnId: "client-turn-1"
    }).success).toBe(true);
    expect(gatewayResponseResultSchema("thread/read").safeParse({}).success).toBe(false);
    expect(gatewayResponseResultSchema("plugin/list").safeParse({
      plugins: [],
      count: 0,
      codex_authority: { kind: "codex", readiness: "unavailable" },
      authorities: []
    }).success).toBe(true);
    expect(gatewayResponseResultSchema("plugin/list").safeParse({ plugins: [] }).success)
      .toBe(false);
    expect(gatewayResponseResultSchema("plugin/list").safeParse([]).success).toBe(false);
  });

  it("validates method-specific Codex marketplace mutation results", () => {
    expect(gatewayResponseResultSchema("plugin/catalog/add").safeParse({
      marketplaceName: "tools",
      installedRoot: "/tmp/codex/plugins/tools",
      alreadyAdded: false
    }).success).toBe(true);
    expect(gatewayResponseResultSchema("plugin/catalog/remove").safeParse({
      marketplaceName: "tools",
      installedRoot: null
    }).success).toBe(true);
    expect(gatewayResponseResultSchema("plugin/catalog/upgrade").safeParse({
      selectedMarketplaces: ["tools"],
      upgradedRoots: ["/tmp/codex/plugins/tools"],
      errors: [{ marketplaceName: "other", message: "not available" }]
    }).success).toBe(true);

    expect(gatewayResponseResultSchema("plugin/catalog/add").safeParse({
      marketplaceName: "tools"
    }).success).toBe(false);
    expect(gatewayResponseResultSchema("plugin/catalog/remove").safeParse({}).success).toBe(false);
    expect(gatewayResponseResultSchema("plugin/catalog/upgrade").safeParse({
      selectedMarketplaces: ["tools"],
      upgradedRoots: []
    }).success).toBe(false);
  });
});

describe("ThreadApplication hard-cut schemas", () => {
  it("projects an optional diagnostic reason on runtime capability facts", () => {
    expect(gatewaySchemas.RuntimeCapabilityView.required).toEqual([
      "enabled",
      "id",
      "stability"
    ]);
    expect(gatewaySchemas.RuntimeCapabilityView.properties.unavailableReason).toEqual({
      default: null,
      type: ["string", "null"]
    });
  });

  it("exports semantic history owners without the retired runtime owner", () => {
    expect(gatewaySchemas.ThreadHistoryOwnerView.enum).toEqual([
      "psychevo",
      "agent",
      "process"
    ]);
  });

  it("rejects producerless runtime state and child events", () => {
    expect(GatewayEventSchema.safeParse({
      type: "runtimeStateChanged",
      runtimeRef: "acp:codex",
      threadId: "thread-1",
      state: "ready",
      detail: null,
      processEpoch: 2,
      instanceEpoch: 1
    }).success).toBe(false);
    expect(GatewayEventSchema.safeParse({
      type: "runtimeChildChanged",
      runtimeRef: "acp:codex",
      parentThreadId: "thread-1",
      threadId: "thread-child",
      dedupKey: "child-1",
      status: "running",
      readOnly: true
    }).success).toBe(false);
  });
});

describe("ThreadSnapshotSchema", () => {
  it("parses the Gateway web snapshot shape", () => {
    const parsed = ThreadSnapshotSchema.parse({
      source: {
        kind: "web",
        rawId: "cwd:abc",
        lifetime: "persistent",
        rawIdentity: null,
        visibleName: "psychevo"
      },
      scope: {
        cwd: "/tmp/project",
        source: {
          kind: "web",
          rawId: "cwd:abc",
          lifetime: "persistent",
          rawIdentity: null,
          visibleName: "psychevo"
        }
      },
      thread: {
        id: "s1",
        backend: { kind: "native", sessionHandle: "s1" },
        sourceKey: "web:cwd:abc"
      },
      history: { owner: "psychevo", fidelity: "full", cursor: null, hint: null },
      entries: [
        {
          id: "message:1:user",
          threadId: "s1",
          turnId: "message:1",
          messageSeq: 1,
          role: "user",
          status: "completed",
          source: "runtime.message",
          blocks: [
            {
              id: "message:1:user:text",
              kind: "text",
              status: "completed",
              order: 0,
              source: "runtime.message",
              title: null,
              body: "hello",
              preview: "hello",
              detail: "hello",
              artifactIds: [],
              metadata: null,
              result: null,
              createdAtMs: 1,
              updatedAtMs: 1
            }
          ],
          metadata: null,
          usage: null,
          accounting: null,
          createdAtMs: 1,
          updatedAtMs: 1
        },
        {
          id: "message:2:assistant",
          threadId: "s1",
          turnId: "message:2",
          messageSeq: 2,
          role: "assistant",
          status: "completed",
          source: "runtime.message",
          blocks: [
            {
              id: "message:2:assistant:text",
              kind: "text",
              status: "completed",
              order: 0,
              source: "runtime.message",
              title: null,
              body: "hi",
              preview: "hi",
              detail: "hi",
              artifactIds: [],
              metadata: null,
              result: null,
              createdAtMs: 2,
              updatedAtMs: 2
            }
          ],
          metadata: null,
          usage: null,
          accounting: null,
          createdAtMs: 2,
          updatedAtMs: 2
        }
      ],
      activity: {
        activities: [{
          owner: "framework_turn",
          activityId: "turn-1",
          turnId: "turn-1",
          kind: "root",
          queuedTurns: 0
        }],
        running: true,
        activeTurnId: "turn-1",
        queuedTurns: 0
      },
      turnStartReceipts: [{ clientTurnId: "client-turn-1", turnId: "turn-1" }],
      pendingActions: []
    });

    expect(parsed.thread?.id).toBe("s1");
    expect(parsed.activity.activities).toEqual([{
      owner: "framework_turn",
      activityId: "turn-1",
      turnId: "turn-1",
      kind: "root",
      queuedTurns: 0
    }]);
    expect(parsed.history).toEqual({ owner: "psychevo", fidelity: "full", cursor: null, hint: null });
    expect(parsed.entries).toHaveLength(2);
    expect(parsed.turnStartReceipts).toEqual([
      { clientTurnId: "client-turn-1", turnId: "turn-1" }
    ]);
  });
});
