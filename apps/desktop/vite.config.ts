import react from "@vitejs/plugin-react";
import { configDefaults, defineConfig } from "vitest/config";

const CHUNK_SIZE_WARNING_LIMIT_KB = 700;

function packageSegment(packageName: string): string {
  return packageName.replace("/", "+");
}

function includesNodePackage(id: string, packageName: string): boolean {
  return (
    id.includes(`/node_modules/${packageName}/`) ||
    id.includes(`/node_modules/.pnpm/${packageSegment(packageName)}@`)
  );
}

function includesNodePackagePrefix(id: string, prefix: string): boolean {
  return (
    id.includes(`/node_modules/${prefix}`) ||
    id.includes(`/node_modules/.pnpm/${packageSegment(prefix)}`)
  );
}

function normalizedModuleId(id: string): string {
  return id.replace(/\\/g, "/");
}

function protocolSchemaChunkName(id: string): string | null {
  const prefix = "/packages/protocol/src/generated/schemas/";
  const schemaPath = id.split(prefix)[1]?.split(/[?#]/)[0];
  if (!schemaPath?.endsWith(".ts")) {
    return null;
  }
  return `protocol-schema-${schemaPath
    .replace(/\.ts$/, "")
    .replace(/[^A-Za-z0-9_-]/g, "-")}`;
}

function isMarkdownVendor(id: string): boolean {
  const normalized = normalizedModuleId(id);
  return (
    includesNodePackage(normalized, "react-markdown") ||
    includesNodePackagePrefix(normalized, "remark-") ||
    includesNodePackage(normalized, "unified") ||
    includesNodePackagePrefix(normalized, "micromark") ||
    includesNodePackagePrefix(normalized, "mdast-") ||
    includesNodePackagePrefix(normalized, "hast-") ||
    includesNodePackagePrefix(normalized, "unist-") ||
    includesNodePackagePrefix(normalized, "vfile") ||
    includesNodePackage(normalized, "markdown-table") ||
    includesNodePackage(normalized, "property-information")
  );
}

function isValidationVendor(id: string): boolean {
  const normalized = normalizedModuleId(id);
  return (
    includesNodePackage(normalized, "ajv") ||
    includesNodePackage(normalized, "fast-uri") ||
    includesNodePackage(normalized, "json-schema-traverse")
  );
}

function isMermaidParserVendor(id: string): boolean {
  return includesNodePackage(normalizedModuleId(id), "@mermaid-js/parser");
}

function isMermaidLayoutVendor(id: string): boolean {
  const normalized = normalizedModuleId(id);
  return (
    includesNodePackage(normalized, "cytoscape-cose-bilkent") ||
    includesNodePackage(normalized, "cytoscape-fcose") ||
    includesNodePackage(normalized, "cose-base") ||
    includesNodePackage(normalized, "layout-base")
  );
}

function isMermaidCytoscapeVendor(id: string): boolean {
  return includesNodePackage(normalizedModuleId(id), "cytoscape");
}

function isMermaidMathVendor(id: string): boolean {
  return includesNodePackage(normalizedModuleId(id), "katex");
}

function isMermaidRendererVendor(id: string): boolean {
  const normalized = normalizedModuleId(id);
  return (
    includesNodePackage(normalized, "@iconify/utils") ||
    includesNodePackage(normalized, "@upsetjs/venn.js") ||
    includesNodePackage(normalized, "dagre-d3-es") ||
    includesNodePackagePrefix(normalized, "d3-") ||
    includesNodePackage(normalized, "dayjs") ||
    includesNodePackage(normalized, "dompurify") ||
    includesNodePackage(normalized, "es-toolkit") ||
    includesNodePackage(normalized, "khroma") ||
    includesNodePackage(normalized, "lodash-es") ||
    includesNodePackage(normalized, "marked") ||
    includesNodePackage(normalized, "roughjs") ||
    includesNodePackage(normalized, "stylis")
  );
}

function isMermaidPackageVendor(id: string): boolean {
  return includesNodePackage(normalizedModuleId(id), "mermaid");
}

export default defineConfig({
  clearScreen: false,
  plugins: [react()],
  build: {
    rolldownOptions: {
      output: {
        codeSplitting: {
          groups: [
            {
              name: "vendor-react",
              priority: 100,
              test: (id) => {
                const normalized = normalizedModuleId(id);
                return (
                  includesNodePackage(normalized, "react") ||
                  includesNodePackage(normalized, "react-dom") ||
                  includesNodePackage(normalized, "scheduler")
                );
              }
            },
            {
              name: "vendor-mermaid-parser",
              priority: 99,
              test: isMermaidParserVendor
            },
            {
              name: "vendor-mermaid-layout",
              priority: 98,
              test: isMermaidLayoutVendor
            },
            {
              name: "vendor-mermaid-cytoscape",
              priority: 97,
              test: isMermaidCytoscapeVendor
            },
            {
              name: "vendor-mermaid-math",
              priority: 96,
              test: isMermaidMathVendor
            },
            {
              name: "vendor-mermaid-renderer",
              priority: 94,
              test: isMermaidRendererVendor
            },
            {
              name: "vendor-validation",
              priority: 95,
              test: isValidationVendor
            },
            {
              name: "vendor-highlight",
              priority: 90,
              test: (id) => includesNodePackage(normalizedModuleId(id), "highlight.js")
            },
            {
              name: "vendor-markdown",
              priority: 85,
              test: isMarkdownVendor
            },
            {
              name: "vendor-icons",
              priority: 80,
              test: (id) => includesNodePackage(normalizedModuleId(id), "lucide-react")
            },
            {
              name: "vendor-terminal",
              priority: 75,
              test: (id) => includesNodePackagePrefix(normalizedModuleId(id), "@xterm/")
            },
            {
              name: "vendor",
              priority: 70,
              test: (id) => normalizedModuleId(id).includes("/node_modules/") && !isMermaidPackageVendor(id)
            },
            {
              name: (id) => protocolSchemaChunkName(normalizedModuleId(id)),
              priority: 65
            },
            {
              name: "workbench-app",
              priority: 62,
              test: (id) => normalizedModuleId(id).includes("/apps/workbench/src/")
            },
            {
              name: "floating-app",
              priority: 61,
              test: (id) => normalizedModuleId(id).includes("/packages/floating/src/")
            },
            {
              name: "protocol-runtime",
              priority: 60,
              test: (id) => normalizedModuleId(id).includes("/packages/protocol/")
            },
            {
              name: "ui-components",
              priority: 55,
              test: (id) => normalizedModuleId(id).includes("/packages/components/")
            },
            {
              name: "client-runtime",
              priority: 50,
              test: (id) => normalizedModuleId(id).includes("/packages/client/")
            },
            {
              name: "host-runtime",
              priority: 45,
              test: (id) => normalizedModuleId(id).includes("/packages/host/")
            },
            {
              name: "assets",
              priority: 40,
              test: (id) => normalizedModuleId(id).includes("/packages/assets/")
            }
          ]
        }
      }
    },
    sourcemap: true,
    chunkSizeWarningLimit: CHUNK_SIZE_WARNING_LIMIT_KB
  },
  server: {
    host: "127.0.0.1",
    port: 5175,
    strictPort: true
  },
  test: {
    exclude: [...configDefaults.exclude, "src-tauri/**", "wdio/**"]
  }
});
