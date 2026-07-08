function bindServeSelectionControls(target) {
  if (!serveMode()) return;
  target.querySelectorAll(".select-box").forEach(control => {
    control.addEventListener("click", event => event.stopPropagation());
  });
  target.querySelectorAll("[data-row-select]").forEach(input => {
    input.addEventListener("click", event => event.stopPropagation());
    input.addEventListener("change", event => {
      event.stopPropagation();
      const key = input.dataset.rowSelect;
      if (!key) return;
      if (input.checked) state.rowSelection.add(key);
      else state.rowSelection.delete(key);
      renderComparisonPanels({ trace: false });
    });
  });
  target.querySelectorAll("[data-select-visible]").forEach(input => {
    input.indeterminate = input.hasAttribute("data-partial");
    input.addEventListener("click", event => event.stopPropagation());
    input.addEventListener("change", event => {
      event.stopPropagation();
      const rows = leaderboardRows();
      const visibleKeys = rows.map(row => row.trial_key).filter(Boolean);
      const allSelected = visibleKeys.length > 0 && visibleKeys.every(key => state.rowSelection.has(key));
      visibleKeys.forEach(key => {
        if (allSelected) state.rowSelection.delete(key);
        else state.rowSelection.add(key);
      });
      renderComparisonPanels({ trace: false });
    });
  });
}
function bindServeExportControls(target) {
  if (!serveMode()) return;
  target.querySelectorAll("[data-export-kind]").forEach(button => {
    button.addEventListener("click", event => {
      event.stopPropagation();
      exportCurrentScope(button.dataset.exportKind || "xlsx");
      button.closest("details")?.removeAttribute("open");
    });
  });
}
function bindTrialSelection(root) {
  root.querySelectorAll("tr[data-trial-key]").forEach(node => {
    node.addEventListener("click", event => {
      event.stopPropagation();
      state.selectedTrial = node.getAttribute("data-trial-key");
      const sourceKey = sourceKeyForTrialKey(state.selectedTrial);
      if (sourceKey) state.selectedSourceKey = sourceKey;
      state.selectedStep = firstUserStepSelection(state.selectedTrial);
      renderComparisonPanels();
    });
  });
}
function firstUserStepSelection(trialKey) {
  const step = listValue(trajectoryFor(trialKey)?.steps).find(item => {
    return lower(item?.source) === "user" && item?.step_id !== null && item?.step_id !== undefined;
  });
  return step ? { trialKey, stepId: String(step.step_id) } : null;
}
function exportScopeRows() {
  const rows = leaderboardRows();
  const selected = rows.filter(row => state.rowSelection.has(row.trial_key));
  return selected.length ? selected : rows;
}
function exportCurrentScope(kind) {
  const rows = exportScopeRows();
  if (kind === "json") {
    downloadText("peval-report-v19.json", "application/json", JSON.stringify(reportSubset(rows), null, 2));
    return;
  }
  if (kind === "html") {
    downloadText("peval-report.html", "text/html", htmlReportForSubset(reportSubset(rows)));
    return;
  }
  downloadBlob(
    "peval-leaderboard-visible.xlsx",
    "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
    new Blob([xlsxBytesForRows(rows)], {
      type: "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"
    })
  );
}
function xlsxTableRows(rows) {
  const columns = leaderboardColumns();
  return [
    columns.map(column => column.label),
    ...rows.map(row => columns.map(column => tableText(row, column)))
  ];
}
function xlsxBytesForRows(rows) {
  return zipFiles([
    {
      name: "[Content_Types].xml",
      text: xmlDeclaration() + `<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/><Override PartName="/xl/worksheets/sheet1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/></Types>`
    },
    {
      name: "_rels/.rels",
      text: xmlDeclaration() + `<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/></Relationships>`
    },
    {
      name: "xl/workbook.xml",
      text: xmlDeclaration() + `<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets><sheet name="Leaderboard" sheetId="1" r:id="rId1"/></sheets></workbook>`
    },
    {
      name: "xl/_rels/workbook.xml.rels",
      text: xmlDeclaration() + `<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/></Relationships>`
    },
    {
      name: "xl/worksheets/sheet1.xml",
      text: worksheetXml(xlsxTableRows(rows))
    }
  ]);
}
function xmlDeclaration() {
  return `<?xml version="1.0" encoding="UTF-8" standalone="yes"?>`;
}
function worksheetXml(rows) {
  const sheetData = rows.map((row, rowIndex) => {
    const rowNumber = rowIndex + 1;
    const cells = row.map((value, columnIndex) => {
      const cellRef = `${xlsxColumnName(columnIndex)}${rowNumber}`;
      return `<c r="${cellRef}" t="inlineStr"><is><t>${xmlEsc(value)}</t></is></c>`;
    }).join("");
    return `<row r="${rowNumber}">${cells}</row>`;
  }).join("");
  return xmlDeclaration() + `<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData>${sheetData}</sheetData></worksheet>`;
}
function xlsxColumnName(index) {
  let value = index + 1;
  let name = "";
  while (value > 0) {
    const remainder = (value - 1) % 26;
    name = String.fromCharCode(65 + remainder) + name;
    value = Math.floor((value - 1) / 26);
  }
  return name;
}
function xmlEsc(value) {
  return String(value ?? "").replace(/[&<>'"]/g, ch => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", "'": "&apos;", '"': "&quot;" }[ch]));
}
function zipFiles(files) {
  const encoder = new TextEncoder();
  const localParts = [];
  const centralParts = [];
  const zipTime = 0;
  const zipDate = 0x0021;
  let offset = 0;
  files.forEach(file => {
    const nameBytes = encoder.encode(file.name);
    const data = encoder.encode(file.text);
    const crc = crc32(data);
    const localHeader = concatBytes([
      u32(0x04034b50), u16(20), u16(0x0800), u16(0), u16(zipTime), u16(zipDate),
      u32(crc), u32(data.length), u32(data.length), u16(nameBytes.length), u16(0),
      nameBytes
    ]);
    localParts.push(localHeader, data);
    centralParts.push(concatBytes([
      u32(0x02014b50), u16(20), u16(20), u16(0x0800), u16(0), u16(zipTime), u16(zipDate),
      u32(crc), u32(data.length), u32(data.length), u16(nameBytes.length),
      u16(0), u16(0), u16(0), u16(0), u32(0), u32(offset), nameBytes
    ]));
    offset += localHeader.length + data.length;
  });
  const centralDirectory = concatBytes(centralParts);
  const end = concatBytes([
    u32(0x06054b50), u16(0), u16(0), u16(files.length), u16(files.length),
    u32(centralDirectory.length), u32(offset), u16(0)
  ]);
  return concatBytes([...localParts, centralDirectory, end]);
}
function u16(value) {
  const bytes = new Uint8Array(2);
  new DataView(bytes.buffer).setUint16(0, value, true);
  return bytes;
}
function u32(value) {
  const bytes = new Uint8Array(4);
  new DataView(bytes.buffer).setUint32(0, value >>> 0, true);
  return bytes;
}
function concatBytes(parts) {
  const length = parts.reduce((total, part) => total + part.length, 0);
  const out = new Uint8Array(length);
  let offset = 0;
  parts.forEach(part => {
    out.set(part, offset);
    offset += part.length;
  });
  return out;
}
let CRC32_TABLE = null;
function crc32(bytes) {
  const table = crc32Table();
  let crc = 0xffffffff;
  bytes.forEach(byte => {
    crc = (crc >>> 8) ^ table[(crc ^ byte) & 0xff];
  });
  return (crc ^ 0xffffffff) >>> 0;
}
function crc32Table() {
  if (CRC32_TABLE) return CRC32_TABLE;
  CRC32_TABLE = Array.from({ length: 256 }, (_, index) => {
    let crc = index;
    for (let bit = 0; bit < 8; bit += 1) {
      crc = (crc & 1) ? (0xedb88320 ^ (crc >>> 1)) : (crc >>> 1);
    }
    return crc >>> 0;
  });
  return CRC32_TABLE;
}
function reportSubset(rows) {
  const original = state.view || {};
  const metas = original.trajectory_meta || [];
  const trajectories = original.trajectory || [];
  const selectedKeys = new Set(rows.map(row => row.trial_key));
  const orderedMeta = [];
  const orderedTrajectories = [];
  rows.forEach(row => {
    const index = metas.findIndex(meta => meta.trial_key === row.trial_key);
    if (index >= 0) {
      orderedMeta.push({ ...metas[index] });
      orderedTrajectories.push(trajectories[index]);
    }
  });
  const subset = {
    schema_version: original.schema_version,
    includes: listValue(original.includes).filter(item => item !== "comparison"),
    trajectory: orderedTrajectories,
    trajectory_meta: orderedMeta
  };
  if (original.annotations) {
    subset.annotations = {
      ...original.annotations,
      notes: (original.annotations.notes || []).filter(note => selectedKeys.has(note.trial_key)),
      analysis: (original.annotations.analysis || []).filter(item => selectedKeys.has(item.trial_key))
    };
  }
  return subset;
}
function htmlReportForSubset(report) {
  const clone = document.documentElement.cloneNode(true);
  clone.querySelectorAll("[data-serve-only]").forEach(node => node.remove());
  ["report-notes", "comparison", "trace"].forEach(id => {
    const node = clone.querySelector(`#${id}`);
    if (node) node.innerHTML = "";
  });
  const dataNode = clone.querySelector("#peval-py-data");
  if (dataNode) dataNode.textContent = safeJsonForScript(JSON.stringify(report));
  const optionsNode = clone.querySelector("#peval-py-render-options");
  if (optionsNode) optionsNode.textContent = safeJsonForScript(JSON.stringify({ mode: "report" }));
  const body = clone.querySelector("body");
  if (body) {
    body.classList.remove("serve-mode");
    body.classList.add("report-mode");
  }
  return `<!doctype html>\n${clone.outerHTML}`;
}
function safeJsonForScript(value) {
  return String(value).replace(/&/g, "\\u0026").replace(/</g, "\\u003c").replace(/>/g, "\\u003e");
}
function downloadText(filename, mime, text) {
  const blob = new Blob([text], { type: `${mime};charset=utf-8` });
  downloadBlob(filename, mime, blob);
}
function downloadBlob(filename, mime, blob) {
  const url = URL.createObjectURL(blob);
  const link = document.createElement("a");
  link.href = url;
  link.download = filename;
  document.body.appendChild(link);
  link.click();
  link.remove();
  URL.revokeObjectURL(url);
}
