import styles from "./TableDesignerTab.module.scss";
import { Index, JSX, Match, Show, Switch, createEffect, createMemo, createSignal, on, untrack } from "solid-js";
import { createStore, produce, reconcile, unwrap } from "solid-js/store";
import { Dynamic } from "solid-js/web";
import {
  ArrowDown, ArrowRight, ArrowUp, CircleAlert, Columns3, Fingerprint, KeyRound, Link2, ListTree, Plus, RotateCcw,
  Trash2, TriangleAlert, X,
} from "lucide-solid";
import { databaseModule } from "@core/databases";
import { safeInvoke } from "@core/helpers/safeInvoke";
import { errorMessage } from "@core/helpers/errors";
import { batchFailureMessage } from "@core/helpers/batch";
import { DefaultProblem, checkDefault } from "@core/helpers/defaultValues";
import useWorkspaceState from "@core/states/workspaceState";
import useToastState from "@core/states/toastState";
import {
  BatchOutcome, CatalogColumn, ColumnDesign, DesignOptions, ForeignKeyDesign, IndexDesign, IndexKind, TableDesign,
} from "@core/types";
import Button from "@core/components/Button/Button";
import Input from "@core/components/Input/Input";
import Select from "@core/components/Select/Select";
import SuggestInput from "@core/components/SuggestInput/SuggestInput";
import SegmentedControl from "@core/components/SegmentedControl/SegmentedControl";
import Toggle from "@core/components/Switch/Switch";
import Spinner from "@core/components/Spinner/Spinner";
import EmptyState from "@core/components/EmptyState/EmptyState";
import { tooltip } from "@core/components/Tooltip/Tooltip";
import Field, { FieldRow } from "@core/components/Field/Field";
import ReviewSqlModal, { ApplySafety } from "@core/components/ReviewSqlModal/ReviewSqlModal";

export type TableDesignerTabProps = {
  sessionId: string,
  database: string,
  tabId: string,
  table: string | null,
};

type Section = "columns" | "indexes" | "foreignKeys";

const DEFAULT_OPTION = "";
const KIND_LABELS: Record<IndexKind, string> = {
  primary: "Primary",
  unique: "Unique",
  index: "Index",
  fulltext: "Fulltext",
  spatial: "Spatial",
};

function clone<T>(value: T): T {
  return JSON.parse(JSON.stringify(value));
}

function newColumn(name: string, dataType: string): ColumnDesign {
  return {
    originalName: null,
    name,
    dataType,
    nullable: true,
    defaultValue: null,
    autoIncrement: false,
    onUpdate: null,
    comment: "",
    collation: null,
    generatedExpression: null,
    generatedStored: false,
  };
}

function blankDesign(engine: string | null): TableDesign {
  const id = { ...newColumn("id", "INT"), nullable: false, autoIncrement: true };
  return {
    name: "",
    comment: "",
    engine,
    charset: null,
    collation: null,
    columns: [id],
    indexes: [{
      originalName: null,
      name: "PRIMARY",
      kind: "primary",
      columns: [{ name: "id", length: null }],
      method: null,
      constraint: false,
    }],
    foreignKeys: [],
  };
}

function uniqueName(base: string, taken: string[]): string {
  const lower = new Set(taken.map(name => name.toLowerCase()));
  if (!lower.has(base.toLowerCase())) return base;
  for (let index = 2; ; index++) {
    const candidate = `${base}_${index}`;
    if (!lower.has(candidate.toLowerCase())) return candidate;
  }
}

function indexIcon(kind: IndexKind) {
  if (kind === "primary") return KeyRound;
  if (kind === "unique") return Fingerprint;
  return ListTree;
}

function Inspector(props: { kind: string, title: string, children: JSX.Element }) {
  return (
    <aside class={styles.Inspector}>
      <header class={styles.InspectorHeader}>
        <span class={styles.InspectorKind}>{props.kind}</span>
        <span class={styles.InspectorTitle} ref={element => tooltip(element, () => props.title)}>{props.title}</span>
      </header>
      <div class={styles.InspectorBody}>{props.children}</div>
    </aside>
  );
}

function IdleInspector(props: { text: string }) {
  return (
    <aside class={`${styles.Inspector} ${styles.Idle}`}>
      <span>{props.text}</span>
    </aside>
  );
}

function TableDesignerTab(props: TableDesignerTabProps) {
  const workspace = useWorkspaceState;
  const { pushToast, pushError } = useToastState;
  const kind = untrack(() => workspace.store.sessions[props.sessionId].info.kind);
  const module = databaseModule(kind);
  const atomicDdl = untrack(() => workspace.store.sessions[props.sessionId].info.server.atomicDdl);
  const transactionalDdl = untrack(() => workspace.store.sessions[props.sessionId].info.server.transactionalDdl);
  const applySafety: ApplySafety = transactionalDdl ? { kind: "transaction" } : { kind: "schema", atomic: atomicDdl };

  const [design, setDesign] = createStore<TableDesign>(blankDesign(null));
  const [original, setOriginal] = createSignal<TableDesign | null>(null);
  const [options, setOptions] = createSignal<DesignOptions | null>(null);
  const [loading, setLoading] = createSignal(true);
  const [loadError, setLoadError] = createSignal<unknown>(null);
  const [section, setSection] = createSignal<Section>("columns");
  const [selectedColumn, setSelectedColumn] = createSignal<number | null>(0);
  const [selectedIndex, setSelectedIndex] = createSignal<number | null>(null);
  const [selectedKey, setSelectedKey] = createSignal<number | null>(null);
  const [review, setReview] = createSignal<string[] | null>(null);
  const [baseline, setBaseline] = createSignal("");

  const session = createMemo(() => workspace.store.sessions[props.sessionId]);
  const isNew = createMemo(() => original() === null);
  const modified = createMemo(() => JSON.stringify(design) !== baseline());
  const columnNames = createMemo(() => design.columns.map(column => column.name));
  const primaryIndex = createMemo(() => design.indexes.findIndex(index => index.kind === "primary"));
  const capabilities = createMemo(() => options()?.capabilities ?? null);
  const canReorder = createMemo(() => isNew() || (capabilities()?.reorderColumns ?? false));
  const hasMethods = createMemo(() => (options()?.indexMethods.length ?? 0) > 0);
  const methodItems = createMemo(() => [
    { value: DEFAULT_OPTION, label: "Server default" },
    ...(options()?.indexMethods ?? []).map(method => ({ value: method, label: method })),
  ]);
  const collationChoices = createMemo(() => [
    { value: DEFAULT_OPTION, label: capabilities()?.charsets ? "Table default" : "Default" },
    ...(options()?.collations ?? []).map(collation => ({ value: collation.name, label: collation.name })),
  ]);
  const storageOptions = createMemo(() => [
    ...(capabilities()?.virtualColumns ? [{ value: "virtual" as const, label: "Virtual" }] : []),
    { value: "stored" as const, label: "Stored" },
  ]);
  const columnItems = createMemo(() => design.columns.map(column => ({ value: column.name, label: column.name })));

  const tables = createMemo(() =>
    (session()?.objects[props.database]?.items ?? []).filter(object => object.kind === "table").map(object => object.name),
  );
  const tableItems = createMemo(() => tables().map(table => ({ value: table, label: table })));

  const activeColumn = createMemo(() => {
    const position = selectedColumn();
    return position === null ? undefined : design.columns[position];
  });
  const activeIndex = createMemo(() => {
    const position = selectedIndex();
    return position === null ? undefined : design.indexes[position];
  });
  const activeKey = createMemo(() => {
    const position = selectedKey();
    return position === null ? undefined : design.foreignKeys[position];
  });

  const columnTitle = createMemo(() => activeColumn()?.name || "Unnamed column");
  const activeDefaultProblem = createMemo(() => {
    const column = activeColumn();
    return column ? defaultProblem(column) : null;
  });
  const defaultLocked = createMemo(() => {
    const column = activeColumn();
    return column !== undefined && (column.generatedExpression !== null || column.autoIncrement);
  });
  const defaultPlaceholder = createMemo(() => (defaultLocked() ? "Not used" : "None"));
  const expressionMissing = createMemo(() => activeColumn()?.generatedExpression?.trim() === "");
  const defaultHint = createMemo(() => {
    const column = activeColumn();
    if (!column || activeDefaultProblem()) return undefined;
    if (column.generatedExpression !== null) return "Generated columns compute their own value.";
    if (column.autoIncrement) return "Auto increment fills this in.";
    return "SQL, quotes included. For example 'draft', 0 or CURRENT_TIMESTAMP.";
  });
  const collationHint = createMemo(() => {
    const column = activeColumn();
    return column && !module.isTextType(column.dataType) ? "Only text columns have a collation." : undefined;
  });

  const indexKindLabel = createMemo(() => `${KIND_LABELS[activeIndex()?.kind ?? "index"]} index`);
  const indexTitle = createMemo(() => activeIndex()?.name || "Unnamed index");
  const indexNameHint = createMemo(() => (activeIndex()?.kind === "primary" ? "The server names the primary key." : undefined));
  const remainingColumns = createMemo(() => {
    const index = activeIndex();
    if (!index) return [];
    return columnItems().filter(item => !index.columns.some(column => column.name === item.value));
  });
  const addColumnPlaceholder = createMemo(() => (remainingColumns().length > 0 ? "Add a column" : "Every column is in this index"));

  const keyTitle = createMemo(() => activeKey()?.name || "Unnamed foreign key");
  const keyFromLabel = createMemo(() => design.name.trim() || "This table");
  const keyToLabel = createMemo(() => activeKey()?.referencedTable || "Referenced table");
  const keyIssue = createMemo(() => {
    const key = activeKey();
    return key ? keyProblem(key) : null;
  });

  function defaultProblem(column: ColumnDesign): DefaultProblem | null {
    if (column.generatedExpression !== null || column.autoIncrement) return null;
    return checkDefault(module.defaultKind(column.dataType), column.defaultValue ?? "", module.backslashEscapes);
  }

  function setDefault(position: number, value: string) {
    updateColumn(position, current => { current.defaultValue = value.trim() ? value : null; });
  }

  function catalogColumns(table: string): CatalogColumn[] {
    return (session()?.columns[props.database]?.items ?? []).filter(column => column.table === table);
  }

  function localType(name: string): string | undefined {
    return design.columns.find(column => column.name === name)?.dataType;
  }

  function referencedType(table: string, name: string): string | undefined {
    return catalogColumns(table).find(column => column.name === name)?.dataType;
  }

  // Unknown types never block a pair. The server has the final say when the change is applied.
  function typesMatch(left: string | undefined, right: string | undefined): boolean {
    if (!left || !right) return true;
    return module.keyTypeFamily(left) === module.keyTypeFamily(right);
  }

  function keyProblem(key: ForeignKeyDesign): string | null {
    if (!key.referencedTable) return "Pick the table this key points to.";
    if (key.columns.length === 0) return "Add at least one column pair.";
    if (key.columns.some(column => !column) || key.referencedColumns.some(column => !column)) {
      return "Every pair needs a column on both sides.";
    }
    for (const [pair, column] of key.columns.entries()) {
      const referenced = key.referencedColumns[pair];
      const local = localType(column);
      const remote = referencedType(key.referencedTable, referenced);
      if (!typesMatch(local, remote)) {
        return `${column} is ${local} but ${key.referencedTable}.${referenced} is ${remote}, so they can't be linked.`;
      }
    }
    return null;
  }

  async function load(table: string | null) {
    setLoading(true);
    setLoadError(null);
    try {
      const [designOptions, loaded] = await Promise.all([
        options() ?? safeInvoke("get_design_options", { sessionId: props.sessionId }),
        table ? safeInvoke("get_table_design", { sessionId: props.sessionId, database: props.database, table }) : null,
      ]);
      setOptions(designOptions);
      setOriginal(loaded);
      const initial = loaded ? clone(loaded) : blankDesign(designOptions.defaultEngine);
      setDesign(reconcile(initial));
      setBaseline(JSON.stringify(initial));
      setSelectedColumn(initial.columns.length > 0 ? 0 : null);
      setSelectedIndex(initial.indexes.length > 0 ? 0 : null);
      setSelectedKey(initial.foreignKeys.length > 0 ? 0 : null);
      workspace.ensureCatalog(props.sessionId, props.database);
    } catch (error) {
      setLoadError(error);
    } finally {
      setLoading(false);
    }
  }

  createEffect(on(() => props.table, table => load(table)));

  function revert() {
    const initial = JSON.parse(baseline()) as TableDesign;
    setDesign(reconcile(initial));
    setSelectedColumn(initial.columns.length > 0 ? 0 : null);
    setSelectedIndex(initial.indexes.length > 0 ? 0 : null);
    setSelectedKey(initial.foreignKeys.length > 0 ? 0 : null);
  }

  function addColumn() {
    const name = uniqueName("column", columnNames());
    setDesign("columns", columns => [...columns, newColumn(name, "VARCHAR(45)")]);
    setSelectedColumn(design.columns.length - 1);
  }

  function removeColumn(position: number) {
    const name = design.columns[position].name;
    setDesign(produce(table => {
      table.columns.splice(position, 1);
      for (const index of table.indexes) index.columns = index.columns.filter(column => column.name !== name);
      table.indexes = table.indexes.filter(index => index.columns.length > 0);
      for (const key of table.foreignKeys) {
        const keep = key.columns.map(column => column !== name);
        key.columns = key.columns.filter((_, at) => keep[at]);
        key.referencedColumns = key.referencedColumns.filter((_, at) => keep[at]);
      }
      table.foreignKeys = table.foreignKeys.filter(key => key.columns.length > 0);
    }));
    setSelectedColumn(design.columns.length === 0 ? null : Math.min(position, design.columns.length - 1));
    if ((selectedIndex() ?? 0) >= design.indexes.length) setSelectedIndex(design.indexes.length > 0 ? 0 : null);
    if ((selectedKey() ?? 0) >= design.foreignKeys.length) setSelectedKey(design.foreignKeys.length > 0 ? 0 : null);
  }

  function moveColumn(position: number, delta: number) {
    const target = position + delta;
    if (target < 0 || target >= design.columns.length) return;
    setDesign("columns", produce(columns => {
      const [column] = columns.splice(position, 1);
      columns.splice(target, 0, column);
    }));
    setSelectedColumn(target);
  }

  function renameColumn(position: number, name: string) {
    const previous = design.columns[position].name;
    setDesign(produce(table => {
      table.columns[position].name = name;
      for (const index of table.indexes) {
        for (const column of index.columns) if (column.name === previous) column.name = name;
      }
      for (const key of table.foreignKeys) {
        key.columns = key.columns.map(column => (column === previous ? name : column));
      }
    }));
  }

  function updateColumn(position: number, change: (column: ColumnDesign) => void) {
    setDesign("columns", position, produce(change));
  }

  function hasTypeFlag(dataType: string, flag: string): boolean {
    return new RegExp(`\\b${flag}\\b`, "i").test(dataType);
  }

  function withTypeFlag(dataType: string, flag: string, on: boolean): string {
    const stripped = dataType.replace(new RegExp(`\\s*\\b${flag}\\b`, "gi"), "").trim();
    return on ? `${stripped} ${flag}` : stripped;
  }

  // ZEROFILL implies UNSIGNED, so turning UNSIGNED off drops ZEROFILL too.
  function setUnsigned(position: number, on: boolean) {
    updateColumn(position, current => {
      current.dataType = withTypeFlag(current.dataType, "UNSIGNED", on);
      if (!on) current.dataType = withTypeFlag(current.dataType, "ZEROFILL", false);
    });
  }

  function setZerofill(position: number, on: boolean) {
    updateColumn(position, current => {
      current.dataType = withTypeFlag(current.dataType, "ZEROFILL", on);
      if (on && !hasTypeFlag(current.dataType, "UNSIGNED")) current.dataType = withTypeFlag(current.dataType, "UNSIGNED", true);
    });
  }

  function setGenerated(position: number, on: boolean) {
    updateColumn(position, current => {
      if (!on) {
        current.generatedExpression = null;
        return;
      }
      current.generatedExpression ??= "";
      current.defaultValue = null;
      current.autoIncrement = false;
      if (!capabilities()?.virtualColumns) current.generatedStored = true;
    });
    setSelectedColumn(position);
  }

  function uniqueIndexOf(name: string): number {
    return design.indexes.findIndex(index =>
      index.kind === "unique" && index.columns.length === 1 && index.columns[0].name === name);
  }

  function toggleUnique(name: string) {
    const existing = uniqueIndexOf(name);
    if (existing >= 0) {
      removeIndex(existing);
      return;
    }
    const indexName = uniqueName(`${name}_unique`, design.indexes.map(index => index.name));
    setDesign("indexes", indexes => [
      ...indexes,
      { originalName: null, name: indexName, kind: "unique", columns: [{ name, length: null }], method: null, constraint: false },
    ]);
  }

  function isPrimary(name: string): boolean {
    const index = primaryIndex();
    return index >= 0 && design.indexes[index].columns.some(column => column.name === name);
  }

  function togglePrimary(name: string) {
    setDesign(produce(table => {
      let index = table.indexes.find(candidate => candidate.kind === "primary");
      if (!index) {
        index = { originalName: null, name: "PRIMARY", kind: "primary", columns: [], method: null, constraint: false };
        table.indexes.unshift(index);
      }

      if (index.columns.some(column => column.name === name)) {
        index.columns = index.columns.filter(column => column.name !== name);
        if (index.columns.length === 0) table.indexes = table.indexes.filter(candidate => candidate !== index);
      } else {
        index.columns.push({ name, length: null });
        const column = table.columns.find(candidate => candidate.name === name);
        if (column) column.nullable = false;
      }
    }));
  }

  function addIndex() {
    const name = uniqueName(`${design.name || "table"}_idx`, design.indexes.map(index => index.name));
    const column = selectedColumn() !== null ? design.columns[selectedColumn()!]?.name : undefined;
    setDesign("indexes", indexes => [
      ...indexes,
      {
        originalName: null,
        name,
        kind: "index",
        columns: column ? [{ name: column, length: null }] : [],
        method: null,
        constraint: false,
      },
    ]);
    setSelectedIndex(design.indexes.length - 1);
  }

  function removeIndex(position: number) {
    setDesign("indexes", indexes => indexes.filter((_, at) => at !== position));
    setSelectedIndex(design.indexes.length === 0 ? null : Math.min(position, design.indexes.length - 1));
  }

  function updateIndex(position: number, change: (index: IndexDesign) => void) {
    setDesign("indexes", position, produce(change));
  }

  function moveIndexColumn(position: number, from: number, delta: number) {
    updateIndex(position, index => {
      const target = from + delta;
      if (target < 0 || target >= index.columns.length) return;
      const [column] = index.columns.splice(from, 1);
      index.columns.splice(target, 0, column);
    });
  }

  function addKey() {
    const name = uniqueName(`fk_${design.name || "table"}`, design.foreignKeys.map(key => key.name));
    const action = module.referentialActions[0];
    const column = selectedColumn() !== null ? design.columns[selectedColumn()!]?.name ?? "" : "";
    setDesign("foreignKeys", keys => [
      ...keys,
      {
        originalName: null,
        name,
        columns: [column],
        referencedDatabase: props.database,
        referencedTable: "",
        referencedColumns: [""],
        onUpdate: action,
        onDelete: action,
      },
    ]);
    setSelectedKey(design.foreignKeys.length - 1);
  }

  function removeKey(position: number) {
    setDesign("foreignKeys", keys => keys.filter((_, at) => at !== position));
    setSelectedKey(design.foreignKeys.length === 0 ? null : Math.min(position, design.foreignKeys.length - 1));
  }

  function updateKey(position: number, change: (key: ForeignKeyDesign) => void) {
    setDesign("foreignKeys", position, produce(change));
  }

  // Each pair keeps or picks a referenced column whose type fits its own column, `id` first. None fits, none is picked.
  function pointKeyAt(position: number, table: string) {
    const available = catalogColumns(table);

    function pick(column: string, current: string): string {
      const type = localType(column);
      if (!type) return "";
      const fitting = available.filter(candidate => typesMatch(type, candidate.dataType));
      if (fitting.some(candidate => candidate.name === current)) return current;
      return (fitting.find(candidate => candidate.name === "id") ?? fitting[0])?.name ?? "";
    }

    updateKey(position, key => {
      key.referencedTable = table;
      key.referencedDatabase = props.database;
      if (key.columns.length === 0) key.columns.push("");
      key.referencedColumns = key.columns.map((column, pair) => pick(column, key.referencedColumns[pair] ?? ""));
    });
  }

  async function startReview() {
    try {
      const statements = await safeInvoke("preview_table_design", {
        sessionId: props.sessionId,
        database: props.database,
        original: original(),
        design: clone(unwrap(design)),
      });
      if (statements.length === 0) {
        pushToast("Nothing to change", "info", 2000);
        return;
      }
      setReview(statements);
    } catch (error) {
      pushError(error);
    }
  }

  async function apply(): Promise<string | null> {
    const statements = review();
    if (!statements) return null;

    let outcome: BatchOutcome;
    try {
      outcome = await safeInvoke("execute_batch", {
        sessionId: props.sessionId,
        database: props.database,
        statements,
        transactional: transactionalDdl,
      });
    } catch (error) {
      return errorMessage(error);
    }
    if (outcome.error) return batchFailureMessage(outcome);

    const name = design.name.trim();
    pushToast(isNew() ? `Created ${name}` : `Updated ${name}`, "success", 2500);
    workspace.refreshCatalog(props.sessionId, props.database);

    if (name !== props.table) {
      workspace.updateTab(props.sessionId, props.database, props.tabId, tab => {
        if (tab.kind !== "design") return;
        tab.table = name;
        tab.title = name;
      });
    } else {
      await load(name);
    }
    return null;
  }

  const charsetItems = createMemo(() => [
    { value: DEFAULT_OPTION, label: "Server default" },
    ...(options()?.charsets ?? []).map(charset => ({ value: charset.name, label: charset.name })),
  ]);

  const collationItems = createMemo(() => {
    const charset = design.charset;
    const collations = (options()?.collations ?? []).filter(collation => !charset || collation.charset === charset);
    return [
      { value: DEFAULT_OPTION, label: "Default" },
      ...collations.map(collation => ({ value: collation.name, label: collation.name })),
    ];
  });

  const engineItems = createMemo(() => [
    { value: DEFAULT_OPTION, label: "Server default" },
    ...(options()?.engines ?? []).map(engine => ({ value: engine, label: engine })),
  ]);

  const actionItems = module.referentialActions.map(action => ({ value: action, label: action }));
  const indexKindItems = module.indexKinds.map(item => ({ value: item.value, label: item.label }));

  return (
    <div class={styles.Designer}>
      <Switch>
        <Match when={loadError()}>
          <EmptyState
            tone="danger"
            icon={CircleAlert}
            title={`Couldn't load ${props.table ?? "the designer"}`}
            description={errorMessage(loadError())}
            actions={<Button variant="subtle" onClick={() => load(props.table)}>Try again</Button>}
          />
        </Match>
        <Match when={loading() && !options()}>
          <div class={styles.Center}><Spinner size={18} /></div>
        </Match>
        <Match when={true}>
          <div class={styles.TableForm}>
            <Field label="Table name" width={240}>
              <Input value={design.name} onValue={value => setDesign("name", value)} placeholder="new_table" autofocus={isNew()} />
            </Field>
            <Show when={capabilities()?.engines}>
              <Field label="Engine" width={150}>
                <Select
                  value={design.engine ?? DEFAULT_OPTION}
                  items={engineItems()}
                  onChange={value => setDesign("engine", value || null)}
                />
              </Field>
            </Show>
            <Show when={capabilities()?.charsets}>
              <Field label="Character set" width={150}>
                <Select
                  value={design.charset ?? DEFAULT_OPTION}
                  items={charsetItems()}
                  onChange={value => {
                    const charset = options()?.charsets.find(item => item.name === value);
                    setDesign(produce(table => {
                      table.charset = value || null;
                      table.collation = charset?.defaultCollation ?? null;
                    }));
                  }}
                />
              </Field>
              <Field label="Collation" width={210}>
                <Select
                  value={design.collation ?? DEFAULT_OPTION}
                  items={collationItems()}
                  onChange={value => setDesign("collation", value || null)}
                />
              </Field>
            </Show>
            <Field label="Comment" grow>
              <Input value={design.comment} onValue={value => setDesign("comment", value)} placeholder="What this table holds" />
            </Field>
          </div>

          <div class={styles.SectionBar}>
            <SegmentedControl
              value={section()}
              onChange={setSection}
              options={[
                { value: "columns", label: "Columns", count: design.columns.length },
                { value: "indexes", label: "Indexes", count: design.indexes.length },
                { value: "foreignKeys", label: "Foreign keys", count: design.foreignKeys.length },
              ]}
            />
            <div class={styles.Spacer} />
            <Switch>
              <Match when={section() === "columns"}>
                <Show when={canReorder()}>
                  <Button icon size="sm" tooltip="Move up" disabled={!selectedColumn()} onClick={() => moveColumn(selectedColumn()!, -1)}>
                    <ArrowUp size={14} />
                  </Button>
                  <Button
                    icon
                    size="sm"
                    tooltip="Move down"
                    disabled={selectedColumn() === null || selectedColumn()! >= design.columns.length - 1}
                    onClick={() => moveColumn(selectedColumn()!, 1)}
                  >
                    <ArrowDown size={14} />
                  </Button>
                  <div class={styles.BarDivider} />
                </Show>
                <Button size="sm" variant="subtle" onClick={addColumn}><Plus size={14} />Add column</Button>
              </Match>
              <Match when={section() === "indexes"}>
                <Button size="sm" variant="subtle" onClick={addIndex}><Plus size={14} />Add index</Button>
              </Match>
              <Match when={section() === "foreignKeys"}>
                <Button size="sm" variant="subtle" onClick={addKey}><Plus size={14} />Add foreign key</Button>
              </Match>
            </Switch>
          </div>

          <div class={styles.Body}>
            <Switch>
              <Match when={section() === "columns"}>
                <Show
                  when={design.columns.length > 0}
                  fallback={
                    <EmptyState
                      icon={Columns3}
                      title="No columns"
                      description="A table needs at least one column."
                      actions={<Button variant="subtle" onClick={addColumn}><Plus size={14} />Add column</Button>}
                    />
                  }
                >
                  <div class={styles.GridWrap}>
                    <table class={styles.Grid}>
                      <thead>
                        <tr>
                          <th class={styles.Lead} />
                          <th class={styles.NameHead}>Name</th>
                          <th class={styles.TypeHead}>Type</th>
                          <th class={styles.Check} ref={element => tooltip(element, () => "Primary key")}>PK</th>
                          <th class={styles.Check} ref={element => tooltip(element, () => "Not null")}>NN</th>
                          <th class={styles.Check} ref={element => tooltip(element, () => "Unique")}>UQ</th>
                          <Show when={capabilities()?.unsigned}>
                            <th class={styles.Check} ref={element => tooltip(element, () => "Unsigned")}>UN</th>
                          </Show>
                          <Show when={capabilities()?.zerofill}>
                            <th class={styles.Check} ref={element => tooltip(element, () => "Zero fill, pads numbers with leading zeros")}>ZF</th>
                          </Show>
                          <th class={styles.Check} ref={element => tooltip(element, () => "Auto increment")}>AI</th>
                          <th class={styles.Check} ref={element => tooltip(element, () => "Generated")}>G</th>
                          <th>Default</th>
                          <th>Comment</th>
                          <th class={styles.Lead} />
                        </tr>
                      </thead>
                      <tbody>
                        <Index each={design.columns}>
                          {(column, position) => (
                            <tr
                              classList={{ [styles.SelectedRow]: selectedColumn() === position }}
                              onFocusIn={() => setSelectedColumn(position)}
                              onClick={() => setSelectedColumn(position)}
                            >
                              <td class={styles.Lead}>
                                <Show when={isPrimary(column().name)}><KeyRound size={12} class={styles.KeyIcon} /></Show>
                              </td>
                              <td>
                                <input
                                  class={styles.CellInput}
                                  value={column().name}
                                  spellcheck={false}
                                  onChange={event => renameColumn(position, event.currentTarget.value.trim())}
                                />
                              </td>
                              <td>
                                <SuggestInput
                                  class={`${styles.CellInput} ${styles.Mono}`}
                                  mono
                                  value={column().dataType}
                                  suggestions={module.dataTypes}
                                  onCommit={value => updateColumn(position, current => {
                                    current.dataType = value;
                                    if (!module.isTextType(value)) current.collation = null;
                                  })}
                                />
                              </td>
                              <td class={styles.Check}>
                                <input type="checkbox" checked={isPrimary(column().name)} onChange={() => togglePrimary(column().name)} />
                              </td>
                              <td class={styles.Check}>
                                <input
                                  type="checkbox"
                                  checked={!column().nullable}
                                  onChange={event => {
                                    const checked = event.currentTarget.checked;
                                    updateColumn(position, current => { current.nullable = !checked; });
                                  }}
                                />
                              </td>
                              <td class={styles.Check}>
                                <input
                                  type="checkbox"
                                  checked={uniqueIndexOf(column().name) >= 0}
                                  onChange={() => toggleUnique(column().name)}
                                />
                              </td>
                              <Show when={capabilities()?.unsigned}>
                                <td class={styles.Check}>
                                  <input
                                    type="checkbox"
                                    checked={hasTypeFlag(column().dataType, "UNSIGNED")}
                                    disabled={!module.allowsUnsigned(column().dataType)}
                                    onChange={event => setUnsigned(position, event.currentTarget.checked)}
                                  />
                                </td>
                              </Show>
                              <Show when={capabilities()?.zerofill}>
                                <td class={styles.Check}>
                                  <input
                                    type="checkbox"
                                    checked={hasTypeFlag(column().dataType, "ZEROFILL")}
                                    disabled={!module.allowsUnsigned(column().dataType)}
                                    onChange={event => setZerofill(position, event.currentTarget.checked)}
                                  />
                                </td>
                              </Show>
                              <td class={styles.Check}>
                                <input
                                  type="checkbox"
                                  checked={column().autoIncrement}
                                  disabled={column().generatedExpression !== null}
                                  onChange={event => {
                                    const checked = event.currentTarget.checked;
                                    updateColumn(position, current => {
                                      current.autoIncrement = checked;
                                      if (checked) current.defaultValue = null;
                                    });
                                  }}
                                />
                              </td>
                              <td class={styles.Check}>
                                <input
                                  type="checkbox"
                                  checked={column().generatedExpression !== null}
                                  onChange={event => setGenerated(position, event.currentTarget.checked)}
                                />
                              </td>
                              <td>
                                <input
                                  class={`${styles.CellInput} ${styles.Mono}`}
                                  classList={{ [styles.Invalid]: defaultProblem(column()) !== null }}
                                  ref={element => tooltip(element, () => defaultProblem(column())?.message)}
                                  value={column().defaultValue ?? ""}
                                  placeholder={column().generatedExpression !== null ? "generated" : column().autoIncrement ? "auto" : "none"}
                                  disabled={column().generatedExpression !== null || column().autoIncrement}
                                  spellcheck={false}
                                  onChange={event => setDefault(position, event.currentTarget.value)}
                                />
                              </td>
                              <td>
                                <input
                                  class={styles.CellInput}
                                  value={column().comment}
                                  onChange={event => {
                                    const value = event.currentTarget.value;
                                    updateColumn(position, current => { current.comment = value; });
                                  }}
                                />
                              </td>
                              <td class={styles.Lead}>
                                <Button icon size="sm" class={styles.RowAction} tooltip="Remove column" onClick={() => removeColumn(position)}>
                                  <Trash2 size={14} />
                                </Button>
                              </td>
                            </tr>
                          )}
                        </Index>
                      </tbody>
                    </table>
                  </div>

                  <Show when={activeColumn()} fallback={<IdleInspector text="Select a column to see more of its settings." />}>
                    {column => (
                      <Inspector kind="Column" title={columnTitle()}>
                        <Field label="Default" hint={defaultHint()}>
                          <Input
                            mono
                            value={column().defaultValue ?? ""}
                            placeholder={defaultPlaceholder()}
                            disabled={defaultLocked()}
                            onValue={value => setDefault(selectedColumn()!, value)}
                          />
                          <Show when={activeDefaultProblem()}>
                            {problem => (
                              <div class={styles.FieldProblem}>
                                <TriangleAlert size={14} />
                                <span>{problem().message}</span>
                                <Show when={problem().fix}>
                                  {fix => (
                                    <Button size="sm" variant="subtle" class={styles.FixButton} onClick={() => setDefault(selectedColumn()!, fix())}>
                                      Use {fix()}
                                    </Button>
                                  )}
                                </Show>
                              </div>
                            )}
                          </Show>
                        </Field>
                        <Show when={capabilities()?.columnCollations}>
                          <Field label="Collation" hint={collationHint()}>
                            <Select
                              value={column().collation ?? DEFAULT_OPTION}
                              disabled={!module.isTextType(column().dataType)}
                              items={collationChoices()}
                              onChange={value => updateColumn(selectedColumn()!, current => { current.collation = value || null; })}
                            />
                          </Field>
                        </Show>
                        <Show when={capabilities()?.onUpdate}>
                          <Field label="On update" hint="For example CURRENT_TIMESTAMP.">
                            <Input
                              mono
                              value={column().onUpdate ?? ""}
                              onValue={value => updateColumn(selectedColumn()!, current => { current.onUpdate = value.trim() ? value : null; })}
                            />
                          </Field>
                        </Show>

                        <div class={styles.Group}>
                          <div class={styles.GroupSwitch}>
                            <div class={styles.GroupTitle}>Generated column</div>
                            <Toggle
                              checked={column().generatedExpression !== null}
                              onChange={checked => setGenerated(selectedColumn()!, checked)}
                            />
                          </div>
                          <Show
                            when={column().generatedExpression !== null}
                            fallback={<div class={styles.GroupHint}>Computes its value from other columns instead of storing what you insert.</div>}
                          >
                            <Field label="Expression">
                              <Input
                                mono
                                value={column().generatedExpression ?? ""}
                                placeholder="price * quantity"
                                onValue={value => updateColumn(selectedColumn()!, current => { current.generatedExpression = value; })}
                              />
                              <Show when={expressionMissing()}>
                                <div class={styles.FieldProblem}>
                                  <TriangleAlert size={14} />
                                  <span>Needs an expression.</span>
                                </div>
                              </Show>
                            </Field>
                            <Field label="Storage">
                              <SegmentedControl
                                fill
                                value={column().generatedStored ? "stored" : "virtual"}
                                onChange={value => updateColumn(selectedColumn()!, current => { current.generatedStored = value === "stored"; })}
                                options={storageOptions()}
                              />
                            </Field>
                          </Show>
                        </div>
                      </Inspector>
                    )}
                  </Show>
                </Show>
              </Match>

              <Match when={section() === "indexes"}>
                <Show
                  when={design.indexes.length > 0}
                  fallback={
                    <EmptyState
                      icon={ListTree}
                      title="No indexes"
                      description="Indexes speed up lookups and can keep values unique."
                      actions={<Button variant="subtle" onClick={addIndex}><Plus size={14} />Add index</Button>}
                    />
                  }
                >
                  <div class={styles.GridWrap}>
                    <table class={styles.Grid}>
                      <thead>
                        <tr>
                          <th class={styles.Lead} />
                          <th class={styles.NameHead}>Name</th>
                          <th>Columns</th>
                          <th class={styles.TagHead}>Type</th>
                          <th class={styles.Lead} />
                        </tr>
                      </thead>
                      <tbody>
                        <Index each={design.indexes}>
                          {(index, position) => (
                            <tr
                              class={styles.ListRow}
                              classList={{ [styles.SelectedRow]: selectedIndex() === position }}
                              onClick={() => setSelectedIndex(position)}
                            >
                              <td class={styles.Lead}>
                                <span
                                  class={styles.KindIcon}
                                  classList={{ [styles.KeyIcon]: index().kind === "primary" }}
                                >
                                  <Dynamic component={indexIcon(index().kind)} size={14} />
                                </span>
                              </td>
                              <td class={styles.NameCell}>{index().name || <span class={styles.Faint}>Unnamed</span>}</td>
                              <td>
                                <Show
                                  when={index().columns.length > 0}
                                  fallback={<span class={styles.Warning}>No columns</span>}
                                >
                                  <div class={styles.Chips}>
                                    <Index each={index().columns}>
                                      {column => (
                                        <span class={styles.Chip}>
                                          {column().name}
                                          <Show when={column().length}>{length => <span class={styles.ChipNote}>({length()})</span>}</Show>
                                        </span>
                                      )}
                                    </Index>
                                  </div>
                                </Show>
                              </td>
                              <td>
                                <div class={styles.Tags}>
                                  <span class={styles.Tag}>{KIND_LABELS[index().kind]}</span>
                                  <Show when={index().method}>{method => <span class={styles.Tag}>{method()}</span>}</Show>
                                </div>
                              </td>
                              <td class={styles.Lead}>
                                <Button
                                  icon
                                  size="sm"
                                  class={styles.RowAction}
                                  tooltip="Remove index"
                                  onClick={event => {
                                    event.stopPropagation();
                                    removeIndex(position);
                                  }}
                                >
                                  <Trash2 size={14} />
                                </Button>
                              </td>
                            </tr>
                          )}
                        </Index>
                      </tbody>
                    </table>
                  </div>

                  <Show when={activeIndex()} fallback={<IdleInspector text="Select an index to edit it." />}>
                    {index => (
                      <Inspector kind={indexKindLabel()} title={indexTitle()}>
                        <Field label="Name" hint={indexNameHint()}>
                          <Input
                            value={index().name}
                            disabled={index().kind === "primary"}
                            onValue={value => updateIndex(selectedIndex()!, current => { current.name = value.trim(); })}
                          />
                        </Field>
                        <FieldRow>
                          <Field label="Type" grow>
                            <Select
                              value={index().kind}
                              items={indexKindItems}
                              onChange={value => updateIndex(selectedIndex()!, current => {
                                current.kind = value;
                                if (value === "primary") current.name = "PRIMARY";
                              })}
                            />
                          </Field>
                          <Show when={hasMethods() && index().kind !== "primary"}>
                            <Field label="Method" grow>
                              <Select
                                value={index().method ?? DEFAULT_OPTION}
                                items={methodItems()}
                                onChange={value => updateIndex(selectedIndex()!, current => { current.method = value || null; })}
                              />
                            </Field>
                          </Show>
                        </FieldRow>

                        <div class={styles.Group}>
                          <div class={styles.GroupTitle}>
                            Columns
                            <span class={styles.GroupNote}>in index order</span>
                          </div>
                          <Show
                            when={index().columns.length > 0}
                            fallback={<div class={styles.GroupEmpty}>No columns yet. Add one below.</div>}
                          >
                            <div class={styles.OrderList}>
                              <Index each={index().columns}>
                                {(column, at) => (
                                  <div class={styles.OrderItem}>
                                    <span class={styles.Ordinal}>{at + 1}</span>
                                    <span class={`${styles.OrderName} ${styles.Mono}`}>{column().name}</span>
                                    <Show when={capabilities()?.indexLengths}>
                                      <input
                                        class={`${styles.CellInput} ${styles.LengthInput}`}
                                        placeholder="length"
                                        ref={element => tooltip(element, () => "Index only this many leading characters")}
                                        value={column().length ?? ""}
                                        onChange={event => {
                                          const length = Number.parseInt(event.currentTarget.value, 10);
                                          updateIndex(selectedIndex()!, current => {
                                            current.columns[at].length = Number.isNaN(length) || length <= 0 ? null : length;
                                          });
                                        }}
                                      />
                                    </Show>
                                    <Button icon size="sm" tooltip="Move up" disabled={at === 0} onClick={() => moveIndexColumn(selectedIndex()!, at, -1)}>
                                      <ArrowUp size={14} />
                                    </Button>
                                    <Button
                                      icon
                                      size="sm"
                                      tooltip="Move down"
                                      disabled={at === index().columns.length - 1}
                                      onClick={() => moveIndexColumn(selectedIndex()!, at, 1)}
                                    >
                                      <ArrowDown size={14} />
                                    </Button>
                                    <Button
                                      icon
                                      size="sm"
                                      tooltip="Remove from index"
                                      onClick={() => updateIndex(selectedIndex()!, current => { current.columns.splice(at, 1); })}
                                    >
                                      <X size={14} />
                                    </Button>
                                  </div>
                                )}
                              </Index>
                            </div>
                          </Show>
                          <Select
                            size="sm"
                            value={DEFAULT_OPTION}
                            placeholder={addColumnPlaceholder()}
                            items={remainingColumns()}
                            disabled={remainingColumns().length === 0}
                            onChange={value => updateIndex(selectedIndex()!, current => { current.columns.push({ name: value, length: null }); })}
                          />
                        </div>
                      </Inspector>
                    )}
                  </Show>
                </Show>
              </Match>

              <Match when={section() === "foreignKeys"}>
                <Show
                  when={design.foreignKeys.length > 0}
                  fallback={
                    <EmptyState
                      icon={Link2}
                      title="No foreign keys"
                      description="A foreign key makes sure a value here matches a row in another table."
                      actions={<Button variant="subtle" onClick={addKey}><Plus size={14} />Add foreign key</Button>}
                    />
                  }
                >
                  <div class={styles.GridWrap}>
                    <table class={styles.Grid}>
                      <thead>
                        <tr>
                          <th class={styles.Lead} />
                          <th class={styles.NameHead}>Name</th>
                          <th>Link</th>
                          <th class={styles.TagHead}>Actions</th>
                          <th class={styles.Lead} />
                        </tr>
                      </thead>
                      <tbody>
                        <Index each={design.foreignKeys}>
                          {(key, position) => (
                            <tr
                              class={styles.ListRow}
                              classList={{ [styles.SelectedRow]: selectedKey() === position }}
                              onClick={() => setSelectedKey(position)}
                            >
                              <td class={styles.Lead}>
                                <span class={styles.KindIcon}><Link2 size={14} /></span>
                              </td>
                              <td class={styles.NameCell}>{key().name || <span class={styles.Faint}>Unnamed</span>}</td>
                              <td>
                                <Show
                                  when={!keyProblem(key())}
                                  fallback={<span class={styles.Warning}>{keyProblem(key())}</span>}
                                >
                                  <span class={styles.Link}>
                                    <span class={styles.Mono}>{key().columns.join(", ")}</span>
                                    <ArrowRight size={12} class={styles.LinkArrow} />
                                    <span class={styles.Mono}>
                                      {key().referencedTable}
                                      <span class={styles.Faint}> ({key().referencedColumns.join(", ")})</span>
                                    </span>
                                  </span>
                                </Show>
                              </td>
                              <td>
                                <div class={styles.Tags}>
                                  <span class={styles.Tag}><span class={styles.TagLabel}>Delete</span>{key().onDelete}</span>
                                  <span class={styles.Tag}><span class={styles.TagLabel}>Update</span>{key().onUpdate}</span>
                                </div>
                              </td>
                              <td class={styles.Lead}>
                                <Button
                                  icon
                                  size="sm"
                                  class={styles.RowAction}
                                  tooltip="Remove foreign key"
                                  onClick={event => {
                                    event.stopPropagation();
                                    removeKey(position);
                                  }}
                                >
                                  <Trash2 size={14} />
                                </Button>
                              </td>
                            </tr>
                          )}
                        </Index>
                      </tbody>
                    </table>
                  </div>

                  <Show when={activeKey()} fallback={<IdleInspector text="Select a foreign key to edit it." />}>
                    {key => (
                      <Inspector kind="Foreign key" title={keyTitle()}>
                        <Field label="Name">
                          <Input
                            value={key().name}
                            onValue={value => updateKey(selectedKey()!, current => { current.name = value.trim(); })}
                          />
                        </Field>
                        <Field label="References">
                          <Select
                            value={key().referencedTable}
                            placeholder="Pick a table"
                            items={tableItems()}
                            onChange={value => pointKeyAt(selectedKey()!, value)}
                          />
                        </Field>

                        <div class={styles.Group}>
                          <div class={styles.GroupTitle}>Column pairs</div>
                          <Show
                            when={key().columns.length > 0}
                            fallback={<div class={styles.GroupEmpty}>No pairs yet.</div>}
                          >
                            <div class={styles.PairHead}>
                              <span>{keyFromLabel()}</span>
                              <span />
                              <span>{keyToLabel()}</span>
                              <span />
                            </div>
                            <Index each={key().columns}>
                              {(column, pair) => {
                                const referenced = createMemo(() => key().referencedColumns[pair]);
                                const localItems = createMemo(() => {
                                  const remote = referencedType(key().referencedTable, referenced());
                                  return design.columns.map(item => ({
                                    value: item.name,
                                    label: item.name,
                                    hint: item.dataType,
                                    disabled: item.name !== column() && !typesMatch(item.dataType, remote),
                                  }));
                                });
                                const remoteItems = createMemo(() => {
                                  const local = localType(column());
                                  return catalogColumns(key().referencedTable).map(item => ({
                                    value: item.name,
                                    label: item.name,
                                    hint: item.dataType,
                                    disabled: item.name !== referenced() && !typesMatch(local, item.dataType),
                                  }));
                                });

                                return (
                                  <div class={styles.Pair}>
                                    <Select
                                      size="sm"
                                      value={column()}
                                      placeholder="Column"
                                      items={localItems()}
                                      onChange={value => updateKey(selectedKey()!, current => { current.columns[pair] = value; })}
                                    />
                                    <ArrowRight size={14} class={styles.LinkArrow} />
                                    <Select
                                      size="sm"
                                      value={referenced()}
                                      placeholder="Column"
                                      disabled={!key().referencedTable}
                                      items={remoteItems()}
                                      onChange={value => updateKey(selectedKey()!, current => { current.referencedColumns[pair] = value; })}
                                    />
                                    <Button
                                      icon
                                      size="sm"
                                      tooltip="Remove pair"
                                      onClick={() => updateKey(selectedKey()!, current => {
                                        current.columns.splice(pair, 1);
                                        current.referencedColumns.splice(pair, 1);
                                      })}
                                    >
                                      <X size={14} />
                                    </Button>
                                  </div>
                                );
                              }}
                            </Index>
                          </Show>
                          <Button
                            size="sm"
                            variant="subtle"
                            class={styles.AddRow}
                            onClick={() => updateKey(selectedKey()!, current => {
                              current.columns.push("");
                              current.referencedColumns.push("");
                            })}
                          >
                            <Plus size={14} />
                            Add pair
                          </Button>
                        </div>

                        <FieldRow>
                          <Field label="On delete" grow>
                            <Select
                              value={key().onDelete}
                              items={actionItems}
                              onChange={value => updateKey(selectedKey()!, current => { current.onDelete = value; })}
                            />
                          </Field>
                          <Field label="On update" grow>
                            <Select
                              value={key().onUpdate}
                              items={actionItems}
                              onChange={value => updateKey(selectedKey()!, current => { current.onUpdate = value; })}
                            />
                          </Field>
                        </FieldRow>

                        <Show when={keyIssue()}>
                          {problem => (
                            <div class={styles.Problem}>
                              <TriangleAlert size={14} />
                              <span>{problem()}</span>
                            </div>
                          )}
                        </Show>
                      </Inspector>
                    )}
                  </Show>
                </Show>
              </Match>
            </Switch>
          </div>

          <div class={styles.Footer}>
            <Show when={loading()}><Spinner size={12} /></Show>
            <span class={styles.FooterNote} classList={{ [styles.Pending]: modified() && !isNew() }}>
              {isNew() ? `New table in ${props.database}` : modified() ? "Unapplied changes" : "No changes"}
            </span>
            <div class={styles.Spacer} />
            <Button size="sm" onClick={revert} disabled={!modified()}>
              <RotateCcw size={14} />
              Revert
            </Button>
            <Button size="sm" variant="primary" onClick={startReview} disabled={!design.name.trim() || (!isNew() && !modified())}>
              Review SQL
            </Button>
          </div>
        </Match>
      </Switch>

      <ReviewSqlModal
        statements={review()}
        title={isNew() ? "Create table" : "Alter table"}
        description={isNew() ? `New table in ${props.database}` : `Changes to the structure of ${props.database}.${props.table}`}
        dialect={module.sqlDialect}
        safety={applySafety}
        onApply={apply}
        onClose={() => setReview(null)}
      />
    </div>
  );
}

export default TableDesignerTab;
