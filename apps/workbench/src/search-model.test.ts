import type { ThreadSnapshot } from "@psychevo/protocol";
import { describe, expect, it } from "vitest";
import { transcriptMayContainWorkspaceFile } from "./search-model";

function assistantEntries(body: string): ThreadSnapshot["entries"] {
  return [{
    id: "assistant-1",
    role: "assistant",
    blocks: [{ kind: "text", body }]
  }] as unknown as ThreadSnapshot["entries"];
}

function completedToolEntry(toolName: string, path: string): ThreadSnapshot["entries"] {
  return [{
    id: `tool-${toolName}`,
    role: "assistant",
    blocks: [{
      kind: "file",
      status: "completed",
      metadata: {
        projection: "tool",
        tool_name: toolName,
        args: { path }
      },
      result: { status: "completed", isError: false }
    }]
  }] as unknown as ThreadSnapshot["entries"];
}

describe("transcript workspace-file demand", () => {
  it.each([
    "Generated README.md.",
    "Inspect `reports/result.html:42` for details.",
    "Open the [rendered output](reports/result.html#L8)."
  ])("loads the inventory for a supported Markdown file form: %s", (body) => {
    expect(transcriptMayContainWorkspaceFile(assistantEntries(body))).toBe(true);
  });

  it("does not load the inventory for prose without a file candidate", () => {
    expect(transcriptMayContainWorkspaceFile(assistantEntries("No generated files were referenced."))).toBe(false);
  });

  it.each(["read", "edit", "write"])(
    "loads the inventory for a completed %s path argument",
    (toolName) => {
      expect(transcriptMayContainWorkspaceFile(completedToolEntry(toolName, "reports/result.html"))).toBe(true);
    }
  );

  it("loads the inventory for an extensionless completed file-tool path", () => {
    expect(transcriptMayContainWorkspaceFile(completedToolEntry("read", "Dockerfile"))).toBe(true);
  });

  it.each([
    ["pending read", "read", "pending", false],
    ["failed write", "write", "failed", true],
    ["unrelated tool", "custom_publish", "completed", false]
  ])("does not load the inventory for a %s", (_label, toolName, status, isError) => {
    const entries = completedToolEntry(String(toolName), "reports/result.html");
    const block = entries[0]!.blocks[0]!;
    block.status = status as typeof block.status;
    block.result = {
      ...block.result!,
      status: status as NonNullable<typeof block.result>["status"],
      isError: Boolean(isError)
    };
    expect(transcriptMayContainWorkspaceFile(entries)).toBe(false);
  });
});
