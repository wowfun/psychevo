import { describe, expect, it, vi } from "vitest";
import { saveDesktopDownload } from "./runtime";
import type { DesktopDownloadSessionResult } from "./bridge";

describe("saveDesktopDownload", () => {
  it("creates a Blob download from native content without using token-like fields", async () => {
    const anchor = {
      download: "",
      href: "",
      style: { display: "" },
      click: vi.fn(),
      remove: vi.fn()
    };
    const document = {
      body: {
        append: vi.fn()
      },
      createElement: vi.fn(() => anchor)
    };
    let capturedBlob: Blob | null = null;
    const url = {
      createObjectURL: vi.fn((blob: Blob) => {
        capturedBlob = blob;
        return "blob:desktop-download";
      }),
      revokeObjectURL: vi.fn()
    };
    const result = {
      content: [35, 32, 83, 104, 97, 114, 101],
      contentType: "text/markdown",
      filename: "session.md",
      token: "secret-token"
    } as DesktopDownloadSessionResult & { token: string };

    saveDesktopDownload(result, { BlobCtor: Blob, document, url });

    expect(anchor.href).toBe("blob:desktop-download");
    expect(anchor.download).toBe("session.md");
    expect(anchor.href).not.toContain("secret-token");
    expect(anchor.download).not.toContain("secret-token");
    expect(anchor.style.display).toBe("none");
    expect(document.body.append).toHaveBeenCalledWith(anchor);
    expect(anchor.click).toHaveBeenCalled();
    expect(anchor.remove).toHaveBeenCalled();
    expect(url.revokeObjectURL).toHaveBeenCalledWith("blob:desktop-download");
    const downloadedBlob = capturedBlob as Blob | null;
    expect(downloadedBlob).toBeInstanceOf(Blob);
    expect(downloadedBlob!.type).toBe("text/markdown");
    expect(await downloadedBlob!.text()).toBe("# Share");
  });
});
