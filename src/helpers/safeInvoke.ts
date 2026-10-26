import { Channel, invoke, InvokeOptions } from "@tauri-apps/api/core";
import {
  AgentApproval, AgentConfig, AgentHistoryEntry, AgentServerStatus, AgentSessionInfo,
  BatchOutcome, CatalogColumn, ColumnChoices, ColumnMeta, ConnectionProfile, DatabaseInfo, DatabaseKind, DatabaseSize, DesignOptions,
  HostKeyInfo, ObjectInfo, ObjectKind, ObjectStructure, PortForwardInfo, RowChange, SavedQuery, ServerInfo,
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
  "start_port_forward": Command<{ connectionId: string, localPort: number, secrets: Secrets }, PortForwardInfo>,
  "stop_port_forward": Command<{ connectionId: string }>,
  "list_port_forwards": Command<undefined, PortForwardInfo[]>,

  "open_session": Command<{ connectionId: string, secrets: Secrets }, SessionInfo>,
  "test_connection": Command<{ profile: ConnectionProfile, secrets: Secrets }, ServerInfo>,
  "close_session": Command<{ sessionId: string }>,
  "list_catalogs": Command<{ sessionId: string }, string[]>,
  "switch_catalog": Command<{ sessionId: string, catalog: string }, SessionInfo>,
  "list_databases": Command<{ sessionId: string }, DatabaseInfo[]>,
  "get_database_sizes": Command<{ sessionId: string }, DatabaseSize[]>,
  "list_objects": Command<{ sessionId: string, database: string }, ObjectInfo[]>,
  "get_object_structure": Command<{ sessionId: string, database: string, name: string, kind: ObjectKind, signature: string | null }, ObjectStructure>,
  "get_primary_key": Command<{ sessionId: string, database: string, table: string }, string[]>,
  "get_column_choices": Command<{ sessionId: string, database: string, table: string, column: string }, ColumnChoices | null>,
  "open_query_context": Command<{ sessionId: string, database: string | null }, number>,
  "close_query_context": Command<{ sessionId: string, contextId: number }>,
  "execute_query": Command<{ sessionId: string, contextId: number, sql: string, rowLimit: number, onOutcome: Channel<StatementOutcome> }, number>,
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

  "show_main_window": Command,

  "get_agent_config": Command<undefined, AgentConfig>,
  "set_agent_config": Command<{ config: AgentConfig }, AgentServerStatus>,
  "get_agent_status": Command<undefined, AgentServerStatus>,
  "list_agent_sessions": Command<undefined, AgentSessionInfo[]>,
  "close_agent_session": Command<{ id: string }>,
  "close_all_agent_sessions": Command,
  "list_agent_approvals": Command<undefined, AgentApproval[]>,
  "answer_agent_request": Command<{ id: string, approved: boolean }>,
  "list_agent_history": Command<{ from: number, to: number }, AgentHistoryEntry[]>,
  "get_agent_history_entry": Command<{ id: string, at: number }, AgentHistoryEntry | null>,
  "get_agent_history_size": Command<undefined, number>,
  "clear_agent_history": Command,
};
