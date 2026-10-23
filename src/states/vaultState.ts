import { createRoot, createSignal } from "solid-js";
import { safeInvoke } from "@core/helpers/safeInvoke";
import { VaultStatus } from "@core/types";
import useToastState from "./toastState";

function useVaultStateInner() {
  const { pushError } = useToastState;
  const [status, setStatus] = createSignal<VaultStatus | null>(null);

  async function refresh() {
    try {
      setStatus(await safeInvoke("get_vault_status"));
    } catch (error) {
      pushError(error);
    }
  }

  async function reset() {
    try {
      setStatus(await safeInvoke("reset_vault"));
    } catch (error) {
      pushError(error);
    }
  }

  return { status, refresh, reset };
}

const useVaultState = createRoot(useVaultStateInner);
export default useVaultState;
