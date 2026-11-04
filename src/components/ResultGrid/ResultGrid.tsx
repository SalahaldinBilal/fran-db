import styles from "./ResultGrid.module.scss";
import { For, Index, Show, createEffect, createMemo, createSignal, on, onCleanup, onMount } from "solid-js";
import { createStore } from "solid-js/store";
import { Ban, Braces, ClipboardList, Copy, KeyRound, Maximize2, Rows3, TextCursorInput, Trash2, Undo2 } from "lucide-solid";
import { ColumnMeta, ValueCategory } from "@core/types";
import useContextMenuState, { MenuItem } from "@core/states/contextMenuState";
import useSettingsState, { CellEditMode } from "@core/states/settingsState";
import useToastState from "@core/states/toastState";
import { copyText } from "@core/helpers/clipboard";
import { CopyFormat, formatRows } from "@core/helpers/copyRows";
import { safeInvoke } from "@core/helpers/safeInvoke";
import { plural } from "@core/helpers";
import { looksLikeJson } from "@core/helpers/values";
import ValueViewer from "./ValueViewer";
import { tooltip } from "../Tooltip/Tooltip";
import { canonicalTemporal, fractionDigitsOf, isZonedType, temporalKind } from "./views/temporal";

const ROW_HEIGHT = 26;
const HEADER_HEIGHT = 40;
const OVERSCAN = 8;
const CHAR_WIDTH = 7.4;
const MIN_COLUMN_WIDTH = 72;
const MAX_COLUMN_WIDTH = 380;
const SAMPLE_ROWS = 200;
const DISPLAY_LIMIT = 300;
const INLINE_EDIT_LIMIT = 200;
const DRAG_EDGE = 24;
const DRAG_MAX_SPEED = 40;

const NUMERIC: ReadonlySet<ValueCategory> = new Set(["integer", "decimal", "float", "bit"]);

export type RowState = "new" | "deleted" | null;

export type GridEditing = {
  rowState: (row: number) => RowState,
  isCellEdited: (row: number, column: number) => boolean,
  usesDefault: (row: number, column: number) => boolean,
  setCell: (row: number, column: number, value: string | null) => void,
  revertCell: (row: number, column: number) => void,
  deleteRow: (row: number) => void,
  restoreRow: (row: number) => void,
};

export type ResultGridProps = {
  columns: ColumnMeta[],
  rows: Array<Array<string | null>>,
  rowOffset?: number,
  editing?: GridEditing,
  highlight?: string,
  filter?: boolean,
  sessionId?: string,
};

type Cell = { row: number, column: number };
type ViewTarget = Cell & { draft?: string };
type SelectionBox = { top: number, bottom: number, left: number, right: number };
type CopyTarget = { rows: number[], columns: number[] };
type TextPart = { text: string, match: boolean };

function measureColumn(column: ColumnMeta, index: number, rows: ResultGridProps["rows"]): number {
  let longest = Math.max(column.name.length, column.typeName.length + 2);
  const sample = Math.min(rows.length, SAMPLE_ROWS);
  for (let row = 0; row < sample; row++) {
    const value = rows[row][index];
    longest = Math.max(longest, value === null ? 4 : Math.min(value.length, 60));
  }
  return Math.round(Math.min(MAX_COLUMN_WIDTH, Math.max(MIN_COLUMN_WIDTH, longest * CHAR_WIDTH + 28)));
}

function displayValue(value: string): string {
  const clipped = value.length > DISPLAY_LIMIT ? `${value.slice(0, DISPLAY_LIMIT)}…` : value;
  return clipped.replace(/\r?\n/g, " ↵ ");
}

function splitMatches(text: string, term: string): TextPart[] {
  const haystack = text.toLowerCase();
  const needle = term.toLowerCase();
  if (!needle || haystack.length !== text.length) return [{ text, match: false }];

  const parts: TextPart[] = [];
  let start = 0;
  let found = haystack.indexOf(needle);
  while (found !== -1) {
    if (found > start) parts.push({ text: text.slice(start, found), match: false });
    parts.push({ text: text.slice(found, found + needle.length), match: true });
    start = found + needle.length;
    found = haystack.indexOf(needle, start);
  }
  if (start < text.length) parts.push({ text: text.slice(start), match: false });
  return parts;
}

function CellText(props: { text: string, term: string }) {
  const parts = createMemo(() => splitMatches(props.text, props.term));
  return (
    <span class={styles.Text}>
      <For each={parts()}>
        {part => (part.match ? <mark class={styles.Match}>{part.text}</mark> : part.text)}
      </For>
    </span>
  );
}

function ResultGrid(props: ResultGridProps) {
  const { openContextMenu } = useContextMenuState;
  const { settings } = useSettingsState;

  const [scrollTop, setScrollTop] = createSignal(0);
  const [viewportHeight, setViewportHeight] = createSignal(400);
  const [viewportWidth, setViewportWidth] = createSignal(0);
  const { pushError } = useToastState;
  const [selected, setSelected] = createSignal<Cell | null>(null);
  // The selection runs from `anchor` to `selected`. Whole-row mode adds the rows picked with Ctrl, and has no anchor when only those count.
  const [anchor, setAnchor] = createSignal<Cell | null>(null);
  const [wholeRows, setWholeRows] = createSignal(false);
  const [picked, setPicked] = createSignal<ReadonlySet<number>>(new Set());
  const [dragging, setDragging] = createSignal<"cells" | "rows" | null>(null);
  const [inlineEdit, setInlineEdit] = createSignal<Cell | null>(null);
  const [viewing, setViewing] = createSignal<ViewTarget | null>(null);
  const [widths, setWidths] = createStore<number[]>([]);
  let viewport!: HTMLDivElement;

  createEffect(on(() => props.columns, columns => {
    setWidths(columns.map((column, index) => measureColumn(column, index, props.rows)));
    setSelected(null);
    clearRange();
    setInlineEdit(null);
    viewport?.scrollTo({ top: 0, left: 0 });
    setScrollTop(0);
  }));

  onMount(() => {
    const observer = new ResizeObserver(() => {
      setViewportHeight(viewport.clientHeight);
      setViewportWidth(viewport.clientWidth);
    });
    observer.observe(viewport);
    window.addEventListener("mousemove", trackDrag);
    window.addEventListener("mouseup", stopDragging);
    onCleanup(() => {
      observer.disconnect();
      window.removeEventListener("mousemove", trackDrag);
      window.removeEventListener("mouseup", stopDragging);
      stopDragging();
    });
  });

  let pointer: { x: number, y: number } | null = null;
  let dragFrame = 0;

  function trackDrag(event: MouseEvent) {
    if (!dragging()) return;
    pointer = { x: event.clientX, y: event.clientY };
    if (!dragFrame) dragFrame = requestAnimationFrame(dragStep);
  }

  function stopDragging() {
    setDragging(null);
    pointer = null;
    cancelAnimationFrame(dragFrame);
    dragFrame = 0;
  }

  function cellAtPoint(x: number, y: number): Cell | null {
    if (rowCount() === 0) return null;
    const rect = viewport.getBoundingClientRect();
    const position = Math.floor((y - rect.top + viewport.scrollTop - HEADER_HEIGHT) / ROW_HEIGHT);
    const row = rowAt(Math.min(rowCount() - 1, Math.max(0, position)));
    let offset = x - rect.left + viewport.scrollLeft - rowNumberWidth();
    let column = 0;
    while (column < props.columns.length - 1 && offset >= widths[column]) {
      offset -= widths[column];
      column++;
    }
    return { row, column };
  }

  // How far past the edge zone the pointer is, so scrolling speeds up the further out it goes.
  function edgeOverflow(at: number, start: number, end: number): number {
    if (at < start + DRAG_EDGE) return at - (start + DRAG_EDGE);
    if (at > end - DRAG_EDGE) return at - (end - DRAG_EDGE);
    return 0;
  }

  function dragStep() {
    dragFrame = 0;
    const kind = dragging();
    if (!kind || !pointer) return;
    const rect = viewport.getBoundingClientRect();
    const overY = edgeOverflow(pointer.y, rect.top + HEADER_HEIGHT, rect.bottom);
    const overX = kind === "cells" ? edgeOverflow(pointer.x, rect.left + rowNumberWidth(), rect.right) : 0;
    const speed = (over: number) => Math.sign(over) * Math.min(DRAG_MAX_SPEED, Math.abs(over) / 2);
    if (overX !== 0 || overY !== 0) viewport.scrollBy(speed(overX), speed(overY));

    const cell = cellAtPoint(pointer.x, pointer.y);
    if (cell) setSelected(kind === "rows" ? { row: cell.row, column: selected()?.column ?? 0 } : cell);
    if (overX !== 0 || overY !== 0) dragFrame = requestAnimationFrame(dragStep);
  }

  const rowNumberWidth = createMemo(() => {
    const largest = (props.rowOffset ?? 0) + props.rows.length;
    return Math.max(44, String(largest).length * 8 + 22);
  });

  const totalWidth = createMemo(() => rowNumberWidth() + widths.reduce((sum, width) => sum + width, 0));

  // With `filter`, only rows containing the highlight are shown. New rows always stay visible.
  const shownRows = createMemo<number[] | null>(() => {
    const term = props.highlight?.trim().toLowerCase();
    if (!props.filter || !term) return null;
    const shown: number[] = [];
    props.rows.forEach((row, index) => {
      const matches = row.some(value => value !== null && value.toLowerCase().includes(term));
      if (matches || props.editing?.rowState(index) === "new") shown.push(index);
    });
    return shown;
  });

  const positions = createMemo(() => {
    const shown = shownRows();
    return shown ? new Map(shown.map((row, position) => [row, position])) : null;
  });

  const rowCount = createMemo(() => shownRows()?.length ?? props.rows.length);

  function rowAt(position: number): number {
    return shownRows()?.[position] ?? position;
  }

  function positionOf(row: number): number {
    return positions()?.get(row) ?? row;
  }

  const visibleRows = createMemo(() => {
    const first = Math.max(0, Math.floor(scrollTop() / ROW_HEIGHT) - OVERSCAN);
    const count = Math.ceil(viewportHeight() / ROW_HEIGHT) + OVERSCAN * 2;
    const last = Math.min(rowCount(), first + count);
    const rows: number[] = [];
    for (let position = first; position < last; position++) rows.push(rowAt(position));
    return rows;
  });

  function isShown(row: number): boolean {
    const shown = positions();
    return row < props.rows.length && (shown === null || shown.has(row));
  }

  const range = createMemo<SelectionBox | null>(() => {
    const focus = selected();
    if (!focus || (wholeRows() && anchor() === null)) return null;
    const start = anchor() ?? focus;
    const from = positionOf(start.row);
    const to = positionOf(focus.row);
    const allColumns = wholeRows();
    return {
      top: Math.min(from, to),
      bottom: Math.max(from, to),
      left: allColumns ? 0 : Math.min(start.column, focus.column),
      right: allColumns ? props.columns.length - 1 : Math.max(start.column, focus.column),
    };
  });

  const isMulti = createMemo(() => {
    if (wholeRows()) return true;
    const current = range();
    return current !== null && (current.top !== current.bottom || current.left !== current.right);
  });

  function rowInRange(row: number): boolean {
    const current = range();
    if (!current || !isShown(row)) return false;
    const position = positionOf(row);
    return position >= current.top && position <= current.bottom;
  }

  function rowSelected(row: number): boolean {
    return wholeRows() && (picked().has(row) || rowInRange(row));
  }

  function cellInSelection(cell: Cell): boolean {
    if (wholeRows()) return rowSelected(cell.row);
    const current = range();
    return current !== null && rowInRange(cell.row) && cell.column >= current.left && cell.column <= current.right;
  }

  function columnSelected(column: number): boolean {
    const current = range();
    return !wholeRows() && current !== null && rowCount() > 1
      && current.top === 0 && current.bottom === rowCount() - 1
      && column >= current.left && column <= current.right;
  }

  function selectionRows(): number[] {
    const rows = new Set<number>();
    const current = range();
    if (current) {
      for (let position = current.top; position <= current.bottom; position++) rows.add(rowAt(position));
    }
    if (wholeRows()) picked().forEach(row => rows.add(row));
    return [...rows].filter(isShown).sort((a, b) => positionOf(a) - positionOf(b));
  }

  function selectionColumns(): number[] {
    const current = range();
    const left = current && !wholeRows() ? current.left : 0;
    const right = current && !wholeRows() ? current.right : props.columns.length - 1;
    const columns: number[] = [];
    for (let column = left; column <= right; column++) columns.push(column);
    return columns;
  }

  function clearRange() {
    setAnchor(null);
    setWholeRows(false);
    setPicked(new Set<number>());
  }

  function selectCell(cell: Cell) {
    clearRange();
    setAnchor(cell);
    setSelected(cell);
  }

  function extendTo(cell: Cell) {
    if (!anchor()) setAnchor(selected() ?? cell);
    setSelected(cell);
  }

  function selectRow(row: number) {
    const cell = { row, column: selected()?.column ?? 0 };
    clearRange();
    setWholeRows(true);
    setAnchor(cell);
    setSelected(cell);
  }

  function toggleRow(row: number) {
    const rows = new Set(wholeRows() ? selectionRows() : []);
    if (rows.has(row)) rows.delete(row);
    else rows.add(row);
    setWholeRows(true);
    setAnchor(null);
    setPicked(rows);
    setSelected({ row, column: selected()?.column ?? 0 });
  }

  function selectAll() {
    if (rowCount() === 0) return;
    const column = selected()?.column ?? 0;
    clearRange();
    setWholeRows(true);
    setAnchor({ row: rowAt(0), column });
    setSelected({ row: rowAt(rowCount() - 1), column });
  }

  function selectColumn(column: number, extend: boolean) {
    if (rowCount() === 0) return;
    const top = rowAt(0);
    const bottom = rowAt(rowCount() - 1);
    const start = extend && !wholeRows() ? anchor()?.column ?? column : column;
    clearRange();
    setAnchor({ row: top, column: start });
    setSelected({ row: bottom, column });
  }

  // Keeps focus on the grid while cancelling the text selection a press would start.
  function takeFocus(event: MouseEvent) {
    event.preventDefault();
    viewport.focus({ preventScroll: true });
  }

  createEffect(on(shownRows, () => {
    const current = selected();
    if (current) selectCell(current);
  }, { defer: true }));

  function cellValue(cell: Cell): string | null {
    return props.rows[cell.row]?.[cell.column] ?? null;
  }

  function canEdit(cell: Cell): boolean {
    return props.editing !== undefined && props.editing.rowState(cell.row) !== "deleted";
  }

  const viewingColumn = createMemo(() => {
    const cell = viewing();
    return cell ? props.columns[cell.column] : null;
  });
  const viewingValue = createMemo(() => {
    const cell = viewing();
    return cell ? cell.draft ?? cellValue(cell) : null;
  });
  const viewingEditable = createMemo(() => {
    const cell = viewing();
    return cell ? canEdit(cell) : false;
  });

  function openViewer(cell: Cell, draft?: string) {
    setInlineEdit(null);
    selectCell({ row: cell.row, column: cell.column });
    const changed = draft !== undefined && draft !== (cellValue(cell) ?? "");
    setViewing({ row: cell.row, column: cell.column, draft: changed ? draft : undefined });
  }

  function fitsInline(cell: Cell): boolean {
    const value = cellValue(cell);
    const category = props.columns[cell.column].category;
    if (category === "binary" || category === "json") return false;
    return value === null || (value.length <= INLINE_EDIT_LIMIT && !value.includes("\n") && !looksLikeJson(value));
  }

  function otherEditMode(): CellEditMode {
    return settings.cellEditMode === "viewer" ? "inline" : "viewer";
  }

  function startEditing(cell: Cell, mode: CellEditMode = settings.cellEditMode) {
    if (canEdit(cell) && mode === "inline" && fitsInline(cell)) setInlineEdit(cell);
    else openViewer(cell);
  }

  const insertTarget = createMemo(() => {
    const first = props.columns[0];
    if (!props.sessionId || !first?.database || !first.table) return null;
    const oneTable = props.columns.every(column => column.sourceName && column.database === first.database && column.table === first.table);
    return oneTable ? { sessionId: props.sessionId, database: first.database, table: first.table } : null;
  });

  // A single cell copies on its own with Ctrl+C, but every other copy of it takes its whole row.
  function copyTarget(): CopyTarget {
    const rows = selectionRows();
    if (isMulti() && rows.length > 0) return { rows, columns: selectionColumns() };
    const focus = selected();
    return focus ? { rows: [focus.row], columns: props.columns.map((_, index) => index) } : { rows: [], columns: [] };
  }

  function targetValues(target: CopyTarget): Array<Array<string | null>> {
    return target.rows.map(row => target.columns.map(column => props.rows[row]?.[column] ?? null));
  }

  function copyRows(format: CopyFormat, headers = false) {
    const target = copyTarget();
    if (target.rows.length === 0) return;
    const columns = target.columns.map(index => props.columns[index]);
    copyText(formatRows(format, columns, targetValues(target), headers), plural(target.rows.length, "row"));
  }

  async function copyAsInsert() {
    const table = insertTarget();
    const target = copyTarget();
    if (!table || target.rows.length === 0) return;
    try {
      const statements = await safeInvoke("get_insert_sql", {
        ...table,
        columns: target.columns.map(index => props.columns[index]),
        rows: targetValues(target),
      });
      copyText(statements.map(statement => `${statement};`).join("\n"), `${plural(target.rows.length, "row")} as INSERT`);
    } catch (error) {
      pushError(error);
    }
  }

  function copyShortcut(withHeaders: boolean) {
    const current = selected();
    if (!isMulti() && !withHeaders) {
      if (current) copyText(cellValue(current) ?? "NULL", "value");
      return;
    }
    copyRows("tsv", withHeaders);
  }

  function ensureVisible(cell: Cell) {
    const top = positionOf(cell.row) * ROW_HEIGHT;
    const visibleHeight = viewport.clientHeight - HEADER_HEIGHT;
    if (top < viewport.scrollTop) viewport.scrollTop = top;
    else if (top + ROW_HEIGHT > viewport.scrollTop + visibleHeight) viewport.scrollTop = top + ROW_HEIGHT - visibleHeight;
  }

  function move(cell: Cell, extend = false) {
    if (extend) extendTo(cell);
    else selectCell(cell);
    ensureVisible(cell);
  }

  createEffect(on(() => props.rows.length, (length, previous) => {
    const last = length - 1;
    if (previous === undefined || length <= previous || props.editing?.rowState(last) !== "new") return;
    move({ row: last, column: 0 });
    viewport.focus();
  }));

  function onKeyDown(event: KeyboardEvent) {
    // Solid passes events from the viewer's portal up to here, so ignore anything outside the grid itself.
    if (inlineEdit() || !viewport.contains(event.target as Node)) return;
    const current = selected();
    if (!current || rowCount() === 0) return;

    const position = positionOf(current.row);
    const columns = props.columns.length;
    const moves: Record<string, Cell> = {
      ArrowUp: { row: rowAt(Math.max(0, position - 1)), column: current.column },
      ArrowDown: { row: rowAt(Math.min(rowCount() - 1, position + 1)), column: current.column },
      ArrowLeft: { row: current.row, column: Math.max(0, current.column - 1) },
      ArrowRight: { row: current.row, column: Math.min(columns - 1, current.column + 1) },
    };

    const command = event.ctrlKey || event.metaKey;
    if (event.key in moves) {
      event.preventDefault();
      move(moves[event.key], event.shiftKey);
    } else if (command && event.key.toLowerCase() === "c") {
      event.preventDefault();
      copyShortcut(event.shiftKey);
    } else if (command && event.key.toLowerCase() === "a") {
      event.preventDefault();
      selectAll();
    } else if (event.key === "Escape" && isMulti()) {
      event.preventDefault();
      selectCell(current);
    } else if (event.key === "Enter" && event.shiftKey) {
      event.preventDefault();
      startEditing(current, otherEditMode());
    } else if (event.key === "Enter" || event.key === "F2") {
      event.preventDefault();
      startEditing(current);
    }
  }

  function commitInline(cell: Cell, text: string, next?: Cell) {
    setInlineEdit(null);
    const original = cellValue(cell);
    let value = text;
    const column = props.columns[cell.column];
    if (column.category === "temporal" && text !== (original ?? "")) {
      const kind = temporalKind(column.typeName);
      const zoned = isZonedType(column.typeName);
      value = canonicalTemporal(kind, text, fractionDigitsOf(kind, original, zoned), zoned) ?? text;
    }
    if (value !== (original ?? "")) props.editing?.setCell(cell.row, cell.column, value);
    if (next) move(next);
    viewport.focus();
  }

  function startResize(event: MouseEvent, column: number) {
    event.preventDefault();
    event.stopPropagation();
    const startX = event.clientX;
    const startWidth = widths[column];

    function onMove(moveEvent: MouseEvent) {
      setWidths(column, Math.max(48, startWidth + moveEvent.clientX - startX));
    }

    function onUp() {
      window.removeEventListener("mousemove", onMove);
      window.removeEventListener("mouseup", onUp);
      document.body.style.removeProperty("cursor");
    }

    document.body.style.cursor = "col-resize";
    window.addEventListener("mousemove", onMove);
    window.addEventListener("mouseup", onUp);
  }

  function copyAsItem(label: string): MenuItem {
    const table = insertTarget();
    return {
      label,
      icon: ClipboardList,
      children: [
        { label: "CSV", onSelect: () => copyRows("csv") },
        { label: "JSON", onSelect: () => copyRows("json") },
        { label: "Markdown table", onSelect: () => copyRows("markdown") },
        {
          label: "SQL INSERT",
          disabled: table === null,
          hint: table === null ? "Needs columns from one table" : undefined,
          onSelect: copyAsInsert,
        },
      ],
    };
  }

  function selectionCopyItems(): MenuItem[] {
    const count = selectionRows().length;
    const what = wholeRows() ? plural(count, "row") : "selection";
    return [
      { label: `Copy ${what}`, icon: Copy, hint: "Ctrl+C", onSelect: () => copyRows("tsv") },
      { label: "Copy with headers", icon: Rows3, hint: "Ctrl+Shift+C", onSelect: () => copyRows("tsv", true) },
      copyAsItem("Copy as"),
    ];
  }

  function cellMenu(cell: Cell): MenuItem[] {
    if (isMulti()) {
      const actions = rowActions(selectionRows());
      return actions.length > 0 ? [...selectionCopyItems(), { separator: true }, ...actions] : selectionCopyItems();
    }

    const value = cellValue(cell);
    const editable = canEdit(cell);
    const items: MenuItem[] = [
      { label: "Copy value", icon: Copy, hint: "Ctrl+C", onSelect: () => copyText(value ?? "NULL", "value") },
      { label: "Copy row", icon: Rows3, onSelect: () => copyRows("tsv") },
      copyAsItem("Copy row as"),
    ];
    if (!editable) {
      items.push({ label: "Open value", icon: value !== null && looksLikeJson(value) ? Braces : Maximize2, hint: "Enter", onSelect: () => openViewer(cell) });
    }

    const editing = props.editing;
    if (!editing) return items;

    const deleted = editing.rowState(cell.row) === "deleted";
    items.push({ separator: true });
    if (!deleted) {
      const viewerFirst = settings.cellEditMode === "viewer";
      const inlineItem: MenuItem = { label: "Edit in cell", icon: TextCursorInput, hint: viewerFirst ? "Shift+Enter" : "Enter", onSelect: () => startEditing(cell, "inline") };
      const viewerItem: MenuItem = { label: "Edit in viewer", icon: Maximize2, hint: viewerFirst ? "Enter" : "Shift+Enter", onSelect: () => openViewer(cell) };
      const editItems = viewerFirst ? [viewerItem, inlineItem] : [inlineItem, viewerItem];
      items.push(
        ...editItems.filter(item => item !== inlineItem || fitsInline(cell)),
        { label: "Set to NULL", icon: Ban, disabled: !props.columns[cell.column].nullable, onSelect: () => editing.setCell(cell.row, cell.column, null) },
      );
      if (editing.isCellEdited(cell.row, cell.column)) {
        items.push({ label: "Revert value", icon: Undo2, onSelect: () => editing.revertCell(cell.row, cell.column) });
      }
      items.push({ separator: true });
    }

    return [...items, ...rowActions([cell.row])];
  }

  function rowActions(rows: number[]): MenuItem[] {
    const editing = props.editing;
    if (!editing || rows.length === 0) return [];
    const deleted = rows.filter(row => editing.rowState(row) === "deleted");
    const live = rows.filter(row => editing.rowState(row) !== "deleted");
    const items: MenuItem[] = [];
    if (deleted.length > 0) {
      items.push({
        label: deleted.length === 1 ? "Restore row" : `Restore ${plural(deleted.length, "row")}`,
        icon: Undo2,
        onSelect: () => deleted.forEach(row => editing.restoreRow(row)),
      });
    }
    if (live.length > 0) {
      items.push({
        label: live.length === 1 ? "Delete row" : `Delete ${plural(live.length, "row")}`,
        icon: Trash2,
        danger: true,
        onSelect: () => deleteRows(live),
      });
    }
    return items;
  }

  // New rows are removed from the list outright, so the highest go first to keep the other indexes valid.
  function deleteRows(rows: number[]) {
    const editing = props.editing;
    if (!editing) return;
    [...rows].sort((a, b) => b - a).forEach(row => editing.deleteRow(row));
    const current = selected();
    const last = props.rows.length - 1;
    if (current && current.row > last) {
      if (last >= 0) selectCell({ row: last, column: current.column });
      else {
        clearRange();
        setSelected(null);
      }
    } else if (current) {
      selectCell(current);
    }
  }

  function rowMenu(row: number): MenuItem[] {
    if (!rowSelected(row)) selectRow(row);
    const actions = rowActions(selectionRows());
    return actions.length > 0 ? [...selectionCopyItems(), { separator: true }, ...actions] : selectionCopyItems();
  }

  return (
    <div
      ref={viewport}
      class={styles.Viewport}
      tabIndex={0}
      onScroll={event => setScrollTop(event.currentTarget.scrollTop)}
      onKeyDown={onKeyDown}
    >
      <div class={styles.Canvas} style={{ width: `${totalWidth()}px`, height: `${HEADER_HEIGHT + rowCount() * ROW_HEIGHT}px` }}>
        <div class={styles.Header} style={{ height: `${HEADER_HEIGHT}px`, width: `${totalWidth()}px` }}>
          <div class={`${styles.RowNumber} ${styles.HeaderCorner}`} style={{ width: `${rowNumberWidth()}px` }} />
          <For each={props.columns}>
            {(column, index) => {
              const isColumnSelected = createMemo(() => columnSelected(index()));
              return (
                <div
                  ref={element => tooltip(element, () => `${column.name} ${column.typeName}${column.nullable ? "" : " NOT NULL"}${column.table ? ` (${column.table})` : ""}`)}
                  class={styles.HeaderCell}
                  classList={{ [styles.Numeric]: NUMERIC.has(column.category), [styles.ColumnSelected]: isColumnSelected() }}
                  style={{ width: `${widths[index()]}px` }}
                  onMouseDown={event => {
                    if (event.button !== 0) return;
                    takeFocus(event);
                    selectColumn(index(), event.shiftKey);
                  }}
                >
                  <div class={styles.ColumnName}>
                    <Show when={column.primaryKey}><KeyRound size={12} class={styles.KeyIcon} /></Show>
                    <span>{column.name}</span>
                  </div>
                  <div class={styles.ColumnType}>{column.typeName}</div>
                  <div class={styles.Resizer} onMouseDown={event => startResize(event, index())} />
                </div>
              );
            }}
          </For>
        </div>

        <Show when={shownRows()?.length === 0}>
          <div class={styles.NoMatches} style={{ width: `${viewportWidth()}px` }}>
            No rows contain "{props.highlight?.trim()}"
          </div>
        </Show>

        <For each={visibleRows()}>
          {row => {
            const state = createMemo(() => props.editing?.rowState(row) ?? null);
            const position = createMemo(() => positionOf(row));
            const isRowSelected = createMemo(() => rowSelected(row));

            return (
              <div
                class={styles.Row}
                classList={{
                  [styles.Odd]: position() % 2 === 1,
                  [styles.NewRow]: state() === "new",
                  [styles.DeletedRow]: state() === "deleted",
                }}
                style={{ transform: `translateY(${HEADER_HEIGHT + position() * ROW_HEIGHT}px)`, width: `${totalWidth()}px` }}
              >
                <div
                  class={styles.RowNumber}
                  classList={{ [styles.RowSelected]: isRowSelected() }}
                  style={{ width: `${rowNumberWidth()}px` }}
                  onMouseDown={event => {
                    if (event.button !== 0) return;
                    takeFocus(event);
                    if (event.ctrlKey || event.metaKey) {
                      toggleRow(row);
                    } else if (event.shiftKey) {
                      setWholeRows(true);
                      extendTo({ row, column: selected()?.column ?? 0 });
                    } else {
                      selectRow(row);
                      setDragging("rows");
                    }
                  }}
                  onContextMenu={event => openContextMenu(event, rowMenu(row))}
                >
                  {state() === "new" ? "+" : (props.rowOffset ?? 0) + row + 1}
                </div>
                <Index each={props.columns}>
                  {(column, columnIndex) => {
                    const value = createMemo(() => props.rows[row]?.[columnIndex] ?? null);
                    const isSelected = createMemo(() => selected()?.row === row && selected()?.column === columnIndex);
                    const inSelection = createMemo(() => isMulti() && cellInSelection({ row, column: columnIndex }));
                    const isEditing = createMemo(() => inlineEdit()?.row === row && inlineEdit()?.column === columnIndex);
                    const edited = createMemo(() => props.editing?.isCellEdited(row, columnIndex) ?? false);
                    const usesDefault = createMemo(() => props.editing?.usesDefault(row, columnIndex) ?? false);

                    return (
                      <div
                        class={styles.Cell}
                        classList={{
                          [styles.Selected]: isSelected(),
                          [styles.InSelection]: inSelection(),
                          [styles.Edited]: edited(),
                          [styles.Numeric]: NUMERIC.has(column().category),
                          [styles.Binary]: column().category === "binary" || column().category === "spatial",
                          [styles.Temporal]: column().category === "temporal",
                        }}
                        style={{ width: `${widths[columnIndex]}px` }}
                        onMouseDown={event => {
                          if (event.button !== 0) return;
                          const cell = { row, column: columnIndex };
                          takeFocus(event);
                          if (event.shiftKey) {
                            extendTo(cell);
                          } else {
                            selectCell(cell);
                            setDragging("cells");
                          }
                          if (event.detail === 2) startEditing(cell);
                        }}
                        onContextMenu={event => {
                          const cell = { row, column: columnIndex };
                          if (!isMulti() || !cellInSelection(cell)) selectCell(cell);
                          openContextMenu(event, cellMenu(cell));
                        }}
                      >
                        <Show
                          when={isEditing()}
                          fallback={
                            <Show
                              when={value() !== null}
                              fallback={<span class={styles.Null}>{usesDefault() ? "DEFAULT" : "NULL"}</span>}
                            >
                              <CellText text={displayValue(value()!)} term={props.highlight ?? ""} />
                            </Show>
                          }
                        >
                          <div class={styles.InlineEditor} onMouseDown={event => event.stopPropagation()}>
                            <input
                              class={styles.CellInput}
                              value={value() ?? ""}
                              placeholder={value() === null ? "NULL" : undefined}
                              ref={input => queueMicrotask(() => {
                                input.focus();
                                input.select();
                              })}
                              onKeyDown={event => {
                                event.stopPropagation();
                                const cell = { row, column: columnIndex };
                                if (event.key === "Enter" && event.shiftKey) {
                                  event.preventDefault();
                                  openViewer(cell, event.currentTarget.value);
                                } else if (event.key === "Enter") {
                                  commitInline(cell, event.currentTarget.value, { row: rowAt(Math.min(rowCount() - 1, position() + 1)), column: columnIndex });
                                } else if (event.key === "Tab") {
                                  event.preventDefault();
                                  const column = event.shiftKey ? Math.max(0, columnIndex - 1) : Math.min(props.columns.length - 1, columnIndex + 1);
                                  commitInline(cell, event.currentTarget.value, { row, column });
                                } else if (event.key === "Escape") {
                                  setInlineEdit(null);
                                  viewport.focus();
                                }
                              }}
                              onBlur={event => {
                                if (isEditing()) commitInline({ row, column: columnIndex }, event.currentTarget.value);
                              }}
                            />
                            <button
                              ref={element => tooltip(element, () => "Edit in viewer (Shift+Enter)")}
                              class={styles.ExpandEditor}
                              tabIndex={-1}
                              onMouseDown={event => event.preventDefault()}
                              onClick={event => {
                                const input = event.currentTarget.previousElementSibling as HTMLInputElement;
                                openViewer({ row, column: columnIndex }, input.value);
                              }}
                            >
                              <Maximize2 size={12} />
                            </button>
                          </div>
                        </Show>
                      </div>
                    );
                  }}
                </Index>
              </div>
            );
          }}
        </For>
      </div>

      <ValueViewer
        cell={viewing()}
        column={viewingColumn()}
        value={viewingValue()}
        editable={viewingEditable()}
        sessionId={props.sessionId}
        onSave={value => {
          const cell = viewing();
          if (cell) props.editing?.setCell(cell.row, cell.column, value);
        }}
        onClose={() => {
          setViewing(null);
          viewport.focus();
        }}
      />
    </div>
  );
}

export default ResultGrid;
