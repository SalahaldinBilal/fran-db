import { PostgreSQL } from "@codemirror/lang-sql";
import { PostgresOptions } from "@core/types";
import { DefaultKind } from "@core/helpers/defaultValues";
import { DatabaseModule } from "../types";
import PostgresConnectionForm from "./PostgresConnectionForm";

function quoteIdentifier(name: string): string {
  return `"${name.replaceAll("\"", "\"\"")}"`;
}

const TYPE_FAMILIES: Record<string, string> = {
  smallint: "integer", int2: "integer", smallserial: "integer", serial2: "integer",
  integer: "integer", int: "integer", int4: "integer", serial: "integer", serial4: "integer",
  bigint: "integer", int8: "integer", bigserial: "integer", serial8: "integer",
  real: "float", float4: "float", "double precision": "float", float8: "float", float: "float",
  numeric: "numeric", decimal: "numeric",
  text: "text", varchar: "text", "character varying": "text", char: "text", character: "text", bpchar: "text", name: "text",
  boolean: "boolean", bool: "boolean",
  timestamp: "timestamp", "timestamp without time zone": "timestamp",
  timestamptz: "timestamptz", "timestamp with time zone": "timestamptz",
  time: "time", "time without time zone": "time",
  timetz: "timetz", "time with time zone": "timetz",
};

// Postgres compares across all integer types and across the string types, anything else needs the same type.
function keyTypeFamily(dataType: string): string {
  const lower = dataType.trim().toLowerCase().replace(/\s+/g, " ");
  if (lower.endsWith("[]")) return `${keyTypeFamily(lower.slice(0, -2))}[]`;
  const base = lower.replace(/\([^)]*\)/g, "").replace(/\s+/g, " ").trim().replaceAll("\"", "");
  const unqualified = base.slice(base.lastIndexOf(".") + 1);
  return TYPE_FAMILIES[unqualified] ?? unqualified;
}

const DEFAULT_KINDS: Record<string, DefaultKind> = {
  integer: "number", float: "number", numeric: "number", money: "number",
  text: "text", citext: "text", uuid: "text", json: "text", jsonb: "text", xml: "text", inet: "text", cidr: "text",
  macaddr: "text", bytea: "text",
  boolean: "boolean",
  date: "temporal", timestamp: "temporal", timestamptz: "temporal", time: "temporal", timetz: "temporal",
  interval: "temporal",
};

function defaultKind(dataType: string): DefaultKind {
  const family = keyTypeFamily(dataType);
  return family.endsWith("[]") ? "other" : DEFAULT_KINDS[family] ?? "other";
}

function describeTarget(options: PostgresOptions): string {
  const user = options.user || "(no user)";
  const database = options.database || user;
  switch (options.method) {
    case "tcp":
      return `${user}@${options.host}:${options.port}/${database}`;
    case "socket":
      return `${user}@${options.socketPath || "(no socket)"}/${database}`;
    case "sshTunnel":
      return `${user}@${options.host}:${options.port}/${database} via ${options.ssh.user}@${options.ssh.host}`;
  }
}

function suggestName(options: PostgresOptions): string {
  switch (options.method) {
    case "tcp":
      return options.host.trim() || "PostgreSQL";
    case "socket":
      return "Local PostgreSQL";
    case "sshTunnel":
      return options.ssh.host.trim() || options.host.trim() || "PostgreSQL";
  }
}

function methodLabel(options: PostgresOptions): string {
  switch (options.method) {
    case "tcp":
      return "TCP/IP";
    case "socket":
      return "Socket";
    case "sshTunnel":
      return "SSH";
  }
}

const postgresModule: DatabaseModule<"postgres"> = {
  kind: "postgres",
  label: "PostgreSQL",
  namespace: { singular: "schema", plural: "schemas" },
  searchPath: schema => (schema === "public" ? ["public"] : [schema, "public"]),
  isSystemTable: name => name.startsWith("pg_"),
  catalogLabel: "Database",
  defaultOptions: () => ({
    method: "tcp",
    host: "127.0.0.1",
    port: 5432,
    socketPath: "",
    user: "postgres",
    database: "postgres",
    defaultSchema: "public",
    ssh: { host: "", port: 22, user: "", auth: "password", keyFile: "", agentSocket: "" },
    ssl: { mode: "prefer", caFile: "", certFile: "", keyFile: "" },
    connectTimeoutSecs: 10,
    initSql: "",
  }),
  ConnectionForm: PostgresConnectionForm,
  describeTarget,
  suggestName,
  methodLabel,
  sshTarget: options => (options.method === "sshTunnel" ? { host: options.host, port: options.port } : null),
  defaultDatabase: options => options.defaultSchema.trim() || null,
  sqlDialect: PostgreSQL,
  quoteIdentifier,
  selectRowsSql: table => `SELECT * FROM ${quoteIdentifier(table)};`,
  dataTypes: [
    "integer", "bigint", "smallint", "serial", "bigserial", "numeric(10,2)", "real", "double precision", "boolean",
    "text", "varchar(255)", "char(10)", "citext", "uuid", "bytea",
    "date", "time", "timetz", "timestamp", "timestamptz", "interval",
    "json", "jsonb", "xml", "inet", "cidr", "macaddr", "money", "tsvector",
    "integer[]", "text[]", "int4range", "tstzrange",
  ],
  isTextType: dataType => /char|text|name/i.test(dataType),
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

export default postgresModule;
