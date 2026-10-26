import styles from "./ResultGrid.module.scss";
import { For, Index, Show, createEffect, createMemo, createSignal, on, onCleanup, onMount } from "solid-js";
import { createStore } from "solid-js/store";
import { Ban, Braces, Copy, KeyRound, Maximize2, Rows3, TextCursorInput, Trash2, Undo2 } from "lucide-solid";
import { ColumnMeta, ValueCategory } from "@core/types";
import useContextMenuState, { MenuItem } from "@core/states/contextMenuState";
import useSettingsState, { CellEditMode } from "@core/states/settingsState";
import { copyText } from "@core/helpers/clipboard";
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
  const [selected, setSelected] = createSignal<Cell | null>(null);
  const [inlineEdit, setInlineEdit] = createSignal<Cell | null>(null);
  const [viewing, setViewing] = createSignal<ViewTarget | null>(null);
  const [widths, setWidths] = createStore<number[]>([]);
  let viewport!: HTMLDivElement;

  createEffect(on(() => props.columns, columns => {
    setWidths(columns.map((column, index) => measureColumn(column, index, props.rows)));
    setSelected(null);
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
    onCleanup(() => observer.disconnect());
  });

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
    setSelected({ row: cell.row, column: cell.column });
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

  function copyRow(row: number) {
    copyText(props.rows[row].map(value => value ?? "NULL").join("\t"), "row");
  }

  function ensureVisible(cell: Cell) {
    const top = positionOf(cell.row) * ROW_HEIGHT;
    const visibleHeight = viewport.clientHeight - HEADER_HEIGHT;
    if (top < viewport.scrollTop) viewport.scrollTop = top;
    else if (top + ROW_HEIGHT > viewport.scrollTop + visibleHeight) viewport.scrollTop = top + ROW_HEIGHT - visibleHeight;
  }

  function move(cell: Cell) {
    setSelected(cell);
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

    if (event.key in moves) {
      event.preventDefault();
      move(moves[event.key]);
    } else if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "c") {
      event.preventDefault();
      copyText(cellValue(current) ?? "NULL", "value");
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

  function cellMenu(cell: Cell): MenuItem[] {
    const value = cellValue(cell);
    const editable = canEdit(cell);
    const items: MenuItem[] = [
      { label: "Copy value", icon: Copy, hint: "Ctrl+C", onSelect: () => copyText(value ?? "NULL", "value") },
      { label: "Copy row", icon: Rows3, onSelect: () => copyRow(cell.row) },
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

    return [...items, ...rowActions(cell.row)];
  }

  function rowActions(row: number): MenuItem[] {
    const editing = props.editing;
    if (!editing) return [];
    return editing.rowState(row) === "deleted"
      ? [{ label: "Restore row", icon: Undo2, onSelect: () => editing.restoreRow(row) }]
      : [{ label: "Delete row", icon: Trash2, danger: true, onSelect: () => editing.deleteRow(row) }];
  }

  function rowMenu(row: number): MenuItem[] {
    const items: MenuItem[] = [{ label: "Copy row", icon: Rows3, onSelect: () => copyRow(row) }];
    const actions = rowActions(row);
    return actions.length > 0 ? [...items, { separator: true }, ...actions] : items;
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
            {(column, index) => (
              <div
                ref={element => tooltip(element, () => `${column.name} ${column.typeName}${column.nullable ? "" : " NOT NULL"}${column.table ? ` (${column.table})` : ""}`)}
                class={styles.HeaderCell}
                classList={{ [styles.Numeric]: NUMERIC.has(column.category) }}
                style={{ width: `${widths[index()]}px` }}
              >
                <div class={styles.ColumnName}>
                  <Show when={column.primaryKey}><KeyRound size={12} class={styles.KeyIcon} /></Show>
                  <span>{column.name}</span>
                </div>
                <div class={styles.ColumnType}>{column.typeName}</div>
                <div class={styles.Resizer} onMouseDown={event => startResize(event, index())} />
              </div>
            )}
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
                  style={{ width: `${rowNumberWidth()}px` }}
                  onContextMenu={event => openContextMenu(event, rowMenu(row))}
                >
                  {state() === "new" ? "+" : (props.rowOffset ?? 0) + row + 1}
                </div>
                <Index each={props.columns}>
                  {(column, columnIndex) => {
                    const value = createMemo(() => props.rows[row]?.[columnIndex] ?? null);
                    const isSelected = createMemo(() => selected()?.row === row && selected()?.column === columnIndex);
                    const isEditing = createMemo(() => inlineEdit()?.row === row && inlineEdit()?.column === columnIndex);
                    const edited = createMemo(() => props.editing?.isCellEdited(row, columnIndex) ?? false);
                    const usesDefault = createMemo(() => props.editing?.usesDefault(row, columnIndex) ?? false);

                    return (
                      <div
                        class={styles.Cell}
                        classList={{
                          [styles.Selected]: isSelected(),
                          [styles.Edited]: edited(),
                          [styles.Numeric]: NUMERIC.has(column().category),
                          [styles.Binary]: column().category === "binary" || column().category === "spatial",
                          [styles.Temporal]: column().category === "temporal",
                        }}
                        style={{ width: `${widths[columnIndex]}px` }}
                        onMouseDown={event => {
                          if (event.button !== 0) return;
                          setSelected({ row, column: columnIndex });
                          // Repeat clicks would otherwise select the word or line under the pointer.
                          if (event.detail > 1) event.preventDefault();
                          if (event.detail === 2) startEditing({ row, column: columnIndex });
                        }}
                        onContextMenu={event => {
                          setSelected({ row, column: columnIndex });
                          openContextMenu(event, cellMenu({ row, column: columnIndex }));
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
