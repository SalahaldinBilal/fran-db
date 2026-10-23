import { SQLNamespace } from "@codemirror/lang-sql";
import { SessionState } from "@core/states/workspaceState";
import { SchemaCatalog } from "@core/components/SqlEditor/schemaLinter";
import { databaseModule } from "@core/databases";

// Null until the current database's object list is loaded, so nothing gets flagged early.
export function buildCatalog(session: SessionState, database: string): SchemaCatalog | null {
  if (session.objects[database]?.status !== "ready") return null;

  const tables = new Map<string, Map<string, Set<string> | null>>();
  for (const [name, objects] of Object.entries(session.objects)) {
    if (objects.status !== "ready") continue;

    const columns = session.columns[name];
    const columnsByTable = new Map<string, Set<string>>();
    if (columns?.status === "ready") {
      for (const column of columns.items) {
        const table = column.table.toLowerCase();
        if (!columnsByTable.has(table)) columnsByTable.set(table, new Set());
        columnsByTable.get(table)!.add(column.name.toLowerCase());
      }
    }

    const entries = new Map<string, Set<string> | null>();
    for (const object of objects.items) {
      if (object.kind !== "table" && object.kind !== "view") continue;
      const key = object.name.toLowerCase();
      entries.set(key, columns?.status === "ready" ? columnsByTable.get(key) ?? new Set() : null);
    }
    tables.set(name.toLowerCase(), entries);
  }

  const module = databaseModule(session.info.kind);
  return {
    database: database.toLowerCase(),
    databases: new Set(session.databases.items.map(item => item.name.toLowerCase())),
    tables,
    searchPath: module.searchPath(database).map(name => name.toLowerCase()),
    namespace: module.namespace.singular,
    isSystemTable: module.isSystemTable,
  };
}

export function buildCompletionSchema(session: SessionState): SQLNamespace {
  const schema: Record<string, Record<string, string[]>> = {};

  for (const [name, objects] of Object.entries(session.objects)) {
    if (objects.status !== "ready") continue;

    const columns = session.columns[name]?.items ?? [];
    const tables: Record<string, string[]> = {};
    for (const object of objects.items) {
      if (object.kind === "table" || object.kind === "view") tables[object.name] = [];
    }
    for (const column of columns) {
      tables[column.table]?.push(column.name);
    }
    schema[name] = tables;
  }

  for (const database of session.databases.items) {
    schema[database.name] ??= {};
  }

  return schema;
}
