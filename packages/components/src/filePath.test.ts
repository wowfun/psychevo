import { describe, expect, it } from "vitest";
import {
  decodeFilePath,
  encodeFilePath,
  normalizeFilePathInput,
  stripQueryAndHash,
  unquoteGitPath
} from "./filePath";

describe("file path helpers", () => {
  it("encodes Windows paths as valid file URL paths", () => {
    expect(encodeFilePath("D:\\dev\\project\\README.md")).toBe("/D:/dev/project/README.md");
    expect(encodeFilePath("D:\\dev\\project/README.md")).toBe("/D:/dev/project/README.md");
    expect(encodeFilePath("c:\\users\\test\\file.txt")).toBe("/c:/users/test/file.txt");
    expect(encodeFilePath("C:\\Users\\test\\")).toBe("/C:/Users/test/");
    expect(new URL(`file://${encodeFilePath("D:\\logs\\app.log")}`).protocol).toBe("file:");
  });

  it("encodes special characters and unicode path segments", () => {
    expect(encodeFilePath("/path/to/file#name.txt")).toBe("/path/to/file%23name.txt");
    expect(encodeFilePath("/path/to/file?name.txt")).toBe("/path/to/file%3Fname.txt");
    expect(encodeFilePath("/path/to/file%name.txt")).toBe("/path/to/file%25name.txt");
    expect(encodeFilePath("C:\\Program Files\\file with spaces.txt")).toBe(
      "/C:/Program%20Files/file%20with%20spaces.txt"
    );
    expect(encodeFilePath("/home/user/文档/README.md")).toContain("%E6%96%87%E6%A1%A3");
  });

  it("normalizes file URL and absolute path inputs against a Windows root", () => {
    expect(normalizeFilePathInput("C:\\repo\\src\\app.ts", "C:\\repo")).toBe("src\\app.ts");
    expect(normalizeFilePathInput("file:///C:/repo/src/app.ts", "C:\\repo")).toBe("src/app.ts");
    expect(normalizeFilePathInput("file://C:/repo/src/app.ts", "C:\\repo")).toBe("src/app.ts");
    expect(normalizeFilePathInput("c:\\repo\\src\\app.ts", "C:\\repo")).toBe("src\\app.ts");
    expect(normalizeFilePathInput("C:\\repo\\src\\", "C:\\repo")).toBe("src\\");
    expect(normalizeFilePathInput("./src/app.ts", "C:\\repo")).toBe("src/app.ts");
  });

  it("strips query and hash before decoding file path input", () => {
    expect(stripQueryAndHash("a/b.ts#L12?x=1")).toBe("a/b.ts");
    expect(stripQueryAndHash("a/b.ts?x=1#L12")).toBe("a/b.ts");
    expect(decodeFilePath("a%20b.txt")).toBe("a b.txt");
  });

  it("preserves literal query and hash characters in ordinary filesystem paths", () => {
    expect(normalizeFilePathInput("src/a#b.ts", "/repo")).toBe("src/a#b.ts");
    expect(normalizeFilePathInput("src/a?b.ts", "/repo")).toBe("src/a?b.ts");
    expect(normalizeFilePathInput("/repo/src/a#b.ts", "/repo")).toBe("src/a#b.ts");
    expect(normalizeFilePathInput("/repo/src/a?b.ts", "/repo")).toBe("src/a?b.ts");
  });

  it("strips query and hash from file URL inputs before decoding", () => {
    expect(normalizeFilePathInput("file:///repo/src/a%23b.ts#L12", "/repo")).toBe(
      "src/a#b.ts"
    );
    expect(normalizeFilePathInput("file:///repo/src/a%3Fb.ts?raw=1", "/repo")).toBe(
      "src/a?b.ts"
    );
  });

  it("unquotes Git octal path strings", () => {
    expect(unquoteGitPath("\"a/\\303\\251.txt\"")).toBe("a/\u00e9.txt");
    expect(unquoteGitPath("\"plain\\nname\"")).toBe("plain\nname");
    expect(unquoteGitPath("src/app.ts")).toBe("src/app.ts");
  });
});
