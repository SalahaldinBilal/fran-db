import { AgentApproval, AgentHistoryEntry, AgentMode, ColumnMeta } from "@core/types";
import { plural } from "@core/helpers";

export const DAY_MS = 86_400_000;

const CHANGE_TOOLS = new Set(["insert_rows", "update_rows", "delete_rows", "create_table", "alter_table", "drop_object", "execute_sql"]);

const MARK_COLORS = ["#c26a45", "#4a6fd1", "#4fa37a", "#9a6fc4", "#c9a34a", "#3f9bab", "#c25a78"];

export const MODE_LABELS: Record<AgentMode, string> = {
  off: "not shared anymore",
  readOnly: "read only",
  ask: "read and ask to change",
};

export type OutcomeTone = "ok" | "fail" | "warn";

export type CallSummary = { code: string | null, text: string | null };

export type ReplyTable = { columns: ColumnMeta[], rows: Array<Array<string | null>>, truncated: boolean };

export function startOfDay(at: number): number {
  const date = new Date(at);
  date.setHours(0, 0, 0, 0);
  return date.getTime();
}

export function formatClock(at: number, seconds = true): string {
  return new Date(at).toLocaleTimeString(undefined, {
    hour: "2-digit",
    minute: "2-digit",
    second: seconds ? "2-digit" : undefined,
  });
}

export function agentColor(name: string): string {
  let hash = 0;
  for (const character of name) hash = (hash * 31 + character.charCodeAt(0)) >>> 0;
  return MARK_COLORS[hash % MARK_COLORS.length];
}

export function agentInitial(name: string): string {
  return name.trim().charAt(0).toUpperCase() || "?";
}

export function where(connectionName: string | null, database: string | null): string {
  const name = connectionName ?? "No connection";
  return database ? `${name} / ${database}` : name;
}

export function lowerFirst(text: string): string {
  return text.charAt(0).toLowerCase() + text.slice(1);
}

export function isChange(entry: AgentHistoryEntry): boolean {
  return entry.approval !== null || CHANGE_TOOLS.has(entry.tool ?? "");
}

export function outcome(entry: AgentHistoryEntry): { label: string, tone: OutcomeTone } {
  if (entry.approval?.answer === "rejected") return { label: "Rejected", tone: "warn" };
  if (entry.approval?.answer === "timedOut") return { label: "No answer", tone: "warn" };
  switch (entry.outcome) {
    case "ok": return { label: "Done", tone: "ok" };
    case "error": return { label: "Failed", tone: "fail" };
    case "refused": return { label: "Refused", tone: "warn" };
    case "cancelled": return { label: "Cancelled", tone: "warn" };
    case "timedOut": return { label: "Timed out", tone: "warn" };
  }
}

function argumentsOf(value: unknown): Record<string, unknown> {
  return value && typeof value === "object" ? value as Record<string, unknown> : {};
}

function text(value: unknown): string | null {
  return typeof value === "string" && value.trim() ? value : null;
}

function oneLine(sql: string): string {
  return sql.trim().replace(/\s+/g, " ");
}

function rowCount(args: Record<string, unknown>): string | null {
  const rows = Array.isArray(args.rows) ? args.rows : Array.isArray(args.keys) ? args.keys : null;
  return rows ? plural(rows.length, "row") : null;
}

export function tableOf(args: unknown): string | null {
  const given = argumentsOf(args);
  return text(given.table) ?? text(given.name) ?? text(argumentsOf(given.design).name);
}

function describeCall(tool: string, args: Record<string, unknown>, connectionName: string | null): CallSummary {
  switch (tool) {
    case "open_session": {
      const database = text(args.database);
      return { code: null, text: `${connectionName ?? text(args.connection) ?? "A connection"}${database ? `, database ${database}` : ""}` };
    }
    case "run_query":
    case "explain_query":
    case "execute_sql":
      return { code: oneLine(text(args.sql) ?? ""), text: null };
    case "search_schema":
      return { code: text(args.pattern), text: null };
    case "insert_rows":
    case "update_rows":
    case "delete_rows":
      return { code: text(args.table), text: rowCount(args) };
    case "column_values":
      return { code: [text(args.table), text(args.column)].filter(Boolean).join("."), text: null };
    default:
      return { code: tableOf(args) ?? text(args.database), text: null };
  }
}

export function callSummary(entry: AgentHistoryEntry): CallSummary {
  const summary = describeCall(entry.tool ?? "", argumentsOf(entry.arguments), entry.connectionName);
  if (entry.outcome === "ok" || !entry.message) return summary;
  const message = entry.message.split("\n")[0];
  return { code: summary.code, text: summary.text ? `${summary.text}, ${lowerFirst(message)}` : message };
}

export function approvalSummary(approval: AgentApproval): CallSummary {
  return describeCall(approval.tool, argumentsOf(approval.arguments), approval.connectionName);
}

function asTable(value: unknown): ReplyTable | null {
  if (!value || typeof value !== "object") return null;
  const { columns, rows, truncated } = value as Record<string, unknown>;
  if (!Array.isArray(columns) || !Array.isArray(rows)) return null;
  return {
    columns: columns.map(column => ({
      name: String((column as { name?: unknown }).name ?? ""),
      typeName: String((column as { type?: unknown }).type ?? ""),
      category: "text",
      nullable: true,
      primaryKey: false,
      database: null,
      table: null,
      sourceName: null,
      hasChoices: false,
    })),
    rows: rows.map(row => (Array.isArray(row) ? row.map(cell => (cell === null ? null : String(cell))) : [])),
    truncated: truncated === true,
  };
}

// Result tables anywhere in a reply: read_rows returns one, run_query one per statement, explain_query a plan.
export function replyTables(reply: unknown): ReplyTable[] {
  const direct = asTable(reply);
  if (direct) return [direct];
  if (!reply || typeof reply !== "object") return [];
  const { statements, plan } = reply as Record<string, unknown>;
  const fromStatements = Array.isArray(statements)
    ? statements.flatMap(statement => {
      const results = (statement as { results?: unknown }).results;
      return Array.isArray(results) ? results.map(asTable).filter(table => table !== null) : [];
    })
    : [];
  const planTable = asTable(plan);
  return planTable ? [...fromStatements, planTable] : fromStatements;
}
