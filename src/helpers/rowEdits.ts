import { Accessor, createComputed, createMemo, on } from "solid-js";
import { createStore, produce, reconcile } from "solid-js/store";
import { BatchOutcome, ColumnMeta, ResultSet, RowChange } from "@core/types";
import type { GridEditing } from "@core/components/ResultGrid/ResultGrid";
import { safeInvoke } from "./safeInvoke";
import { errorMessage } from "./errors";
import { batchFailureMessage } from "./batch";

type Row = Array<string | null>;

type Edits = {
  cells: Record<number, Record<number, string | null>>,
  deleted: Record<number, true>,
  inserted: { values: Row, touched: Record<number, true> }[],
};

export type RowEdits = {
  editing: GridEditing,
  rows: Accessor<Row[]>,
  changeCount: Accessor<number>,
  addRow: () => void,
  discard: () => void,
  changes: () => RowChange[],
  savedRows: () => Row[],
};

export type EditTarget = {
  database: string,
  table: string,
  columns: ColumnMeta[],
};

export type EditCheck = { target: EditTarget } | { reason: string };

function emptyEdits(): Edits {
  return { cells: {}, deleted: {}, inserted: [] };
}

// Edits reset whenever the source result changes.
export function createRowEdits(source: Accessor<ResultSet | null>): RowEdits {
  const [edits, setEdits] = createStore<Edits>(emptyEdits());

  createComputed(on(source, () => setEdits(reconcile(emptyEdits())), { defer: true }));

  const originalCount = createMemo(() => source()?.rows.length ?? 0);

  const changeCount = createMemo(() =>
    Object.values(edits.cells).filter(row => Object.keys(row).length > 0).length
    + Object.keys(edits.deleted).length
    + edits.inserted.length,
  );

  function withCellEdits(row: Row, index: number): Row {
    const changed = edits.cells[index];
    return changed ? row.map((value, column) => (column in changed ? changed[column] : value)) : row;
  }

  const rows = createMemo(() => {
    const current = source();
    if (!current) return [];
    return [...current.rows.map(withCellEdits), ...edits.inserted.map(row => row.values)];
  });

  const editing: GridEditing = {
    rowState: row => (row >= originalCount() ? "new" : edits.deleted[row] ? "deleted" : null),
    isCellEdited: (row, column) => {
      if (row >= originalCount()) return edits.inserted[row - originalCount()]?.touched[column] ?? false;
      return edits.cells[row] !== undefined && column in edits.cells[row];
    },
    usesDefault: (row, column) => row >= originalCount() && !edits.inserted[row - originalCount()]?.touched[column],
    setCell: (row, column, value) => {
      if (row >= originalCount()) {
        setEdits("inserted", row - originalCount(), produce(inserted => {
          inserted.values[column] = value;
          inserted.touched[column] = true;
        }));
        return;
      }

      const original = source()?.rows[row]?.[column] ?? null;
      setEdits("cells", produce(cells => {
        cells[row] ??= {};
        if (value === original) delete cells[row][column];
        else cells[row][column] = value;
      }));
    },
    revertCell: (row, column) => {
      if (row >= originalCount()) {
        setEdits("inserted", row - originalCount(), produce(inserted => {
          inserted.values[column] = null;
          delete inserted.touched[column];
        }));
        return;
      }
      setEdits("cells", produce(cells => {
        if (cells[row]) delete cells[row][column];
      }));
    },
    deleteRow: row => {
      if (row >= originalCount()) {
        setEdits("inserted", list => list.filter((_, index) => index !== row - originalCount()));
        return;
      }
      setEdits("deleted", row, true);
    },
    restoreRow: row => {
      setEdits("deleted", produce(deleted => { delete deleted[row]; }));
    },
  };

  function addRow() {
    const columns = source()?.columns.length ?? 0;
    setEdits("inserted", list => [...list, { values: Array(columns).fill(null), touched: {} }]);
  }

  function changes(): RowChange[] {
    const current = source();
    if (!current) return [];

    const result: RowChange[] = [];
    for (const [rowKey, cells] of Object.entries(edits.cells)) {
      const row = Number(rowKey);
      const values = Object.entries(cells).map(([column, value]) => ({ column: Number(column), value }));
      if (values.length === 0 || edits.deleted[row]) continue;
      result.push({ kind: "update", key: current.rows[row], values });
    }
    for (const rowKey of Object.keys(edits.deleted)) {
      result.push({ kind: "delete", key: current.rows[Number(rowKey)] });
    }
    for (const inserted of edits.inserted) {
      const values = Object.keys(inserted.touched).map(column => ({ column: Number(column), value: inserted.values[Number(column)] }));
      result.push({ kind: "insert", values });
    }
    return result;
  }

  // Inserted rows are left out because their defaults and generated values are only known to the server.
  function savedRows(): Row[] {
    const current = source();
    if (!current) return [];
    return current.rows.map(withCellEdits).filter((_, index) => !edits.deleted[index]);
  }

  return {
    editing,
    rows,
    changeCount,
    addRow,
    discard: () => setEdits(reconcile(emptyEdits())),
    changes,
    savedRows,
  };
}

function sameName(a: string, b: string): boolean {
  return a.localeCompare(b, undefined, { sensitivity: "accent" }) === 0;
}

// Rows from a query can be edited when every column is a plain column of one table and its whole primary key is there.
export async function findEditTarget(sessionId: string, columns: ColumnMeta[]): Promise<EditCheck> {
  const first = columns[0];
  if (!first?.database || !first.table) return { reason: "Results don't come from a table" };

  if (columns.some(column => !column.sourceName)) {
    return { reason: "Some columns are computed, not read from a table" };
  }
  if (columns.some(column => column.database !== first.database || column.table !== first.table)) {
    return { reason: "Results come from more than one table" };
  }

  const names = columns.map(column => column.sourceName!);
  if (names.some((name, index) => names.findIndex(other => sameName(other, name)) !== index)) {
    return { reason: "A column appears more than once" };
  }

  const key = await safeInvoke("get_primary_key", { sessionId, database: first.database, table: first.table });
  if (key.length === 0) return { reason: `${first.table} has no primary key` };

  const missing = key.filter(part => !names.some(name => sameName(name, part)));
  if (missing.length > 0) return { reason: `Select ${missing.join(", ")} to edit these rows` };

  return {
    target: {
      database: first.database,
      table: first.table,
      columns: columns.map(column => ({ ...column, primaryKey: key.some(part => sameName(part, column.sourceName!)) })),
    },
  };
}

export async function previewRowChanges(sessionId: string, target: EditTarget, changes: RowChange[]): Promise<string[]> {
  return safeInvoke("preview_row_changes", {
    sessionId,
    database: target.database,
    table: target.table,
    columns: target.columns,
    changes,
  });
}

// Resolves to an error message, or null when every statement ran.
export async function runRowChanges(sessionId: string, database: string, statements: string[]): Promise<string | null> {
  let outcome: BatchOutcome;
  try {
    outcome = await safeInvoke("execute_batch", { sessionId, database, statements, transactional: true });
  } catch (error) {
    return errorMessage(error);
  }
  return outcome.error ? batchFailureMessage(outcome) : null;
}
