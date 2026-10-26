import { ClipboardCopy, Copy, FileCode, PencilRuler, RefreshCw, SquarePen, TableProperties } from "lucide-solid";
import { MenuItem } from "@core/states/contextMenuState";
import useWorkspaceState from "@core/states/workspaceState";
import useToastState from "@core/states/toastState";
import { objectKindInfo } from "@core/constants/objectKinds";
import { safeInvoke } from "@core/helpers/safeInvoke";
import { copyText } from "@core/helpers/clipboard";
import { ObjectInfo, SqlTemplate } from "@core/types";

const ROW_TEMPLATES: { template: SqlTemplate, label: string }[] = [
  { template: "select", label: "Select statement" },
  { template: "insert", label: "Insert statement" },
  { template: "update", label: "Update statement" },
  { template: "delete", label: "Delete statement" },
];

async function copyObjectSql(sessionId: string, database: string, object: ObjectInfo, template: SqlTemplate) {
  try {
    const sql = await safeInvoke("get_object_sql", {
      sessionId,
      database,
      name: object.name,
      kind: object.kind,
      signature: object.signature,
      template,
    });
    await copyText(sql, "statement");
  } catch (error) {
    useToastState.pushError(error);
  }
}

async function copyDatabaseSql(sessionId: string, database: string, template: SqlTemplate) {
  try {
    await copyText(await safeInvoke("get_database_sql", { sessionId, database, template }), "statement");
  } catch (error) {
    useToastState.pushError(error);
  }
}

export function databaseMenu(sessionId: string, database: string): MenuItem[] {
  const workspace = useWorkspaceState;
  return [
    { label: "New query", icon: SquarePen, onSelect: () => workspace.newQueryTab(sessionId, database) },
    { label: "Create table", icon: TableProperties, onSelect: () => workspace.openDesigner(sessionId, database, null) },
    { separator: true },
    {
      label: "Copy SQL",
      icon: ClipboardCopy,
      children: [
        { label: "Name", onSelect: () => copyText(database, "name") },
        { label: "Create statement", onSelect: () => copyDatabaseSql(sessionId, database, "create") },
        { label: "Drop statement", onSelect: () => copyDatabaseSql(sessionId, database, "drop") },
      ],
    },
    { label: "Refresh", icon: RefreshCw, onSelect: () => workspace.refreshCatalog(sessionId, database) },
  ];
}

export function folderMenu(sessionId: string, database: string, tables: boolean): MenuItem[] {
  const workspace = useWorkspaceState;
  const items: MenuItem[] = [];
  if (tables) {
    items.push({ label: "Create table", icon: TableProperties, onSelect: () => workspace.openDesigner(sessionId, database, null) });
  }
  items.push({ label: "Refresh", icon: RefreshCw, onSelect: () => workspace.refreshCatalog(sessionId, database) });
  return items;
}

export function objectMenu(sessionId: string, database: string, object: ObjectInfo): MenuItem[] {
  const workspace = useWorkspaceState;
  const info = objectKindInfo(object.kind);
  const items: MenuItem[] = [];

  if (info.hasRows) {
    items.push({
      label: `Query ${info.singular}`,
      icon: SquarePen,
      onSelect: () => workspace.queryTable(sessionId, database, object.name),
    });
  }

  if (object.kind === "table") {
    items.push({ label: "Alter table", icon: PencilRuler, onSelect: () => workspace.openDesigner(sessionId, database, object.name) });
  }

  items.push(
    {
      label: info.hasColumns ? "Structure" : "Definition",
      icon: FileCode,
      onSelect: () => workspace.openStructure(sessionId, database, object),
    },
    { separator: true },
    {
      label: "Copy SQL",
      icon: ClipboardCopy,
      children: [
        { label: "Name", onSelect: () => copyText(object.name, "name") },
        { label: "Qualified name", onSelect: () => copyText(`${database}.${object.name}`, "name") },
        { separator: true },
        { label: "Create statement", onSelect: () => copyObjectSql(sessionId, database, object, "create") },
        { label: "Drop statement", onSelect: () => copyObjectSql(sessionId, database, object, "drop") },
        ...(info.hasRows
          ? [
            { separator: true } as const,
            ...ROW_TEMPLATES.map(row => ({
              label: row.label,
              onSelect: () => copyObjectSql(sessionId, database, object, row.template),
            })),
          ]
          : []),
      ],
    },
    { label: "Copy name", icon: Copy, onSelect: () => copyText(object.name, "name") },
  );

  return items;
}
