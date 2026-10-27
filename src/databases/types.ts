import { Component } from "solid-js";
import { SQLDialect } from "@codemirror/lang-sql";
import { DatabaseKind, IndexKind, OptionsByKind } from "@core/types";
import { DefaultKind } from "@core/helpers/defaultValues";

// undefined means untouched, null means remove from the vault on save.
export type PendingSecret = string | null | undefined;

export type SecretEditor = {
  isStored: (slot: string) => boolean,
  pending: (slot: string) => PendingSecret,
  setPending: (slot: string, value: PendingSecret) => void,
};

export type ConnectionFormProps<Options> = {
  options: Options,
  update: (mutate: (options: Options) => void) => void,
  secrets: SecretEditor,
};

export type DatabaseModule<K extends DatabaseKind> = {
  kind: K,
  label: string,
  // What the tree's top level holds: MySQL databases, or the schemas of one Postgres database.
  namespace: { singular: string, plural: string },
  // Where unqualified table names are looked up, first match wins.
  searchPath: (database: string) => string[],
  // Built-in tables any statement can use unqualified, which the schema checker shouldn't flag.
  isSystemTable: (name: string) => boolean,
  // Set when a session works in one of the server's databases and can switch to another.
  catalogLabel: string | null,
  defaultOptions: () => OptionsByKind[K],
  ConnectionForm: Component<ConnectionFormProps<OptionsByKind[K]>>,
  describeTarget: (options: OptionsByKind[K]) => string,
  suggestName: (options: OptionsByKind[K]) => string,
  methodLabel: (options: OptionsByKind[K]) => string,
  // The database endpoint behind the SSH tunnel, when the connection uses one. Port forwards go there.
  sshTarget: (options: OptionsByKind[K]) => { host: string, port: number } | null,
  defaultDatabase: (options: OptionsByKind[K]) => string | null,
  sqlDialect: SQLDialect,
  quoteIdentifier: (name: string) => string,
  // A limit of 0 means no LIMIT clause.
  selectRowsSql: (table: string, limit: number) => string,
  // Throws when the SQL can't be parsed.
  formatSql: (sql: string) => string,
  dataTypes: readonly string[],
  isTextType: (dataType: string) => boolean,
  // Two columns can be joined by a foreign key when their types map to the same family.
  keyTypeFamily: (dataType: string) => string,
  // What literal a column's DEFAULT should be, for the designer's check.
  defaultKind: (dataType: string) => DefaultKind,
  // Whether a backslash escapes the next character inside string literals.
  backslashEscapes: boolean,
  // Whether UNSIGNED and ZEROFILL can follow this type.
  allowsUnsigned: (dataType: string) => boolean,
  referentialActions: readonly string[],
  indexKinds: readonly { value: IndexKind, label: string }[],
};
