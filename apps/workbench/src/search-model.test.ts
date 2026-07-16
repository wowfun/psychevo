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
});
