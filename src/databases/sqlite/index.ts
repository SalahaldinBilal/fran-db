import { SQLite } from "@codemirror/lang-sql";
import { DefaultKind } from "@core/helpers/defaultValues";
import { DatabaseModule } from "../types";
import SqliteConnectionForm from "./SqliteConnectionForm";

function quoteIdentifier(name: string): string {
  return `"${name.replaceAll("\"", "\"\"")}"`;
}

// SQLite's type affinity rules, which decide how values compare.
function keyTypeFamily(dataType: string): string {
  const upper = dataType.toUpperCase();
  if (upper.includes("INT")) return "integer";
  if (/CHAR|CLOB|TEXT/.test(upper)) return "text";
  if (upper.trim() === "" || upper.includes("BLOB")) return "blob";
  if (/REAL|FLOA|DOUB/.test(upper)) return "real";
  return "numeric";
}

function defaultKind(dataType: string): DefaultKind {
  const upper = dataType.toUpperCase();
  if (/DATE|TIME/.test(upper)) return "temporal";
  if (upper.includes("BOOL")) return "boolean";
  switch (keyTypeFamily(dataType)) {
    case "integer":
    case "real":
    case "numeric":
      return "number";
    case "text":
      return "text";
    default:
      return "other";
  }
}

function fileName(path: string): string {
  return path.split(/[\\/]/).filter(Boolean).pop() ?? "";
}

const sqliteModule: DatabaseModule<"sqlite"> = {
  kind: "sqlite",
  label: "SQLite",
  namespace: { singular: "database", plural: "databases" },
  searchPath: database => (database === "main" ? ["main"] : ["main", database]),
  isSystemTable: name => name.startsWith("sqlite_") || name.startsWith("pragma_"),
  catalogLabel: null,
  defaultOptions: () => ({
    path: "",
    readOnly: false,
    createIfMissing: false,
    foreignKeys: true,
    busyTimeoutSecs: 5,
    initSql: "",
  }),
  ConnectionForm: SqliteConnectionForm,
  describeTarget: options => options.path.trim() || "(no file)",
  suggestName: options => fileName(options.path.trim()) || "SQLite",
  methodLabel: options => (options.readOnly ? "File, read only" : "File"),
  sshTarget: () => null,
  defaultDatabase: () => "main",
  sqlDialect: SQLite,
  quoteIdentifier,
  selectRowsSql: (table, limit) => `SELECT * FROM ${quoteIdentifier(table)}${limit > 0 ? ` LIMIT ${limit}` : ""};`,
  dataTypes: [
    "INTEGER", "REAL", "TEXT", "BLOB", "NUMERIC",
    "BOOLEAN", "DATE", "DATETIME", "VARCHAR(255)", "DECIMAL(10,2)", "JSON",
  ],
  isTextType: dataType => /CHAR|CLOB|TEXT/i.test(dataType),
  keyTypeFamily,
  defaultKind,
  backslashEscapes: false,
  allowsUnsigned: () => false,
  referentialActions: ["NO ACTION", "RESTRICT", "CASCADE", "SET NULL", "SET DEFAULT"],
  indexKinds: [
    { value: "primary", label: "Primary" },
    { value: "unique", label: "Unique" },
    { value: "index", label: "Index" },
  ],
};

export default sqliteModule;
