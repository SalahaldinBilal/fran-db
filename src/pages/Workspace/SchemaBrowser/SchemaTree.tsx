import styles from "./SchemaBrowser.module.scss";
import { For, Match, Show, Switch, createMemo, createSignal, untrack } from "solid-js";
import { Dynamic } from "solid-js/web";
import { ChevronRight, CircleAlert, Database, Folder, Search } from "lucide-solid";
import useWorkspaceState from "@core/states/workspaceState";
import useContextMenuState from "@core/states/contextMenuState";
import { OBJECT_KINDS, objectKindInfo } from "@core/constants/objectKinds";
import { databaseModule } from "@core/databases";
import { errorMessage } from "@core/helpers/errors";
import { formatBytes, formatCount } from "@core/helpers";
import { DatabaseInfo, ObjectInfo } from "@core/types";
import Input from "@core/components/Input/Input";
import Spinner from "@core/components/Spinner/Spinner";
import { databaseMenu, folderMenu, objectMenu } from "./menus";

// Workbench-style: these folders always show, the rest only when they have something in them.
const ALWAYS_SHOWN = new Set(["table", "view", "procedure", "function"]);

function SchemaTree(props: { sessionId: string }) {
  const workspace = useWorkspaceState;
  const { openContextMenu } = useContextMenuState;

  const [filter, setFilter] = createSignal("");
  const [showSystem, setShowSystem] = createSignal(false);
  const [openFolders, setOpenFolders] = createSignal<Set<string>>(new Set());

  const session = createMemo(() => workspace.store.sessions[props.sessionId]);
  const databaseKind = untrack(() => workspace.store.sessions[props.sessionId].info.kind);
  const namespace = databaseModule(databaseKind).namespace;
  const needle = createMemo(() => filter().trim().toLowerCase());

  const visibleDatabases = createMemo(() => {
    const databases = session()?.databases.items ?? [];
    const query = needle();
    if (!query) return databases;

    return databases.filter(database =>
      database.name.toLowerCase().includes(query)
      || (session()?.objects[database.name]?.items ?? []).some(object => object.name.toLowerCase().includes(query)),
    );
  });

  const userDatabases = createMemo(() => visibleDatabases().filter(database => !database.system));
  const systemDatabases = createMemo(() => visibleDatabases().filter(database => database.system));

  function toggleFolder(key: string) {
    setOpenFolders(folders => {
      const next = new Set(folders);
      if (next.has(key)) next.delete(key);
      else next.add(key);
      return next;
    });
  }

  function openObject(database: string, object: ObjectInfo) {
    if (objectKindInfo(object.kind).hasRows) {
      workspace.queryTable(props.sessionId, database, object.name);
    } else {
      workspace.openStructure(props.sessionId, database, object);
    }
  }

  function DatabaseNode(nodeProps: { database: DatabaseInfo }) {
    const name = createMemo(() => nodeProps.database.name);
    const expanded = createMemo(() => session()?.expanded[name()] ?? false);
    const selected = createMemo(() => session()?.selectedDatabase === name());
    const objects = createMemo(() => session()?.objects[name()]);
    const filteredObjects = createMemo(() => {
      const items = objects()?.items ?? [];
      const query = needle();
      if (!query || name().toLowerCase().includes(query)) return items;
      return items.filter(object => object.name.toLowerCase().includes(query));
    });

    return (
      <div>
        <div
          class={styles.Row}
          classList={{ [styles.Selected]: selected(), [styles.System]: nodeProps.database.system }}
          style={{ "--depth": "0" }}
          onClick={() => workspace.selectDatabase(props.sessionId, name())}
          onDblClick={() => workspace.toggleExpanded(props.sessionId, name())}
          onContextMenu={event => {
            workspace.selectDatabase(props.sessionId, name());
            openContextMenu(event, databaseMenu(props.sessionId, name()));
          }}
        >
          <button
            class={styles.Chevron}
            classList={{ [styles.Open]: expanded() }}
            aria-label={expanded() ? "Collapse" : "Expand"}
            onClick={event => {
              event.stopPropagation();
              workspace.toggleExpanded(props.sessionId, name());
            }}
            onDblClick={event => event.stopPropagation()}
          >
            <ChevronRight size={14} />
          </button>
          <Database size={14} class={styles.NodeIcon} />
          <span class={styles.Name}>{name()}</span>
          <Show when={objects()?.status === "loading"}>
            <Spinner size={12} />
          </Show>
        </div>

        <Show when={expanded()}>
          <Switch>
            <Match when={objects()?.status === "error"}>
              <div class={`${styles.Message} ${styles.Error}`} style={{ "--depth": "1" }}>
                <CircleAlert size={14} />
                <span>{errorMessage(objects()?.error)}</span>
              </div>
            </Match>
            <Match when={objects()?.status === "ready" || (objects()?.items.length ?? 0) > 0}>
              <For each={OBJECT_KINDS}>
                {kind => {
                  const items = createMemo(() => filteredObjects().filter(object => object.kind === kind.kind));
                  const key = `${name()}\u0000${kind.kind}`;
                  const open = createMemo(() => openFolders().has(key) || (needle() !== "" && items().length > 0));

                  return (
                    <Show when={items().length > 0 || (ALWAYS_SHOWN.has(kind.kind) && needle() === "")}>
                      <div
                        class={`${styles.Row} ${styles.Folder}`}
                        style={{ "--depth": "1" }}
                        onClick={() => toggleFolder(key)}
                        onContextMenu={event => openContextMenu(
                          event,
                          folderMenu(props.sessionId, name(), kind.kind === "table"),
                        )}
                      >
                        <span class={styles.Chevron} classList={{ [styles.Open]: open(), [styles.Hidden]: items().length === 0 }}>
                          <ChevronRight size={14} />
                        </span>
                        <Folder size={14} class={styles.NodeIcon} />
                        <span class={styles.Name}>{kind.label}</span>
                        <span class={styles.Meta}>{items().length}</span>
                      </div>
                      <Show when={open()}>
                        <For each={items()}>
                          {object => (
                            <div
                              class={styles.Row}
                              style={{ "--depth": "2" }}
                              title={object.comment ?? object.detail ?? undefined}
                              onDblClick={() => openObject(name(), object)}
                              onContextMenu={event => openContextMenu(
                                event,
                                objectMenu(props.sessionId, name(), object),
                              )}
                            >
                              <span class={`${styles.Chevron} ${styles.Hidden}`} />
                              <Dynamic component={kind.icon} size={14} class={styles.NodeIcon} />
                              <span class={styles.Name}>{object.name}</span>
                              <Show when={(object.kind === "function" || object.kind === "procedure") && object.signature !== null}>
                                <span class={styles.Meta} title={`(${object.signature})`}>({object.signature})</span>
                              </Show>
                              <Show when={object.rowEstimate}>
                                {rows => (
                                  <span
                                    class={styles.Meta}
                                    title={object.sizeBytes !== null ? formatBytes(object.sizeBytes) : undefined}
                                  >
                                    {formatCount(rows())}
                                  </span>
                                )}
                              </Show>
                            </div>
                          )}
                        </For>
                      </Show>
                    </Show>
                  );
                }}
              </For>
            </Match>
          </Switch>
        </Show>
      </div>
    );
  }

  return (
    <>
      <div class={styles.Filter}>
        <Input
          type="search"
          value={filter()}
          onValue={setFilter}
          placeholder="Filter"
          before={<Search size={14} />}
        />
      </div>

      <div class={styles.Tree}>
        <Switch>
          <Match when={session()?.databases.status === "loading" && (session()?.databases.items.length ?? 0) === 0}>
            <div class={styles.Center}><Spinner /></div>
          </Match>
          <Match when={session()?.databases.status === "error"}>
            <div class={`${styles.Message} ${styles.Error}`} style={{ "--depth": "0" }}>
              <CircleAlert size={14} />
              <span>{errorMessage(session()?.databases.error)}</span>
            </div>
          </Match>
          <Match when={true}>
            <For each={userDatabases()}>
              {database => <DatabaseNode database={database} />}
            </For>
            <Show when={systemDatabases().length > 0}>
              <div
                class={`${styles.Row} ${styles.SystemHeader}`}
                style={{ "--depth": "0" }}
                onClick={() => setShowSystem(show => !show)}
              >
                <span class={styles.Chevron} classList={{ [styles.Open]: showSystem() || needle() !== "" }}>
                  <ChevronRight size={14} />
                </span>
                <span class={styles.Name}>System {namespace.plural}</span>
                <span class={styles.Meta}>{systemDatabases().length}</span>
              </div>
              <Show when={showSystem() || needle() !== ""}>
                <For each={systemDatabases()}>
                  {database => <DatabaseNode database={database} />}
                </For>
              </Show>
            </Show>
          </Match>
        </Switch>
      </div>
    </>
  );
}

export default SchemaTree;
