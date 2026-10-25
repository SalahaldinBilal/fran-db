import { DatabaseKind } from "./connection";
import { AppError } from "./errors";

export type ServerInfo = {
  product: string,
  version: string,
  atomicDdl: boolean,
  transactionalDdl: boolean,
};

export type PortForwardInfo = {
  connectionId: string,
  localPort: number,
  host: string,
  port: number,
};

export type SessionInfo = {
  id: string,
  connectionId: string,
  kind: DatabaseKind,
  server: ServerInfo,
  catalog: string | null,
};

export type DatabaseInfo = {
  name: string,
  system: boolean,
};

export type ObjectKind =
  | "table" | "view" | "materializedView" | "sequence" | "procedure" | "function" | "trigger" | "event";

export type ObjectInfo = {
  name: string,
  kind: ObjectKind,
  signature: string | null,
  detail: string | null,
  rowEstimate: number | null,
  sizeBytes: number | null,
  comment: string | null,
};

export type ValueCategory =
  | "integer" | "decimal" | "float" | "boolean" | "text" | "binary"
  | "temporal" | "json" | "bit" | "spatial" | "other";

export type ColumnMeta = {
  name: string,
  typeName: string,
  category: ValueCategory,
  nullable: boolean,
  primaryKey: boolean,
  database: string | null,
  table: string | null,
  sourceName: string | null,
  hasChoices: boolean,
};

export type ColumnChoices = {
  multiple: boolean,
  values: string[],
};

export type ResultSet = {
  columns: ColumnMeta[],
  rows: Array<Array<string | null>>,
  truncated: boolean,
};

export type StatementOutcome = {
  sql: string,
  resultSets: ResultSet[],
  affectedRows: number,
  lastInsertId: number | null,
  warnings: number,
  info: string | null,
  elapsedMs: number,
  executionMs: number,
  networkMs: number | null,
  fetchMs: number,
  error: AppError | null,
};

export type StatementRange = {
  start: number,
  end: number,
  text: string,
};

export type ColumnDefinition = {
  name: string,
  dataType: string,
  nullable: boolean,
  defaultValue: string | null,
  primaryKey: boolean,
  extra: string | null,
  comment: string | null,
  collation: string | null,
};

export type IndexDefinition = {
  name: string,
  primary: boolean,
  unique: boolean,
  columns: string[],
  indexType: string | null,
};

export type ForeignKeyDefinition = {
  name: string,
  columns: string[],
  referencedDatabase: string,
  referencedTable: string,
  referencedColumns: string[],
  onUpdate: string | null,
  onDelete: string | null,
};

export type ObjectStructure = {
  columns: ColumnDefinition[],
  indexes: IndexDefinition[],
  foreignKeys: ForeignKeyDefinition[],
  ddl: string | null,
};

export type CatalogColumn = {
  table: string,
  name: string,
  dataType: string,
};

export type SqlTemplate = "create" | "drop" | "select" | "insert" | "update" | "delete";

export type CellEdit = {
  column: number,
  value: string | null,
};

export type RowChange =
  | { kind: "insert", values: CellEdit[] }
  | { kind: "update", key: Array<string | null>, values: CellEdit[] }
  | { kind: "delete", key: Array<string | null> };

export type BatchOutcome = {
  executed: number,
  failedIndex: number | null,
  error: AppError | null,
  rolledBack: boolean,
};

export type IndexKind = "primary" | "unique" | "index" | "fulltext" | "spatial";

export type IndexColumnDesign = {
  name: string,
  length: number | null,
};

export type ColumnDesign = {
  originalName: string | null,
  name: string,
  dataType: string,
  nullable: boolean,
  defaultValue: string | null,
  autoIncrement: boolean,
  onUpdate: string | null,
  comment: string,
  collation: string | null,
  generatedExpression: string | null,
  generatedStored: boolean,
};

export type IndexDesign = {
  originalName: string | null,
  name: string,
  kind: IndexKind,
  columns: IndexColumnDesign[],
  method: string | null,
  constraint: boolean,
};

export type ForeignKeyDesign = {
  originalName: string | null,
  name: string,
  columns: string[],
  referencedDatabase: string,
  referencedTable: string,
  referencedColumns: string[],
  onUpdate: string,
  onDelete: string,
};

export type TableDesign = {
  name: string,
  comment: string,
  engine: string | null,
  charset: string | null,
  collation: string | null,
  columns: ColumnDesign[],
  indexes: IndexDesign[],
  foreignKeys: ForeignKeyDesign[],
};

export type DesignCapabilities = {
  engines: boolean,
  charsets: boolean,
  columnCollations: boolean,
  onUpdate: boolean,
  indexLengths: boolean,
  reorderColumns: boolean,
  virtualColumns: boolean,
  unsigned: boolean,
  zerofill: boolean,
};

export type DesignOptions = {
  engines: string[],
  defaultEngine: string | null,
  charsets: { name: string, defaultCollation: string }[],
  collations: { name: string, charset: string }[],
  indexMethods: string[],
  capabilities: DesignCapabilities,
};

export type SavedQuery = {
  id: string,
  name: string,
  sql: string,
  connectionId: string | null,
  database: string | null,
  updatedAt: number,
};
