import styles from "./App.module.scss";
import { For, Show, createMemo, createSignal, onMount } from "solid-js";
import { Database, House, Settings, X } from "lucide-solid";
import TabStrip, { TabStripItem } from "./components/TabStrip/TabStrip";
import Button from "./components/Button/Button";
import SettingsModal from "./components/SettingsModal/SettingsModal";
import Toasts from "./components/Toasts/Toasts";
import ContextMenuHost from "./components/ContextMenu/ContextMenuHost";
import TooltipHost from "./components/Tooltip/TooltipHost";
import Prompts from "./components/Prompts/Prompts";
import PortForwardModal from "./components/PortForwardModal/PortForwardModal";
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
import useModalState from "./states/modalState";
import WindowControls from "./components/WindowControls/WindowControls";
import { installGlobalContextMenu } from "./helpers/globalContextMenu";
import { IS_MAC } from "./helpers/platform";
import { safeInvoke } from "./helpers/safeInvoke";
import { keepOrder } from "./helpers";

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
        tint: useSettingsState.settings.colorFill ? session?.profile.color : null,
        closable: true,
        movable: true,
      };
    }),
  );

  const titleBarLeading = createMemo(() => (IS_MAC ? <div class={styles.TrafficLights} data-tauri-drag-region /> : undefined));

  // Pages keep their DOM order when tabs move, since moving a node resets its scroll.
  const sessionIds = createMemo<string[]>(previous => keepOrder(
    previous,
    workspace.store.globalTabs.flatMap(tab => (tab.kind === "session" ? [tab.sessionId] : [])),
  ), []);

  installGlobalContextMenu();

  async function checkForUpdates() {
    const found = await useUpdaterState.checkForUpdate();
    if (!found) return;
    const hint = useUpdaterState.selfUpdate ? "Open Settings to install it." : "Update it through Nix.";
    useToastState.pushToast(`Fran DB ${found.version} is available. ${hint}`, "info", 8000);
  }

  onMount(() => {
    // Two frames so the first paint is done before the window swaps in, otherwise it shows blank.
    requestAnimationFrame(() => requestAnimationFrame(() => safeInvoke("show_main_window")));
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
        onReorder={workspace.moveGlobalTab}
        onContextMenu={(event, id) => {
          if (id === HOME_TAB_ID) return;
          openContextMenu(event, [
            { label: "Disconnect", icon: X, onSelect: () => workspace.closeSession(id) },
          ]);
        }}
        dragRegion
        locked={useModalState.anyOpen()}
        leading={titleBarLeading()}
        trailing={
          <>
            <div class={styles.Actions} classList={{ [styles.Locked]: useModalState.anyOpen() }}>
              <Button icon size="sm" tooltip="Settings" onClick={() => setSettingsOpen(true)}>
                <Settings size={16} />
              </Button>
            </div>
            <Show when={!IS_MAC}>
              <WindowControls />
            </Show>
          </>
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
      <PortForwardModal />
      <ContextMenuHost />
      <TooltipHost />
      <Toasts />
    </div>
  );
}

export default App;
