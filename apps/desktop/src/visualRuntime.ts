import { GatewayClient, type GatewayRawMessageHandler, type GatewayTransport } from "@psychevo/client";
import type { FloatingActivation, FloatingRuntime } from "@psychevo/floating";
import type { HostRect } from "@psychevo/host";
import type { ThreadContextReadResult, TranscriptEntry } from "@psychevo/protocol";
import { createDesktopFloatingWindowControls } from "./windowControls";

export function createVisualFloatingRuntime(): FloatingRuntime {
  return {
    ...createDesktopFloatingWindowControls(),
    async beginRegionPicker() {
      return {
        capability: "floating.beginRegionPicker",
        message: "Linux Wayland portal region capture is unavailable in deterministic visual mode.",
        ok: false,
        reason: "unavailable"
      };
    },
    async captureRegion(_bounds: HostRect) {
      return {
        capability: "floating.captureRegion",
        message: "Region screenshot capture is unavailable in deterministic visual mode.",
        ok: false,
        reason: "unavailable"
      };
    },
    async captureSelection() {
      return visualActivation("rescan");
    },
    async connectGateway() {
      const client = new GatewayClient(new VisualGatewayTransport());
      await client.connect();
      return client;
    },
    async initialActivation() {
      return visualActivation("initial");
    },
    async openThreadInWorkbench(_threadId) {
      return undefined;
    },
    async turnControls() {
      const context = visualThreadContext();
      return {
        context,
        controls: {
          targetId: context.selectedTargetId ?? "target:visual-native",
          turnOverrides: {
            mode: "default",
            model: "visual/model",
            permissionMode: "default"
          }
        }
      };
    },
    locale: "English"
  };
}

function visualThreadContext(): ThreadContextReadResult {
  return {
    selectedTargetId: "target:visual-native",
    suggestedTargetId: null,
    runtimeProfileRef: "native",
    selectionState: "prospective",
    profiles: [],
    binding: null,
    controls: [],
    stability: "stable",
    capabilities: [],
    compatibleTargets: [{
      targetId: "target:visual-native",
      agentRef: null,
      runtimeProfileRef: "native",
      agentLabel: "Default Agent",
      profileLabel: "Psychevo (Native)",
      label: "Default Agent · Psychevo (Native)",
      ready: true,
      unavailableReason: null
    }],
    inputCapabilities: ["text", "image", "embeddedContext"].map((kind) => ({
      kind,
      enabled: true,
      unavailableReason: null
    })),
    actions: [],
    sendability: { allowed: true, reason: null, recoveryAction: null },
    history: { owner: "psychevo", fidelity: "unavailable", cursor: null, hint: null },
    pendingInteractions: [],
    contextRevision: "visual-context",
    controlRevision: "visual-controls"
  };
}

function visualActivation(label: string): FloatingActivation {
  return {
    activationId: `visual-${label}`,
    anchor: { x: 220, y: 42, width: 320, height: 28 },
    attachments: [
      {
        bounds: { x: 220, y: 42, width: 320, height: 28 },
        id: `selection:${label}`,
        kind: "textSelection",
        name: "Selected text",
        preview: "The floating capsule keeps the current work locus visible.",
        sourceApp: "Visual Fixture",
        text: "The floating capsule keeps the current work locus visible.",
        visibleToModel: true
      }
    ],
    cwd: "/visual/workspace"
  };
}

class VisualGatewayTransport implements GatewayTransport {
  private connected = false;
  private readonly disconnectHandlers = new Set<(message: string) => void>();
  private readonly messageHandlers = new Set<GatewayRawMessageHandler>();
  private threadId = "thread-visual-floating";
  private turnId = 0;

  async connect(): Promise<void> {
    this.connected = true;
  }

  close(): void {
    this.connected = false;
  }

  onDisconnect(handler: (message: string) => void): () => void {
    this.disconnectHandlers.add(handler);
    return () => this.disconnectHandlers.delete(handler);
  }

  onMessage(handler: GatewayRawMessageHandler): () => void {
    this.messageHandlers.add(handler);
    return () => this.messageHandlers.delete(handler);
  }

  send(data: string): void {
    if (!this.connected) {
      throw new Error("Visual Gateway transport is not connected");
    }
    const message = JSON.parse(data) as {
      id: string;
      method: string;
      params?: Record<string, unknown>;
    };
    if (message.method === "thread/draft/open") {
      const origin = message.params?.origin;
      this.respond(message.id, {
        snapshot: {
          activity: { activeTurnId: null, queuedTurns: 0, running: false },
          entries: [],
          pendingActions: [],
          scope: origin,
          source: (origin as { source?: unknown } | undefined)?.source ?? null,
          thread: null
        },
        context: visualThreadContext(),
        problem: null
      });
      return;
    }
    if (message.method === "turn/start") {
      const requestedThreadId = typeof message.params?.threadId === "string" ? message.params.threadId : this.threadId;
      this.threadId = requestedThreadId;
      const turnId = `turn-visual-${++this.turnId}`;
      const liveAnswer = "The selected text describes the capsule's job: keep context visible";
      const finalAnswer = "The selected text describes the capsule's job: keep context visible, stay compact, and avoid stealing focus.";
      const completedAtMs = Date.now();
      window.setTimeout(() => {
        this.emit({
          jsonrpc: "2.0",
          method: "gateway/event",
          params: {
            selectedSkills: [],
            threadId: null,
            turnId,
            type: "turnStarted"
          }
        });
        this.emit({
          jsonrpc: "2.0",
          method: "gateway/event",
          params: {
            entry: transcriptTextEntry({
              body: liveAnswer,
              id: `assistant:${turnId}`,
              role: "assistant",
              status: "running",
              threadId: requestedThreadId,
              turnId,
              updatedAtMs: completedAtMs - 900
            }),
            turnId,
            type: "entryUpdated"
          }
        });
      }, 0);
      this.respond(message.id, {
        accepted: true,
        threadId: requestedThreadId,
        turnId,
        thread: {
          backend: { kind: "native", sessionHandle: requestedThreadId, runtimeRef: "native" },
          id: requestedThreadId,
          sourceKey: null
        }
      });
      window.setTimeout(() => {
        this.emit({
          jsonrpc: "2.0",
          method: "gateway/event",
          params: {
            type: "turnCompleted",
            threadId: requestedThreadId,
            turnId,
            committedEntries: [transcriptTextEntry({
              body: finalAnswer,
              id: `assistant:${turnId}`,
              role: "assistant",
              threadId: requestedThreadId,
              turnId,
              updatedAtMs: completedAtMs
            })],
            turn: {
              completedAtMs,
              error: null,
              id: turnId,
              outcome: "completed",
              startedAtMs: completedAtMs - 1_200,
              status: "completed",
              threadId: requestedThreadId
            }
          }
        });
      }, 1_200);
      return;
    }
    if (message.method === "thread/action/run") {
      this.respond(message.id, {
        kind: "interrupt",
        threadId: "visual-thread",
        interrupted: true,
        cleared: 0
      });
      return;
    }
    this.respond(message.id, {});
  }

  private respond(id: string, result: unknown): void {
    window.setTimeout(() => {
      this.emit({ id, jsonrpc: "2.0", result });
    }, 20);
  }

  private emit(message: unknown): void {
    const raw = JSON.stringify(message);
    for (const handler of this.messageHandlers) {
      handler(raw);
    }
  }
}

function transcriptTextEntry({
  body,
  id,
  role,
  status = "completed",
  threadId,
  turnId,
  updatedAtMs
}: {
  body: string;
  id: string;
  role: "assistant" | "user";
  status?: TranscriptEntry["status"];
  threadId: string;
  turnId: string;
  updatedAtMs: number;
}): TranscriptEntry {
  return {
    accounting: null,
    blocks: [{
      artifactIds: [],
      body,
      createdAtMs: updatedAtMs,
      detail: body,
      id: `${id}:text`,
      kind: "text",
      metadata: null,
      order: 0,
      preview: body.slice(0, 240),
      result: null,
      source: "visual",
      status,
      title: null,
      updatedAtMs
    }],
    createdAtMs: updatedAtMs,
    id,
    messageSeq: null,
    metadata: null,
    role,
    source: "visual",
    status,
    threadId,
    turnId,
    updatedAtMs,
    usage: null
  };
}
