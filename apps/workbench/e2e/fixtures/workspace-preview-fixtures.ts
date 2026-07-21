import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import path from "node:path";
import { deflateSync } from "node:zlib";
import JSZip from "jszip";

type FixtureRecord = {
  bytes: number;
  format: string;
  generatedBy: string;
  path: string;
  sha256: string;
};

export type WorkspacePreviewFixtureManifest = {
  fixtures: FixtureRecord[];
  generator: "psychevo-workspace-preview-v1";
  tools: { ffmpeg: string | null; heic: string; node: string };
  unavailable: string[];
};

const ZIP_DATE = new Date("1980-01-01T00:00:00.000Z");

export async function writeWorkspacePreviewFixtures(
  cwd: string
): Promise<WorkspacePreviewFixtureManifest> {
  mkdirSync(cwd, { recursive: true });
  const fixtures: FixtureRecord[] = [];
  const write = (filename: string, format: string, generatedBy: string, bytes: Uint8Array | string) => {
    const body = typeof bytes === "string" ? Buffer.from(bytes) : Buffer.from(bytes);
    writeFileSync(path.join(cwd, filename), body);
    fixtures.push({
      bytes: body.byteLength,
      format,
      generatedBy,
      path: filename,
      sha256: createHash("sha256").update(body).digest("hex")
    });
  };

  write("fixture.png", "png", "node:zlib+png-chunks", generatedPng());
  write("fixture.heic", "heic", "libheif-1.17.6+x265-hevc-q80", generatedHeic());
  write("hostile.svg", "svg", "inline-svg", hostileSvg());
  write("fixture.pdf", "pdf", "minimal-pdf-xref", generatedPdf());
  write("fixture.csv", "csv", "utf8-delimited", "Name,Score\nAda,99\nLin,97\n");
  write("fixture.tsv", "tsv", "utf8-delimited", "Name\tLanguage\nAda\tRust\nLin\tTypeScript\n");
  write("fixture.excalidraw", "excalidraw", "excalidraw-json", excalidrawDocument());
  write("hostile.zip", "zip", "jszip-directory-only", await hostileZip());
  write("fixture.docx", "docx", "generated-ooxml-security", await generatedDocx());
  write("fixture.xlsx", "xlsx", "generated-ooxml", await generatedXlsx());
  write("fixture.pptx", "pptx", "generated-ooxml", await generatedPptx());
  write("fixture.rtf", "rtf", "generated-rtf", generatedRtf());
  write("fixture.odt", "odt", "generated-odf", await generatedOdt());
  write("fixture.ods", "ods", "generated-odf", await generatedOds());
  write("fixture.odp", "odp", "generated-odf", await generatedOdp());
  write("fixture.ofd", "ofd", "generated-ofd-package", await generatedOfd());

  const unavailable: string[] = [];
  const ffmpegVersion = commandVersion("ffmpeg");
  if (ffmpegVersion !== null) {
    generateMediaFixtures(cwd);
    for (const [filename, format, generatedBy] of [
      ["fixture.mp4", "mp4", "ffmpeg:h264+aac"],
      ["fixture.webm", "webm", "ffmpeg:vp9+opus"],
      ["fixture.mp3", "mp3", "ffmpeg:libmp3lame"]
    ] as const) {
      const body = readFileSync(path.join(cwd, filename));
      fixtures.push({
        bytes: body.byteLength,
        format,
        generatedBy,
        path: filename,
        sha256: createHash("sha256").update(body).digest("hex")
      });
    }
  } else {
    unavailable.push("media:ffmpeg-not-installed");
  }
  const manifest: WorkspacePreviewFixtureManifest = {
    fixtures: fixtures.sort((left, right) => left.path.localeCompare(right.path)),
    generator: "psychevo-workspace-preview-v1",
    tools: {
      ffmpeg: ffmpegVersion,
      heic: "libheif 1.17.6 + x265 plugin; embedded deterministic 64x64 RGB image encoded at quality 80 as HEVC Main still picture",
      node: process.version
    },
    unavailable
  };
  writeFileSync(
    path.join(cwd, "preview-fixtures.manifest.json"),
    `${JSON.stringify(manifest, null, 2)}\n`
  );
  return manifest;
}

function generatedHeic(): Uint8Array {
  return Buffer.from(
    "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",
    "base64"
  );
}

function generatedPng(): Uint8Array {
  const width = 4;
  const height = 4;
  const scanlines = Buffer.alloc(height * (1 + width * 4));
  for (let y = 0; y < height; y += 1) {
    const row = y * (1 + width * 4);
    scanlines[row] = 0;
    for (let x = 0; x < width; x += 1) {
      const offset = row + 1 + x * 4;
      const orange = (x + y) % 2 === 0;
      scanlines.set(orange ? [245, 158, 11, 255] : [14, 116, 144, 255], offset);
    }
  }
  return Buffer.concat([
    Buffer.from([137, 80, 78, 71, 13, 10, 26, 10]),
    pngChunk("IHDR", Buffer.concat([u32(width), u32(height), Buffer.from([8, 6, 0, 0, 0])])),
    pngChunk("IDAT", deflateSync(scanlines, { level: 9 })),
    pngChunk("IEND", Buffer.alloc(0))
  ]);
}

function pngChunk(type: string, body: Uint8Array): Buffer {
  const typeBytes = Buffer.from(type, "ascii");
  const data = Buffer.from(body);
  return Buffer.concat([u32(data.byteLength), typeBytes, data, u32(crc32(Buffer.concat([typeBytes, data])))]);
}

function u32(value: number): Buffer {
  const bytes = Buffer.alloc(4);
  bytes.writeUInt32BE(value >>> 0);
  return bytes;
}

function crc32(bytes: Uint8Array): number {
  let crc = 0xffffffff;
  for (const byte of bytes) {
    crc ^= byte;
    for (let bit = 0; bit < 8; bit += 1) {
      crc = (crc >>> 1) ^ (0xedb88320 & -(crc & 1));
    }
  }
  return (crc ^ 0xffffffff) >>> 0;
}

function hostileSvg(): string {
  return [
    '<svg xmlns="http://www.w3.org/2000/svg" width="320" height="180" viewBox="0 0 320 180">',
    '<rect width="320" height="180" rx="16" fill="#0e7490"/>',
    '<text x="24" y="96" fill="white" font-family="sans-serif" font-size="24">SVG fixture visible</text>',
    '<script>parent.__PSYCHEVO_PREVIEW_PWNED__=true;fetch("https://preview-security.invalid/svg-script")</script>',
    '<image href="https://preview-security.invalid/svg-image.png" width="1" height="1"/>',
    "</svg>"
  ].join("");
}

function generatedPdf(): Uint8Array {
  const stream = "BT /F1 24 Tf 72 720 Td (PDF fixture visible) Tj ET";
  const objects = [
    "<< /Type /Catalog /Pages 2 0 R >>",
    "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
    "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 4 0 R >> >> /Contents 5 0 R >>",
    "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>",
    `<< /Length ${Buffer.byteLength(stream)} >>\nstream\n${stream}\nendstream`
  ];
  let source = "%PDF-1.7\n% Psychevo generated fixture\n";
  source += `${("% deterministic range padding ".padEnd(78, "x") + "\n").repeat(3_400)}`;
  const offsets = [0];
  objects.forEach((object, index) => {
    offsets.push(Buffer.byteLength(source));
    source += `${index + 1} 0 obj\n${object}\nendobj\n`;
  });
  const xref = Buffer.byteLength(source);
  source += `xref\n0 ${objects.length + 1}\n0000000000 65535 f \n`;
  source += offsets.slice(1).map((offset) => `${String(offset).padStart(10, "0")} 00000 n \n`).join("");
  source += `trailer\n<< /Size ${objects.length + 1} /Root 1 0 R >>\nstartxref\n${xref}\n%%EOF\n`;
  return Buffer.from(source, "latin1");
}

function excalidrawDocument(): string {
  const element = (
    id: string,
    type: string,
    x: number,
    y: number,
    width: number,
    height: number,
    extra: Record<string, unknown> = {}
  ) => ({
    angle: 0,
    backgroundColor: "transparent",
    boundElements: null,
    fillStyle: "solid",
    frameId: "fixture-frame",
    groupIds: [],
    height,
    id,
    index: null,
    isDeleted: false,
    link: null,
    locked: false,
    opacity: 100,
    roughness: 1,
    roundness: null,
    seed: id.length * 997,
    strokeColor: "#0f172a",
    strokeStyle: "solid",
    strokeWidth: 2,
    type,
    updated: 1,
    version: 1,
    versionNonce: id.length * 101,
    width,
    x,
    y,
    ...extra
  });
  const imageData = `data:image/png;base64,${Buffer.from(generatedPng()).toString("base64")}`;
  return JSON.stringify({
    appState: {
      exportBackground: true,
      theme: "light",
      viewBackgroundColor: "#ffffff"
    },
    type: "excalidraw",
    version: 2,
    elements: [
      element("fixture-frame", "frame", 20, 20, 620, 310, {
        frameId: null,
        name: "Official renderer fixture"
      }),
      element("fixture-box", "rectangle", 55, 65, 290, 110, {
        backgroundColor: "#bae6fd",
        boundElements: [{ id: "fixture-label", type: "text" }],
        link: "https://preview-security.invalid/excalidraw-link",
        roundness: { type: 3 }
      }),
      element("fixture-label", "text", 75, 92, 250, 55, {
        autoResize: false,
        containerId: "fixture-box",
        fontFamily: 1,
        fontSize: 24,
        lineHeight: 1.25,
        originalText: "Excalidraw fixture visible",
        text: "Excalidraw fixture visible",
        textAlign: "center",
        verticalAlign: "middle"
      }),
      element("fixture-arrow", "arrow", 375, 80, 185, 75, {
        elbowed: false,
        endArrowhead: "arrow",
        endBinding: null,
        lastCommittedPoint: null,
        points: [[0, 0], [95, 20], [185, 75]],
        startArrowhead: null,
        startBinding: null
      }),
      element("fixture-stroke", "freedraw", 390, 190, 115, 60, {
        lastCommittedPoint: null,
        points: [[0, 28], [20, 5], [45, 45], [75, 10], [115, 32]],
        pressures: [],
        simulatePressure: true
      }),
      element("fixture-image", "image", 70, 215, 72, 72, {
        crop: null,
        fileId: "fixture-image-file",
        scale: [1, 1],
        status: "saved"
      }),
      element("fixture-embed", "embeddable", 500, 205, 100, 70, {
        link: "https://preview-security.invalid/excalidraw-embed"
      })
    ],
    files: {
      "fixture-image-file": {
        created: 1,
        dataURL: imageData,
        id: "fixture-image-file",
        lastRetrieved: 1,
        mimeType: "image/png"
      }
    }
  });
}

async function hostileZip(): Promise<Uint8Array> {
  const archive = new JSZip();
  const options = { createFolders: false, date: ZIP_DATE, unixPermissions: 0o100644 };
  archive.file("safe/readme.txt", "ZIP fixture visible\n", options);
  archive.file("../escape.txt", "must never be extracted\n", options);
  archive.file("/absolute.txt", "must remain an inert name\n", options);
  archive.file("C:\\escape.txt", "must not retain a drive prefix\n", options);
  archive.file("safe/../../escape2.txt", "must never be extracted\n", options);
  return archive.generateAsync({
    type: "uint8array",
    compression: "DEFLATE",
    compressionOptions: { level: 9 },
    platform: "UNIX",
    streamFiles: false
  });
}

async function generatedDocx(): Promise<Uint8Array> {
  return officeZip({
    "[Content_Types].xml": contentTypes([
      ["/word/document.xml", "application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"],
      ["/word/styles.xml", "application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml"]
    ]),
    "_rels/.rels": relationships([
      ["rId1", "http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument", "word/document.xml"]
    ]),
    "word/document.xml": xml(`
      <w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:v="urn:schemas-microsoft-com:vml">
        <w:body><w:p><w:r><w:t>DOCX fixture visible</w:t></w:r></w:p><w:p><w:r><w:pict><v:shape id="external-image"><v:imagedata r:id="external"/></v:shape></w:pict></w:r></w:p><w:sectPr><w:pgSz w:w="12240" w:h="15840"/><w:pgMar w:top="1440" w:right="1440" w:bottom="1440" w:left="1440"/></w:sectPr></w:body>
      </w:document>`),
    "word/styles.xml": xml(`
      <w:styles xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:style w:type="paragraph" w:default="1" w:styleId="Normal"><w:name w:val="Normal"/></w:style></w:styles>`),
    "word/_rels/document.xml.rels": xml(`
      <Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
        <Relationship Id="external" TargetMode="Extern&#x61;l" Target="https&#58;//preview-security.invalid/docx-image" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image"/>
        <Relationship Id="macro" Target="vbaProject.bin" Type="http://schemas.microsoft.com/office/2006/relationships/vbaProject"/>
        <Relationship Id="ole" Target="embeddings/oleObject1.bin" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/oleObject"/>
        <Relationship Id="active-x" Target="activeX/activeX1.bin" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/control"/>
      </Relationships>`),
    "word/vbaProject.bin": new Uint8Array([0xd0, 0xcf, 0x11, 0xe0, 0x00, 0x01]),
    "word/embeddings/oleObject1.bin": new Uint8Array([0xd0, 0xcf, 0x11, 0xe0, 0x00, 0x02]),
    "word/activeX/activeX1.bin": new Uint8Array([0xd0, 0xcf, 0x11, 0xe0, 0x00, 0x03])
  });
}

async function generatedXlsx(): Promise<Uint8Array> {
  return officeZip({
    "[Content_Types].xml": contentTypes([
      ["/xl/workbook.xml", "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"],
      ["/xl/worksheets/sheet1.xml", "application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"],
      ["/xl/styles.xml", "application/vnd.openxmlformats-officedocument.spreadsheetml.styles+xml"],
      ["/xl/sharedStrings.xml", "application/vnd.openxmlformats-officedocument.spreadsheetml.sharedStrings+xml"]
    ]),
    "_rels/.rels": relationships([
      ["rId1", "http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument", "xl/workbook.xml"]
    ]),
    "xl/workbook.xml": xml(`
      <workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets><sheet name="Fixture" sheetId="1" r:id="rId1"/></sheets></workbook>`),
    "xl/_rels/workbook.xml.rels": relationships([
      ["rId1", "http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet", "worksheets/sheet1.xml"],
      ["rId2", "http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles", "styles.xml"],
      ["rId3", "http://schemas.openxmlformats.org/officeDocument/2006/relationships/sharedStrings", "sharedStrings.xml"]
    ]),
    "xl/worksheets/sheet1.xml": xml(`
      <worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><dimension ref="A1:B2"/><sheetData><row r="1"><c r="A1" t="s"><v>0</v></c><c r="B1" t="s"><v>1</v></c></row><row r="2"><c r="A2" t="s"><v>2</v></c><c r="B2"><v>99</v></c></row></sheetData></worksheet>`),
    "xl/worksheets/_rels/sheet1.xml.rels": encodedExternalRelationships("https&#58;//preview-security.invalid/xlsx-link"),
    "xl/styles.xml": xml(`
      <styleSheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><fonts count="1"><font><sz val="11"/><name val="Calibri"/></font></fonts><fills count="2"><fill><patternFill patternType="none"/></fill><fill><patternFill patternType="gray125"/></fill></fills><borders count="1"><border><left/><right/><top/><bottom/><diagonal/></border></borders><cellStyleXfs count="1"><xf numFmtId="0" fontId="0" fillId="0" borderId="0"/></cellStyleXfs><cellXfs count="1"><xf numFmtId="0" fontId="0" fillId="0" borderId="0" xfId="0"/></cellXfs></styleSheet>`),
    "xl/sharedStrings.xml": xml(`
      <sst xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" count="3" uniqueCount="3"><si><t>XLSX fixture visible</t></si><si><t>Score</t></si><si><t>Ada</t></si></sst>`),
    "xl/fixture-padding.bin": deterministicNoise(1_150_000)
  });
}

async function generatedPptx(): Promise<Uint8Array> {
  return officeZip({
    "[Content_Types].xml": contentTypes([
      ["/ppt/presentation.xml", "application/vnd.openxmlformats-officedocument.presentationml.presentation.main+xml"],
      ["/ppt/slides/slide1.xml", "application/vnd.openxmlformats-officedocument.presentationml.slide+xml"],
      ["/ppt/slideMasters/slideMaster1.xml", "application/vnd.openxmlformats-officedocument.presentationml.slideMaster+xml"],
      ["/ppt/slideLayouts/slideLayout1.xml", "application/vnd.openxmlformats-officedocument.presentationml.slideLayout+xml"],
      ["/ppt/theme/theme1.xml", "application/vnd.openxmlformats-officedocument.theme+xml"]
    ]),
    "_rels/.rels": relationships([
      ["rId1", "http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument", "ppt/presentation.xml"]
    ]),
    "ppt/presentation.xml": xml(`
      <p:presentation xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main"><p:sldMasterIdLst><p:sldMasterId id="2147483648" r:id="rId1"/></p:sldMasterIdLst><p:sldIdLst><p:sldId id="256" r:id="rId2"/></p:sldIdLst><p:sldSz cx="9144000" cy="5143500" type="screen16x9"/><p:notesSz cx="6858000" cy="9144000"/></p:presentation>`),
    "ppt/_rels/presentation.xml.rels": relationships([
      ["rId1", "http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideMaster", "slideMasters/slideMaster1.xml"],
      ["rId2", "http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide", "slides/slide1.xml"]
    ]),
    "ppt/slides/slide1.xml": xml(`
      <p:sld xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main"><p:cSld><p:spTree>${presentationGroup()}<p:sp><p:nvSpPr><p:cNvPr id="2" name="Fixture title"/><p:cNvSpPr/><p:nvPr/></p:nvSpPr><p:spPr><a:xfrm><a:off x="914400" y="1371600"/><a:ext cx="7315200" cy="1828800"/></a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom><a:solidFill><a:srgbClr val="DFF6FF"/></a:solidFill></p:spPr><p:txBody><a:bodyPr/><a:lstStyle/><a:p><a:r><a:rPr lang="en-US" sz="2800"/><a:t>PPTX fixture visible</a:t></a:r><a:endParaRPr lang="en-US"/></a:p></p:txBody></p:sp></p:spTree></p:cSld><p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr></p:sld>`),
    "ppt/slides/_rels/slide1.xml.rels": xml(`
      <Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideLayout" Target="../slideLayouts/slideLayout1.xml"/><Relationship Id="external" TargetMode="Extern&#x61;l" Target="https&#58;//preview-security.invalid/pptx-image" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image"/></Relationships>`),
    "ppt/slideMasters/slideMaster1.xml": xml(`
      <p:sldMaster xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main"><p:cSld><p:spTree>${presentationGroup()}</p:spTree></p:cSld><p:clrMap accent1="accent1" accent2="accent2" accent3="accent3" accent4="accent4" accent5="accent5" accent6="accent6" bg1="lt1" bg2="lt2" folHlink="folHlink" hlink="hlink" tx1="dk1" tx2="dk2"/><p:sldLayoutIdLst><p:sldLayoutId id="1" r:id="rId1"/></p:sldLayoutIdLst><p:txStyles><p:titleStyle/><p:bodyStyle/><p:otherStyle/></p:txStyles></p:sldMaster>`),
    "ppt/slideMasters/_rels/slideMaster1.xml.rels": relationships([
      ["rId1", "http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideLayout", "../slideLayouts/slideLayout1.xml"],
      ["rId2", "http://schemas.openxmlformats.org/officeDocument/2006/relationships/theme", "../theme/theme1.xml"]
    ]),
    "ppt/slideLayouts/slideLayout1.xml": xml(`
      <p:sldLayout xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" type="blank"><p:cSld name="Blank"><p:spTree>${presentationGroup()}</p:spTree></p:cSld><p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr></p:sldLayout>`),
    "ppt/slideLayouts/_rels/slideLayout1.xml.rels": relationships([
      ["rId1", "http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideMaster", "../slideMasters/slideMaster1.xml"]
    ]),
    "ppt/theme/theme1.xml": xml(`
      <a:theme xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" name="Psychevo"><a:themeElements><a:clrScheme name="Psychevo"><a:dk1><a:srgbClr val="0F172A"/></a:dk1><a:lt1><a:srgbClr val="FFFFFF"/></a:lt1><a:dk2><a:srgbClr val="1E293B"/></a:dk2><a:lt2><a:srgbClr val="F8FAFC"/></a:lt2><a:accent1><a:srgbClr val="0E7490"/></a:accent1><a:accent2><a:srgbClr val="F59E0B"/></a:accent2><a:accent3><a:srgbClr val="22C55E"/></a:accent3><a:accent4><a:srgbClr val="8B5CF6"/></a:accent4><a:accent5><a:srgbClr val="EC4899"/></a:accent5><a:accent6><a:srgbClr val="64748B"/></a:accent6><a:hlink><a:srgbClr val="0000FF"/></a:hlink><a:folHlink><a:srgbClr val="800080"/></a:folHlink></a:clrScheme><a:fontScheme name="Psychevo"><a:majorFont><a:latin typeface="Arial"/><a:ea typeface=""/><a:cs typeface=""/></a:majorFont><a:minorFont><a:latin typeface="Arial"/><a:ea typeface=""/><a:cs typeface=""/></a:minorFont></a:fontScheme><a:fmtScheme name="Psychevo"><a:fillStyleLst><a:solidFill><a:schemeClr val="phClr"/></a:solidFill></a:fillStyleLst><a:lnStyleLst><a:ln w="9525"><a:solidFill><a:schemeClr val="phClr"/></a:solidFill></a:ln></a:lnStyleLst><a:effectStyleLst><a:effectStyle><a:effectLst/></a:effectStyle></a:effectStyleLst><a:bgFillStyleLst><a:solidFill><a:schemeClr val="phClr"/></a:solidFill></a:bgFillStyleLst></a:fmtScheme></a:themeElements></a:theme>`)
  });
}

function generatedRtf(): string {
  return String.raw`{\rtf1\ansi\deff0{\fonttbl{\f0 Arial;}}\viewkind4\uc1\pard\f0\fs28 RTF fixture visible\par}`;
}

async function generatedOdt(): Promise<Uint8Array> {
  return odfZip(
    "application/vnd.oasis.opendocument.text",
    xml(`
      <office:document-content xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0" office:version="1.3">
        <office:body><office:text><text:p>ODT fixture visible</text:p></office:text></office:body>
      </office:document-content>`)
  );
}

async function generatedOds(): Promise<Uint8Array> {
  return odfZip(
    "application/vnd.oasis.opendocument.spreadsheet",
    xml(`
      <office:document-content xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:table="urn:oasis:names:tc:opendocument:xmlns:table:1.0" xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0" office:version="1.3">
        <office:body><office:spreadsheet><table:table table:name="Fixture"><table:table-row><table:table-cell office:value-type="string"><text:p>ODS fixture visible</text:p></table:table-cell><table:table-cell office:value-type="float" office:value="99"><text:p>99</text:p></table:table-cell></table:table-row></table:table></office:spreadsheet></office:body>
      </office:document-content>`)
  );
}

async function generatedOdp(): Promise<Uint8Array> {
  return odfZip(
    "application/vnd.oasis.opendocument.presentation",
    xml(`
      <office:document-content xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:draw="urn:oasis:names:tc:opendocument:xmlns:drawing:1.0" xmlns:presentation="urn:oasis:names:tc:opendocument:xmlns:presentation:1.0" xmlns:svg="urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0" xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0" office:version="1.3">
        <office:body><office:presentation><draw:page draw:name="Fixture" presentation:class="standard"><draw:frame svg:x="2cm" svg:y="2cm" svg:width="20cm" svg:height="3cm"><draw:text-box><text:p>ODP fixture visible</text:p></draw:text-box></draw:frame></draw:page></office:presentation></office:body>
      </office:document-content>`)
  );
}

async function odfZip(mimeType: string, content: string): Promise<Uint8Array> {
  const archive = new JSZip();
  archive.file("mimetype", mimeType, {
    createFolders: false,
    date: ZIP_DATE,
    compression: "STORE",
    unixPermissions: 0o100644
  });
  archive.file("content.xml", content, {
    createFolders: false,
    date: ZIP_DATE,
    unixPermissions: 0o100644
  });
  archive.file("styles.xml", xml(`
    <office:document-styles xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:style="urn:oasis:names:tc:opendocument:xmlns:style:1.0" office:version="1.3">
      <office:styles><style:default-style style:family="paragraph"/></office:styles>
    </office:document-styles>`), {
    createFolders: false,
    date: ZIP_DATE,
    unixPermissions: 0o100644
  });
  archive.file("META-INF/manifest.xml", xml(`
    <manifest:manifest xmlns:manifest="urn:oasis:names:tc:opendocument:xmlns:manifest:1.0" manifest:version="1.3">
      <manifest:file-entry manifest:full-path="/" manifest:media-type="${mimeType}"/>
      <manifest:file-entry manifest:full-path="content.xml" manifest:media-type="text/xml"/>
      <manifest:file-entry manifest:full-path="styles.xml" manifest:media-type="text/xml"/>
    </manifest:manifest>`), {
    createFolders: false,
    date: ZIP_DATE,
    unixPermissions: 0o100644
  });
  return archive.generateAsync({
    type: "uint8array",
    compression: "DEFLATE",
    compressionOptions: { level: 9 },
    platform: "UNIX",
    streamFiles: false
  });
}

async function generatedOfd(): Promise<Uint8Array> {
  return officeZip({
    "OFD.xml": xml(`
      <ofd:OFD xmlns:ofd="http://www.ofdspec.org/2016"><ofd:DocBody><ofd:DocInfo><ofd:DocID>psychevo-fixture-ofd</ofd:DocID><ofd:Creator>Psychevo</ofd:Creator><ofd:CreationDate>2026-07-19</ofd:CreationDate></ofd:DocInfo><ofd:DocRoot>Doc_0/Document.xml</ofd:DocRoot></ofd:DocBody></ofd:OFD>`),
    "Doc_0/Document.xml": xml(`
      <ofd:Document xmlns:ofd="http://www.ofdspec.org/2016"><ofd:CommonData><ofd:MaxUnitID>6</ofd:MaxUnitID><ofd:PageArea><ofd:PhysicalBox>0 0 210 297</ofd:PhysicalBox></ofd:PageArea><ofd:PublicRes>PublicRes.xml</ofd:PublicRes></ofd:CommonData><ofd:Pages><ofd:Page ID="3" BaseLoc="Pages/Page_0/Content.xml"/></ofd:Pages></ofd:Document>`),
    "Doc_0/PublicRes.xml": xml(`
      <ofd:Res xmlns:ofd="http://www.ofdspec.org/2016" BaseLoc="Res"><ofd:Fonts><ofd:Font ID="1" FontName="Arial" FamilyName="Arial"/></ofd:Fonts></ofd:Res>`),
    "Doc_0/Pages/Page_0/Content.xml": xml(`
      <ofd:Page xmlns:ofd="http://www.ofdspec.org/2016"><ofd:Area><ofd:PhysicalBox>0 0 210 297</ofd:PhysicalBox></ofd:Area><ofd:Content><ofd:Layer ID="4"><ofd:TextObject ID="5" Boundary="20 20 160 20" Font="1" Size="7"><ofd:FillColor Value="15 23 42"/><ofd:TextCode X="0" Y="9">OFD fixture visible</ofd:TextCode></ofd:TextObject></ofd:Layer></ofd:Content></ofd:Page>`)
  });
}

function presentationGroup(): string {
  return '<p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="0" cy="0"/><a:chOff x="0" y="0"/><a:chExt cx="0" cy="0"/></a:xfrm></p:grpSpPr>';
}

async function officeZip(files: Record<string, string | Uint8Array>): Promise<Uint8Array> {
  const archive = new JSZip();
  Object.entries(files).forEach(([filename, content]) => archive.file(filename, content, {
    createFolders: false,
    date: ZIP_DATE,
    unixPermissions: 0o100644
  }));
  return archive.generateAsync({
    type: "uint8array",
    compression: "DEFLATE",
    compressionOptions: { level: 9 },
    platform: "UNIX",
    streamFiles: false
  });
}

function contentTypes(overrides: [string, string][]): string {
  return xml(`<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/>${overrides.map(([part, type]) => `<Override PartName="${part}" ContentType="${type}"/>`).join("")}</Types>`);
}

function relationships(items: [string, string, string][]): string {
  return xml(`<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">${items.map(([id, type, target]) => `<Relationship Id="${id}" Type="${type}" Target="${target}"/>`).join("")}</Relationships>`);
}

function encodedExternalRelationships(target: string): string {
  return xml(`<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="external" TargetMode="Extern&#x61;l" Target="${target}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image"/></Relationships>`);
}

function xml(body: string): string {
  return `<?xml version="1.0" encoding="UTF-8" standalone="yes"?>${body.replace(/>\s+</g, "><").trim()}`;
}

function deterministicNoise(length: number): Uint8Array {
  const bytes = new Uint8Array(length);
  let state = 0x9e3779b9;
  for (let index = 0; index < bytes.length; index += 1) {
    state ^= state << 13;
    state ^= state >>> 17;
    state ^= state << 5;
    bytes[index] = state & 0xff;
  }
  return bytes;
}

function commandVersion(command: string): string | null {
  const result = spawnSync(command, ["-version"], { encoding: "utf8" });
  if (result.status !== 0) return null;
  return result.stdout.split(/\r?\n/, 1)[0]?.trim() || command;
}

function generateMediaFixtures(cwd: string) {
  runFfmpeg(cwd, [
    "-f", "lavfi", "-i", "testsrc2=size=320x180:rate=24:duration=4",
    "-f", "lavfi", "-i", "sine=frequency=440:sample_rate=48000:duration=4",
    "-map", "0:v:0", "-map", "1:a:0", "-shortest",
    "-fflags", "+bitexact", "-flags:v", "+bitexact", "-flags:a", "+bitexact",
    "-c:v", "libx264", "-pix_fmt", "yuv420p", "-profile:v", "baseline", "-level:v", "3.0",
    "-preset", "veryslow", "-crf", "28", "-x264-params", "scenecut=0:keyint=24:min-keyint=24", "-threads", "1",
    "-c:a", "aac", "-b:a", "64k", "-movflags", "+faststart", "-map_metadata", "-1",
    "fixture.mp4"
  ]);
  runFfmpeg(cwd, [
    "-f", "lavfi", "-i", "testsrc2=size=320x180:rate=24:duration=4",
    "-f", "lavfi", "-i", "sine=frequency=659.25:sample_rate=48000:duration=4",
    "-map", "0:v:0", "-map", "1:a:0", "-shortest",
    "-fflags", "+bitexact", "-flags:v", "+bitexact", "-flags:a", "+bitexact",
    "-c:v", "libvpx-vp9", "-pix_fmt", "yuv420p", "-deadline", "good", "-cpu-used", "0",
    "-crf", "36", "-b:v", "0", "-g", "24", "-row-mt", "0", "-threads", "1", "-lag-in-frames", "0",
    "-c:a", "libopus", "-b:a", "48k", "-map_metadata", "-1", "fixture.webm"
  ]);
  runFfmpeg(cwd, [
    "-f", "lavfi", "-i", "sine=frequency=523.25:sample_rate=44100:duration=4",
    "-fflags", "+bitexact", "-flags:a", "+bitexact",
    "-c:a", "libmp3lame", "-b:a", "64k", "-write_xing", "0",
    "-id3v2_version", "0", "-map_metadata", "-1", "fixture.mp3"
  ]);
}

function runFfmpeg(cwd: string, args: string[]) {
  const result = spawnSync("ffmpeg", ["-hide_banner", "-loglevel", "error", "-y", ...args], {
    cwd,
    encoding: "utf8"
  });
  if (result.status !== 0) {
    throw new Error(`ffmpeg fixture generation failed: ${result.stderr || result.error?.message || result.status}`);
  }
}
