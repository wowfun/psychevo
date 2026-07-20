import { describe, expect, it } from "vitest";
import { parseDelimitedText } from "./workspace-file-delimited";

describe("parseDelimitedText", () => {
  it("preserves quoted fields and CRLF for an untruncated table", () => {
    expect(parseDelimitedText(
      "name,score\r\n\"Ada, Lovelace\",42\r\n",
      ","
    )).toMatchObject({
      rows: [["name", "score"], ["Ada, Lovelace", "42"]],
      truncated: false
    });
  });

  it("stops after the configured row limit", () => {
    expect(parseDelimitedText(
      "h1,h2\n1,2\n3,4\n",
      ",",
      { maxCells: 100, maxColumns: 10, maxRows: 2 }
    )).toMatchObject({
      rows: [["h1", "h2"], ["1", "2"]],
      truncated: true
    });
  });

  it("caps columns and total cells without parsing the remainder", () => {
    expect(parseDelimitedText(
      "h1,h2,h3\n1,2,3\n",
      ",",
      { maxCells: 100, maxColumns: 2, maxRows: 10 }
    )).toMatchObject({
      rows: [["h1", "h2"], ["1", "2"]],
      truncated: true
    });
    expect(parseDelimitedText(
      "h1,h2\n1,2\n3,4\n",
      ",",
      { maxCells: 3, maxColumns: 10, maxRows: 10 }
    )).toMatchObject({
      rows: [["h1", "h2"], ["1"]],
      truncated: true
    });
  });
});
