import hljs from "highlight.js/lib/core";
import bash from "highlight.js/lib/languages/bash";
import css from "highlight.js/lib/languages/css";
import diff from "highlight.js/lib/languages/diff";
import go from "highlight.js/lib/languages/go";
import ini from "highlight.js/lib/languages/ini";
import javascript from "highlight.js/lib/languages/javascript";
import json from "highlight.js/lib/languages/json";
import markdown from "highlight.js/lib/languages/markdown";
import python from "highlight.js/lib/languages/python";
import rust from "highlight.js/lib/languages/rust";
import typescript from "highlight.js/lib/languages/typescript";
import xml from "highlight.js/lib/languages/xml";
import yaml from "highlight.js/lib/languages/yaml";

hljs.registerLanguage("bash", bash);
hljs.registerLanguage("css", css);
hljs.registerLanguage("diff", diff);
hljs.registerLanguage("go", go);
hljs.registerLanguage("ini", ini);
hljs.registerLanguage("javascript", javascript);
hljs.registerLanguage("json", json);
hljs.registerLanguage("markdown", markdown);
hljs.registerLanguage("python", python);
hljs.registerLanguage("rust", rust);
hljs.registerLanguage("typescript", typescript);
hljs.registerLanguage("xml", xml);
hljs.registerLanguage("yaml", yaml);

const EXTENSION_LANGUAGES: Record<string, string> = {
  bash: "bash",
  cjs: "javascript",
  css: "css",
  diff: "diff",
  go: "go",
  html: "xml",
  js: "javascript",
  json: "json",
  jsx: "javascript",
  mjs: "javascript",
  md: "markdown",
  markdown: "markdown",
  patch: "diff",
  py: "python",
  rs: "rust",
  sh: "bash",
  toml: "ini",
  ts: "typescript",
  tsx: "typescript",
  xml: "xml",
  yaml: "yaml",
  yml: "yaml",
  zsh: "bash"
};

const CACHE_LIMIT = 160;
const htmlCache = new Map<string, string>();

export function languageForPath(path: string): string {
  const extension = path.split(/[\\/]/).pop()?.split(".").pop()?.toLowerCase() ?? "";
  const language = EXTENSION_LANGUAGES[extension] ?? "";
  return language && hljs.getLanguage(language) ? language : "";
}

export function highlightToHtml(code: string, language: string): string {
  if (!language || !hljs.getLanguage(language)) {
    return escapeHtml(code);
  }
  const key = `${language}\0${code}`;
  const cached = htmlCache.get(key);
  if (cached !== undefined) {
    htmlCache.delete(key);
    htmlCache.set(key, cached);
    return cached;
  }
  try {
    const html = hljs.highlight(code, { language, ignoreIllegals: true }).value;
    htmlCache.set(key, html);
    while (htmlCache.size > CACHE_LIMIT) {
      const oldest = htmlCache.keys().next().value;
      if (oldest === undefined) {
        break;
      }
      htmlCache.delete(oldest);
    }
    return html;
  } catch {
    return escapeHtml(code);
  }
}

function escapeHtml(value: string): string {
  return value.replace(/[&<>]/g, (char) => {
    if (char === "&") {
      return "&amp;";
    }
    if (char === "<") {
      return "&lt;";
    }
    return "&gt;";
  });
}
