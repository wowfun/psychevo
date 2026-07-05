function renderManualNote(row) {
  const sourcePath = row?.source_ref?.relative_path;
  const meta = row?.label || sourcePath
    ? `<div class="note-meta"><strong>${esc(row?.label || t("notes", "Notes"))}</strong>${sourcePath ? `<code>${esc(sourcePath)}</code>` : ""}</div>`
    : "";
  return `<article class="manual-note">${meta}<div class="note-body">${renderMarkdown(row.markdown || "")}</div></article>`;
}
function renderMarkdown(markdown) {
  const lines = String(markdown ?? "").split(/\r?\n/);
  const out = [];
  let paragraph = [];
  let list = [];
  let code = [];
  let inCode = false;
  function flushParagraph() {
    if (paragraph.length) {
      out.push(`<p>${inlineMarkdown(paragraph.join(" "))}</p>`);
      paragraph = [];
    }
  }
  function flushList() {
    if (list.length) {
      out.push(`<ul>${list.map(item => `<li>${inlineMarkdown(item)}</li>`).join("")}</ul>`);
      list = [];
    }
  }
  function flushCode() {
    if (code.length) {
      out.push(`<pre class="note-code">${esc(code.join("\n"))}</pre>`);
      code = [];
    }
  }
  for (let index = 0; index < lines.length; index += 1) {
    const line = lines[index];
    if (line.trim().startsWith("```")) {
      if (inCode) {
        flushCode();
        inCode = false;
      } else {
        flushParagraph();
        flushList();
        inCode = true;
      }
      continue;
    }
    if (inCode) {
      code.push(line);
      continue;
    }
    if (!line.trim()) {
      flushParagraph();
      flushList();
      continue;
    }
    const table = markdownTableAt(lines, index);
    if (table) {
      flushParagraph();
      flushList();
      out.push(renderMarkdownTable(table));
      index = table.endIndex;
      continue;
    }
    const heading = line.match(/^(#{1,6})\s+(.+?)\s*#*\s*$/);
    if (heading) {
      flushParagraph();
      flushList();
      const rank = Math.min(3, Math.max(1, heading[1].length));
      const tag = `h${rank + 3}`;
      out.push(`<${tag} class="markdown-heading markdown-heading-${rank}">${inlineMarkdown(heading[2])}</${tag}>`);
      continue;
    }
    const bullet = line.match(/^[-*]\s+(.+)$/);
    if (bullet) {
      flushParagraph();
      list.push(bullet[1]);
      continue;
    }
    paragraph.push(line.trim());
  }
  flushParagraph();
  flushList();
  flushCode();
  return out.join("") || "<p></p>";
}
function markdownTableAt(lines, index) {
  const header = parseMarkdownTableRow(lines[index]);
  const separator = parseMarkdownTableRow(lines[index + 1]);
  if (!header || !separator || !isMarkdownTableSeparator(separator)) return null;
  const alignments = separator.map(markdownTableAlignment);
  const body = [];
  let cursor = index + 2;
  while (cursor < lines.length) {
    const row = parseMarkdownTableRow(lines[cursor]);
    if (!row) break;
    body.push(row);
    cursor += 1;
  }
  return { header, alignments, body, endIndex: cursor - 1 };
}
function parseMarkdownTableRow(line) {
  const text = String(line ?? "").trim();
  if (!text || !text.includes("|")) return null;
  let inner = text;
  if (inner.startsWith("|")) inner = inner.slice(1);
  if (inner.endsWith("|")) inner = inner.slice(0, -1);
  const cells = [];
  let current = "";
  let escaped = false;
  for (const char of inner) {
    if (escaped) {
      current += char;
      escaped = false;
      continue;
    }
    if (char === "\\") {
      escaped = true;
      continue;
    }
    if (char === "|") {
      cells.push(current.trim());
      current = "";
      continue;
    }
    current += char;
  }
  cells.push(current.trim());
  return cells.length >= 2 ? cells : null;
}
function isMarkdownTableSeparator(cells) {
  return cells.length >= 2 && cells.every(cell => /^:?-{3,}:?$/.test(String(cell || "").trim()));
}
function markdownTableAlignment(value) {
  const text = String(value || "").trim();
  if (/^:-+:$/.test(text)) return "center";
  if (/^-+:$/.test(text)) return "right";
  if (/^:-+$/.test(text)) return "left";
  return "";
}
function renderMarkdownTable(table) {
  const width = table.header.length;
  const header = `<tr>${normalizedMarkdownTableRow(table.header, width).map((cell, index) => renderMarkdownTableCell("th", cell, table.alignments[index])).join("")}</tr>`;
  const body = table.body.length
    ? `<tbody>${table.body.map(row => `<tr>${normalizedMarkdownTableRow(row, width).map((cell, index) => renderMarkdownTableCell("td", cell, table.alignments[index])).join("")}</tr>`).join("")}</tbody>`
    : "";
  return `<div class="markdown-table-wrap"><table class="markdown-table"><thead>${header}</thead>${body}</table></div>`;
}
function normalizedMarkdownTableRow(row, width) {
  return Array.from({ length: width }, (_, index) => row[index] ?? "");
}
function renderMarkdownTableCell(tag, value, alignment) {
  const classAttr = alignment ? ` class="align-${alignment}"` : "";
  return `<${tag}${classAttr}>${inlineMarkdown(value)}</${tag}>`;
}
function inlineMarkdown(value) {
  const parts = [];
  const text = String(value ?? "");
  let cursor = 0;
  text.replace(/`([^`]+)`/g, (match, code, offset) => {
    if (offset > cursor) parts.push(renderInlineMarkdownText(text.slice(cursor, offset)));
    parts.push(`<code>${esc(code)}</code>`);
    cursor = offset + match.length;
    return match;
  });
  if (cursor < text.length) parts.push(renderInlineMarkdownText(text.slice(cursor)));
  return parts.join("");
}
function renderInlineMarkdownText(value) {
  return esc(value)
    .replace(/\*\*([^*]+)\*\*/g, "<strong>$1</strong>")
    .replace(/__([^_]+)__/g, "<strong>$1</strong>")
    .replace(/(^|[^\w])\*([^*\s][^*]*?)\*/g, "$1<em>$2</em>")
    .replace(/(^|[^\w])_([^_\s][^_]*?)_/g, "$1<em>$2</em>");
}
