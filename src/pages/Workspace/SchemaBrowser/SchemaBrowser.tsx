import styles from "./SchemaBrowser.module.scss";
import { Match, Show, Switch, createMemo, createSignal, onMount, untrack } from "solid-js";
import { Cable, Database, RefreshCw, Unplug } from "lucide-solid";
import useWorkspaceState from "@core/states/workspaceState";
import useToastState from "@core/states/toastState";
import usePortForwardState from "@core/states/portForwardState";
import { databaseModule, withDatabaseModule } from "@core/databases";
import { safeInvoke } from "@core/helpers/safeInvoke";
import Button from "@core/components/Button/Button";
import Select from "@core/components/Select/Select";
import Spinner from "@core/components/Spinner/Spinner";
import SegmentedControl from "@core/components/SegmentedControl/SegmentedControl";
import { tooltip } from "@core/components/Tooltip/Tooltip";
import SchemaTree from "./SchemaTree";
import SavedQueriesPanel from "./SavedQueriesPanel";

const MIN_WIDTH = 200;
const MAX_WIDTH = 560;

type Panel = "schemas" | "queries";

function SchemaBrowser(props: { sessionId: string }) {
  const workspace = useWorkspaceState;
  const { pushError } = useToastState;
  const forwards = usePortForwardState;
  const [width, setWidth] = createSignal(280);
  const [panel, setPanel] = createSignal<Panel>("schemas");
  const [catalogs, setCatalogs] = createSignal<string[]>([]);
  const session = createMemo(() => workspace.store.sessions[props.sessionId]);
  const module = databaseModule(untrack(() => workspace.store.sessions[props.sessionId].info.kind));

  const catalog = createMemo(() => (module.catalogLabel ? session()?.info.catalog ?? null : null));
  const catalogItems = createMemo(() => {
    const current = catalog();
    const names = current && !catalogs().includes(current) ? [current, ...catalogs()] : catalogs();
    return names.map(name => ({ value: name, label: name }));
  });

  async function loadCatalogs() {
    if (!module.catalogLabel) return;
    try {
      setCatalogs(await safeInvoke("list_catalogs", { sessionId: props.sessionId }));
    } catch (error) {
      pushError(error);
    }
  }

  function refresh() {
    workspace.loadDatabases(props.sessionId);
    loadCatalogs();
  }

  onMount(loadCatalogs);

  function startResize(event: MouseEvent) {
    event.preventDefault();
    const startX = event.clientX;
    const startWidth = width();

    function onMove(move: MouseEvent) {
      setWidth(Math.min(MAX_WIDTH, Math.max(MIN_WIDTH, startWidth + move.clientX - startX)));
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

  return (
    <aside class={styles.Browser} style={{ width: `${width()}px` }}>
      <Show when={session()}>
        {current => (
          <>
            <header class={styles.Header}>
              <div
                class={styles.Badge}
                style={{ "--badge": current().profile.color ?? "var(--surface-active)" }}
                aria-hidden="true"
              >
                {current().profile.name.trim().charAt(0).toUpperCase()}
              </div>
              <div class={styles.ConnectionText}>
                <div class={styles.ConnectionName} ref={element => tooltip(element, () => current().profile.name)}>
                  {current().profile.name}
                </div>
                <div class={styles.ServerVersion}>
                  {current().info.server.product} {current().info.server.version}
                </div>
              </div>
              <Show when={withDatabaseModule(current().profile, (module, options) => module.sshTarget(options)) !== null}>
                <Button
                  icon
                  size="sm"
                  active={forwards.active(current().profile.id) !== undefined}
                  tooltip="Port forward"
                  onClick={() => forwards.openDialog(current().profile)}
                >
                  <Cable size={14} />
                </Button>
              </Show>
              <Button icon size="sm" tooltip="Refresh" onClick={refresh}>
                <RefreshCw size={14} />
              </Button>
              <Button icon size="sm" tooltip="Disconnect" onClick={() => workspace.closeSession(props.sessionId)}>
                <Unplug size={14} />
              </Button>
            </header>

            <Show when={catalog()}>
              {name => (
                <div class={styles.CatalogPicker} ref={element => tooltip(element, () => module.catalogLabel)}>
                  <Database size={14} class={styles.CatalogIcon} />
                  <div class={styles.CatalogSelect}>
                    <Select
                      size="sm"
                      value={name()}
                      items={catalogItems()}
                      disabled={current().switchingCatalog}
                      onChange={next => workspace.switchCatalog(props.sessionId, next)}
                      style={{ width: "100%" }}
                    />
                  </div>
                  <Show when={current().switchingCatalog}>
                    <Spinner size={12} />
                  </Show>
                </div>
              )}
            </Show>

            <div class={styles.PanelSwitch}>
              <SegmentedControl
                fill
                value={panel()}
                onChange={setPanel}
                options={[
                  { value: "schemas", label: "Schemas" },
                  { value: "queries", label: "Saved queries" },
                ]}
              />
            </div>

            <Switch>
              <Match when={panel() === "schemas"}>
                <SchemaTree sessionId={props.sessionId} />
              </Match>
              <Match when={panel() === "queries"}>
                <SavedQueriesPanel sessionId={props.sessionId} />
              </Match>
            </Switch>
          </>
        )}
      </Show>
      <div class={styles.ResizeHandle} onMouseDown={startResize} />
    </aside>
  );
}

export default SchemaBrowser;
