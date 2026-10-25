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

// Keeps a profile's kind and options paired so callers never cast.
export function withDatabaseModule<K extends DatabaseKind, R>(
  driver: { kind: K, options: OptionsByKind[K] },
  use: (module: DatabaseModule<K>, options: OptionsByKind[K]) => R,
): R {
  return use(modules[driver.kind], driver.options);
}
