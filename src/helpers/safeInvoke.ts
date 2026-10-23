import { invoke, InvokeOptions } from "@tauri-apps/api/core";
import {
  BatchOutcome, CatalogColumn, ColumnChoices, ColumnMeta, ConnectionProfile, DatabaseInfo, DatabaseKind, DesignOptions,
  HostKeyInfo, ObjectInfo, ObjectKind, ObjectStructure, ResultSet, RowChange, SavedQuery, ServerInfo,
  SessionInfo, SqlTemplate, StatementOutcome, StatementRange, TableDesign, VaultStatus,
} from "@core/types";

export async function safeInvoke<T extends keyof Commands>(
  command: T,
  ...[args, options]: SafeInvokeArgs<Commands[T]["parameters"]>
): Promise<Commands[T]["return"]> {
  return await invoke(command, args, options);
}

type SafeInvokeArgs<T> =
  undefined extends T ? [args?: T, options?: InvokeOptions] : [args: T, options?: InvokeOptions];

type Command<P extends object | undefined = undefined, R = undefined> = { parameters: P, return: R };

type Secrets = Record<string, string>;

type Commands = {
  "get_vault_status": Command<undefined, VaultStatus>,
  "reset_vault": Command<undefined, VaultStatus>,

  "list_connections": Command<undefined, ConnectionProfile[]>,
  "save_connection": Command<{ profile: ConnectionProfile }, ConnectionProfile>,
  "duplicate_connection": Command<{ id: string }, ConnectionProfile>,
  "delete_connection": Command<{ id: string }>,
  "get_stored_secrets": Command<{ connectionId: string }, string[]>,
  "set_connection_secret": Command<{ connectionId: string, slot: string, value: string }>,
  "clear_connection_secret": Command<{ connectionId: string, slot: string }>,
  "trust_host_key": Command<{ key: HostKeyInfo }>,

  "open_session": Command<{ connectionId: string, secrets: Secrets }, SessionInfo>,
  "test_connection": Command<{ profile: ConnectionProfile, secrets: Secrets }, ServerInfo>,
  "close_session": Command<{ sessionId: string }>,
  "list_catalogs": Command<{ sessionId: string }, string[]>,
  "switch_catalog": Command<{ sessionId: string, catalog: string }, SessionInfo>,
  "list_databases": Command<{ sessionId: string }, DatabaseInfo[]>,
  "list_objects": Command<{ sessionId: string, database: string }, ObjectInfo[]>,
  "get_object_structure": Command<{ sessionId: string, database: string, name: string, kind: ObjectKind, signature: string | null }, ObjectStructure>,
  "fetch_table_rows": Command<{ sessionId: string, database: string, table: string, offset: number, limit: number, search: string | null }, ResultSet>,
  "get_primary_key": Command<{ sessionId: string, database: string, table: string }, string[]>,
  "get_column_choices": Command<{ sessionId: string, database: string, table: string, column: string }, ColumnChoices | null>,
  "open_query_context": Command<{ sessionId: string, database: string | null }, number>,
  "close_query_context": Command<{ sessionId: string, contextId: number }>,
  "execute_query": Command<{ sessionId: string, contextId: number, sql: string, rowLimit: number }, StatementOutcome[]>,
  "cancel_query": Command<{ sessionId: string, contextId: number }>,
  "split_sql": Command<{ kind: DatabaseKind, sql: string }, StatementRange[]>,
  "list_columns": Command<{ sessionId: string, database: string }, CatalogColumn[]>,
  "get_object_sql": Command<{ sessionId: string, database: string, name: string, kind: ObjectKind, signature: string | null, template: SqlTemplate }, string>,
  "get_database_sql": Command<{ sessionId: string, database: string, template: SqlTemplate }, string>,
  "preview_row_changes": Command<{ sessionId: string, database: string, table: string, columns: ColumnMeta[], changes: RowChange[] }, string[]>,
  "execute_batch": Command<{ sessionId: string, database: string | null, statements: string[], transactional: boolean }, BatchOutcome>,
  "get_table_design": Command<{ sessionId: string, database: string, table: string }, TableDesign>,
  "get_design_options": Command<{ sessionId: string }, DesignOptions>,
  "preview_table_design": Command<{ sessionId: string, database: string, original: TableDesign | null, design: TableDesign }, string[]>,

  "list_saved_queries": Command<undefined, SavedQuery[]>,
  "save_query": Command<{ query: SavedQuery }, SavedQuery>,
  "delete_saved_query": Command<{ id: string }>,

  "get_tab_state": Command<{ connectionId: string }, unknown>,
  "set_tab_state": Command<{ connectionId: string, state: unknown }>,

  "get_settings": Command<undefined, unknown>,
  "set_settings": Command<{ settings: unknown }>,
};
