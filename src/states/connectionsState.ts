import { createRoot, createSignal } from "solid-js";
import { createStore, reconcile } from "solid-js/store";
import { safeInvoke } from "@core/helpers/safeInvoke";
import { ConnectionProfile } from "@core/types";

function useConnectionsStateInner() {
  const [connections, setConnections] = createStore<ConnectionProfile[]>([]);
  const [loaded, setLoaded] = createSignal(false);
  const [loadError, setLoadError] = createSignal<unknown>(null);

  async function load() {
    try {
      setConnections(reconcile(await safeInvoke("list_connections")));
      setLoadError(null);
    } catch (error) {
      setLoadError(error);
    } finally {
      setLoaded(true);
    }
  }

  function find(id: string): ConnectionProfile | undefined {
    return connections.find(profile => profile.id === id);
  }

  async function save(profile: ConnectionProfile): Promise<ConnectionProfile> {
    const saved = await safeInvoke("save_connection", { profile });
    const index = connections.findIndex(existing => existing.id === saved.id);
    if (index >= 0) {
      setConnections(index, reconcile(saved));
    } else {
      setConnections(connections.length, saved);
    }
    return saved;
  }

  async function duplicate(id: string) {
    await safeInvoke("duplicate_connection", { id });
    await load();
  }

  async function remove(id: string) {
    await safeInvoke("delete_connection", { id });
    setConnections(list => list.filter(profile => profile.id !== id));
  }

  return { connections, loaded, loadError, load, find, save, duplicate, remove };
}

const useConnectionsState = createRoot(useConnectionsStateInner);
export default useConnectionsState;
