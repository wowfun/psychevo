#!/usr/bin/env node
import { readFileSync, writeFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { parse as parseYaml } from "yaml";

const scriptDir = path.dirname(fileURLToPath(import.meta.url));
const packageRoot = path.resolve(scriptDir, "..");
const repoRoot = path.resolve(packageRoot, "../..");

export const DEFAULT_DESIGN_PATH = path.join(repoRoot, "specs/075-design-system/DESIGN.md");
export const DEFAULT_CSS_PATH = path.join(packageRoot, "theme.css");
export const DEFAULT_TS_PATH = path.join(packageRoot, "src/design-system.ts");

export function parseDesignSource(source) {
  const match = /^---\r?\n([\s\S]*?)\r?\n---\r?\n?([\s\S]*)$/.exec(source);
  if (!match) {
    throw new Error("DESIGN.md must start with YAML front matter.");
  }
  const [, yamlSource, body] = match;
  const frontmatter = parseYaml(yamlSource);
  if (!frontmatter || typeof frontmatter !== "object") {
    throw new Error("DESIGN.md front matter must be a YAML object.");
  }
  return { body, frontmatter };
}

export function resolveTokenReferences(value, root = value, stack = []) {
  if (typeof value === "string") {
    const ref = /^\{([^}]+)\}$/.exec(value.trim());
    if (!ref) {
      return value;
    }
    const refPath = ref[1];
    if (stack.includes(refPath)) {
      throw new Error(`Circular token reference: ${[...stack, refPath].join(" -> ")}`);
    }
    return resolveTokenReferences(readPath(root, refPath), root, [...stack, refPath]);
  }
  if (Array.isArray(value)) {
    return value.map((item) => resolveTokenReferences(item, root, stack));
  }
  if (value && typeof value === "object") {
    return Object.fromEntries(
      Object.entries(value).map(([key, child]) => [key, resolveTokenReferences(child, root, stack)])
    );
  }
  return value;
}

export function readPath(root, refPath) {
  const parts = refPath.split(".");
  let value = root;
  for (const part of parts) {
    if (!value || typeof value !== "object" || !(part in value)) {
      throw new Error(`Unknown token reference: {${refPath}}`);
    }
    value = value[part];
  }
  return value;
}

export function buildDesignSystem(frontmatter) {
  const resolved = resolveTokenReferences(frontmatter, frontmatter);
  return {
    version: resolved.version ?? "alpha",
    name: resolved.name,
    description: resolved.description ?? "",
    tokens: {
      colors: resolved.colors ?? {},
      typography: resolved.typography ?? {},
      rounded: resolved.rounded ?? {},
      spacing: resolved.spacing ?? {},
      components: resolved.components ?? {}
    },
    themes: resolved.themes ?? {},
    motion: resolved.motion ?? {},
    glyphs: resolved.glyphs ?? {},
    platforms: resolved.platforms ?? {}
  };
}

export function generateOutputsFromSource(source) {
  const { frontmatter } = parseDesignSource(source);
  const designSystem = buildDesignSystem(frontmatter);
  return {
    css: generateCss(frontmatter),
    ts: generateTs(designSystem)
  };
}

export function checkGeneratedOutputsFromSource(source, outputs) {
  const expected = generateOutputsFromSource(source);
  const mismatches = [];
  if (normalizeNewline(outputs.css) !== normalizeNewline(expected.css)) {
    mismatches.push("theme.css");
  }
  if (normalizeNewline(outputs.ts) !== normalizeNewline(expected.ts)) {
    mismatches.push("src/design-system.ts");
  }
  return { expected, mismatches };
}

export function generateCss(frontmatter) {
  const themes = frontmatter.themes;
  if (!themes || typeof themes !== "object") {
    throw new Error("DESIGN.md front matter must define themes.");
  }
  const order = orderedKeys(themes, ["dark", "light", "warm"]);
  const blocks = order.map((name) => formatThemeBlock(themes[name], name));
  const componentBlocks = ["control", "field"]
    .map((name) => formatComponentTokenBlock(name, frontmatter.components?.[name], frontmatter))
    .filter(Boolean);
  return `${generatedHeader("css")}
${componentBlocks.length > 0 ? `${componentBlocks.join("\n\n")}\n\n` : ""}${blocks.join("\n\n")}

* {
  box-sizing: border-box;
}

body {
  margin: 0;
  background: var(--pevo-bg);
  color: var(--pevo-ink);
  font-size: var(--pevo-font-size-base);
  line-height: 1.45;
}

button,
input,
select,
textarea {
  font: inherit;
}

button {
  touch-action: manipulation;
}
`;
}

function formatComponentTokenBlock(name, component, frontmatter) {
  if (!component || typeof component !== "object") {
    return "";
  }
  const resolved = resolveTokenReferences(component, frontmatter);
  const lines = [":root {"];
  for (const [key, value] of Object.entries(resolved)) {
    if (!["string", "number"].includes(typeof value)) {
      throw new Error(`${name} token ${key} must resolve to a string or number.`);
    }
    const cssKey = key.replace(/[A-Z]/g, (letter) => `-${letter.toLowerCase()}`);
    lines.push(`  --pevo-${name}-${cssKey}: ${String(value)};`);
  }
  lines.push("}");
  return lines.join("\n");
}

export function generateTs(designSystem) {
  const appearances = designSystem.platforms?.web?.appearances ?? Object.keys(designSystem.themes);
  return `${generatedHeader("ts")}
export const pevoAppearances = ${JSON.stringify(appearances, null, 2)} as const;
export type PevoAppearance = typeof pevoAppearances[number];

export const psychevoDesignSystem = ${JSON.stringify(designSystem, null, 2)} as const;
`;
}

function formatThemeBlock(theme, name) {
  if (!theme || typeof theme !== "object") {
    throw new Error(`Theme ${name} must be an object.`);
  }
  const selector = theme.selector ?? (name === "dark" ? ":root" : `html[data-pevo-appearance="${name}"]`);
  const variables = theme.cssVariables;
  if (!variables || typeof variables !== "object") {
    throw new Error(`Theme ${name} must define cssVariables.`);
  }
  const lines = [`${selector} {`, `  color-scheme: ${theme.colorScheme ?? "dark"};`];
  for (const [key, value] of Object.entries(variables)) {
    lines.push(`  --pevo-${key}: ${value};`);
  }
  if (theme.fontFamily) {
    lines.push(`  font-family:`);
    lines.push(`    ${theme.fontFamily};`);
    lines.push(`  font-variant-numeric: tabular-nums;`);
    lines.push(`  -webkit-font-smoothing: antialiased;`);
    lines.push(`  -moz-osx-font-smoothing: grayscale;`);
  }
  lines.push("}");
  return lines.join("\n");
}

function orderedKeys(record, preferred) {
  return [
    ...preferred.filter((key) => Object.prototype.hasOwnProperty.call(record, key)),
    ...Object.keys(record).filter((key) => !preferred.includes(key))
  ];
}

function generatedHeader(kind) {
  const comment = `Generated from specs/075-design-system/DESIGN.md. Do not edit directly.`;
  return kind === "css" ? `/* ${comment} */` : `// ${comment}`;
}

function normalizeNewline(value) {
  return value.replace(/\r\n/g, "\n").trimEnd();
}

function readCurrentFiles(paths) {
  return {
    css: readFileSync(paths.css, "utf8"),
    ts: readFileSync(paths.ts, "utf8")
  };
}

function generateCurrent(paths) {
  const source = readFileSync(paths.design, "utf8");
  return generateOutputsFromSource(source);
}

function writeCurrent(paths) {
  const outputs = generateCurrent(paths);
  writeFileSync(paths.css, outputs.css);
  writeFileSync(paths.ts, outputs.ts);
}

function checkCurrent(paths) {
  const source = readFileSync(paths.design, "utf8");
  const result = checkGeneratedOutputsFromSource(source, readCurrentFiles(paths));
  if (result.mismatches.length === 0) {
    return true;
  }
  for (const mismatch of result.mismatches) {
    console.error(`${mismatch} is out of date. Run pnpm --filter @psychevo/assets design:generate.`);
  }
  return false;
}

function defaultPaths() {
  return {
    css: DEFAULT_CSS_PATH,
    design: DEFAULT_DESIGN_PATH,
    ts: DEFAULT_TS_PATH
  };
}

function main(argv) {
  const command = argv[2] ?? "generate";
  const paths = defaultPaths();
  if (command === "generate") {
    writeCurrent(paths);
    return 0;
  }
  if (command === "check") {
    return checkCurrent(paths) ? 0 : 1;
  }
  console.error(`Unknown design-system command: ${command}`);
  return 1;
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  process.exitCode = main(process.argv);
}
