import { DatabaseKind, DriverOptions, OptionsByKind } from "@core/types";
import { DatabaseModule } from "./types";
import mysqlModule from "./mysql";
import postgresModule from "./postgres";
import sqliteModule from "./sqlite";
import d1Module from "./d1";

const modules: { [K in DatabaseKind]: DatabaseModule<K> } = {
  mysql: mysqlModule,
  postgres: postgresModule,
  sqlite: sqliteModule,
  d1: d1Module,
};

const driverDefaults: { [K in DatabaseKind]: () => { kind: K, options: OptionsByKind[K] } } = {
  mysql: () => ({ kind: "mysql", options: mysqlModule.defaultOptions() }),
  postgres: () => ({ kind: "postgres", options: postgresModule.defaultOptions() }),
  sqlite: () => ({ kind: "sqlite", options: sqliteModule.defaultOptions() }),
  d1: () => ({ kind: "d1", options: d1Module.defaultOptions() }),
};

export const databaseModules: { kind: DatabaseKind, label: string }[] = Object.values(modules);

export function defaultDriver(kind: DatabaseKind): DriverOptions {
  return driverDefaults[kind]();
}

export function databaseModule<K extends DatabaseKind>(kind: K): DatabaseModule<K> {
  return modules[kind];
}

export type ParsedDriver = {
  driver: { kind: DatabaseKind, options: DriverOptions["options"] },
  password: string | null,
  filled: string[],
  ignored: string[],
};

function parseAs<K extends DatabaseKind>(kind: K, text: string, current: OptionsByKind[K]): ParsedDriver | null {
  const parsed = modules[kind].parseConnectionString?.(text, current);
  return parsed ? { driver: { kind, options: parsed.options }, password: parsed.password, filled: parsed.filled, ignored: parsed.ignored } : null;
}

// Tries the form's own kind first, so its other settings (SSH, SSL files) are kept when the string is for the same kind.
export function parseConnectionString<K extends DatabaseKind>(text: string, current: { kind: K, options: OptionsByKind[K] }): ParsedDriver | null {
  if (!text.trim()) return null;
  const own = parseAs(current.kind, text, current.options);
  if (own) return own;
  for (const kind of Object.keys(modules) as DatabaseKind[]) {
    if (kind === current.kind) continue;
    const driver = defaultDriver(kind);
    const parsed = parseAs(driver.kind, text, driver.options);
    if (parsed) return parsed;
  }
  return null;
}

// Keeps a profile's kind and options paired so callers never cast.
export function withDatabaseModule<K extends DatabaseKind, R>(
  driver: { kind: K, options: OptionsByKind[K] },
  use: (module: DatabaseModule<K>, options: OptionsByKind[K]) => R,
): R {
  return use(modules[driver.kind], driver.options);
}
