import { MySQL } from "@codemirror/lang-sql";
import { MysqlOptions } from "@core/types";
import { DefaultKind } from "@core/helpers/defaultValues";
import { DatabaseModule } from "../types";
import MysqlConnectionForm from "./MysqlConnectionForm";

function quoteIdentifier(name: string): string {
  return `\`${name.replaceAll("`", "``")}\``;
}

const INTEGER_TYPES = new Set(["tinyint", "smallint", "mediumint", "int", "bigint"]);
const DECIMAL_TYPES = new Set(["decimal", "numeric", "dec", "fixed"]);

// Integer and decimal types must match in size, sign and precision. String lengths may differ.
function keyTypeFamily(dataType: string): string {
  const lower = dataType.trim().toLowerCase().replace(/\s+/g, " ");
  const unsigned = /\bunsigned\b/.test(lower) ? " unsigned" : "";
  const base = lower.replace(/\b(unsigned|signed|zerofill)\b/g, "").trim();
  const match = /^([a-z]+)\s*(?:\(([^)]*)\))?/.exec(base);
  if (!match) return base;

  const [, rawName, args = ""] = match;
  const name = rawName === "integer" ? "int" : rawName === "bool" || rawName === "boolean" ? "tinyint" : rawName;
  if (INTEGER_TYPES.has(name)) return `${name}${unsigned}`;
  if (DECIMAL_TYPES.has(name)) {
    const [precision = "10", scale = "0"] = args.split(",").map(part => part.trim());
    return `decimal(${precision},${scale})${unsigned}`;
  }
  if (name === "real") return "double";
  if (name === "char" || name === "varchar") return "string";
  if (name === "binary" || name === "varbinary") return "binary";
  if (name === "enum" || name === "set") return base;
  return name;
}

function defaultKind(dataType: string): DefaultKind {
  const name = /^[a-z]+/.exec(dataType.trim().toLowerCase())?.[0] ?? "";
  if (INTEGER_TYPES.has(name) || DECIMAL_TYPES.has(name)
    || ["integer", "bool", "boolean", "float", "double", "real", "bit", "year"].includes(name)) {
    return "number";
  }
  if (["char", "varchar", "tinytext", "text", "mediumtext", "longtext", "enum", "set", "json"].includes(name)) return "text";
  if (["date", "time", "datetime", "timestamp"].includes(name)) return "temporal";
  return "other";
}

const UNSIGNED_TYPES = new Set([...INTEGER_TYPES, ...DECIMAL_TYPES, "integer", "float", "double", "real"]);

function allowsUnsigned(dataType: string): boolean {
  return UNSIGNED_TYPES.has(/^[a-z]+/.exec(dataType.trim().toLowerCase())?.[0] ?? "");
}

function describeTarget(options: MysqlOptions): string {
  const user = options.user || "(no user)";
  switch (options.method) {
    case "tcp":
      return `${user}@${options.host}:${options.port}`;
    case "socket":
      return `${user}@${options.socketPath || "(no socket)"}`;
    case "sshTunnel":
      return `${user}@${options.host}:${options.port} via ${options.ssh.user}@${options.ssh.host}`;
  }
}

function suggestName(options: MysqlOptions): string {
  switch (options.method) {
    case "tcp":
      return options.host.trim() || "MySQL";
    case "socket":
      return "Local MySQL";
    case "sshTunnel":
      return options.ssh.host.trim() || options.host.trim() || "MySQL";
  }
}

function methodLabel(options: MysqlOptions): string {
  switch (options.method) {
    case "tcp":
      return "TCP/IP";
    case "socket":
      return "Socket";
    case "sshTunnel":
      return "SSH";
  }
}

const mysqlModule: DatabaseModule<"mysql"> = {
  kind: "mysql",
  label: "MySQL",
  namespace: { singular: "database", plural: "databases" },
  searchPath: database => [database],
  isSystemTable: () => false,
  catalogLabel: null,
  defaultOptions: () => ({
    method: "tcp",
    host: "127.0.0.1",
    port: 3306,
    socketPath: "",
    user: "root",
    defaultSchema: "",
    ssh: { host: "", port: 22, user: "", auth: "password", keyFile: "", agentSocket: "" },
    ssl: { mode: "preferred", caFile: "", certFile: "", keyFile: "" },
    compression: false,
    cleartextAuth: false,
    connectTimeoutSecs: 10,
    initSql: "",
  }),
  ConnectionForm: MysqlConnectionForm,
  describeTarget,
  suggestName,
  methodLabel,
  sshTarget: options => (options.method === "sshTunnel" ? { host: options.host, port: options.port } : null),
  defaultDatabase: options => options.defaultSchema.trim() || null,
  sqlDialect: MySQL,
  quoteIdentifier,
  selectRowsSql: table => `SELECT * FROM ${quoteIdentifier(table)};`,
  dataTypes: [
    "INT", "INT UNSIGNED", "BIGINT", "BIGINT UNSIGNED", "TINYINT", "TINYINT(1)", "SMALLINT", "MEDIUMINT",
    "DECIMAL(10,2)", "FLOAT", "DOUBLE", "BIT(1)",
    "CHAR(36)", "VARCHAR(45)", "VARCHAR(255)", "TINYTEXT", "TEXT", "MEDIUMTEXT", "LONGTEXT",
    "BINARY(16)", "VARBINARY(255)", "BLOB", "MEDIUMBLOB", "LONGBLOB",
    "DATE", "TIME", "DATETIME", "TIMESTAMP", "YEAR", "JSON", "ENUM('a','b')", "SET('a','b')",
    "GEOMETRY", "POINT",
  ],
  isTextType: dataType => /char|text|enum|set/i.test(dataType),
  keyTypeFamily,
  defaultKind,
  backslashEscapes: true,
  allowsUnsigned,
  referentialActions: ["RESTRICT", "CASCADE", "SET NULL", "NO ACTION"],
  indexKinds: [
    { value: "primary", label: "Primary" },
    { value: "unique", label: "Unique" },
    { value: "index", label: "Index" },
    { value: "fulltext", label: "Fulltext" },
    { value: "spatial", label: "Spatial" },
  ],
};

export default mysqlModule;
