import type { GatewayInputPart, GatewayRequestScope } from "@psychevo/protocol";

export type CapsuleMode = "hidden" | "toolbar" | "submitting" | "expanded" | "running" | "parked" | "error";
export type CapsuleAction = "ask" | "explain" | "translate" | "rewrite";

export interface Rect {
  height: number;
  width: number;
  x: number;
  y: number;
}

export interface FloatingViewport {
  height: number;
  width: number;
}

export interface FloatingAttachmentBase {
  id: string;
  kind: "textSelection" | "image" | "file";
  name: string;
  preview: string;
  sizeLabel?: string;
  sourceApp?: string | null;
  visibleToModel: boolean;
}

export interface TextSelectionAttachment extends FloatingAttachmentBase {
  bounds?: Rect | null;
  kind: "textSelection";
  text: string;
}

export interface ImageAttachment extends FloatingAttachmentBase {
  dataUrl: string;
  kind: "image";
}

export interface FileAttachment extends FloatingAttachmentBase {
  dataUrl?: string | null;
  kind: "file";
  mime?: string | null;
  text?: string | null;
}

export type FloatingAttachment = TextSelectionAttachment | ImageAttachment | FileAttachment;

export interface CapsuleState {
  activationId: string | null;
  anchor: Rect | null;
  attachments: FloatingAttachment[];
  draft: string;
  error: string | null;
  mode: CapsuleMode;
  pendingAction: CapsuleAction | null;
  running: boolean;
  threadId: string | null;
  unseenCompletion: boolean;
}

export type CapsuleEvent =
  | { type: "freshShow"; activationId: string; anchor: Rect | null; attachments?: FloatingAttachment[]; draft?: string }
  | { type: "restore" }
  | { type: "close" }
  | { type: "park" }
  | { type: "setDraft"; draft: string }
  | { type: "addAttachment"; attachment: FloatingAttachment }
  | { type: "removeAttachment"; id: string }
  | { type: "submit"; action: CapsuleAction; prompt: string }
  | { type: "accepted"; threadId: string }
  | { type: "completed" }
  | { type: "running"; running: boolean }
  | { type: "error"; message: string }
  | { type: "clearError" };

export const initialCapsuleState: CapsuleState = {
  activationId: null,
  anchor: null,
  attachments: [],
  draft: "",
  error: null,
  mode: "hidden",
  pendingAction: null,
  running: false,
  threadId: null,
  unseenCompletion: false
};

export function capsuleReducer(state: CapsuleState, event: CapsuleEvent): CapsuleState {
  switch (event.type) {
    case "freshShow":
      return {
        ...initialCapsuleState,
        activationId: event.activationId,
        anchor: event.anchor,
        attachments: event.attachments ?? [],
        draft: event.draft ?? "",
        mode: "toolbar"
      };
    case "restore":
      if (!state.activationId || state.mode === "hidden") {
        return state;
      }
      return { ...state, mode: state.threadId ? "expanded" : "toolbar", unseenCompletion: false };
    case "close":
      return { ...initialCapsuleState };
    case "park":
      return state.activationId ? { ...state, mode: "parked" } : state;
    case "setDraft":
      return { ...state, draft: event.draft };
    case "addAttachment":
      return { ...state, attachments: [...state.attachments, event.attachment] };
    case "removeAttachment":
      return { ...state, attachments: state.attachments.filter((attachment) => attachment.id !== event.id) };
    case "submit":
      return {
        ...state,
        draft: "",
        error: null,
        mode: "submitting",
        pendingAction: event.action,
        running: true,
        unseenCompletion: false
      };
    case "accepted":
      return { ...state, mode: "running", threadId: event.threadId };
    case "completed":
      return {
        ...state,
        mode: state.mode === "parked" ? "parked" : state.threadId ? "expanded" : state.mode,
        running: false,
        unseenCompletion: state.mode === "parked"
      };
    case "running":
      return {
        ...state,
        mode: state.mode === "parked" ? "parked" : event.running ? "running" : state.threadId ? "expanded" : state.mode,
        running: event.running
      };
    case "error":
      return { ...state, error: event.message, mode: "error", running: false };
    case "clearError":
      return { ...state, error: null, mode: state.threadId ? "expanded" : "toolbar" };
  }
}

export function floatingScope(cwd: string, activationId: string): GatewayRequestScope {
  return {
    cwd,
    source: {
      kind: "floating",
      rawId: activationId,
      lifetime: "process",
      rawIdentity: null,
      visibleName: "Floating"
    }
  };
}

export function actionPrompt(action: CapsuleAction, draft: string, locale = "system"): string {
  const trimmed = draft.trim();
  switch (action) {
    case "ask":
      return trimmed || "Answer the question using the attached context.";
    case "explain":
      return trimmed || "Explain the selected content clearly and concisely.";
    case "translate":
      return trimmed || `Translate the selected content into ${locale}.`;
    case "rewrite":
      return trimmed || "Rewrite the selected content while preserving its intent.";
  }
}

export function attachmentInputParts(attachments: FloatingAttachment[]): GatewayInputPart[] {
  return attachments
    .filter((attachment) => attachment.visibleToModel)
    .map(attachmentInputPart);
}

export function attachmentInputPart(attachment: FloatingAttachment): GatewayInputPart {
  if (attachment.kind === "image") {
    return {
      input: { kind: "url", url: attachment.dataUrl },
      type: "image"
    };
  }
  if (attachment.kind === "textSelection") {
    return {
      label: contextLabel("Selection", attachment.name, attachment.sourceApp),
      text: attachment.text,
      type: "context",
      visibleToModel: true
    };
  }
  if (attachment.dataUrl && attachment.mime?.startsWith("image/")) {
    return {
      input: { kind: "url", url: attachment.dataUrl },
      type: "image"
    };
  }
  return {
    label: contextLabel("Attachment", attachment.name, null),
    text: attachment.text?.trim()
      ? attachment.text
      : [
          `Attached file: ${attachment.name}`,
          attachment.mime ? `MIME: ${attachment.mime}` : null,
          attachment.sizeLabel ? `Size: ${attachment.sizeLabel}` : null,
          "Binary content is selected in Floating but is not embedded as model text."
        ].filter(Boolean).join("\n"),
    type: "context",
    visibleToModel: true
  };
}

export function placeCapsule(anchor: Rect | null, viewport: FloatingViewport, surface: { height: number; width: number }): Rect {
  const margin = 12;
  const preferredX = anchor ? anchor.x + anchor.width / 2 - surface.width / 2 : viewport.width / 2 - surface.width / 2;
  const preferredY = anchor ? anchor.y + anchor.height + 8 : 24;
  const x = clamp(preferredX, margin, viewport.width - surface.width - margin);
  const y = clamp(preferredY, margin, viewport.height - surface.height - margin);
  return { x, y, width: surface.width, height: surface.height };
}

function contextLabel(prefix: string, name: string, sourceApp?: string | null): string {
  return sourceApp ? `${prefix}: ${name} (${sourceApp})` : `${prefix}: ${name}`;
}

function clamp(value: number, min: number, max: number): number {
  return Math.min(Math.max(value, min), Math.max(min, max));
}
