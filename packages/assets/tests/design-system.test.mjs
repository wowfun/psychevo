import { describe, expect, it } from "vitest";
import {
  checkGeneratedOutputsFromSource,
  generateOutputsFromSource,
  parseDesignSource,
  resolveTokenReferences
} from "../scripts/design-system.mjs";

const SAMPLE = `---
name: Sample
colors:
  ink: "#111111"
  paper: "#ffffff"
typography:
  body-md:
    fontFamily: Inter
    fontSize: 16px
    fontWeight: 400
    lineHeight: 1.5
    letterSpacing: 0em
rounded:
  sm: 4px
spacing:
  sm: 8px
components:
  button-primary:
    backgroundColor: "{colors.ink}"
    textColor: "{colors.paper}"
    typography: "{typography.body-md}"
    rounded: "{rounded.sm}"
    padding: "{spacing.sm}"
themes:
  dark:
    colorScheme: dark
    selector: ":root"
    fontFamily: Inter, sans-serif
    cssVariables:
      bg: "#111111"
      ink: "#ffffff"
      font-size-base: 16px
  light:
    colorScheme: light
    selector: 'html[data-pevo-appearance="light"]'
    cssVariables:
      bg: "#ffffff"
      ink: "#111111"
motion:
  feedback: 120ms
glyphs:
  prompt: ">"
platforms:
  web:
    cssVariablePrefix: "--pevo-"
    appearances:
      - dark
      - light
  embeddedTerminal:
    appearances:
      dark:
        background: "#111111"
        foreground: "#ffffff"
---

## Overview

Sample design.
`;

describe("DESIGN.md asset generator", () => {
  it("parses front matter and preserves markdown body", () => {
    const parsed = parseDesignSource(SAMPLE);
    expect(parsed.frontmatter.name).toBe("Sample");
    expect(parsed.body).toContain("Sample design.");
  });

  it("resolves scalar and composite token references", () => {
    const { frontmatter } = parseDesignSource(SAMPLE);
    const resolved = resolveTokenReferences(frontmatter.components, frontmatter);
    expect(resolved["button-primary"].backgroundColor).toBe("#111111");
    expect(resolved["button-primary"].typography.fontFamily).toBe("Inter");
    expect(resolved["button-primary"].rounded).toBe("4px");
  });

  it("keeps Psychevo extensions in the generated TypeScript model", () => {
    const outputs = generateOutputsFromSource(SAMPLE);
    expect(outputs.ts).toContain("\"motion\"");
    expect(outputs.ts).toContain("\"glyphs\"");
    expect(outputs.ts).toContain("\"embeddedTerminal\"");
    expect(outputs.css).toContain("--pevo-bg: #111111;");
  });

  it("reports generated output drift", () => {
    const outputs = generateOutputsFromSource(SAMPLE);
    const result = checkGeneratedOutputsFromSource(SAMPLE, {
      css: outputs.css.replace("--pevo-bg: #111111;", "--pevo-bg: #000000;"),
      ts: outputs.ts
    });
    expect(result.mismatches).toEqual(["theme.css"]);
  });
});
