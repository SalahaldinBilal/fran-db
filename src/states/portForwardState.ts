import { createRoot, createSignal } from "solid-js";
import { createStore, produce } from "solid-js/store";
import { ConnectionProfile, PortForwardInfo } from "@core/types";
import { safeInvoke } from "@core/helpers/safeInvoke";
import { withConnectPrompts } from "@core/helpers/connectFlow";
import useConnectionsState from "@core/states/connectionsState";

function usePortForwardStateInner() {
  const [forwards, setForwards] = createStore<Record<string, PortForwardInfo>>({});
  const [dialogProfile, setDialogProfile] = createSignal<ConnectionProfile | null>(null);

  async function load() {
    const list = await safeInvoke("list_port_forwards");
    setForwards(Object.fromEntries(list.map(info => [info.connectionId, info])));
  }

  function active(connectionId: string): PortForwardInfo | undefined {
    return forwards[connectionId];
  }

  // Null means the user cancelled a credential or host key prompt.
  async function start(profile: ConnectionProfile, localPort: number): Promise<PortForwardInfo | null> {
    const info = await withConnectPrompts(
      { connectionId: profile.id, connectionName: profile.name },
      secrets => safeInvoke("start_port_forward", { connectionId: profile.id, localPort, secrets }),
    );
    if (info) {
      setForwards(info.connectionId, info);
      if (profile.forwardPort !== localPort) await useConnectionsState.save({ ...profile, forwardPort: localPort });
    }
    return info;
  }

  async function stop(connectionId: string) {
    await safeInvoke("stop_port_forward", { connectionId });
    setForwards(produce(all => { delete all[connectionId]; }));
  }

  function openDialog(profile: ConnectionProfile) {
    setDialogProfile(profile);
  }

  function closeDialog() {
    setDialogProfile(null);
  }

  void load();

  return { active, start, stop, dialogProfile, openDialog, closeDialog };
}

const usePortForwardState = createRoot(usePortForwardStateInner);
export default usePortForwardState;
