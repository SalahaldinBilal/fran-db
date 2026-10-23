import styles from "./StructureTab.module.scss";
import { For, Match, Show, Switch, createMemo, createSignal, onMount, untrack } from "solid-js";
import { CircleAlert, KeyRound, PencilRuler, RefreshCw } from "lucide-solid";
import { databaseModule } from "@core/databases";
import { safeInvoke } from "@core/helpers/safeInvoke";
import { errorMessage } from "@core/helpers/errors";
import { objectKindInfo } from "@core/constants/objectKinds";
import useWorkspaceState from "@core/states/workspaceState";
import { ObjectKind, ObjectStructure } from "@core/types";
import Button from "@core/components/Button/Button";
import SegmentedControl, { SegmentedOption } from "@core/components/SegmentedControl/SegmentedControl";
import Spinner from "@core/components/Spinner/Spinner";
import EmptyState from "@core/components/EmptyState/EmptyState";
import SqlEditor from "@core/components/SqlEditor/SqlEditor";

type Section = "columns" | "indexes" | "foreignKeys" | "ddl";

export type StructureTabProps = {
  sessionId: string,
  database: string,
  objectName: string,
  objectKind: ObjectKind,
  objectSignature: string | null,
};

function StructureTab(props: StructureTabProps) {
  const workspace = useWorkspaceState;
  const kind = untrack(() => workspace.store.sessions[props.sessionId].info.kind);
  const module = databaseModule(kind);
  const hasColumns = untrack(() => objectKindInfo(props.objectKind).hasColumns);

  const [structure, setStructure] = createSignal<ObjectStructure | null>(null);
  const [loading, setLoading] = createSignal(false);
  const [error, setError] = createSignal<unknown>(null);
  const [section, setSection] = createSignal<Section>(hasColumns ? "columns" : "ddl");
  const [ddlVersion, setDdlVersion] = createSignal(0);

  const sections = createMemo(() => {
    const options: SegmentedOption<Section>[] = [];
    if (hasColumns) {
      options.push({ value: "columns", label: "Columns", count: structure()?.columns.length });
    }
    if (props.objectKind === "table") {
      options.push({ value: "indexes", label: "Indexes", count: structure()?.indexes.length });
      options.push({ value: "foreignKeys", label: "Foreign keys", count: structure()?.foreignKeys.length });
    }
    options.push({ value: "ddl", label: "DDL" });
    return options;
  });

  async function load() {
    setLoading(true);
    setError(null);
    try {
      setStructure(await safeInvoke("get_object_structure", {
        sessionId: props.sessionId,
        database: props.database,
        name: props.objectName,
        kind: props.objectKind,
        signature: props.objectSignature,
      }));
      setDdlVersion(version => version + 1);
    } catch (loadError) {
      setError(loadError);
    } finally {
      setLoading(false);
    }
  }

  onMount(load);

  return (
    <div class={styles.StructureTab}>
      <div class={styles.Toolbar}>
        <span class={styles.Kind}>{objectKindInfo(props.objectKind).singular}</span>
        <span class={styles.Name}>{props.objectName}</span>
        <SegmentedControl value={section()} options={sections()} onChange={setSection} />
        <div class={styles.Spacer} />
        <Show when={loading()}><Spinner size={14} /></Show>
        <Show when={props.objectKind === "table"}>
          <Button size="sm" onClick={() => workspace.openDesigner(props.sessionId, props.database, props.objectName)}>
            <PencilRuler size={14} />
            Alter table
          </Button>
        </Show>
        <Button icon size="sm" tooltip="Reload" onClick={load} disabled={loading()}>
          <RefreshCw size={14} />
        </Button>
      </div>

      <div class={styles.Content}>
        <Switch>
          <Match when={error()}>
            <EmptyState
              tone="danger"
              icon={CircleAlert}
              title={`Couldn't read ${props.objectName}`}
              description={errorMessage(error())}
              actions={<Button variant="subtle" onClick={load}>Try again</Button>}
            />
          </Match>
          <Match when={!structure()}>
            <div class={styles.Center}><Spinner size={20} /></div>
          </Match>
          <Match when={section() === "columns"}>
            <div class={styles.TableWrap}>
              <table class={styles.Table}>
                <thead>
                  <tr>
                    <th class={styles.Narrow}>#</th>
                    <th>Name</th>
                    <th>Type</th>
                    <th>Nullable</th>
                    <th>Default</th>
                    <th>Extra</th>
                    <th>Collation</th>
                    <th>Comment</th>
                  </tr>
                </thead>
                <tbody>
                  <For each={structure()!.columns}>
                    {(column, index) => (
                      <tr>
                        <td class={styles.Muted}>{index() + 1}</td>
                        <td class={styles.Strong}>
                          <span class={styles.NameCell}>
                            <Show when={column.primaryKey}><KeyRound size={12} class={styles.KeyIcon} /></Show>
                            {column.name}
                          </span>
                        </td>
                        <td class={styles.Mono}>{column.dataType}</td>
                        <td>{column.nullable ? "Yes" : "No"}</td>
                        <td class={styles.Mono}>
                          <Show when={column.defaultValue !== null} fallback={<span class={styles.Null}>NULL</span>}>
                            {column.defaultValue}
                          </Show>
                        </td>
                        <td class={styles.Muted}>{column.extra ?? ""}</td>
                        <td class={styles.Muted}>{column.collation ?? ""}</td>
                        <td class={styles.Muted}>{column.comment ?? ""}</td>
                      </tr>
                    )}
                  </For>
                </tbody>
              </table>
            </div>
          </Match>
          <Match when={section() === "indexes"}>
            <Show when={structure()!.indexes.length > 0} fallback={<EmptyState title="No indexes" />}>
              <div class={styles.TableWrap}>
                <table class={styles.Table}>
                  <thead>
                    <tr>
                      <th>Name</th>
                      <th>Columns</th>
                      <th>Unique</th>
                      <th>Type</th>
                    </tr>
                  </thead>
                  <tbody>
                    <For each={structure()!.indexes}>
                      {index => (
                        <tr>
                          <td class={styles.Strong}>
                            <span class={styles.NameCell}>
                              <Show when={index.primary}><KeyRound size={12} class={styles.KeyIcon} /></Show>
                              {index.name}
                            </span>
                          </td>
                          <td class={styles.Mono}>{index.columns.join(", ")}</td>
                          <td>{index.unique ? "Yes" : "No"}</td>
                          <td class={styles.Muted}>{index.indexType ?? ""}</td>
                        </tr>
                      )}
                    </For>
                  </tbody>
                </table>
              </div>
            </Show>
          </Match>
          <Match when={section() === "foreignKeys"}>
            <Show when={structure()!.foreignKeys.length > 0} fallback={<EmptyState title="No foreign keys" />}>
              <div class={styles.TableWrap}>
                <table class={styles.Table}>
                  <thead>
                    <tr>
                      <th>Name</th>
                      <th>Columns</th>
                      <th>References</th>
                      <th>On update</th>
                      <th>On delete</th>
                    </tr>
                  </thead>
                  <tbody>
                    <For each={structure()!.foreignKeys}>
                      {key => (
                        <tr>
                          <td class={styles.Strong}>{key.name}</td>
                          <td class={styles.Mono}>{key.columns.join(", ")}</td>
                          <td class={styles.Mono}>
                            {key.referencedDatabase === props.database ? "" : `${key.referencedDatabase}.`}
                            {key.referencedTable} ({key.referencedColumns.join(", ")})
                          </td>
                          <td class={styles.Muted}>{key.onUpdate ?? ""}</td>
                          <td class={styles.Muted}>{key.onDelete ?? ""}</td>
                        </tr>
                      )}
                    </For>
                  </tbody>
                </table>
              </div>
            </Show>
          </Match>
          <Match when={section() === "ddl"}>
            <Show
              when={structure()!.ddl}
              fallback={
                <EmptyState
                  title="No definition available"
                  description="The server didn't return a definition. Your account may lack the privileges to see it."
                />
              }
            >
              {ddl => (
                <Show when={ddlVersion()} keyed>
                  <SqlEditor initialValue={ddl()} dialect={module.sqlDialect} readOnly />
                </Show>
              )}
            </Show>
          </Match>
        </Switch>
      </div>
    </div>
  );
}

export default StructureTab;
