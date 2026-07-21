import { posix, win32 } from "node:path";
import { describe, expect, it } from "vitest";
import { resolveAssetRequestPath } from "../../excalidraw-assets-vite-plugin";

describe("resolveAssetRequestPath", () => {
  it("accepts only strict descendants with POSIX path semantics", () => {
    expect(resolveAssetRequestPath("/repo/fonts", "Virgil.woff2", posix)).toBe("/repo/fonts/Virgil.woff2");
    expect(resolveAssetRequestPath("/repo/fonts", "nested/Cascadia.woff2", posix)).toBe("/repo/fonts/nested/Cascadia.woff2");
    expect(resolveAssetRequestPath("/repo/fonts", "", posix)).toBeNull();
    expect(resolveAssetRequestPath("/repo/fonts", "../secret.txt", posix)).toBeNull();
    expect(resolveAssetRequestPath("/repo/fonts", "/etc/passwd", posix)).toBeNull();
  });

  it("rejects Windows cross-drive, drive-relative, UNC, and parent escapes", () => {
    const fontsRoot = "C:\\repo\\node_modules\\@excalidraw\\excalidraw\\dist\\prod\\fonts";
    expect(resolveAssetRequestPath(fontsRoot, "Virgil.woff2", win32)).toBe(`${fontsRoot}\\Virgil.woff2`);
    expect(resolveAssetRequestPath(fontsRoot, "D:\\secret\\token.txt", win32)).toBeNull();
    expect(resolveAssetRequestPath(fontsRoot, "D:secret\\token.txt", win32)).toBeNull();
    expect(resolveAssetRequestPath(fontsRoot, "\\\\server\\share\\token.txt", win32)).toBeNull();
    expect(resolveAssetRequestPath(fontsRoot, "..\\secret.txt", win32)).toBeNull();
    expect(resolveAssetRequestPath(fontsRoot, "", win32)).toBeNull();
  });
});
