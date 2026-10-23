import { createRoot } from "solid-js";
import { createStore, produce } from "solid-js/store";
import { errorMessage } from "@core/helpers/errors";

export type ToastVariant = "info" | "success" | "error";
export type Toast = { id: number, message: string, variant: ToastVariant };

function useToastStateInner() {
  const [toasts, setToasts] = createStore<Toast[]>([]);
  let nextId = 0;

  function pushToast(message: string, variant: ToastVariant = "info", durationMs = 4500) {
    const id = nextId++;
    setToasts(produce(list => list.push({ id, message, variant })));
    if (durationMs > 0) setTimeout(() => dismissToast(id), durationMs);
  }

  function pushError(error: unknown) {
    pushToast(errorMessage(error), "error", 7000);
  }

  function dismissToast(id: number) {
    setToasts(list => list.filter(toast => toast.id !== id));
  }

  return { toasts, pushToast, pushError, dismissToast };
}

const useToastState = createRoot(useToastStateInner);
export default useToastState;
