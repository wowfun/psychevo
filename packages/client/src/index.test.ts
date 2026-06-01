import { describe, expect, it } from "vitest";
import { scopeForWorkdir } from "./index";

describe("scopeForWorkdir", () => {
  it("creates a persistent web source scope", () => {
    expect(scopeForWorkdir("/tmp/project")).toEqual({
      workdir: "/tmp/project",
      source: {
        kind: "web",
        rawId: null,
        lifetime: "persistent",
        rawIdentity: null,
        visibleName: null
      }
    });
  });
});
