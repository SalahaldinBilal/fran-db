import styles from "./DatabaseTabs.module.scss";
import { For, Match, Show, Switch, createMemo, onMount } from "solid-js";
import { Database, FileCode, PencilRuler, Plus, SquareTerminal, X } from "lucide-solid";
import useWorkspaceState, { DatabaseTab } from "@core/states/workspaceState";
import useSavedQueriesState from "@core/states/savedQueriesState";
import useContextMenuState from "@core/states/contextMenuState";
import usePromptState from "@core/states/promptState";
import { objectKindInfo } from "@core/constants/objectKinds";
import TabStrip, { TabStripApi, TabStripItem } from "@core/components/TabStrip/TabStrip";
import Button from "@core/components/Button/Button";
import EmptyState from "@core/components/EmptyState/EmptyState";
import { tooltip } from "@core/components/Tooltip/Tooltip";
import { keepOrder } from "@core/helpers";
import QueryTab from "../../tabs/QueryTab/QueryTab";
import StructureTab from "../../tabs/StructureTab/StructureTab";
import TableDesignerTab from "../../tabs/TableDesignerTab/TableDesignerTab";

export type DatabaseTabsProps = {
  sessionId: string,
  database: string,
};

function tabIcon(tab: DatabaseTab) {
  switch (tab.kind) {
    case "query":
      return SquareTerminal;
    case "structure":
      return tab.objectKind === "table" ? FileCode : objectKindInfo(tab.objectKind).icon;
    case "design":
      return PencilRuler;
  }
}

function DatabaseTabs(props: DatabaseTabsProps) {
  const workspace = useWorkspaceState;
  const savedQueries = useSavedQueriesState;
  const { openContextMenu } = useContextMenuState;
  const { confirm } = usePromptState;
  let strip: TabStripApi | undefined;

  const scope = createMemo(() => workspace.store.sessions[props.sessionId]?.scopes[props.database]);
  const items = createMemo<TabStripItem[]>(() =>
    (scope()?.tabs ?? []).map(tab => ({
      id: tab.id,
      label: tab.title,
      icon: tabIcon(tab),
      closable: true,
      renamable: true,
      movable: true,
    })),
  );
  // Panels keep their DOM order when tabs move, since moving a node resets its scroll.
  const panelTabs = createMemo<DatabaseTab[]>(previous => keepOrder(previous, scope()?.tabs ?? []), []);

  onMount(() => workspace.ensureCatalog(props.sessionId, props.database));

  function newQuery() {
    workspace.newQueryTab(props.sessionId, props.database);
  }

  function findTab(id: string) {
    return scope()?.tabs.find(tab => tab.id === id);
  }

  async function closeTab(id: string) {
    const tab = findTab(id);
    const text = tab?.kind === "query" ? workspace.queryText(tab) : "";
    const kept = savedQueries.queries.some(query => query.sql === text);
    if (text.trim() !== "" && !kept) {
      const confirmed = await confirm({
        title: `Close ${tab?.title}?`,
        message: "Its text isn't in any saved query and will be lost. Press Ctrl+S in the tab to save it as a query first.",
        confirmLabel: "Close",
        danger: true,
      });
      if (!confirmed) return;
    }
    workspace.closeTab(props.sessionId, props.database, id);
  }

  function renameTab(id: string, title: string) {
    workspace.updateTab(props.sessionId, props.database, id, current => { current.title = title; });
  }

  return (
    <div class={styles.DatabaseTabs}>
      <div class={styles.Bar}>
        <div class={styles.DatabaseName} ref={element => tooltip(element, () => props.database)}>
          <Database size={14} />
          <span>{props.database}</span>
        </div>
        <TabStrip
          items={items()}
          activeId={scope()?.activeTabId ?? null}
          onReady={api => { strip = api; }}
          onSelect={id => workspace.activateTab(props.sessionId, props.database, id)}
          onClose={closeTab}
          onRename={renameTab}
          onReorder={(id, toIndex) => workspace.moveTab(props.sessionId, props.database, id, toIndex)}
          onContextMenu={(event, id) => openContextMenu(event, [
            { label: "Rename", hint: "F2", onSelect: () => strip?.startRename(id) },
            { separator: true },
            { label: "Close", icon: X, onSelect: () => closeTab(id) },
            { label: "Close others", onSelect: () => workspace.closeOtherTabs(props.sessionId, props.database, id) },
          ])}
          trailing={
            <Button icon size="sm" tooltip="New query" onClick={newQuery}>
              <Plus size={16} />
            </Button>
          }
        />
      </div>

      <div class={styles.Body}>
        <Show
          when={(scope()?.tabs.length ?? 0) > 0}
          fallback={
            <EmptyState
              title={`Nothing open in ${props.database}`}
              description="Open a query editor, or double-click a table on the left to browse its rows."
              actions={
                <Button variant="subtle" onClick={newQuery}>
                  <Plus size={14} />
                  New query
                </Button>
              }
            />
          }
        >
          <For each={panelTabs()}>
            {tab => (
              <div class={styles.Panel} classList={{ [styles.Hidden]: scope()?.activeTabId !== tab.id }}>
                <Switch>
                  <Match when={tab.kind === "query" && tab}>
                    {query => (
                      <QueryTab
                        sessionId={props.sessionId}
                        database={props.database}
                        tab={query()}
                        active={scope()?.activeTabId === tab.id}
                      />
                    )}
                  </Match>
                  <Match when={tab.kind === "structure" && tab}>
                    {structure => (
                      <StructureTab
                        sessionId={props.sessionId}
                        database={props.database}
                        objectName={structure().objectName}
                        objectKind={structure().objectKind}
                        objectSignature={structure().objectSignature}
                      />
                    )}
                  </Match>
                  <Match when={tab.kind === "design" && tab}>
                    {design => (
                      <TableDesignerTab
                        sessionId={props.sessionId}
                        database={props.database}
                        tabId={design().id}
                        table={design().table}
                      />
                    )}
                  </Match>
                </Switch>
              </div>
            )}
          </For>
        </Show>
      </div>
    </div>
  );
}

export default DatabaseTabs;
