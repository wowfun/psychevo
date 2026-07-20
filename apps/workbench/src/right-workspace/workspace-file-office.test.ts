import { describe, expect, it } from "vitest";
import JSZip from "jszip";
import { sanitizeOfficePreview } from "./workspace-file-office";

describe("sanitizeOfficePreview", () => {
  it("removes external OOXML relationships, macros, scripts, and embedded OLE parts", async () => {
    const source = new JSZip();
    source.file("[Content_Types].xml", "<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"/>");
    source.file("word/document.xml", "<w:document xmlns:w=\"urn:w\"><w:body/></w:document>");
    source.file("word/_rels/document.xml.rels", [
      "<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">",
      "<Relationship Id=\"external\" TargetMode=\"Extern&#x61;l\" Target=\"https&#58;//preview-security.invalid/office-image\" Type=\"image\"/>",
      "<Relationship Id=\"internal\" Target=\"media/image.png\" Type=\"image\"/>",
      "</Relationships>"
    ].join(""));
    source.file("word/vbaProject.bin", new Uint8Array([1, 2, 3]));
    source.file("word/embeddings/oleObject1.bin", new Uint8Array([4, 5, 6]));
    source.file("word/activeX/activeX1.bin", new Uint8Array([7]));
    source.file("word/media/image.png", new Uint8Array([137, 80, 78, 71]));
    const input = await source.generateAsync({ type: "uint8array" });

    const sanitized = await sanitizeOfficePreview(input, "unsafe.docm", new AbortController().signal);
    const output = await JSZip.loadAsync(sanitized);
    const relationships = await output.file("word/_rels/document.xml.rels")?.async("text");

    expect(relationships).not.toContain("preview-security.invalid");
    expect(relationships).not.toContain("TargetMode=\"External\"");
    expect(relationships).toContain("media/image.png");
    expect(output.file("word/vbaProject.bin")).toBeNull();
    expect(output.file("word/embeddings/oleObject1.bin")).toBeNull();
    expect(output.file("word/activeX/activeX1.bin")).toBeNull();
    expect(output.file("word/media/image.png")).not.toBeNull();
  });

  it("removes prefixed external OOXML relationships without removing internal relationships", async () => {
    const source = new JSZip();
    source.file("[Content_Types].xml", "<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"/>");
    source.file("word/document.xml", "<w:document xmlns:w=\"urn:w\"><w:body/></w:document>");
    source.file("word/_rels/document.xml.rels", [
      "<r:Relationships xmlns:r=\"http://schemas.openxmlformats.org/package/2006/relationships\">",
      "<r:Relationship Id=\"external\" TargetMode=\"External\" Target=\"https://preview-security.invalid/prefixed\" Type=\"image\"/>",
      "<r:Relationship Id=\"internal\" Target=\"media/image.png\" Type=\"image\"/>",
      "</r:Relationships>"
    ].join(""));
    source.file("word/media/image.png", new Uint8Array([137, 80, 78, 71]));
    const input = await source.generateAsync({ type: "uint8array" });

    const sanitized = await sanitizeOfficePreview(input, "prefixed.docx", new AbortController().signal);
    const output = await JSZip.loadAsync(sanitized);
    const relationships = await output.file("word/_rels/document.xml.rels")?.async("text");

    expect(relationships).not.toContain("preview-security.invalid");
    expect(relationships).not.toContain("Id=\"external\"");
    expect(relationships).toContain("Id=\"internal\"");
    expect(relationships).toContain("media/image.png");
  });

  it("removes network-bearing links from OpenDocument XML", async () => {
    const source = new JSZip();
    source.file("mimetype", "application/vnd.oasis.opendocument.text");
    source.file("content.xml", [
      "<office:document-content xmlns:office=\"urn:office\" xmlns:xlink=\"http://www.w3.org/1999/xlink\">",
      "<office:body><office:text>",
      "<office:a xlink:href=\"https&#58;//preview-security.invalid/odt\">outside</office:a>",
      "<office:a xlink:href=\"Pictures/local.png\">inside</office:a>",
      "</office:text></office:body></office:document-content>"
    ].join(""));
    source.file("Pictures/local.png", new Uint8Array([1]));
    const input = await source.generateAsync({ type: "uint8array" });

    const sanitized = await sanitizeOfficePreview(input, "unsafe.odt", new AbortController().signal);
    const output = await JSZip.loadAsync(sanitized);
    const content = await output.file("content.xml")?.async("text");

    expect(content).not.toContain("preview-security.invalid");
    expect(content).toContain("Pictures/local.png");
  });

  it("writes fixed-size OpenDocument ZIP records that spreadsheet readers accept", async () => {
    const source = new JSZip();
    source.file("mimetype", "application/vnd.oasis.opendocument.spreadsheet", {
      compression: "STORE"
    });
    source.file("content.xml", [
      "<office:document-content xmlns:office=\"urn:oasis:names:tc:opendocument:xmlns:office:1.0\"",
      " xmlns:table=\"urn:oasis:names:tc:opendocument:xmlns:table:1.0\"",
      " xmlns:text=\"urn:oasis:names:tc:opendocument:xmlns:text:1.0\">",
      "<office:body><office:spreadsheet><table:table table:name=\"Fixture\">",
      "<table:table-row><table:table-cell office:value-type=\"string\">",
      "<text:p>ODS fixture visible</text:p>",
      "</table:table-cell></table:table-row></table:table></office:spreadsheet></office:body>",
      "</office:document-content>"
    ].join(""));
    const input = await source.generateAsync({ type: "uint8array", streamFiles: false });

    const sanitized = await sanitizeOfficePreview(input, "fixture.ods", new AbortController().signal);
    const output = await JSZip.loadAsync(sanitized);

    expect(await output.file("mimetype")?.async("text"))
      .toBe("application/vnd.oasis.opendocument.spreadsheet");
    expect(await output.file("content.xml")?.async("text")).toContain("ODS fixture visible");
    expect(localFileRecords(sanitized)).toEqual(expect.arrayContaining([
      expect.objectContaining({ compressionMethod: 0, dataDescriptor: false }),
      expect.objectContaining({ dataDescriptor: false })
    ]));
    expect(localFileRecords(sanitized).every((record) => !record.dataDescriptor)).toBe(true);
  });

  it("sanitizes RTF network targets without changing non-ASCII source bytes", async () => {
    const prefix = new TextEncoder().encode(String.raw`{\rtf1\ansi Caf`);
    const suffix = new TextEncoder().encode(
      String.raw` https://preview-security.invalid/rtf\par}`
    );
    const input = new Uint8Array(prefix.byteLength + 1 + suffix.byteLength);
    input.set(prefix);
    input[prefix.byteLength] = 0xe9;
    input.set(suffix, prefix.byteLength + 1);

    const sanitized = await sanitizeOfficePreview(input, "unsafe.rtf", new AbortController().signal);
    const expectedSuffix = new TextEncoder().encode(String.raw` \par}`);
    const expected = new Uint8Array(prefix.byteLength + 1 + expectedSuffix.byteLength);
    expected.set(prefix);
    expected[prefix.byteLength] = 0xe9;
    expected.set(expectedSuffix, prefix.byteLength + 1);

    expect(sanitized).toEqual(expected);
  });

  it("rejects sanitized RTF output larger than 32 MiB", async () => {
    const input = new Uint8Array(32 * 1024 * 1024 + 1).fill(0x61);

    await expect(
      sanitizeOfficePreview(input, "oversized.rtf", new AbortController().signal)
    ).rejects.toThrow("Sanitized Office preview exceeds the 32 MiB limit.");
  }, 10_000);
});

function localFileRecords(bytes: Uint8Array): Array<{
  compressionMethod: number;
  dataDescriptor: boolean;
}> {
  const records: Array<{ compressionMethod: number; dataDescriptor: boolean }> = [];
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  for (let offset = 0; offset + 30 <= bytes.byteLength; offset += 1) {
    if (view.getUint32(offset, true) !== 0x04034b50) {
      continue;
    }
    const flags = view.getUint16(offset + 6, true);
    records.push({
      compressionMethod: view.getUint16(offset + 8, true),
      dataDescriptor: (flags & 0x08) !== 0
    });
  }
  return records;
}
