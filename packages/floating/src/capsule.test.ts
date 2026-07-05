import { describe, expect, it } from "vitest";
import {
  actionPrompt,
  attachmentInputPart,
  attachmentInputParts,
  capsuleReducer,
  floatingScope,
  initialCapsuleState,
  placeCapsule,
  type FloatingAttachment
} from "./capsule";

describe("capsuleReducer", () => {
  it("fresh show resets previous thread state", () => {
    const current = {
      ...initialCapsuleState,
      activationId: "old",
      draft: "old draft",
      mode: "expanded" as const,
      threadId: "thread-old"
    };

    const next = capsuleReducer(current, {
      activationId: "new",
      anchor: null,
      draft: "new draft",
      type: "freshShow"
    });

    expect(next).toMatchObject({
      activationId: "new",
      draft: "new draft",
      mode: "toolbar",
      threadId: null
    });
  });

  it("restore keeps the active capsule thread and attachments", () => {
    const state = capsuleReducer({
      ...initialCapsuleState,
      activationId: "a1",
      attachments: [textAttachment()],
      mode: "parked",
      threadId: "thread-1"
    }, { type: "restore" });

    expect(state.mode).toBe("expanded");
    expect(state.threadId).toBe("thread-1");
    expect(state.attachments).toHaveLength(1);
  });

  it("accepts a mini-chat follow-up into the same thread", () => {
    const submitted = capsuleReducer({
      ...initialCapsuleState,
      activationId: "a1",
      mode: "expanded",
      threadId: "thread-1"
    }, { type: "submit", action: "ask", prompt: "follow up" });
    const accepted = capsuleReducer(submitted, { type: "accepted", threadId: "thread-1" });

    expect(accepted.threadId).toBe("thread-1");
    expect(accepted.mode).toBe("running");
    expect(accepted.pendingAction).toBe("ask");
  });

  it("keeps parked capsules parked and marks unseen completion", () => {
    const parked = {
      ...initialCapsuleState,
      activationId: "a1",
      mode: "parked" as const,
      running: true,
      threadId: "thread-1"
    };

    const completed = capsuleReducer(parked, { type: "completed" });

    expect(completed.mode).toBe("parked");
    expect(completed.running).toBe(false);
    expect(completed.unseenCompletion).toBe(true);
    expect(capsuleReducer(completed, { type: "restore" }).unseenCompletion).toBe(false);
  });
});

describe("actionPrompt", () => {
  it("compiles default actions into visible prompt text", () => {
    expect(actionPrompt("ask", "What is this?")).toBe("What is this?");
    expect(actionPrompt("explain", "")).toContain("Explain");
    expect(actionPrompt("translate", "", "Chinese")).toContain("Chinese");
    expect(actionPrompt("rewrite", "")).toContain("Rewrite");
  });
});

describe("attachmentInputPart", () => {
  it("maps selected text to context input", () => {
    expect(attachmentInputPart(textAttachment())).toEqual({
      label: "Selection: highlighted text (Editor)",
      text: "selected text",
      type: "context",
      visibleToModel: true
    });
  });

  it("maps visible images to image input", () => {
    expect(attachmentInputPart({
      dataUrl: "data:image/png;base64,AA==",
      id: "img",
      kind: "image",
      name: "screen.png",
      preview: "screen",
      visibleToModel: true
    })).toEqual({
      input: { kind: "url", url: "data:image/png;base64,AA==" },
      type: "image"
    });
  });

  it("keeps hidden attachments out of model input", () => {
    expect(attachmentInputParts([{ ...textAttachment(), visibleToModel: false }])).toEqual([]);
  });

  it("maps binary files to metadata-only context", () => {
    const input = attachmentInputPart({
      id: "file",
      kind: "file",
      mime: "application/octet-stream",
      name: "archive.bin",
      preview: "archive.bin",
      sizeLabel: "4 KiB",
      visibleToModel: true
    });

    expect(input).toMatchObject({
      label: "Attachment: archive.bin",
      type: "context",
      visibleToModel: true
    });
    expect(input.type === "context" ? input.text : "").toContain("Binary content");
  });
});

describe("floating source and geometry", () => {
  it("uses per-activation process source scope", () => {
    expect(floatingScope("/repo", "capsule-1")).toEqual({
      cwd: "/repo",
      source: {
        kind: "floating",
        rawId: "capsule-1",
        lifetime: "process",
        rawIdentity: null,
        visibleName: "Floating"
      }
    });
  });

  it("places the capsule near selection and clamps to viewport", () => {
    expect(placeCapsule(
      { x: 780, y: 10, width: 80, height: 18 },
      { width: 820, height: 500 },
      { width: 360, height: 56 }
    )).toEqual({ x: 448, y: 36, width: 360, height: 56 });
  });
});

function textAttachment(): FloatingAttachment {
  return {
    id: "text",
    kind: "textSelection",
    name: "highlighted text",
    preview: "selected text",
    sourceApp: "Editor",
    text: "selected text",
    visibleToModel: true
  };
}
