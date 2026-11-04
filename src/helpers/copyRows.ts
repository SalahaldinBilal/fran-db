import { ColumnMeta } from "@core/types";

export type CopyFormat = "tsv" | "csv" | "json" | "markdown";

type Row = Array<string | null>;

const JSON_NUMBER = /^-?(0|[1-9]\d*)(\.\d+)?([eE][+-]?\d+)?$/;

function quoted(value: string): string {
  return `"${value.replace(/"/g, '""')}"`;
}

// Spreadsheets read quoted fields in pasted tab-separated text the same way they do in CSV.
function tsvField(value: string | null): string {
  if (value === null) return "NULL";
  return /[\t\r\n"]/.test(value) ? quoted(value) : value;
}

function csvField(value: string | null): string {
  if (value === null) return "";
  return /[,\r\n"]/.test(value) ? quoted(value) : value;
}

function markdownField(value: string | null): string {
  if (value === null) return "NULL";
  return value.replace(/\\/g, "\\\\").replace(/\|/g, "\\|").replace(/\r?\n/g, "<br>");
}

function jsonValue(value: string | null, column: ColumnMeta): string {
  if (value === null) return "null";
  switch (column.category) {
    case "integer":
    case "decimal":
    case "float":
      return JSON_NUMBER.test(value) ? value : JSON.stringify(value);
    case "boolean":
      if (/^(t|true|1)$/i.test(value)) return "true";
      if (/^(f|false|0)$/i.test(value)) return "false";
      return JSON.stringify(value);
    case "json":
      try {
        return JSON.stringify(JSON.parse(value));
      } catch {
        return JSON.stringify(value);
      }
    default:
      return JSON.stringify(value);
  }
}

function uniqueNames(columns: ColumnMeta[]): string[] {
  const seen = new Map<string, number>();
  return columns.map(column => {
    const count = (seen.get(column.name) ?? 0) + 1;
    seen.set(column.name, count);
    return count === 1 ? column.name : `${column.name}_${count}`;
  });
}

function delimited(columns: ColumnMeta[], rows: Row[], separator: string, field: (value: string | null) => string, headers: boolean): string {
  const lines = rows.map(row => row.map(field).join(separator));
  if (headers) lines.unshift(columns.map(column => field(column.name)).join(separator));
  return lines.join("\r\n");
}

function markdown(columns: ColumnMeta[], rows: Row[]): string {
  const line = (cells: string[]) => `| ${cells.join(" | ")} |`;
  return [
    line(columns.map(column => markdownField(column.name))),
    line(columns.map(column => (["integer", "decimal", "float"].includes(column.category) ? "---:" : "---"))),
    ...rows.map(row => line(row.map(markdownField))),
  ].join("\n");
}

function json(columns: ColumnMeta[], rows: Row[]): string {
  const names = uniqueNames(columns).map(name => JSON.stringify(name));
  const objects = rows.map(row => `  {${row.map((value, index) => `${names[index]}: ${jsonValue(value, columns[index])}`).join(", ")}}`);
  return `[\n${objects.join(",\n")}\n]`;
}

export function formatRows(format: CopyFormat, columns: ColumnMeta[], rows: Row[], headers = false): string {
  switch (format) {
    case "tsv":
      return delimited(columns, rows, "\t", tsvField, headers);
    case "csv":
      return delimited(columns, rows, ",", csvField, true);
    case "markdown":
      return markdown(columns, rows);
    case "json":
      return json(columns, rows);
  }
}
