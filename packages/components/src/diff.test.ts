import { describe, expect, it } from "vitest";
import { diffDisplayPath, diffFilesStats, diffLineStats, parseStrictGitPatchDiff, parseUnifiedDiff } from "./diff";

describe("diff parser", () => {
  it("parses a strict single-file edit diff", () => {
    const files = parseStrictGitPatchDiff([
      "diff --git a/primes.py b/primes.py",
      "index 1111111..2222222 100644",
      "--- a/primes.py",
      "+++ b/primes.py",
      "@@ -1,3 +1,3 @@",
      " def is_prime(n):",
      "-    return False",
      "+    return n > 1",
      " "
    ].join("\n"));

    expect(files).toHaveLength(1);
    expect(files[0]?.path).toBe("primes.py");
    expect(diffLineStats(files[0]!)).toEqual({ additions: 1, deletions: 1 });
  });

  it("parses strict multi-file edit stats", () => {
    const files = parseStrictGitPatchDiff([
      "diff --git a/a.txt b/a.txt",
      "--- a/a.txt",
      "+++ b/a.txt",
      "@@ -1 +1 @@",
      "-old",
      "+new",
      "diff --git a/b.txt b/b.txt",
      "--- a/b.txt",
      "+++ b/b.txt",
      "@@ -1,0 +1,2 @@",
      "+first",
      "+second"
    ].join("\n"));

    expect(files.map((file) => file.path)).toEqual(["a.txt", "b.txt"]);
    expect(diffFilesStats(files)).toEqual({ additions: 3, deletions: 1 });
  });

  it("accepts strict rename-only Git patches", () => {
    const files = parseStrictGitPatchDiff([
      "diff --git a/old-name.md b/new-name.md",
      "similarity index 100%",
      "rename from old-name.md",
      "rename to new-name.md"
    ].join("\n"));

    expect(files).toHaveLength(1);
    expect(diffDisplayPath(files[0]!)).toBe("old-name.md -> new-name.md");
    expect(diffLineStats(files[0]!)).toEqual({ additions: 0, deletions: 0 });
  });

  it("rejects malformed strict patches while tolerant parsing still keeps preview text", () => {
    expect(parseStrictGitPatchDiff("not a git patch")).toEqual([]);
    expect(parseStrictGitPatchDiff("diff --git a/a.txt b/a.txt\nthis is not enough")).toEqual([]);

    const tolerant = parseUnifiedDiff("@@ -1 +1 @@\n-old\n+new");
    expect(tolerant).toHaveLength(1);
    expect(tolerant[0]?.path).toBe("Diff");
    expect(diffLineStats(tolerant[0]!)).toEqual({ additions: 1, deletions: 1 });
  });
});
