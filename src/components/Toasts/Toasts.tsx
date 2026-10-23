import styles from "./Toasts.module.scss";
import { For, Match, Switch } from "solid-js";
import { CircleAlert, CircleCheck, Info, X } from "lucide-solid";
import useToastState from "@core/states/toastState";

function Toasts() {
  const { toasts, dismissToast } = useToastState;

  return (
    <div class={styles.Container} role="status" aria-live="polite">
      <For each={toasts}>
        {toast => (
          <div class={`${styles.Toast} ${styles[toast.variant]}`}>
            <span class={styles.Icon}>
              <Switch fallback={<Info size={16} />}>
                <Match when={toast.variant === "success"}><CircleCheck size={16} /></Match>
                <Match when={toast.variant === "error"}><CircleAlert size={16} /></Match>
              </Switch>
            </span>
            <span class={styles.Message}>{toast.message}</span>
            <button class={styles.Close} onClick={() => dismissToast(toast.id)} aria-label="Dismiss">
              <X size={14} />
            </button>
          </div>
        )}
      </For>
    </div>
  );
}

export default Toasts;
