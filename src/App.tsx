import styles from "./App.module.scss";
import { For, createMemo, createSignal, onMount } from "solid-js";
import { Database, House, Settings, X } from "lucide-solid";
import TabStrip, { TabStripItem } from "./components/TabStrip/TabStrip";
import Button from "./components/Button/Button";
import SettingsModal from "./components/SettingsModal/SettingsModal";
import Toasts from "./components/Toasts/Toasts";
import ContextMenuHost from "./components/ContextMenu/ContextMenuHost";
import Prompts from "./components/Prompts/Prompts";
import Home from "./pages/Home/Home";
import Workspace from "./pages/Workspace/Workspace";
import useWorkspaceState, { HOME_TAB_ID } from "./states/workspaceState";
import useContextMenuState from "./states/contextMenuState";
import useConnectionsState from "./states/connectionsState";
import useVaultState from "./states/vaultState";
import useSavedQueriesState from "./states/savedQueriesState";
import useSettingsState from "./states/settingsState";
import useUpdaterState from "./states/updaterState";
import useToastState from "./states/toastState";
import { installGlobalContextMenu } from "./helpers/globalContextMenu";

function App() {
  const workspace = useWorkspaceState;
  const { openContextMenu } = useContextMenuState;
  const [settingsOpen, setSettingsOpen] = createSignal(false);

  const tabItems = createMemo<TabStripItem[]>(() =>
    workspace.store.globalTabs.map(tab => {
      if (tab.kind === "home") {
        return { id: tab.id, label: "Connections", icon: House };
      }

      const session = workspace.store.sessions[tab.sessionId];
      return {
        id: tab.id,
        label: session?.profile.name ?? "Connection",
        icon: session?.profile.color ? undefined : Database,
        dot: session?.profile.color,
        closable: true,
      };
    }),
  );

  const sessionIds = createMemo(() =>
    workspace.store.globalTabs.flatMap(tab => (tab.kind === "session" ? [tab.sessionId] : [])),
  );

  installGlobalContextMenu();

  async function checkForUpdates() {
    const found = await useUpdaterState.checkForUpdate();
    if (found) useToastState.pushToast(`Fran DB ${found.version} is available. Open Settings to install it.`, "info", 8000);
  }

  onMount(() => {
    useSettingsState.load();
    checkForUpdates();
    useConnectionsState.load();
    useSavedQueriesState.load();
    useVaultState.refresh();
  });

  return (
    <div class={styles.App}>
      <TabStrip
        variant="global"
        items={tabItems()}
        activeId={workspace.store.activeGlobalTabId}
        onSelect={workspace.activateGlobalTab}
        onClose={workspace.closeSession}
        onContextMenu={(event, id) => {
          if (id === HOME_TAB_ID) return;
          openContextMenu(event, [
            { label: "Disconnect", icon: X, onSelect: () => workspace.closeSession(id) },
          ]);
        }}
        trailing={
          <Button icon size="sm" tooltip="Settings" onClick={() => setSettingsOpen(true)}>
            <Settings size={16} />
          </Button>
        }
      />
      <main class={styles.Content}>
        <div class={styles.Page} classList={{ [styles.Hidden]: workspace.store.activeGlobalTabId !== HOME_TAB_ID }}>
          <Home />
        </div>
        <For each={sessionIds()}>
          {sessionId => (
            <div class={styles.Page} classList={{ [styles.Hidden]: workspace.store.activeGlobalTabId !== sessionId }}>
              <Workspace sessionId={sessionId} />
            </div>
          )}
        </For>
      </main>
      <SettingsModal show={settingsOpen()} onClose={() => setSettingsOpen(false)} />
      <Prompts />
      <ContextMenuHost />
      <Toasts />
    </div>
  );
}

export default App;
