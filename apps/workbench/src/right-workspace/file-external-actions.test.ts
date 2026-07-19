import { describe, expect, it } from "vitest";
import type {
  WorkspaceExternalFileCategory,
  WorkspaceExternalHostPlatform
} from "@psychevo/protocol";
import {
  workspaceExternalActionLabel,
  workspaceExternalActionMenuItems
} from "./file-external-actions";

describe("workspace external file action labels", () => {
  it.each([
    ["webpage", "Open in Default Browser"],
    ["image", "Open in Default Image Viewer"],
    ["media", "Open in Default Player"],
    ["pdf", "Open in Default PDF Reader"],
    ["office", "Open in Default Office Application"],
    ["text", "Open with Default Application"],
    ["other", "Open with Default Application"]
  ] satisfies Array<[WorkspaceExternalFileCategory, string]>)(
    "uses the %s category for the system-default label",
    (category, label) => {
      expect(workspaceExternalActionLabel("systemDefault", category, "linux")).toBe(label);
    }
  );

  it.each([
    ["macos", "Show in Finder"],
    ["windows", "Show in File Explorer"],
    ["linux", "Show in File Manager"]
  ] satisfies Array<[WorkspaceExternalHostPlatform, string]>)(
    "uses the %s reveal label",
    (platform, label) => {
      expect(workspaceExternalActionLabel("reveal", "other", platform)).toBe(label);
    }
  );

  it("keeps the Gateway action order and separates reveal", () => {
    expect(workspaceExternalActionMenuItems({
      path: "site/index.html",
      category: "webpage",
      textLike: true,
      platform: "macos",
      preferredAction: "systemDefault",
      availableActions: ["systemDefault", "vscode", "reveal"]
    })).toEqual([
      { disabled: false, id: "systemDefault", label: "Open in Default Browser", separatorBefore: false },
      { disabled: false, id: "vscode", label: "Open in VS Code", separatorBefore: false },
      { disabled: false, id: "reveal", label: "Show in Finder", separatorBefore: true }
    ]);
  });
});
