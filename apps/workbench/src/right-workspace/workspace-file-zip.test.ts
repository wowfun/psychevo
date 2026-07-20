import { describe, expect, it } from "vitest";
import JSZip from "jszip";
import { readZipDirectory } from "./workspace-file-zip";

describe("workspace ZIP preview", () => {
  it("shows hostile archive paths only as sanitized relative names", async () => {
    const archive = new JSZip();
    archive.file("safe/readme.txt", "safe");
    archive.file("../escape.txt", "escape");
    archive.file("/absolute.txt", "absolute");
    archive.file("C:\\escape.txt", "drive");
    archive.file("safe/../../escape2.txt", "escape2");
    const bytes = await archive.generateAsync({ type: "uint8array" });

    const entries = await readZipDirectory(bytes, new AbortController().signal);

    expect(entries.map((entry) => entry.path)).toEqual([
      "absolute.txt",
      "escape.txt",
      "escape.txt",
      "escape2.txt",
      "safe",
      "safe/readme.txt"
    ]);
  });
});
