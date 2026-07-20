export type DelimitedTableLimits = {
  maxCells: number;
  maxColumns: number;
  maxRows: number;
};

export type DelimitedTablePreview = {
  limits: DelimitedTableLimits;
  rows: string[][];
  truncated: boolean;
};

export const DELIMITED_TABLE_LIMITS: DelimitedTableLimits = {
  maxCells: 20_000,
  maxColumns: 100,
  maxRows: 2_000
};

export function parseDelimitedText(
  source: string,
  delimiter: string,
  requestedLimits: DelimitedTableLimits = DELIMITED_TABLE_LIMITS
): DelimitedTablePreview {
  const limits = normalizedLimits(requestedLimits);
  const rows: string[][] = [];
  let row: string[] = [];
  let field = "";
  let quoted = false;
  let cellCount = 0;
  let truncated = false;

  const commitField = (stripCarriageReturn: boolean): boolean => {
    if (row.length >= limits.maxColumns) {
      truncated = true;
      return true;
    }
    if (cellCount >= limits.maxCells) {
      truncated = true;
      return false;
    }
    row.push(stripCarriageReturn ? field.replace(/\r$/, "") : field);
    cellCount += 1;
    return true;
  };
  const commitPartialRow = () => {
    if (row.length > 0 && rows.length < limits.maxRows) {
      rows.push(row);
    }
  };
  const commitRow = (hasMoreInput: boolean): boolean => {
    if (rows.length >= limits.maxRows) {
      truncated = true;
      return false;
    }
    rows.push(row);
    row = [];
    if (hasMoreInput && rows.length >= limits.maxRows) {
      truncated = true;
      return false;
    }
    return true;
  };

  for (let index = 0; index < source.length; index += 1) {
    const character = source[index] ?? "";
    if (quoted) {
      if (character === '"' && source[index + 1] === '"') {
        field += '"';
        index += 1;
      } else if (character === '"') {
        quoted = false;
      } else {
        field += character;
      }
      continue;
    }
    if (character === '"' && field.length === 0) {
      quoted = true;
    } else if (character === delimiter) {
      if (!commitField(false)) {
        commitPartialRow();
        return { limits, rows, truncated };
      }
      field = "";
    } else if (character === "\n") {
      if (!commitField(true)) {
        commitPartialRow();
        return { limits, rows, truncated };
      }
      field = "";
      if (!commitRow(index + 1 < source.length)) {
        return { limits, rows, truncated };
      }
    } else {
      field += character;
    }
  }
  if (field.length > 0 || row.length > 0) {
    if (!commitField(true)) {
      commitPartialRow();
      return { limits, rows, truncated };
    }
    commitRow(false);
  }
  return {
    limits,
    rows: rows.length > 0 ? rows : [[""]],
    truncated
  };
}

function normalizedLimits(limits: DelimitedTableLimits): DelimitedTableLimits {
  return {
    maxCells: positiveInteger(limits.maxCells),
    maxColumns: positiveInteger(limits.maxColumns),
    maxRows: positiveInteger(limits.maxRows)
  };
}

function positiveInteger(value: number): number {
  return Math.max(1, Math.floor(value));
}
