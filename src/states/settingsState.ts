import { createRoot, createSignal } from "solid-js";
import { createStore, reconcile } from "solid-js/store";
import { safeInvoke } from "@core/helpers/safeInvoke";
import useToastState from "./toastState";

export type CellEditMode = "viewer" | "inline";

export type AppSettings = {
  cellEditMode: CellEditMode,
  runTableQueryOnOpen: boolean,
};

const CELL_EDIT_MODES: readonly CellEditMode[] = ["viewer", "inline"];

export const DEFAULT_SETTINGS: AppSettings = {
  cellEditMode: "viewer",
  runTableQueryOnOpen: true,
};

// Keys missing from an older file, or holding values this version doesn't know, take the default.
function fromStored(stored: unknown): AppSettings {
  const values = (stored && typeof stored === "object" ? stored : {}) as Partial<Record<keyof AppSettings, unknown>>;
  return {
    cellEditMode: CELL_EDIT_MODES.includes(values.cellEditMode as CellEditMode)
      ? values.cellEditMode as CellEditMode
      : DEFAULT_SETTINGS.cellEditMode,
    runTableQueryOnOpen: typeof values.runTableQueryOnOpen === "boolean"
      ? values.runTableQueryOnOpen
      : DEFAULT_SETTINGS.runTableQueryOnOpen,
  };
}

function useSettingsStateInner() {
  const { pushError } = useToastState;
  const [settings, setSettings] = createStore<AppSettings>({ ...DEFAULT_SETTINGS });
  const [loadError, setLoadError] = createSignal<unknown>(null);

  async function load() {
    try {
      setSettings(reconcile(fromStored(await safeInvoke("get_settings"))));
      setLoadError(null);
    } catch (error) {
      setLoadError(error);
    }
  }

  async function update(patch: Partial<AppSettings>) {
    const previous = { ...settings };
    const next = { ...previous, ...patch };
    setSettings(reconcile(next));
    try {
      await safeInvoke("set_settings", { settings: next });
    } catch (error) {
      setSettings(reconcile(previous));
      pushError(error);
    }
  }

  return { settings, loadError, load, update };
}

const useSettingsState = createRoot(useSettingsStateInner);
export default useSettingsState;
