export const WORD_OPENXML_EXTENSIONS = ["docx", "docm", "dotx", "dotm"] as const;
export const OPEN_DOCUMENT_EXTENSIONS = ["rtf", "odt", "odp"] as const;
export const SPREADSHEET_EXTENSIONS = [
  "xlsx", "xlsm", "xlsb", "xltx", "xltm", "ods"
] as const;
export const PRESENTATION_EXTENSIONS = [
  "pptx", "pptm", "potx", "potm", "ppsx", "ppsm"
] as const;
export const FILE_VIEWER_ASSET_FORMATS = [
  "pdf",
  ...WORD_OPENXML_EXTENSIONS,
  ...OPEN_DOCUMENT_EXTENSIONS,
  ...SPREADSHEET_EXTENSIONS,
  ...PRESENTATION_EXTENSIONS,
  "ofd",
  "heic",
  "heif"
] as const;
